//! Signal extraction — effective rating, completion time, and keywords.
//!
//! Deep module: owns the precedence logic for external data signals that
//! junk classification, playlist rule evaluation, recommendation scoring,
//! and discover all depend on. [`Game`] stays pure data; all "which source
//! wins" decisions live here.
//!
//! ## Signals
//!
//! - **Rating** (0–5 scale): manual override > RAWG (native 0–5) > IGDB
//!   (0–100, converted to 0–5 by dividing by 20).
//! - **Completion time** (seconds): manual override > HLTB main story.
//! - **Keywords** (lowercase, sorted, deduplicated): IGDB keywords/themes/
//!   genres, falling back to RAWG tags/genres when IGDB data is absent.

use std::collections::HashMap;

use crate::models::{Game, HltbSource, ManualOverrides, RatingSource};

// ---------------------------------------------------------------------------
// Rating
// ---------------------------------------------------------------------------

/// Return the effective rating on a 0–5 scale and its source.
///
/// Priority: `overrides` > RAWG (native 0–5) > IGDB (0–100, converted).
/// Returns `None` when no rating data is available.
pub fn effective_rating(
    game: &Game,
    overrides: Option<&HashMap<u32, f32>>,
) -> Option<(f32, RatingSource)> {
    if let Some(overrides) = overrides {
        if let Some(&rating) = overrides.get(&game.app_id) {
            return Some((rating, RatingSource::ManualOverride));
        }
    }
    if let Some(rawg) = &game.rawg {
        if let Some(r) = rawg.rating_0_5 {
            return Some((r, RatingSource::Rawg));
        }
    }
    if let Some(igdb) = &game.igdb {
        if let Some(r) = igdb.rating_0_100 {
            return Some((r / 20.0, RatingSource::Igdb));
        }
        if let Some(r) = igdb.total_rating_0_100 {
            return Some((r / 20.0, RatingSource::Igdb));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Completion time
// ---------------------------------------------------------------------------

/// Return the effective completion time in seconds and its source.
///
/// Manual overrides take precedence over scraped HLTB data.
pub fn effective_completion_time(
    game: &Game,
    overrides: &ManualOverrides,
) -> Option<(u32, HltbSource)> {
    if let Some(&seconds) = overrides.manual_hltb.get(&game.app_id) {
        return Some((seconds, HltbSource::ManualOverride));
    }
    game.hltb
        .as_ref()
        .and_then(|h| h.main_story_seconds.map(|s| (s, h.source.clone())))
}

// ---------------------------------------------------------------------------
// Keywords
// ---------------------------------------------------------------------------

/// Return lowercase keywords (genres, themes, tags) for similarity matching.
///
/// Prefers IGDB keywords/themes/genres; falls back to RAWG tags/genres when
/// IGDB data is absent. The result is sorted and deduplicated.
pub fn keywords_lower(game: &Game) -> Vec<String> {
    let mut kws: Vec<String> = Vec::new();
    if let Some(igdb) = &game.igdb {
        kws.extend(igdb.keywords.iter().map(|s| s.to_lowercase()));
        kws.extend(igdb.themes.iter().map(|s| s.to_lowercase()));
        kws.extend(igdb.genres.iter().map(|s| s.to_lowercase()));
    }
    if kws.is_empty() {
        if let Some(rawg) = &game.rawg {
            kws.extend(rawg.tags.iter().map(|s| s.to_lowercase()));
            kws.extend(rawg.genres.iter().map(|s| s.to_lowercase()));
        }
    }
    kws.sort();
    kws.dedup();
    kws
}
