//! Bounded real-environment enrichment probe.
//!
//! Scans the local Steam library, takes the top-N games by playtime, and
//! enriches only those against the real network (Steam Store, ProtonDB,
//! PCGW; IGDB/RAWG when credentials are configured). Prints what each
//! source returned, including the Steam-Store name backfill for
//! placeholder-named games.
//!
//! Usage: `cargo run -p vapourfly-api --example enrich_probe [N]`

use vapourfly_api::cache::DiskCache;
use vapourfly_api::enrichment::{
    self, ALL_SOURCES, EnrichmentOptions, SourceCredentials, enrich_games_with,
};
use vapourfly_api::http::HttpClient;
use vapourfly_core::steam::{self, ScanOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let count: usize = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(5);

    let steam_dir = steam::detect_steam_dirs(None)
        .into_iter()
        .next()
        .ok_or("no Steam directory detected")?;
    let scan = steam::scan_library(&ScanOptions {
        steam_dir,
        account: None,
        fixtures: None,
    })?;
    println!(
        "scanned {} games, {} warnings",
        scan.games.len(),
        scan.warnings.len()
    );
    for w in &scan.warnings {
        println!("  warning[{}]: {}", w.code, w.message);
    }

    let mut games: Vec<_> = scan.games;
    games.sort_by_key(|g| std::cmp::Reverse(g.playtime_minutes.unwrap_or(0)));
    games.truncate(count);

    let cache = DiskCache::new(vapourfly_core::config::default_cache_dir());
    let options = EnrichmentOptions {
        sources: ALL_SOURCES.iter().map(|s| (*s).to_string()).collect(),
        offline: false,
        force: false,
    };
    let summary = enrich_games_with(
        &mut games,
        &cache,
        &options,
        &SourceCredentials::from_env(),
        &HttpClient::new(),
    );
    // Hydration pass mirrors workflow::prepare (applies cache + name backfill).
    enrichment::hydrate_from_cache(&mut games, &cache);

    println!(
        "\nenrichment: {} fetches, {} cache hits, {} errors",
        summary.network_fetches,
        summary.cache_hits,
        summary.errors.len()
    );
    for e in &summary.errors {
        println!("  error[{}] app {}: {}", e.source, e.app_id, e.message);
    }

    println!();
    for g in &games {
        println!(
            "{:>8}  {:<44} pt={:>6}m  store={} protondb={} pcgw={} hltb={} igdb={} rawg={}",
            g.app_id,
            g.name.chars().take(44).collect::<String>(),
            g.playtime_minutes.unwrap_or(0),
            g.steam_store.is_some(),
            g.protondb
                .as_ref()
                .map(|p| format!("{:?}", p.tier))
                .unwrap_or_else(|| "-".into()),
            g.pcgw.is_some(),
            g.hltb.is_some(),
            g.igdb.is_some(),
            g.rawg.is_some(),
        );
    }
    Ok(())
}
