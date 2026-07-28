//! Scoring primitives — taste vector, taste overlap, and high-rating check.
//!
//! Deep module: owns the shared scoring primitives that [`recommend`] and
//! [`discover`] both depend on. Each caller applies its own weights and
//! thresholds; the signal extraction and normalization live here.
//!
//! ## Primitives
//!
//! - [`build_taste_vector`]: maps keywords to log-scaled playtime weights.
//! - [`taste_overlap`]: normalized overlap between a game's keywords and a
//!   taste vector (0.0–1.0).
//! - [`is_high_rating`]: true when RAWG rates ≥ 4.0 or IGDB rates ≥ 80,
//!   evaluated as an independent OR over the two sources.

use std::collections::HashMap;

use crate::models::Game;
use crate::signal;

/// Build a taste vector from the user's library.
///
/// Each entry maps a keyword (genre, theme, or tag) to a weight derived from
/// log-scaled lifetime playtime.  Only non-hidden, non-junk games with
/// meaningful playtime (>= 1 hour) contribute.
///
/// Prefer IGDB genres/themes/keywords; fall back to RAWG genres/tags.
pub fn build_taste_vector(games: &[Game]) -> HashMap<String, f32> {
    let mut vector: HashMap<String, f32> = HashMap::new();

    for game in games {
        if game.is_hidden || game.is_junk {
            continue;
        }

        let playtime = match game.playtime_minutes {
            Some(m) if m >= 60 => m as f32,
            _ => continue, // skip games with no meaningful playtime
        };

        let weight = (1.0 + playtime).ln();

        let keywords = signal::keywords_lower(game);
        for kw in keywords {
            *vector.entry(kw).or_insert(0.0) += weight;
        }
    }

    vector
}

/// Compute the normalized taste overlap between a game and a taste vector.
///
/// Returns a value in [0.0, 1.0] representing the fraction of the taste
/// vector's total weight that the game's keywords cover. Returns 0.0 when
/// the taste vector is empty or the game has no matching keywords.
pub fn taste_overlap(game: &Game, taste_vector: &HashMap<String, f32>) -> f32 {
    if taste_vector.is_empty() {
        return 0.0;
    }
    let total_taste: f32 = taste_vector.values().sum();
    if total_taste <= 0.0 {
        return 0.0;
    }
    let keywords = signal::keywords_lower(game);
    if keywords.is_empty() {
        return 0.0;
    }
    let overlap: f32 = keywords.iter().filter_map(|kw| taste_vector.get(kw)).sum();
    overlap / total_taste
}

/// Check whether a game is highly rated: RAWG ≥ 4.0 **or** IGDB ≥ 80.
///
/// The two sources are evaluated independently (PRD: "RAWG ≥4.0 或 IGDB ≥80"),
/// so a game whose RAWG rating is mediocre still qualifies when IGDB rates it
/// highly. This differs from [`signal::effective_rating`], which resolves a
/// single rating by source precedence.
pub fn is_high_rating(game: &Game) -> bool {
    let rawg_high = game
        .rawg
        .as_ref()
        .and_then(|r| r.rating_0_5)
        .is_some_and(|r| r >= 4.0);
    let igdb_high = game
        .igdb
        .as_ref()
        .and_then(|i| i.rating_0_100.or(i.total_rating_0_100))
        .is_some_and(|r| r >= 80.0);
    rawg_high || igdb_high
}

/// A "short game" fits a single short session: HLTB main story ≤ 4 hours.
pub const SHORT_GAME_MAX_SECONDS: u32 = 4 * 3600;

/// A "long game" earns a marathon: HLTB main story ≥ 20 hours.
pub const LONG_GAME_MIN_SECONDS: u32 = 20 * 3600;

/// True when the game's main story fits a short session (≤ 4 hours).
/// Fails closed when HLTB data is missing.
pub fn is_short_game(game: &Game) -> bool {
    signal::main_story_seconds(game).is_some_and(|s| s <= SHORT_GAME_MAX_SECONDS)
}

/// True when the game's main story is marathon-length (≥ 20 hours).
/// Fails closed when HLTB data is missing.
pub fn is_long_game(game: &Game) -> bool {
    signal::main_story_seconds(game).is_some_and(|s| s >= LONG_GAME_MIN_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{HltbData, HltbSource, IgdbData, RawgData, SteamAppType};

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

    fn igdb_with(genres: &[&str], rating: Option<f32>) -> IgdbData {
        IgdbData {
            igdb_id: 1,
            name: "g".into(),
            slug: None,
            rating_0_100: rating,
            total_rating_0_100: None,
            genres: genres.iter().map(|s| s.to_string()).collect(),
            themes: vec![],
            keywords: vec![],
            similar_game_ids: vec![],
            steam_app_id_confirmed: false,
            time_to_beat: None,
        }
    }

    fn rawg_with(rating: Option<f32>) -> RawgData {
        RawgData {
            rawg_id: 1,
            rating_0_5: rating,
            ratings_count: None,
            genres: vec![],
            tags: vec![],
            stores: vec![],
        }
    }

    fn hltb_secs(secs: u32) -> HltbData {
        HltbData {
            main_story_seconds: Some(secs),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        }
    }

    #[test]
    fn taste_vector_skips_hidden_junk_and_low_playtime() {
        let mut liked = game(1);
        liked.playtime_minutes = Some(600);
        liked.igdb = Some(igdb_with(&["Strategy"], None));

        let mut hidden = game(2);
        hidden.playtime_minutes = Some(600);
        hidden.is_hidden = true;
        hidden.igdb = Some(igdb_with(&["Racing"], None));

        let mut barely_played = game(3);
        barely_played.playtime_minutes = Some(30);
        barely_played.igdb = Some(igdb_with(&["Puzzle"], None));

        let vector = build_taste_vector(&[liked, hidden, barely_played]);
        assert!(vector.contains_key("strategy"));
        assert!(
            !vector.contains_key("racing"),
            "hidden games contribute nothing"
        );
        assert!(
            !vector.contains_key("puzzle"),
            "sub-hour playtime contributes nothing"
        );
    }

    #[test]
    fn taste_overlap_is_normalized_fraction() {
        let mut liked = game(1);
        liked.playtime_minutes = Some(600);
        liked.igdb = Some(igdb_with(&["Strategy", "Roguelike"], None));
        let vector = build_taste_vector(std::slice::from_ref(&liked));

        let mut half_match = game(2);
        half_match.igdb = Some(igdb_with(&["Strategy"], None));
        let overlap = taste_overlap(&half_match, &vector);
        assert!(
            (overlap - 0.5).abs() < 1e-6,
            "one of two equal-weight keywords = 0.5"
        );

        let no_keywords = game(3);
        assert_eq!(taste_overlap(&no_keywords, &vector), 0.0);
        assert_eq!(taste_overlap(&half_match, &HashMap::new()), 0.0);
    }

    #[test]
    fn high_rating_is_independent_or_over_sources() {
        let mut igdb_only = game(1);
        igdb_only.igdb = Some(igdb_with(&[], Some(85.0)));
        igdb_only.rawg = Some(rawg_with(Some(2.0))); // mediocre RAWG must not veto
        assert!(is_high_rating(&igdb_only));

        let mut rawg_only = game(2);
        rawg_only.rawg = Some(rawg_with(Some(4.5)));
        assert!(is_high_rating(&rawg_only));

        let mut low = game(3);
        low.rawg = Some(rawg_with(Some(3.9)));
        low.igdb = Some(igdb_with(&[], Some(79.0)));
        assert!(!is_high_rating(&low));

        assert!(!is_high_rating(&game(4)), "no data fails closed");
    }

    #[test]
    fn session_length_predicates_fail_closed_at_boundaries() {
        let mut short = game(1);
        short.hltb = Some(hltb_secs(SHORT_GAME_MAX_SECONDS));
        assert!(is_short_game(&short));
        assert!(!is_long_game(&short));

        let mut long = game(2);
        long.hltb = Some(hltb_secs(LONG_GAME_MIN_SECONDS));
        assert!(is_long_game(&long));
        assert!(!is_short_game(&long));

        let no_data = game(3);
        assert!(!is_short_game(&no_data));
        assert!(!is_long_game(&no_data));
    }
}
