//! Workflow orchestration — the read half and the network-dependent verbs.
//!
//! Deep module: [`prepare`] hides the scan → resolve-names → hydrate → junk
//! sequence, and [`match_playlist_full`] hides the two-pass Playlist match
//! with Steam Store details for missing entries. Both CLI and GUI call
//! these instead of wiring the steps independently.
//!
//! **Instant first paint (ADR-0009, supersedes ADR-0002's lazy fetch):**
//! `prepare` performs no bulk network enrichment. It is scan + at most one
//! bounded network request (the owned-games name map, when a Steam Web API
//! key is configured) + cache-only hydration + junk classification —
//! seconds on any library size. Populating the cache is a separate,
//! explicit or background concern: [`crate::enrichment::enrich_games`]
//! (missing/stale only) or a forced `cache refresh`. Bounded per-item
//! fetches (missing Playlist entry prices) remain on demand. Fetch failures
//! degrade gracefully — evaluation uses whatever data is available.
//!
//! The act half of each workflow (Junk apply/hide, Recommendation
//! collection, Playlist sync) needs no network and lives in
//! [`vapourfly_core::actions`].

use std::collections::HashMap;

use vapourfly_core::error::Result;
use vapourfly_core::junk::apply_junk_flags;
use vapourfly_core::models::{
    Game, JunkMode, JunkRules, PlaylistFile, PlaylistMatchReport, ScanResult,
};
use vapourfly_core::playlist;
use vapourfly_core::steam::{self, ScanOptions};

use crate::cache::DiskCache;
use crate::enrichment::{SourceCredentials, hydrate_from_cache, resolve_missing_store_details};
use crate::http::HttpClient;
use crate::steam_web;

/// Options for preparing a library through the workflow pipeline.
#[derive(Clone, Debug)]
pub struct WorkflowOptions {
    /// Steam installation directory.
    pub steam_dir: std::path::PathBuf,
    /// Optional account override. If `None`, the most recent account is selected.
    pub account: Option<String>,
    /// Optional fixtures directory for testing. If `None`, real Steam files are read.
    pub fixtures: Option<std::path::PathBuf>,
    /// Junk classification mode applied after hydration.
    pub junk_mode: JunkMode,
    /// When `true`, never make network requests — not even the bounded
    /// name-map fetch (ADR-0009).
    pub offline: bool,
    /// Cache root override. `None` uses the platform default
    /// ([`vapourfly_core::config::default_cache_dir`]); tests point this at
    /// a temp dir.
    pub cache_root: Option<std::path::PathBuf>,
}

/// Prepare a library for evaluation: scan → resolve names → hydrate →
/// classify junk. **Never** performs bulk network enrichment (ADR-0009) —
/// wall-clock is seconds regardless of library size or cache state.
///
/// This is the single entry point for workflow commands (junk preview,
/// recommend, playlist match, discover, dynamic templates) and the GUI's
/// library scan. It:
///
/// 1. Scans the local Steam library (`steam::scan_library`).
/// 2. Resolves placeholder names from the Steam Web API's owned-games map
///    — at most one bounded request, only when a Steam Web API key is
///    configured, skipped when `offline`. Cache-first (1-day TTL).
/// 3. Hydrates external metadata from the disk cache only (stale entries
///    included; Steam Store data backfills remaining placeholder names).
/// 4. Applies junk classification with default rules and **optional**
///    [`ManualOverrides`] loaded from the platform default path
///    ([`vapourfly_core::junk::load_default_manual_overrides`]). Callers that
///    re-classify (different junk mode, cache re-hydrate) must pass the same
///    overrides — use `load_default_manual_overrides()` again, not
///    `ManualOverrides::default()`, or force-include/exclude will be wiped.
///
/// Views that need a different junk mode can re-classify the stored result
/// with [`vapourfly_core::junk::apply_junk_flags`] without re-running the
/// full workflow, provided they load the same overrides as step 4.
pub fn prepare(options: &WorkflowOptions) -> Result<ScanResult> {
    prepare_with(options, &SourceCredentials::resolve(), &HttpClient::new())
}

/// [`prepare`] with injected credentials and HTTP client (testable seam).
pub fn prepare_with(
    options: &WorkflowOptions,
    credentials: &SourceCredentials,
    http: &HttpClient,
) -> Result<ScanResult> {
    let mut scan_result = steam::scan_library(&ScanOptions {
        steam_dir: options.steam_dir.clone(),
        account: options.account.clone(),
        fixtures: options.fixtures.clone(),
    })?;

    let cache_root = options
        .cache_root
        .clone()
        .unwrap_or_else(vapourfly_core::config::default_cache_dir);
    let cache = DiskCache::new(cache_root);

    if scan_result.games.iter().any(|g| g.has_placeholder_name()) {
        let steam_root = options
            .fixtures
            .clone()
            .unwrap_or_else(|| options.steam_dir.clone());
        let steam_id64 = steam::detect_accounts(&steam_root)
            .ok()
            .and_then(|accounts| {
                steam::select_account(&accounts, options.account.as_deref())
                    .ok()
                    .map(|a| a.steam_id64.clone())
            });
        if let Some(steam_id64) = steam_id64 {
            let resolved = steam_web::resolve_owned_names(
                &mut scan_result.games,
                &cache,
                http,
                credentials.steam_api_key.as_deref(),
                &steam_id64,
                options.offline,
            );
            if resolved > 0 {
                tracing::info!(resolved, "resolved names from Steam Web API");
            }
        }
    }

    let hydration = hydrate_from_cache(&mut scan_result.games, &cache);
    tracing::debug!(
        hydrated = hydration.fields_hydrated,
        stale = hydration.stale_fields_used,
        "hydrated cached metadata for workflow"
    );

    // The scan emitted its unresolved-names warning before the Steam Web
    // API map and cached store names could backfill — recompute it.
    scan_result
        .warnings
        .retain(|w| w.code != "unresolved_names");
    let still_unresolved = scan_result
        .games
        .iter()
        .filter(|g| g.has_placeholder_name())
        .count();
    if still_unresolved > 0 {
        scan_result
            .warnings
            .push(vapourfly_core::models::ScanWarning {
                code: "unresolved_names".into(),
                message: format!(
                    "{still_unresolved} games have placeholder names (no local name source); \
                 names backfill from Steam Store hydration when online"
                ),
            });
    }

    let overrides = vapourfly_core::junk::load_default_manual_overrides();
    apply_junk_flags(
        &mut scan_result.games,
        &JunkRules::default(),
        &options.junk_mode,
        &overrides,
    );

    Ok(scan_result)
}

/// Match a Playlist against the library with Steam Store details for
/// missing entries, so `completion_price` reflects missing non-free entries.
///
/// Two passes: a preliminary match finds the missing AppIDs, their store
/// details are resolved (network fetch + cache when online, cache-only when
/// `offline`), and the final match prices them. Both CLI and GUI call this
/// instead of wiring the passes themselves.
///
/// `cc` and `lang` control pricing locale (e.g. `"US"`, `"english"`).
pub fn match_playlist_full(
    pf: &PlaylistFile,
    games: &[Game],
    cache: &DiskCache,
    offline: bool,
    cc: &str,
    lang: &str,
) -> Result<PlaylistMatchReport> {
    let empty: HashMap<u32, vapourfly_core::models::SteamStoreDetails> = HashMap::new();
    let preliminary = playlist::match_playlist(pf, games, &empty)?;
    let missing_details =
        resolve_missing_store_details(&preliminary.missing, cache, offline, cc, lang);
    playlist::match_playlist(pf, games, &missing_details)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use vapourfly_core::models::{Playlist, PlaylistContent, VAPOURFLY_PLAYLIST_SCHEMA};

    fn fixtures_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal")
    }

    fn prepare_options(cache_root: PathBuf) -> WorkflowOptions {
        WorkflowOptions {
            steam_dir: fixtures_dir(),
            account: None,
            fixtures: Some(fixtures_dir()),
            junk_mode: JunkMode::Default,
            offline: true,
            cache_root: Some(cache_root),
        }
    }

    #[test]
    fn prepare_scans_hydrates_and_classifies_offline() {
        let cache_tmp = TempDir::new().unwrap();
        let result = prepare(&prepare_options(cache_tmp.path().to_path_buf()))
            .expect("prepare must succeed against fixtures");

        assert!(
            !result.games.is_empty(),
            "fixtures library must produce games"
        );
        // Offline + empty cache: no enrichment fields, but classification ran
        // (is_junk is a derived flag; here it must at least be consistent —
        // junk always requires playtime present and low).
        for game in &result.games {
            if game.is_junk {
                assert!(
                    game.playtime_minutes.is_some(),
                    "junk requires the playtime signal (Strict-subset invariant)"
                );
            }
        }
    }

    #[test]
    fn prepare_online_never_does_bulk_network_enrichment() {
        // ADR-0009 contract: prepare is instant-first-paint — even when NOT
        // offline, it must succeed with zero network available (an empty
        // MockBackend fails every request). Only bounded, gracefully
        // degrading fetches are allowed.
        let cache_tmp = TempDir::new().unwrap();
        let mut opts = prepare_options(cache_tmp.path().to_path_buf());
        opts.offline = false;

        let http = crate::http::HttpClient::with_backend(Box::new(crate::http::MockBackend::new()));
        let credentials = SourceCredentials {
            steam_api_key: Some("k".into()), // even with a key: degrade, don't fail
            ..Default::default()
        };

        let result = prepare_with(&opts, &credentials, &http)
            .expect("prepare must not depend on the network");
        assert!(!result.games.is_empty());
    }

    #[test]
    fn prepare_is_repeatable_with_same_overrides() {
        // The documented invariant: re-running prepare with the same inputs
        // must classify identically (overrides are reloaded, not wiped).
        let cache_tmp = TempDir::new().unwrap();
        let opts = prepare_options(cache_tmp.path().to_path_buf());
        let a = prepare(&opts).unwrap();
        let b = prepare(&opts).unwrap();

        let junk_a: Vec<u32> = a
            .games
            .iter()
            .filter(|g| g.is_junk)
            .map(|g| g.app_id)
            .collect();
        let junk_b: Vec<u32> = b
            .games
            .iter()
            .filter(|g| g.is_junk)
            .map(|g| g.app_id)
            .collect();
        assert_eq!(junk_a, junk_b);
    }

    #[test]
    fn match_playlist_full_offline_prices_from_cache_only() {
        let cache_tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(cache_tmp.path());
        let scan = prepare(&prepare_options(cache_tmp.path().to_path_buf())).unwrap();

        // A manual playlist with one owned and one missing entry.
        let owned_id = scan.games[0].app_id;
        let pf = PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "test".into(),
            playlist: Playlist {
                id: "match-test".into(),
                name: "Match Test".into(),
                description: String::new(),
                content: PlaylistContent::Manual {
                    app_ids: vec![owned_id, 999_999_999],
                },
            },
        };

        let report = match_playlist_full(&pf, &scan.games, &cache, true, "US", "english").unwrap();
        assert_eq!(report.owned, vec![owned_id]);
        assert_eq!(report.missing, vec![999_999_999]);
        // Offline with an empty cache: no price data available.
        assert!(report.completion_price.is_none());
    }
}
