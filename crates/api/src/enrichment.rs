//! API enrichment service.
//!
//! Bridges the gap between local Steam library scans and external API data.
//! For each game, checks the disk cache first and fetches from network only
//! when data is missing or stale. Results are persisted to [`DiskCache`] so
//! subsequent runs are fast and offline-capable.

use std::path::Path;
use std::time::Duration;

use vapourfly_core::models::{Game, HltbData, IgdbData, PcgwData, ProtonDbData, RawgData};

use crate::cache::DiskCache;
use crate::http::{CacheRecord, HttpClient};

// ---------------------------------------------------------------------------
// Cache TTLs per source
// ---------------------------------------------------------------------------

const IGDB_TTL: Duration = Duration::from_secs(7 * 24 * 3600); // 7 days
const RAWG_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const PROTONDB_TTL: Duration = Duration::from_secs(24 * 3600); // 1 day
const PCGW_TTL: Duration = Duration::from_secs(7 * 24 * 3600);
const HLTB_TTL: Duration = Duration::from_secs(30 * 24 * 3600); // 30 days
#[allow(dead_code)]
const STEAM_STORE_TTL: Duration = Duration::from_secs(24 * 3600);

// ---------------------------------------------------------------------------
// Source name constants
// ---------------------------------------------------------------------------

pub const SOURCE_IGDB: &str = "igdb";
pub const SOURCE_RAWG: &str = "rawg";
pub const SOURCE_PROTONDB: &str = "protondb";
pub const SOURCE_PCGW: &str = "pcgw";
pub const SOURCE_HLTB: &str = "hltb";
pub const SOURCE_STEAM_STORE: &str = "steam-store";

pub const ALL_SOURCES: &[&str] = &[
    SOURCE_IGDB,
    SOURCE_RAWG,
    SOURCE_PROTONDB,
    SOURCE_PCGW,
    SOURCE_HLTB,
    SOURCE_STEAM_STORE,
];

// ---------------------------------------------------------------------------
// Enrichment options
// ---------------------------------------------------------------------------

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
            sources: ALL_SOURCES.iter().map(|s| s.to_string()).collect(),
            offline: false,
            force: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Enrichment result
// ---------------------------------------------------------------------------

/// Summary of what the enrichment pass did.
#[derive(Clone, Debug, Default)]
pub struct EnrichmentSummary {
    /// Number of games processed.
    pub games_processed: usize,
    /// Number of cache hits (data was already fresh).
    pub cache_hits: usize,
    /// Number of network fetches performed.
    pub network_fetches: usize,
    /// Number of fetches that failed (logged, not fatal).
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

// ---------------------------------------------------------------------------
// Source status (for `sources status` command)
// ---------------------------------------------------------------------------

/// Status information about a single cached source.
#[derive(Clone, Debug)]
pub struct SourceStatus {
    pub name: String,
    pub cache_entries: usize,
    pub stale_entries: usize,
    pub last_success: Option<chrono::DateTime<chrono::Utc>>,
    pub last_error: Option<String>,
    pub cache_dir_exists: bool,
}

// ---------------------------------------------------------------------------
// Enrichment service
// ---------------------------------------------------------------------------

/// Enrich a list of games with data from external APIs.
///
/// For each game and each source in `options.sources`:
/// 1. Check if a fresh cache entry exists → skip if so.
/// 2. If `options.offline`, skip network fetches.
/// 3. Otherwise, call the API client and cache the result.
///
/// Returns a summary of what was done. Errors for individual games are
/// collected into the summary rather than failing the whole batch.
pub fn enrich_games(
    games: &mut [Game],
    cache: &DiskCache,
    options: &EnrichmentOptions,
) -> EnrichmentSummary {
    let mut summary = EnrichmentSummary {
        games_processed: games.len(),
        ..Default::default()
    };

    for source_name in &options.sources {
        let stats = enrich_source(games, cache, source_name, options, &mut summary.errors);
        summary.cache_hits += stats.entries_skipped;
        summary.network_fetches += stats.entries_refreshed;
        summary.source_stats.push(stats);
    }

    summary
}

/// Enrich games from a single source.
fn enrich_source(
    games: &mut [Game],
    cache: &DiskCache,
    source: &str,
    options: &EnrichmentOptions,
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
            for game in games.iter_mut() {
                let key = format!("app/{}", game.app_id);
                if !options.force {
                    if let Ok(Some(record)) = cache.get::<ProtonDbData>(source, &key) {
                        if !record.stale {
                            game.protondb = Some(record.data);
                            stats.entries_skipped += 1;
                            continue;
                        }
                    }
                }
                if options.offline {
                    continue;
                }
                match crate::protondb::ProtonDbClient::new().fetch_summary(game.app_id) {
                    Ok(data) => {
                        let record = CacheRecord {
                            source: source.to_string(),
                            key: key.clone(),
                            fetched_at: chrono::Utc::now(),
                            ttl: PROTONDB_TTL,
                            data: data.clone(),
                            stale: false,
                            etag: None,
                        };
                        let _ = cache.put(&record);
                        game.protondb = Some(data);
                        stats.entries_refreshed += 1;
                        stats.last_success = Some(chrono::Utc::now());
                    }
                    Err(e) => {
                        errors.push(EnrichmentError {
                            app_id: game.app_id,
                            source: source.to_string(),
                            message: e.to_string(),
                        });
                        stats.errors += 1;
                        // Try stale cache as fallback
                        if let Ok(Some(record)) = cache.get::<ProtonDbData>(source, &key) {
                            game.protondb = Some(record.data);
                        }
                    }
                }
            }
        }
        SOURCE_PCGW => {
            for game in games.iter_mut() {
                let key = format!("app/{}", game.app_id);
                if !options.force {
                    if let Ok(Some(record)) = cache.get::<PcgwData>(source, &key) {
                        if !record.stale {
                            game.pcgw = Some(record.data);
                            stats.entries_skipped += 1;
                            continue;
                        }
                    }
                }
                if options.offline {
                    continue;
                }
                match crate::pcgw::PcgwClient::new().fetch_by_appid(game.app_id) {
                    Ok(data) => {
                        let record = CacheRecord {
                            source: source.to_string(),
                            key: key.clone(),
                            fetched_at: chrono::Utc::now(),
                            ttl: PCGW_TTL,
                            data: data.clone(),
                            stale: false,
                            etag: None,
                        };
                        let _ = cache.put(&record);
                        game.pcgw = Some(data);
                        stats.entries_refreshed += 1;
                        stats.last_success = Some(chrono::Utc::now());
                    }
                    Err(e) => {
                        errors.push(EnrichmentError {
                            app_id: game.app_id,
                            source: source.to_string(),
                            message: e.to_string(),
                        });
                        stats.errors += 1;
                        if let Ok(Some(record)) = cache.get::<PcgwData>(source, &key) {
                            game.pcgw = Some(record.data);
                        }
                    }
                }
            }
        }
        SOURCE_HLTB => {
            for game in games.iter_mut() {
                let key = format!("name/{}", game.name);
                if !options.force {
                    if let Ok(Some(record)) = cache.get::<HltbData>(source, &key) {
                        if !record.stale {
                            game.hltb = Some(record.data);
                            stats.entries_skipped += 1;
                            continue;
                        }
                    }
                }
                if options.offline {
                    continue;
                }
                match crate::hltb::HltbClient::new().fetch(&game.name) {
                    Ok(Some(data)) => {
                        let record = CacheRecord {
                            source: source.to_string(),
                            key: key.clone(),
                            fetched_at: chrono::Utc::now(),
                            ttl: HLTB_TTL,
                            data: data.clone(),
                            stale: false,
                            etag: None,
                        };
                        let _ = cache.put(&record);
                        game.hltb = Some(data);
                        stats.entries_refreshed += 1;
                        stats.last_success = Some(chrono::Utc::now());
                    }
                    Ok(None) => {
                        stats.entries_skipped += 1;
                    }
                    Err(e) => {
                        errors.push(EnrichmentError {
                            app_id: game.app_id,
                            source: source.to_string(),
                            message: e.to_string(),
                        });
                        stats.errors += 1;
                        if let Ok(Some(record)) = cache.get::<HltbData>(source, &key) {
                            game.hltb = Some(record.data);
                        }
                    }
                }
            }
        }
        SOURCE_RAWG => {
            // RAWG requires an API key; skip silently if not configured.
            let rawg_key = match std::env::var("VAPOURFLY_RAWG_KEY") {
                Ok(k) if !k.is_empty() => k,
                _ => return stats,
            };
            for game in games.iter_mut() {
                let key = format!("name/{}", game.name);
                if !options.force {
                    if let Ok(Some(record)) = cache.get::<RawgData>(source, &key) {
                        if !record.stale {
                            game.rawg = Some(record.data);
                            stats.entries_skipped += 1;
                            continue;
                        }
                    }
                }
                if options.offline {
                    continue;
                }
                match crate::rawg::RawgClient::new(rawg_key.clone(), HttpClient::new())
                    .search_by_name(&game.name)
                {
                    Ok(Some(data)) => {
                        let record = CacheRecord {
                            source: source.to_string(),
                            key: key.clone(),
                            fetched_at: chrono::Utc::now(),
                            ttl: RAWG_TTL,
                            data: data.clone(),
                            stale: false,
                            etag: None,
                        };
                        let _ = cache.put(&record);
                        game.rawg = Some(data);
                        stats.entries_refreshed += 1;
                        stats.last_success = Some(chrono::Utc::now());
                    }
                    Ok(None) => {
                        stats.entries_skipped += 1;
                    }
                    Err(e) => {
                        errors.push(EnrichmentError {
                            app_id: game.app_id,
                            source: source.to_string(),
                            message: e.to_string(),
                        });
                        stats.errors += 1;
                        if let Ok(Some(record)) = cache.get::<RawgData>(source, &key) {
                            game.rawg = Some(record.data);
                        }
                    }
                }
            }
        }
        SOURCE_IGDB => {
            // IGDB requires credentials; skip silently if not configured.
            let igdb_id = match std::env::var("VAPOURFLY_IGDB_CLIENT_ID") {
                Ok(id) if !id.is_empty() => id,
                _ => return stats,
            };
            let igdb_secret = match std::env::var("VAPOURFLY_IGDB_CLIENT_SECRET") {
                Ok(s) if !s.is_empty() => s,
                _ => return stats,
            };
            enrich_igdb(
                games,
                cache,
                &igdb_id,
                &igdb_secret,
                HttpClient::new(),
                options,
                &mut stats,
                errors,
            );
        }
        SOURCE_STEAM_STORE => {
            // Steam Store data is already available from the local scan.
            // This source exists for price/genre enrichment which is a
            // future feature. Mark as skipped.
            stats.entries_skipped = games.len();
        }
        _ => {}
    }

    stats
}

// ---------------------------------------------------------------------------
// IGDB enrichment helper (testable with injected HttpClient)
// ---------------------------------------------------------------------------

/// Enrich games from IGDB using the given credentials and HTTP client.
///
/// Extracted from `enrich_source` so tests can inject a mock HTTP backend.
/// Uses `resolve_by_steam_appid` to map Steam AppID → IGDB ID via
/// `external_games`, then fetches full details and time-to-beat.
#[allow(clippy::too_many_arguments)]
fn enrich_igdb(
    games: &mut [Game],
    cache: &DiskCache,
    igdb_id: &str,
    igdb_secret: &str,
    http: HttpClient,
    options: &EnrichmentOptions,
    stats: &mut SourceRefreshStats,
    errors: &mut Vec<EnrichmentError>,
) {
    let source = SOURCE_IGDB;
    let client = crate::igdb::IgdbClient::new(igdb_id.to_string(), igdb_secret.to_string(), http);
    for game in games.iter_mut() {
        let key = format!("appid/{}", game.app_id);
        if !options.force {
            if let Ok(Some(record)) = cache.get::<IgdbData>(source, &key) {
                if !record.stale {
                    game.igdb = Some(record.data);
                    stats.entries_skipped += 1;
                    continue;
                }
            }
        }
        if options.offline {
            continue;
        }
        // Use resolve_by_steam_appid to map Steam AppID -> IGDB ID
        // via external_games, then fetch game details and time-to-beat.
        // Calling fetch_game_details directly would treat the Steam
        // AppID as an IGDB game ID, returning wrong/empty data.
        match client.resolve_by_steam_appid(game.app_id) {
            Ok(Some(data)) => {
                let record = CacheRecord {
                    source: source.to_string(),
                    key: key.clone(),
                    fetched_at: chrono::Utc::now(),
                    ttl: IGDB_TTL,
                    data: data.clone(),
                    stale: false,
                    etag: None,
                };
                let _ = cache.put(&record);
                game.igdb = Some(data);
                stats.entries_refreshed += 1;
                stats.last_success = Some(chrono::Utc::now());
            }
            Ok(None) => {
                // No IGDB mapping found for this Steam AppID.
                stats.entries_skipped += 1;
            }
            Err(e) => {
                errors.push(EnrichmentError {
                    app_id: game.app_id,
                    source: source.to_string(),
                    message: e.to_string(),
                });
                stats.errors += 1;
                if let Ok(Some(record)) = cache.get::<IgdbData>(source, &key) {
                    game.igdb = Some(record.data);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cache status (for `sources status` CLI command)
// ---------------------------------------------------------------------------

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
            let last_error: Option<String> = None;

            if cache_dir_exists {
                // Walk the source directory and count .json files
                for entry in walkdir::WalkDir::new(&dir).into_iter().flatten() {
                    if entry.file_type().is_file()
                        && entry.path().extension().is_some_and(|e| e == "json")
                    {
                        entries += 1;
                        // Try to read fetched_at for last_success tracking
                        if let Ok(bytes) = std::fs::read(entry.path()) {
                            if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                                if let Some(ts) = val.get("fetched_at").and_then(|v| v.as_str()) {
                                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                                        let utc = dt.with_timezone(&chrono::Utc);
                                        match last_success {
                                            Some(existing) if existing >= utc => {}
                                            _ => last_success = Some(utc),
                                        }
                                    }
                                }
                                if let Some(is_stale) = val.get("stale").and_then(|v| v.as_bool()) {
                                    if is_stale {
                                        stale += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            SourceStatus {
                name: name.to_string(),
                cache_entries: entries,
                stale_entries: stale,
                last_success,
                last_error,
                cache_dir_exists,
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, MockBackend};
    use std::collections::HashMap;
    use tempfile::TempDir;
    use vapourfly_core::models::{Game, IgdbData, SteamAppType};

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
        let mut stats = SourceRefreshStats {
            source: SOURCE_IGDB.to_string(),
            entries_refreshed: 0,
            entries_skipped: 0,
            errors: 0,
            last_success: None,
        };
        let mut errors = Vec::new();

        enrich_igdb(
            &mut games,
            &cache,
            "test_id",
            "test_secret",
            http,
            &options,
            &mut stats,
            &mut errors,
        );

        // Verify the enrichment succeeded via the full chain.
        assert_eq!(stats.entries_refreshed, 1);
        assert_eq!(stats.errors, 0);
        assert!(errors.is_empty());

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
        let r2 = CacheRecord {
            source: "protondb".to_string(),
            key: "app/730".to_string(),
            fetched_at: chrono::Utc::now() - chrono::Duration::hours(48),
            ttl: Duration::from_secs(3600),
            data: serde_json::json!({"tier": "gold"}),
            stale: true,
            etag: None,
        };
        cache.put(&r1).unwrap();
        cache.put(&r2).unwrap();

        let statuses = source_status(tmp.path());
        let protondb = statuses.iter().find(|s| s.name == "protondb").unwrap();
        assert_eq!(protondb.cache_entries, 2);
        assert!(protondb.last_success.is_some());
    }
}
