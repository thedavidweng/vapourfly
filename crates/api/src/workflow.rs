//! Workflow orchestration — prepare a library for evaluation.
//!
//! Deep module: one interface (`prepare`) hides the scan → hydrate → junk
//! sequence. Both CLI and GUI call this instead of wiring the three steps
//! independently. Implements ADR-0002 lazy hydration: when not offline,
//! missing cache entries are fetched on demand; fetch failures degrade
//! gracefully (the game is evaluated with whatever data is available).

use vapourfly_core::error::Result;
use vapourfly_core::junk::apply_junk_flags;
use vapourfly_core::models::{JunkMode, JunkRules, ScanResult};
use vapourfly_core::steam::{self, ScanOptions};

use crate::cache::DiskCache;
use crate::enrichment::{ALL_SOURCES, EnrichmentOptions, enrich_games, hydrate_from_cache};

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
    /// When `true`, only read from cache; never make network requests (ADR-0002).
    pub offline: bool,
}

/// Prepare a library for evaluation: scan → hydrate → classify junk.
///
/// This is the single entry point for workflow commands (junk preview,
/// recommend, playlist match, discover, dynamic templates). It:
///
/// 1. Scans the local Steam library (`steam::scan_library`).
/// 2. Hydrates external metadata. When `offline` is `false`, missing cache
///    entries are fetched on demand (ADR-0002 lazy hydration). Fetch failures
///    degrade gracefully — the game is evaluated with available data.
/// 3. Applies junk classification with default rules and **optional**
///    [`ManualOverrides`] loaded from the platform default path
///    ([`vapourfly_core::junk::load_default_manual_overrides`]). Callers that
///    re-classify (different junk mode, cache re-hydrate) must pass the same
///    overrides — use `load_default_manual_overrides()` again, not
///    `ManualOverrides::default()`, or force-include/exclude will be wiped.
///
/// Views that need a different junk mode can re-classify the stored result
/// with [`vapourfly_core::junk::apply_junk_flags`] without re-running the
/// full workflow, provided they load the same overrides as step 3.
pub fn prepare(options: &WorkflowOptions) -> Result<ScanResult> {
    // 1. Scan
    let mut scan_result = steam::scan_library(&ScanOptions {
        steam_dir: options.steam_dir.clone(),
        account: options.account.clone(),
        fixtures: options.fixtures.clone(),
    })?;

    // 2. Hydrate (ADR-0002: lazy fetch when not offline)
    let cache = DiskCache::new(vapourfly_core::config::default_cache_dir());
    if !options.offline {
        let enrich_opts = EnrichmentOptions {
            sources: ALL_SOURCES.iter().map(|s| (*s).to_string()).collect(),
            offline: false,
            force: false,
        };
        let summary = enrich_games(&mut scan_result.games, &cache, &enrich_opts);
        tracing::info!(
            processed = summary.games_processed,
            cache_hits = summary.cache_hits,
            network_fetches = summary.network_fetches,
            errors = summary.errors.len(),
            "workflow hydration complete"
        );
    }
    // Always apply cached data (including stale entries when offline).
    let hydration = hydrate_from_cache(&mut scan_result.games, &cache);
    tracing::debug!(
        hydrated = hydration.fields_hydrated,
        stale = hydration.stale_fields_used,
        "hydrated cached metadata for workflow"
    );

    // 3. Classify junk with default rules + optional manual overrides file
    let overrides = vapourfly_core::junk::load_default_manual_overrides();
    apply_junk_flags(
        &mut scan_result.games,
        &JunkRules::default(),
        &options.junk_mode,
        &overrides,
    );

    Ok(scan_result)
}
