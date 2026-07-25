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
    if let Some(overrides) = overrides
        && let Some(&rating) = overrides.get(&game.app_id)
    {
        return Some((rating, RatingSource::ManualOverride));
    }
    if let Some(rawg) = &game.rawg
        && let Some(r) = rawg.rating_0_5
    {
        return Some((r, RatingSource::Rawg));
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
/// Manual overrides take precedence over scraped HLTB data. Overrides are a
/// Junk-scoped concept (CONTEXT.md); other consumers read the plain signal
/// via [`main_story_seconds`].
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

/// The raw completion-time signal: HLTB main-story seconds, no overrides.
///
/// Every consumer of "how long is this game" (recommend time-match, mood
/// session predicates, Finish It ratio, `HltbMaxMinutes` rules) reads this
/// accessor rather than chaining through `game.hltb` by hand.
pub fn main_story_seconds(game: &Game) -> Option<u32> {
    game.hltb.as_ref().and_then(|h| h.main_story_seconds)
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
    if kws.is_empty()
        && let Some(rawg) = &game.rawg
    {
        kws.extend(rawg.tags.iter().map(|s| s.to_lowercase()));
        kws.extend(rawg.genres.iter().map(|s| s.to_lowercase()));
    }
    kws.sort();
    kws.dedup();
    kws
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{HltbData, IgdbData, RawgData, SteamAppType};

    fn game(app_id: u32) -> Game {
        Game {
            app_id,
            name: format!("g{app_id}"),
            app_type: SteamAppType::Game,
            installed: true,
            install_dir: None,
            library_folder: None,
            playtime_minutes: Some(0),
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

    fn full_game(app_id: u32) -> Game {
        let mut g = game(app_id);
        g.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: Some(3.0),
            ratings_count: None,
            genres: vec!["Racing".into()],
            tags: vec!["arcade".into()],
            stores: vec![],
        });
        g.igdb = Some(IgdbData {
            igdb_id: 1,
            name: "g".into(),
            slug: None,
            rating_0_100: Some(90.0),
            total_rating_0_100: Some(50.0),
            genres: vec!["Strategy".into()],
            themes: vec!["Fantasy".into()],
            keywords: vec!["roguelike".into()],
            similar_game_ids: vec![],
            steam_app_id_confirmed: false,
            time_to_beat: None,
        });
        g.hltb = Some(HltbData {
            main_story_seconds: Some(7200),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });
        g
    }

    #[test]
    fn rating_precedence_override_then_rawg_then_igdb() {
        let g = full_game(1);

        let mut overrides = HashMap::new();
        overrides.insert(1u32, 5.0f32);
        let (r, src) = effective_rating(&g, Some(&overrides)).unwrap();
        assert_eq!((r, src), (5.0, RatingSource::ManualOverride));

        let (r, src) = effective_rating(&g, None).unwrap();
        assert_eq!((r, src), (3.0, RatingSource::Rawg), "RAWG wins over IGDB");

        let mut igdb_only = g.clone();
        igdb_only.rawg = None;
        let (r, src) = effective_rating(&igdb_only, None).unwrap();
        assert_eq!(
            (r, src),
            (90.0 / 20.0, RatingSource::Igdb),
            "IGDB 0-100 → 0-5"
        );

        assert!(effective_rating(&game(2), None).is_none());
    }

    #[test]
    fn completion_time_prefers_manual_override() {
        let g = full_game(1);

        let mut overrides = ManualOverrides::default();
        overrides.manual_hltb.insert(1, 999);
        let (secs, src) = effective_completion_time(&g, &overrides).unwrap();
        assert_eq!((secs, src), (999, HltbSource::ManualOverride));

        let (secs, src) = effective_completion_time(&g, &ManualOverrides::default()).unwrap();
        assert_eq!((secs, src), (7200, HltbSource::HltbScrape));

        assert_eq!(main_story_seconds(&g), Some(7200));
        assert_eq!(main_story_seconds(&game(2)), None);
    }

    #[test]
    fn keywords_prefer_igdb_and_fall_back_to_rawg() {
        let g = full_game(1);
        let kws = keywords_lower(&g);
        assert_eq!(
            kws,
            vec!["fantasy", "roguelike", "strategy"],
            "IGDB only, lowercased+sorted"
        );

        let mut rawg_only = g.clone();
        rawg_only.igdb = None;
        let kws = keywords_lower(&rawg_only);
        assert_eq!(
            kws,
            vec!["arcade", "racing"],
            "RAWG fallback when IGDB absent"
        );
    }
}
