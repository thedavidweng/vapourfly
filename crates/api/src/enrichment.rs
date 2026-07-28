//! API enrichment service.
//!
//! Bridges the gap between local Steam library scans and external API data.
//! For each game, checks the disk cache first and fetches from network only
//! when data is missing or stale. Results are persisted to [`DiskCache`] so
//! subsequent runs are fast and offline-capable.

use std::path::Path;
use std::time::Duration;

use vapourfly_core::models::{
    Game, HltbData, IgdbData, PcgwData, ProtonDbData, RawgData, SteamStoreDetails,
};

use crate::cache::DiskCache;
use crate::http::{CacheRecord, HttpClient};

// TTLs are sized for background repopulation (ADR-0009): a full-library
// refresh is a rate-limited background job, so short TTLs would schedule a
// large daily job for data that rarely changes.
const IGDB_TTL: Duration = Duration::from_secs(14 * 24 * 3600); // ratings/keywords: slow-moving
const RAWG_TTL: Duration = Duration::from_secs(14 * 24 * 3600);
const PROTONDB_TTL: Duration = Duration::from_secs(7 * 24 * 3600); // tiers change rarely
const PCGW_TTL: Duration = Duration::from_secs(14 * 24 * 3600);
const HLTB_TTL: Duration = Duration::from_secs(30 * 24 * 3600); // 30 days
const STEAM_STORE_TTL: Duration = Duration::from_secs(3 * 24 * 3600); // prices/sales move fastest

pub const SOURCE_IGDB: &str = "igdb";
pub const SOURCE_RAWG: &str = "rawg";
pub const SOURCE_PROTONDB: &str = "protondb";
pub const SOURCE_PCGW: &str = "pcgw";
pub const SOURCE_HLTB: &str = "hltb";
pub const SOURCE_STEAM_STORE: &str = "steam-store";

/// All sources in enrichment order. AppID-keyed sources run first —
/// Steam Store hydration backfills placeholder names ("App <id>"), so the
/// name-keyed sources (HLTB, RAWG) that follow can look those games up.
pub const ALL_SOURCES: &[&str] = &[
    SOURCE_STEAM_STORE,
    SOURCE_IGDB,
    SOURCE_PROTONDB,
    SOURCE_PCGW,
    SOURCE_HLTB,
    SOURCE_RAWG,
];

/// Which sources to enrich from.
#[derive(Clone, Debug)]
pub struct EnrichmentOptions {
    pub sources: Vec<String>,
    /// When `true`, only read from cache; never make network requests.
    pub offline: bool,
    /// When `true`, force re-fetch even if cache is fresh.
    pub force: bool,
}

impl Default for EnrichmentOptions {
    fn default() -> Self {
        Self {
            sources: ALL_SOURCES.iter().map(|s| (*s).to_string()).collect(),
            offline: false,
            force: false,
        }
    }
}

/// Summary of a cache-only hydration pass.
#[derive(Clone, Debug, Default)]
pub struct HydrationSummary {
    pub games_processed: usize,
    pub fields_hydrated: usize,
    pub stale_fields_used: usize,
}

/// Summary of what the enrichment pass did.
#[derive(Clone, Debug, Default)]
pub struct EnrichmentSummary {
    /// Number of games processed.
    pub games_processed: usize,
    /// Number of cache hits (data was already fresh).
    pub cache_hits: usize,
    /// Number of network fetches performed.
    pub network_fetches: usize,
    /// Fetches that failed (logged, not fatal).
    pub errors: Vec<EnrichmentError>,
    /// Per-source stats.
    pub source_stats: Vec<SourceRefreshStats>,
}

/// Per-source refresh statistics.
#[derive(Clone, Debug)]
pub struct SourceRefreshStats {
    pub source: String,
    pub entries_refreshed: usize,
    pub entries_skipped: usize,
    pub errors: usize,
    pub last_success: Option<chrono::DateTime<chrono::Utc>>,
}

/// A non-fatal enrichment error for a single game+source.
#[derive(Clone, Debug)]
pub struct EnrichmentError {
    pub app_id: u32,
    pub source: String,
    pub message: String,
}

/// Status information about a single cached source.
#[derive(Clone, Debug)]
pub struct SourceStatus {
    pub name: String,
    pub cache_entries: usize,
    pub stale_entries: usize,
    pub last_success: Option<chrono::DateTime<chrono::Utc>>,
    pub cache_dir_exists: bool,
}

/// Credentials for the sources that need them, resolved once at the seam.
///
/// A source with missing credentials is skipped silently (its stats stay
/// zero) — the same graceful degradation as any other missing data.
#[derive(Clone, Debug, Default)]
pub struct SourceCredentials {
    pub rawg_key: Option<String>,
    pub igdb_client_id: Option<String>,
    pub igdb_client_secret: Option<String>,
    /// Steam Web API key (<https://steamcommunity.com/dev/apikey>): enables
    /// one-request bulk name resolution via `GetOwnedGames`.
    pub steam_api_key: Option<String>,
}

impl SourceCredentials {
    /// Resolve credentials from the `VAPOURFLY_*` environment variables only.
    pub fn from_env() -> Self {
        fn non_empty(var: &str) -> Option<String> {
            std::env::var(var).ok().filter(|v| !v.is_empty())
        }
        Self {
            rawg_key: non_empty("VAPOURFLY_RAWG_KEY"),
            igdb_client_id: non_empty("VAPOURFLY_IGDB_CLIENT_ID"),
            igdb_client_secret: non_empty("VAPOURFLY_IGDB_CLIENT_SECRET"),
            steam_api_key: non_empty("VAPOURFLY_STEAM_API_KEY"),
        }
    }

    /// Resolve credentials with the full documented precedence: environment
    /// variables first, then the user's `config.toml` (currently only the
    /// Steam Web API key is file-configurable — users create their own at
    /// <https://steamcommunity.com/dev/apikey> via the Settings UI or
    /// `vapourfly settings set steam_api_key <key>`).
    pub fn resolve() -> Self {
        let mut credentials = Self::from_env();
        if credentials.steam_api_key.is_none() {
            credentials.steam_api_key = vapourfly_core::config::resolve_steam_api_key();
        }
        credentials
    }
}

/// Enrich a list of games with data from external APIs.
///
/// For each game and each source in `options.sources`:
/// 1. Check if a fresh cache entry exists → skip if so.
/// 2. If `options.offline`, skip network fetches.
/// 3. Otherwise, call the API client and cache the result.
///
/// Returns a summary of what was done. Errors for individual games are
/// collected into the summary rather than failing the whole batch.
///
/// Credentials come from the environment and HTTP from the real backend;
/// tests inject both via [`enrich_games_with`].
pub fn enrich_games(
    games: &mut [Game],
    cache: &DiskCache,
    options: &EnrichmentOptions,
) -> EnrichmentSummary {
    enrich_games_with(
        games,
        cache,
        options,
        &SourceCredentials::from_env(),
        &HttpClient::new(),
    )
}

/// [`enrich_games`] with injected credentials and HTTP client.
///
/// This is the testable seam: one [`HttpClient`] (real or mock) serves every
/// source, so the full cache/offline/fetch/stale-fallback wiring of all six
/// sources is exercisable without the network.
pub fn enrich_games_with(
    games: &mut [Game],
    cache: &DiskCache,
    options: &EnrichmentOptions,
    credentials: &SourceCredentials,
    http: &HttpClient,
) -> EnrichmentSummary {
    let mut summary = EnrichmentSummary {
        games_processed: games.len(),
        ..Default::default()
    };

    for source_name in &options.sources {
        let stats = enrich_source(
            games,
            cache,
            source_name,
            options,
            credentials,
            http,
            &mut summary.errors,
        );
        summary.cache_hits += stats.entries_skipped;
        summary.network_fetches += stats.entries_refreshed;
        summary.source_stats.push(stats);
    }

    summary
}

// Cache key derivation is owned here, per source: writers (the enrichment
// state-machine) and readers (`hydrate_from_cache`, `missing_store_details`)
// derive keys through these functions only, so the key convention cannot
// drift between them.

fn app_key_for(app_id: u32) -> String {
    format!("app/{app_id}")
}

fn app_key(game: &Game) -> String {
    app_key_for(game.app_id)
}

fn appid_key(game: &Game) -> String {
    format!("appid/{}", game.app_id)
}

fn name_key(game: &Game) -> String {
    format!("name/{}", game.name)
}

/// One Hydration source behind the enrichment seam: how to key the cache,
/// how long entries live, which [`Game`] field it fills, and how to fetch.
///
/// `fetch` returning `Ok(None)` means "the source has no data for this game"
/// (e.g. no IGDB mapping) — counted as skipped, never cached, never an error.
struct SourceAdapter<'a, T> {
    source: &'static str,
    ttl: Duration,
    key: fn(&Game) -> String,
    field: fn(&mut Game) -> &mut Option<T>,
    fetch: &'a dyn Fn(&Game) -> vapourfly_core::error::Result<Option<T>>,
}

/// Persist a freshly fetched record. Cache-write failures are logged, never
/// fatal (the in-memory Game still gets the data).
fn put_fresh_record<T: Clone + serde::Serialize>(
    cache: &DiskCache,
    source: &str,
    key: &str,
    ttl: Duration,
    data: &T,
) {
    let record = CacheRecord {
        source: source.to_string(),
        key: key.to_string(),
        fetched_at: chrono::Utc::now(),
        ttl,
        data: data.clone(),
        stale: false,
        etag: None,
    };
    if let Err(e) = cache.put(&record) {
        tracing::warn!(error = %e, "failed to cache {}/{} record", record.source, record.key);
    }
}

/// The per-source enrichment protocol, written once for all six sources:
///
/// 1. fresh cache hit (unless `force`) → apply, skip
/// 2. `offline` → leave the field for cache-only hydration
/// 3. fetch → cache + apply; `Ok(None)` → skip; error → record it and fall
///    back to whatever cache entry exists (stale included)
fn enrich_with<T>(
    games: &mut [Game],
    cache: &DiskCache,
    adapter: SourceAdapter<'_, T>,
    options: &EnrichmentOptions,
    stats: &mut SourceRefreshStats,
    errors: &mut Vec<EnrichmentError>,
) where
    T: Clone + serde::Serialize + serde::de::DeserializeOwned,
{
    for game in games.iter_mut() {
        let key = (adapter.key)(game);
        if !options.force
            && let Ok(Some(record)) = cache.get::<T>(adapter.source, &key)
            && !record.stale
        {
            *(adapter.field)(game) = Some(record.data);
            stats.entries_skipped += 1;
            continue;
        }
        if options.offline {
            continue;
        }
        match (adapter.fetch)(game) {
            Ok(Some(data)) => {
                put_fresh_record(cache, adapter.source, &key, adapter.ttl, &data);
                *(adapter.field)(game) = Some(data);
                stats.entries_refreshed += 1;
                stats.last_success = Some(chrono::Utc::now());
            }
            Ok(None) => {
                stats.entries_skipped += 1;
            }
            Err(e) => {
                errors.push(EnrichmentError {
                    app_id: game.app_id,
                    source: adapter.source.to_string(),
                    message: e.to_string(),
                });
                stats.errors += 1;
                // Stale-cache fallback: degrade gracefully (ADR-0002).
                if let Ok(Some(record)) = cache.get::<T>(adapter.source, &key) {
                    *(adapter.field)(game) = Some(record.data);
                }
            }
        }
    }
}

/// Enrich games from a single source by binding its adapter and running the
/// shared state-machine. One client per source batch so rate limiting
/// accumulates across games; all clients share `http`'s backend and limiter.
fn enrich_source(
    games: &mut [Game],
    cache: &DiskCache,
    source: &str,
    options: &EnrichmentOptions,
    credentials: &SourceCredentials,
    http: &HttpClient,
    errors: &mut Vec<EnrichmentError>,
) -> SourceRefreshStats {
    let mut stats = SourceRefreshStats {
        source: source.to_string(),
        entries_refreshed: 0,
        entries_skipped: 0,
        errors: 0,
        last_success: None,
    };

    match source {
        SOURCE_PROTONDB => {
            let client = crate::protondb::ProtonDbClient::with_http(http.clone());
            enrich_with(
                games,
                cache,
                SourceAdapter {
                    source: SOURCE_PROTONDB,
                    ttl: PROTONDB_TTL,
                    key: app_key,
                    field: |g| &mut g.protondb,
                    fetch: &|g| client.fetch_summary(g.app_id).map(Some),
                },
                options,
                &mut stats,
                errors,
            );
        }
        SOURCE_PCGW => {
            let client = crate::pcgw::PcgwClient::with_http(http.clone());
            enrich_with(
                games,
                cache,
                SourceAdapter {
                    source: SOURCE_PCGW,
                    ttl: PCGW_TTL,
                    key: app_key,
                    field: |g| &mut g.pcgw,
                    fetch: &|g| client.fetch_by_appid(g.app_id).map(Some),
                },
                options,
                &mut stats,
                errors,
            );
        }
        SOURCE_HLTB => {
            let client = crate::hltb::HltbClient::with_http(http.clone());
            enrich_with(
                games,
                cache,
                SourceAdapter {
                    source: SOURCE_HLTB,
                    ttl: HLTB_TTL,
                    key: name_key,
                    // Placeholder names would query (and cache) garbage —
                    // treat them as "no data" until a real name backfills.
                    fetch: &|g| {
                        if g.has_placeholder_name() {
                            Ok(None)
                        } else {
                            client.fetch(&g.name)
                        }
                    },
                    field: |g| &mut g.hltb,
                },
                options,
                &mut stats,
                errors,
            );
        }
        SOURCE_RAWG => {
            let Some(rawg_key) = credentials.rawg_key.clone() else {
                return stats;
            };
            let client = crate::rawg::RawgClient::new(rawg_key, http.clone());
            enrich_with(
                games,
                cache,
                SourceAdapter {
                    source: SOURCE_RAWG,
                    ttl: RAWG_TTL,
                    key: name_key,
                    field: |g| &mut g.rawg,
                    // Placeholder names would query (and cache) garbage.
                    fetch: &|g| {
                        if g.has_placeholder_name() {
                            Ok(None)
                        } else {
                            client.search_by_name(&g.name)
                        }
                    },
                },
                options,
                &mut stats,
                errors,
            );
        }
        SOURCE_IGDB => {
            let (Some(id), Some(secret)) = (
                credentials.igdb_client_id.clone(),
                credentials.igdb_client_secret.clone(),
            ) else {
                return stats;
            };
            // resolve_by_steam_appid maps Steam AppID -> IGDB ID via
            // external_games, then fetches game details and time-to-beat.
            // Calling fetch_game_details directly would treat the Steam
            // AppID as an IGDB game ID, returning wrong/empty data.
            let client = crate::igdb::IgdbClient::new(id, secret, http.clone());
            enrich_with(
                games,
                cache,
                SourceAdapter {
                    source: SOURCE_IGDB,
                    ttl: IGDB_TTL,
                    key: appid_key,
                    field: |g| &mut g.igdb,
                    fetch: &|g| client.resolve_by_steam_appid(g.app_id),
                },
                options,
                &mut stats,
                errors,
            );
        }
        SOURCE_STEAM_STORE => {
            let client = crate::steam_store::SteamStoreClient::with_http(http.clone());
            let (cc, lang) = ("us", "english");
            enrich_with(
                games,
                cache,
                SourceAdapter {
                    source: SOURCE_STEAM_STORE,
                    ttl: STEAM_STORE_TTL,
                    key: app_key,
                    field: |g| &mut g.steam_store,
                    fetch: &|g| client.fetch_appdetails(g.app_id, cc, lang).map(Some),
                },
                options,
                &mut stats,
                errors,
            );
            // Steam Store is keyed by AppID and carries the real name —
            // backfill placeholder names so name-keyed sources and the UI
            // get real titles even without appinfo.vdf / librarycache.
            backfill_names_from_store(games);
        }
        _ => {}
    }

    stats
}

/// Replace `"App <id>"` placeholder names with the Steam Store name where
/// store details are present. No-op for games with a resolved local name.
fn backfill_names_from_store(games: &mut [Game]) {
    for game in games.iter_mut() {
        if game.has_placeholder_name()
            && let Some(store) = &game.steam_store
            && !store.name.is_empty()
        {
            game.name = store.name.clone();
        }
    }
}

/// Apply one cached field onto a game record, tracking hydration stats.
fn hydrate_field<T: serde::de::DeserializeOwned>(
    cache: &DiskCache,
    source: &str,
    key: &str,
    slot: &mut Option<T>,
    summary: &mut HydrationSummary,
) {
    if let Ok(Some(record)) = cache.get::<T>(source, key) {
        *slot = Some(record.data);
        summary.fields_hydrated += 1;
        if record.stale {
            summary.stale_fields_used += 1;
        }
    }
}

/// Load cached external metadata onto game records without network calls.
///
/// Fresh and stale cache entries are both applied. Missing cache entries are
/// left unset so callers can still degrade gracefully. Keys come from the
/// same per-source derivation the enrichment writer uses.
pub fn hydrate_from_cache(games: &mut [Game], cache: &DiskCache) -> HydrationSummary {
    let mut summary = HydrationSummary {
        games_processed: games.len(),
        ..Default::default()
    };

    for game in games.iter_mut() {
        let app = app_key(game);
        let appid = appid_key(game);

        // AppID-keyed fields first: Steam Store details backfill placeholder
        // names, so the name-keyed lookups below use the real title.
        hydrate_field::<ProtonDbData>(
            cache,
            SOURCE_PROTONDB,
            &app,
            &mut game.protondb,
            &mut summary,
        );
        hydrate_field::<PcgwData>(cache, SOURCE_PCGW, &app, &mut game.pcgw, &mut summary);
        hydrate_field::<IgdbData>(cache, SOURCE_IGDB, &appid, &mut game.igdb, &mut summary);
        hydrate_field::<SteamStoreDetails>(
            cache,
            SOURCE_STEAM_STORE,
            &app,
            &mut game.steam_store,
            &mut summary,
        );
        backfill_names_from_store(std::slice::from_mut(game));

        let name = name_key(game);
        hydrate_field::<HltbData>(cache, SOURCE_HLTB, &name, &mut game.hltb, &mut summary);
        hydrate_field::<RawgData>(cache, SOURCE_RAWG, &name, &mut game.rawg, &mut summary);
    }

    summary
}

/// Read cached Steam Store details for a set of AppIDs that are **not** in the
/// owned library (missing Playlist entries).
///
/// Uses cache-only lookups — no network calls. When online, the caller may
/// additionally fetch missing entries via [`SteamStoreClient`](crate::steam_store::SteamStoreClient)
/// before calling this function to populate the cache.
///
/// Returns a map of AppID → SteamStoreDetails for entries that have a cache
/// record (stale or fresh). AppIDs without a cache entry are simply absent
/// from the map.
pub fn missing_store_details(
    app_ids: &[u32],
    cache: &DiskCache,
) -> std::collections::HashMap<u32, SteamStoreDetails> {
    let mut map = std::collections::HashMap::new();
    for &app_id in app_ids {
        let key = app_key_for(app_id);
        if let Ok(Some(record)) = cache.get::<SteamStoreDetails>(SOURCE_STEAM_STORE, &key) {
            map.insert(app_id, record.data);
        }
    }
    map
}

/// Resolve Steam Store details for missing Playlist entries.
///
/// This is the shared API used by both CLI and GUI for completion-price
/// calculation. In **online mode** it fetches uncached AppIDs via
/// [`SteamStoreClient`](crate::steam_store::SteamStoreClient), writes each
/// result to `cache`, then returns the full map. In **offline mode** it
/// performs cache-only lookups (same as [`missing_store_details`]) and does
/// not issue any network requests.
///
/// `cc` and `lang` control pricing locale (e.g. `"us"`, `"english"`).
///
/// Returns a map of AppID → SteamStoreDetails for every AppID that has either
/// a cache entry or a successful network fetch. AppIDs that fail to fetch
/// (404, network error) are simply absent from the map.
pub fn resolve_missing_store_details(
    app_ids: &[u32],
    cache: &DiskCache,
    offline: bool,
    cc: &str,
    lang: &str,
) -> std::collections::HashMap<u32, SteamStoreDetails> {
    resolve_missing_store_details_with_http(app_ids, cache, offline, cc, lang, &HttpClient::new())
}

/// [`resolve_missing_store_details`] with an injected HTTP client (testable).
pub fn resolve_missing_store_details_with_http(
    app_ids: &[u32],
    cache: &DiskCache,
    offline: bool,
    cc: &str,
    lang: &str,
    http: &HttpClient,
) -> std::collections::HashMap<u32, SteamStoreDetails> {
    let mut map = missing_store_details(app_ids, cache);

    if offline || app_ids.is_empty() {
        return map;
    }

    let client = crate::steam_store::SteamStoreClient::with_http(http.clone());
    for &app_id in app_ids {
        if map.contains_key(&app_id) {
            continue;
        }
        match client.fetch_appdetails(app_id, cc, lang) {
            Ok(data) => {
                put_fresh_record(
                    cache,
                    SOURCE_STEAM_STORE,
                    &app_key_for(app_id),
                    STEAM_STORE_TTL,
                    &data,
                );
                map.insert(app_id, data);
            }
            Err(e) => {
                tracing::warn!(error = %e, app_id, "failed to fetch Steam Store details for missing AppID");
            }
        }
    }

    map
}

/// Read the cache directory and compute status for each known source.
pub fn source_status(cache_root: &Path) -> Vec<SourceStatus> {
    ALL_SOURCES
        .iter()
        .map(|&name| {
            let dir = cache_root.join(name);
            let cache_dir_exists = dir.exists();

            let mut entries = 0usize;
            let mut stale = 0usize;
            let mut last_success: Option<chrono::DateTime<chrono::Utc>> = None;

            if cache_dir_exists {
                for entry in walkdir::WalkDir::new(&dir).into_iter().flatten() {
                    if !entry.file_type().is_file()
                        || entry.path().extension().is_none_or(|e| e != "json")
                    {
                        continue;
                    }
                    entries += 1;
                    let Ok(bytes) = std::fs::read(entry.path()) else {
                        continue;
                    };
                    let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                        continue;
                    };
                    let fetched_utc = val
                        .get("fetched_at")
                        .and_then(|v| v.as_str())
                        .and_then(|ts| chrono::DateTime::parse_from_rfc3339(ts).ok())
                        .map(|dt| dt.with_timezone(&chrono::Utc));
                    if let Some(utc) = fetched_utc
                        && last_success.is_none_or(|existing| existing < utc)
                    {
                        last_success = Some(utc);
                    }
                    // Staleness is computed from fetched_at + ttl (mirroring
                    // CacheRecord::is_expired). The persisted `stale` field
                    // only records the fallback state at write time, which
                    // the normal put path always writes as false.
                    let ttl_secs = val
                        .get("ttl")
                        .and_then(|t| t.get("secs"))
                        .and_then(|v| v.as_u64());
                    if let (Some(fetched), Some(ttl)) = (fetched_utc, ttl_secs) {
                        let age = chrono::Utc::now()
                            .signed_duration_since(fetched)
                            .num_seconds();
                        if age > 0 && age as u64 > ttl {
                            stale += 1;
                        }
                    }
                }
            }

            SourceStatus {
                name: name.to_string(),
                cache_entries: entries,
                stale_entries: stale,
                last_success,
                cache_dir_exists,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, MockBackend};
    use std::collections::HashMap;
    use tempfile::TempDir;
    use vapourfly_core::models::{Game, IgdbData, ProtonDbData, ProtonTier, SteamAppType};

    fn store_details(app_id: u32, name: &str) -> SteamStoreDetails {
        SteamStoreDetails {
            app_id,
            name: name.into(),
            steam_store_type: "game".into(),
            is_free: false,
            short_description: None,
            header_image: None,
            developers: vec![],
            publishers: vec![],
            genres: vec![],
            categories: vec![],
            release_date: None,
            metacritic_score: None,
            platforms: vapourfly_core::models::SteamStorePlatforms {
                windows: true,
                mac: false,
                linux: false,
            },
            coming_soon: false,
            price_overview: None,
        }
    }

    fn put_store_record(cache: &DiskCache, details: SteamStoreDetails) {
        let record = CacheRecord {
            source: SOURCE_STEAM_STORE.to_string(),
            key: format!("app/{}", details.app_id),
            fetched_at: chrono::Utc::now(),
            ttl: STEAM_STORE_TTL,
            data: details,
            stale: false,
            etag: None,
        };
        cache.put(&record).unwrap();
    }

    fn make_test_game(app_id: u32, name: &str) -> Game {
        Game {
            app_id,
            name: name.into(),
            app_type: SteamAppType::Game,
            installed: false,
            install_dir: None,
            library_folder: None,
            playtime_minutes: None,
            playtime_2wks_minutes: None,
            playtime_disconnected_minutes: None,
            last_played_unix: None,
            steam_collections: vec![],
            is_hidden: false,
            is_junk: false,
            hltb: None,
            igdb: None,
            rawg: None,
            protondb: None,
            pcgw: None,
            steam_store: None,
        }
    }

    #[test]
    fn igdb_enrichment_uses_resolve_by_steam_appid() {
        // Verify the enrichment path goes through external_games -> games
        // -> game_time_to_beats (i.e. Steam AppID enters external_games,
        // IGDB ID enters games).
        let mut mock = MockBackend::new();
        // Token endpoint
        mock.register(
            "https://id.twitch.tv/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"{"access_token":"test_token","token_type":"bearer","expires_in":36000}"#
                    .to_vec(),
            },
        );
        // 1. external_games: Steam AppID 292030 -> IGDB game 1942
        mock.register(
            "https://api.igdb.com/v4/external_games",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"[{"game":1942,"uid":"292030","external_game_source":1}]"#.to_vec(),
            },
        );
        // 2. games: IGDB ID 1942 -> full game details
        mock.register(
            "https://api.igdb.com/v4/games",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"[{"id":1942,"name":"The Witcher 3: Wild Hunt","slug":"the-witcher-3-wild-hunt","rating":93.0,"total_rating":92.0,"genres":[{"name":"Role-playing (RPG)"}],"themes":[{"name":"Fantasy"}],"keywords":[{"name":"open world"}],"similar_games":[1234,5678]}]"#.to_vec(),
            },
        );
        // 3. game_time_to_beats: IGDB ID 1942 -> time-to-beat data
        mock.register(
            "https://api.igdb.com/v4/game_time_to_beats",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"[{"game_id":1942,"hastily":12000,"normally":36000,"completely":108000,"comp_count":500}]"#.to_vec(),
            },
        );

        let http = HttpClient::with_backend(Box::new(mock));
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        let options = EnrichmentOptions {
            sources: vec![SOURCE_IGDB.to_string()],
            offline: false,
            force: true,
        };

        let mut games = vec![make_test_game(292030, "The Witcher 3")];
        let credentials = SourceCredentials {
            igdb_client_id: Some("test_id".into()),
            igdb_client_secret: Some("test_secret".into()),
            ..Default::default()
        };

        let summary = enrich_games_with(&mut games, &cache, &options, &credentials, &http);

        // Verify the enrichment succeeded via the full chain.
        assert_eq!(summary.network_fetches, 1);
        assert!(summary.errors.is_empty());

        // Verify game got IGDB data.
        let game = &games[0];
        let igdb = game.igdb.as_ref().expect("IGDB data should be set");
        assert_eq!(igdb.igdb_id, 1942);
        assert_eq!(igdb.name, "The Witcher 3: Wild Hunt");
        assert!(igdb.steam_app_id_confirmed);
        assert!(igdb.time_to_beat.is_some());
        let ttb = igdb.time_to_beat.as_ref().unwrap();
        assert_eq!(ttb.normally_seconds, Some(36000));

        // Verify cache was populated with the correct key.
        let cached = cache
            .get::<IgdbData>(SOURCE_IGDB, "appid/292030")
            .unwrap()
            .expect("cache entry should exist");
        assert_eq!(cached.data.igdb_id, 1942);
    }

    #[test]
    fn steam_store_enrichment_populates_game_and_cache() {
        let body = r#"{
            "292030": {
                "success": true,
                "data": {
                    "type": "game",
                    "name": "The Witcher 3: Wild Hunt",
                    "is_free": false,
                    "developers": ["CD PROJEKT RED"],
                    "publishers": ["CD PROJEKT RED"],
                    "genres": [{"id": 3, "description": "RPG"}],
                    "categories": [{"id": 2, "description": "Single-player"}],
                    "release_date": {"coming_soon": false, "date": "May 18, 2015"},
                    "platforms": {"windows": true, "mac": true, "linux": true},
                    "price_overview": {
                        "currency": "USD",
                        "initial": 3999,
                        "final": 799,
                        "discount_percent": 80
                    }
                }
            }
        }"#;

        let mut mock = MockBackend::new();
        mock.register(
            "https://store.steampowered.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: body.as_bytes().to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        let options = EnrichmentOptions {
            sources: vec![SOURCE_STEAM_STORE.to_string()],
            offline: false,
            force: true,
        };

        let mut games = vec![make_test_game(292030, "The Witcher 3")];
        let summary = enrich_games_with(
            &mut games,
            &cache,
            &options,
            &SourceCredentials::default(),
            &http,
        );

        assert_eq!(summary.network_fetches, 1);
        assert!(summary.errors.is_empty());

        let store = games[0]
            .steam_store
            .as_ref()
            .expect("steam_store should be set");
        assert_eq!(store.name, "The Witcher 3: Wild Hunt");
        assert!(!store.is_free);
        let price = store.price_overview.as_ref().unwrap();
        assert_eq!(price.currency, "USD");
        assert_eq!(price.final_price_cents, 799);

        // Verify cache was populated.
        let cached = cache
            .get::<SteamStoreDetails>(SOURCE_STEAM_STORE, "app/292030")
            .unwrap()
            .expect("cache entry should exist");
        assert_eq!(cached.data.app_id, 292030);
    }

    #[test]
    fn protondb_wiring_fetches_and_caches_under_owned_key() {
        // The ProtonDB arm was previously wired to a real network backend and
        // untestable; this pins the full state-machine path for it.
        let mut mock = MockBackend::new();
        mock.register(
            "https://www.protondb.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"{"tier":"gold","confidence":"high","score":0.92,"total":1500}"#.to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        let options = EnrichmentOptions {
            sources: vec![SOURCE_PROTONDB.to_string()],
            offline: false,
            force: false,
        };

        let mut games = vec![make_test_game(292030, "The Witcher 3")];
        let summary = enrich_games_with(
            &mut games,
            &cache,
            &options,
            &SourceCredentials::default(),
            &http,
        );

        assert_eq!(summary.network_fetches, 1);
        assert!(summary.errors.is_empty());
        assert_eq!(games[0].protondb.as_ref().unwrap().tier, ProtonTier::Gold);

        // Cache key derivation is owned by the enrichment module: the reader
        // must find what the writer wrote.
        let cached = cache
            .get::<ProtonDbData>(SOURCE_PROTONDB, "app/292030")
            .unwrap()
            .expect("cache entry under app/<id>");
        assert_eq!(cached.data.tier, ProtonTier::Gold);

        // Second pass: fresh cache hit, no network fetch.
        let mut games2 = vec![make_test_game(292030, "The Witcher 3")];
        let summary2 = enrich_games_with(
            &mut games2,
            &cache,
            &options,
            &SourceCredentials::default(),
            &http,
        );
        assert_eq!(summary2.network_fetches, 0);
        assert_eq!(summary2.cache_hits, 1);
        assert_eq!(games2[0].protondb.as_ref().unwrap().tier, ProtonTier::Gold);
    }

    #[test]
    fn fetch_failure_falls_back_to_stale_cache() {
        // ADR-0002 degradation: a per-game fetch failure applies whatever
        // cache entry exists (stale included) and never fails the batch.
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        let stale = CacheRecord {
            source: SOURCE_PROTONDB.to_string(),
            key: "app/730".to_string(),
            fetched_at: chrono::Utc::now() - chrono::Duration::days(30),
            ttl: Duration::from_secs(3600),
            data: ProtonDbData {
                tier: ProtonTier::Platinum,
                confidence: Some("high".into()),
                score: None,
            },
            stale: false, // recomputed from fetched_at + ttl on read
            etag: None,
        };
        cache.put(&stale).unwrap();

        let mut mock = MockBackend::new();
        mock.register_error("https://www.protondb.com/", "connection refused");
        let http = HttpClient::with_backend(Box::new(mock));
        let options = EnrichmentOptions {
            sources: vec![SOURCE_PROTONDB.to_string()],
            offline: false,
            force: false,
        };

        let mut games = vec![make_test_game(730, "Counter-Strike 2")];
        let summary = enrich_games_with(
            &mut games,
            &cache,
            &options,
            &SourceCredentials::default(),
            &http,
        );

        assert_eq!(summary.errors.len(), 1);
        assert_eq!(summary.errors[0].source, SOURCE_PROTONDB);
        assert_eq!(
            games[0].protondb.as_ref().unwrap().tier,
            ProtonTier::Platinum,
            "stale cache entry must be applied as fallback"
        );
    }

    #[test]
    fn offline_mode_never_touches_the_network_for_any_source() {
        // No mock registrations: any network call would surface as an error.
        let http = HttpClient::with_backend(Box::new(MockBackend::new()));
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        let options = EnrichmentOptions {
            sources: ALL_SOURCES.iter().map(|s| (*s).to_string()).collect(),
            offline: true,
            force: false,
        };
        let credentials = SourceCredentials {
            rawg_key: Some("k".into()),
            igdb_client_id: Some("id".into()),
            igdb_client_secret: Some("s".into()),
            ..Default::default()
        };

        let mut games = vec![make_test_game(730, "Counter-Strike 2")];
        let summary = enrich_games_with(&mut games, &cache, &options, &credentials, &http);

        assert!(summary.errors.is_empty(), "offline must not fetch");
        assert_eq!(summary.network_fetches, 0);
    }

    #[test]
    fn missing_credentials_skip_sources_silently() {
        let http = HttpClient::with_backend(Box::new(MockBackend::new()));
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        let options = EnrichmentOptions {
            sources: vec![SOURCE_RAWG.to_string(), SOURCE_IGDB.to_string()],
            offline: false,
            force: true,
        };

        let mut games = vec![make_test_game(730, "Counter-Strike 2")];
        let summary = enrich_games_with(
            &mut games,
            &cache,
            &options,
            &SourceCredentials::default(),
            &http,
        );

        assert!(summary.errors.is_empty());
        assert_eq!(summary.network_fetches, 0);
        assert!(games[0].rawg.is_none());
        assert!(games[0].igdb.is_none());
    }

    #[test]
    fn store_hydration_backfills_placeholder_names() {
        // Real-world macOS condition: no appinfo.vdf / librarycache names,
        // so every game scans as "App <id>". Steam Store data (keyed by
        // AppID) must backfill the real title during hydration.
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        put_store_record(&cache, store_details(292030, "The Witcher 3: Wild Hunt"));

        let mut placeholder = make_test_game(292030, "App 292030");
        let mut resolved = make_test_game(292030, "My Local Name");
        hydrate_from_cache(std::slice::from_mut(&mut placeholder), &cache);
        hydrate_from_cache(std::slice::from_mut(&mut resolved), &cache);

        assert_eq!(placeholder.name, "The Witcher 3: Wild Hunt");
        assert_eq!(resolved.name, "My Local Name", "resolved local names win");
    }

    #[test]
    fn name_keyed_sources_skip_placeholder_names() {
        // No mock registration: any network call would error. A placeholder
        // name must be treated as "no data", not queried or cached.
        let http = HttpClient::with_backend(Box::new(MockBackend::new()));
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        let options = EnrichmentOptions {
            sources: vec![SOURCE_HLTB.to_string(), SOURCE_RAWG.to_string()],
            offline: false,
            force: true,
        };
        let credentials = SourceCredentials {
            rawg_key: Some("k".into()),
            ..Default::default()
        };

        let mut games = vec![make_test_game(384300, "App 384300")];
        let summary = enrich_games_with(&mut games, &cache, &options, &credentials, &http);

        assert!(
            summary.errors.is_empty(),
            "placeholder-named games must not hit the network: {:?}",
            summary.errors
        );
        assert_eq!(summary.network_fetches, 0);
        assert!(games[0].hltb.is_none());
        assert!(games[0].rawg.is_none());
    }

    #[test]
    fn hydrate_from_cache_applies_stale_entries() {
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        let record = CacheRecord {
            source: SOURCE_PROTONDB.to_string(),
            key: "app/730".to_string(),
            fetched_at: chrono::Utc::now() - chrono::Duration::days(30),
            ttl: Duration::from_secs(3600),
            data: ProtonDbData {
                tier: ProtonTier::Platinum,
                confidence: Some("high".into()),
                score: None,
            },
            stale: true,
            etag: None,
        };
        cache.put(&record).unwrap();

        let mut games = vec![make_test_game(730, "Counter-Strike 2")];
        let summary = hydrate_from_cache(&mut games, &cache);

        assert_eq!(summary.fields_hydrated, 1);
        assert_eq!(summary.stale_fields_used, 1);
        assert_eq!(
            games[0].protondb.as_ref().unwrap().tier,
            ProtonTier::Platinum
        );
    }

    #[test]
    fn source_status_empty_cache() {
        let tmp = TempDir::new().unwrap();
        let statuses = source_status(tmp.path());
        assert_eq!(statuses.len(), ALL_SOURCES.len());
        for s in &statuses {
            assert_eq!(s.cache_entries, 0);
            assert_eq!(s.stale_entries, 0);
            assert!(s.last_success.is_none());
            assert!(!s.cache_dir_exists);
        }
    }

    #[test]
    fn source_status_counts_entries() {
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());

        // Write a few records
        let r1 = CacheRecord {
            source: "protondb".to_string(),
            key: "app/292030".to_string(),
            fetched_at: chrono::Utc::now(),
            ttl: Duration::from_secs(3600),
            data: serde_json::json!({"tier": "platinum"}),
            stale: false,
            etag: None,
        };
        // Expired by age (48h old, 1h TTL). The persisted `stale` flag is
        // false — exactly what the production put path always writes — so
        // this asserts staleness is recomputed from fetched_at + ttl.
        let r2 = CacheRecord {
            source: "protondb".to_string(),
            key: "app/730".to_string(),
            fetched_at: chrono::Utc::now() - chrono::Duration::hours(48),
            ttl: Duration::from_secs(3600),
            data: serde_json::json!({"tier": "gold"}),
            stale: false,
            etag: None,
        };
        cache.put(&r1).unwrap();
        cache.put(&r2).unwrap();

        let statuses = source_status(tmp.path());
        let protondb = statuses.iter().find(|s| s.name == "protondb").unwrap();
        assert_eq!(protondb.cache_entries, 2);
        assert_eq!(
            protondb.stale_entries, 1,
            "the expired record must be counted stale regardless of its persisted flag"
        );
        assert!(protondb.last_success.is_some());
    }

    #[test]
    fn resolve_missing_store_details_offline_uses_cache_only() {
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        put_store_record(&cache, store_details(292030, "The Witcher 3"));

        // Offline mode: must not issue network requests.
        let result =
            resolve_missing_store_details(&[292030, 999999], &cache, true, "us", "english");

        assert!(result.contains_key(&292030));
        // Uncached AppID is absent (no network fetch in offline mode).
        assert!(!result.contains_key(&999999));
    }

    #[test]
    fn resolve_missing_store_details_online_does_not_crash_on_fetch_failure() {
        // We can't easily mock the SteamStoreClient (it creates its own
        // HttpClient internally), so we verify the online path returns at
        // least the cached entries and doesn't panic on network failures.
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        put_store_record(&cache, store_details(292030, "The Witcher 3"));

        // Online mode with AppID 0 — the fetch will fail but must not panic.
        let result = resolve_missing_store_details(&[292030, 0], &cache, false, "us", "english");

        assert!(result.contains_key(&292030));
    }

    #[test]
    fn resolve_missing_store_details_empty_input_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());

        let result = resolve_missing_store_details(&[], &cache, false, "us", "english");
        assert!(result.is_empty());
    }
}
