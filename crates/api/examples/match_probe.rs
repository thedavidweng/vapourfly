//! Bounded real-environment Playlist match probe.
//!
//! Scans the local Steam library (cache-only hydration — no bulk
//! enrichment), then runs `match_playlist_full` on a small manual playlist
//! whose missing entries are fetched live from the Steam Store for
//! completion-price calculation. Network traffic is bounded to the missing
//! AppIDs only.
//!
//! Usage: `cargo run -p vapourfly-api --example match_probe [appid ...]`
//! Default playlist: 730 (owned or not), 292030, 1245620.

use vapourfly_api::cache::DiskCache;
use vapourfly_api::workflow::{self, WorkflowOptions};
use vapourfly_core::models::{
    JunkMode, Playlist, PlaylistContent, PlaylistFile, VAPOURFLY_PLAYLIST_SCHEMA,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let app_ids: Vec<u32> = {
        let args: Vec<u32> = std::env::args()
            .skip(1)
            .filter_map(|a| a.parse().ok())
            .collect();
        if args.is_empty() {
            vec![730, 292030, 1_245_620]
        } else {
            args
        }
    };

    let steam_dir = vapourfly_core::steam::detect_steam_dirs(None)
        .into_iter()
        .next()
        .ok_or("no Steam directory detected")?;
    // Offline prepare: scan + cache-only hydration (no 865-game enrichment).
    let scan = workflow::prepare(&WorkflowOptions {
        steam_dir,
        account: None,
        fixtures: None,
        junk_mode: JunkMode::Default,
        offline: true,
        cache_root: None,
    })?;

    let pf = PlaylistFile {
        vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
        created_by: "probe".into(),
        playlist: Playlist {
            id: "match-probe".into(),
            name: "Match Probe".into(),
            description: String::new(),
            content: PlaylistContent::Manual { app_ids },
        },
    };

    // Online match: only the MISSING AppIDs are fetched (bounded).
    let cache = DiskCache::new(vapourfly_core::config::default_cache_dir());
    let report = workflow::match_playlist_full(&pf, &scan.games, &cache, false, "US", "english")?;

    println!("owned:    {:?}", report.owned);
    println!("missing:  {:?}", report.missing);
    println!(
        "completion_price: {}",
        report
            .completion_price
            .as_ref()
            .map(|p| p.format())
            .unwrap_or_else(|| "(none)".into())
    );
    println!("price_coverage: {:?}", report.price_coverage);
    Ok(())
}
