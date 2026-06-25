//! Junk detection rules engine.
//!
//! Evaluates each game against a set of configurable thresholds and decides
//! whether it qualifies as "junk" — a game the user is unlikely to ever play.
//! Supports three evaluation modes (Default, Strict, Aggressive) and manual
//! overrides for force-include/exclude and user-supplied metadata.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Result, SafePath, VapourflyError};
use crate::models::{
    Game, HltbSource, JunkDecision, JunkMode, JunkRules, JunkSignal, JunkSignalKind, RatingSource,
};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Manual overrides that let the user force specific games in or out of the
/// junk set, or supply their own HLTB / rating data.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ManualOverrides {
    /// App IDs that should **never** be marked junk, regardless of signals.
    pub force_include: HashSet<u32>,
    /// App IDs that should **always** be marked junk, regardless of signals.
    pub force_exclude: HashSet<u32>,
    /// User-supplied completion time in seconds (replaces HLTB lookup).
    pub manual_hltb: HashMap<u32, u32>,
    /// User-supplied rating on a 0-5 scale (replaces RAWG/IGDB lookup).
    pub manual_rating: HashMap<u32, f32>,
}

/// The top-level result of a junk preview evaluation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JunkPreviewResult {
    pub schema: String,
    pub decisions: Vec<JunkDecision>,
    pub rules: JunkRules,
    pub mode: JunkMode,
}

// ---------------------------------------------------------------------------
// Loading overrides from disk
// ---------------------------------------------------------------------------

/// Load a [`ManualOverrides`] file from disk.  The file must be valid JSON
/// matching the struct layout.
pub fn load_manual_overrides(path: &Path) -> Result<ManualOverrides> {
    let content = fs::read_to_string(path).map_err(|_| VapourflyError::FileNotFound {
        path: SafePath::new(path),
    })?;
    serde_json::from_str(&content).map_err(|e| VapourflyError::ParseError {
        path: SafePath::new(path),
        format: "JSON".into(),
        reason: e.to_string(),
    })
}

// ---------------------------------------------------------------------------
// Signal extraction helpers
// ---------------------------------------------------------------------------

/// Return the effective playtime in minutes for a game.
fn effective_playtime(game: &Game) -> Option<u32> {
    game.playtime_minutes
}

/// Return the effective completion time in seconds and its source.
///
/// Manual overrides take precedence over scraped HLTB data.
fn effective_completion_time(
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

/// Return the effective rating on a 0-5 scale and its source.
///
/// Priority: manual override > RAWG (native 0-5) > IGDB (0-100, converted).
fn effective_rating(game: &Game, overrides: &ManualOverrides) -> Option<(f32, RatingSource)> {
    if let Some(&rating) = overrides.manual_rating.get(&game.app_id) {
        return Some((rating, RatingSource::ManualOverride));
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
// Single-game evaluation
// ---------------------------------------------------------------------------

/// Evaluate a single game against the junk rules and return a decision.
///
/// This does **not** apply force-include/exclude overrides — that is done by
/// the caller so that the matched/missing signals still reflect the raw data.
fn evaluate_game(
    game: &Game,
    rules: &JunkRules,
    mode: &JunkMode,
    overrides: &ManualOverrides,
) -> JunkDecision {
    let mut matched: Vec<JunkSignal> = Vec::new();
    let mut missing: Vec<JunkSignalKind> = Vec::new();
    let mut available: usize = 0;

    // -- Playtime (required signal) ------------------------------------------
    match effective_playtime(game) {
        Some(minutes) => {
            available += 1;
            if minutes < rules.max_playtime_minutes {
                matched.push(JunkSignal::LowPlaytime { minutes });
            }
        }
        None => missing.push(JunkSignalKind::Playtime),
    }

    // -- Completion time (HLTB) ---------------------------------------------
    match effective_completion_time(game, overrides) {
        Some((seconds, source)) => {
            available += 1;
            if seconds < rules.max_main_story_seconds {
                matched.push(JunkSignal::ShortCompletion { seconds, source });
            }
        }
        None => missing.push(JunkSignalKind::CompletionTime),
    }

    // -- Rating -------------------------------------------------------------
    match effective_rating(game, overrides) {
        Some((rating_0_5, source)) => {
            available += 1;
            if rating_0_5 < rules.max_rating_0_5 {
                matched.push(JunkSignal::LowRating { rating_0_5, source });
            }
        }
        None => missing.push(JunkSignalKind::Rating),
    }

    // -- Confidence: fraction of possible signals that are available ----------
    let confidence = available as f32 / 3.0;

    // -- Classify matched signals --------------------------------------------
    let playtime_matched = matched
        .iter()
        .any(|s| matches!(s, JunkSignal::LowPlaytime { .. }));

    let other_matched_count = matched
        .iter()
        .filter(|s| !matches!(s, JunkSignal::LowPlaytime { .. }))
        .count();

    // -- Apply mode logic ----------------------------------------------------
    let is_junk = match mode {
        JunkMode::Default => {
            // Playtime must be low AND at least one other signal must be low,
            // AND we need at least `min_available_signals` data points.
            playtime_matched && other_matched_count >= 1 && available >= rules.min_available_signals
        }
        JunkMode::Strict => {
            // Every available signal must indicate junk.  Additionally require
            // at least `min_available_signals` so that a single low-playtime
            // result alone is not enough.
            playtime_matched
                && matched.len() == available
                && available >= rules.min_available_signals
        }
        JunkMode::Aggressive => {
            // Playtime must be low and at least one other available signal
            // must be low.  No minimum signal count.
            playtime_matched && other_matched_count >= 1
        }
    };

    JunkDecision {
        app_id: game.app_id,
        name: game.name.clone(),
        is_junk,
        confidence,
        matched,
        missing,
        mode: mode.clone(),
    }
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Evaluate every game against the junk rules and return a decision per game.
///
/// Manual overrides are applied **after** signal evaluation:
/// - `force_include` unconditionally sets `is_junk = false`.
/// - `force_exclude` unconditionally sets `is_junk = true`.
///
/// The `matched` / `missing` fields still reflect the raw signal data so that
/// the UI can explain why a game was classified the way it was.
pub fn evaluate_junk(
    games: &[Game],
    rules: &JunkRules,
    mode: &JunkMode,
    overrides: &ManualOverrides,
) -> Vec<JunkDecision> {
    games
        .iter()
        .map(|game| {
            let mut decision = evaluate_game(game, rules, mode, overrides);

            // Force-include: never junk, regardless of signals.
            if overrides.force_include.contains(&game.app_id) {
                decision.is_junk = false;
            }
            // Force-exclude: always junk, regardless of signals.
            if overrides.force_exclude.contains(&game.app_id) {
                decision.is_junk = true;
            }

            decision
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Game, HltbData, RawgData, SteamAppType, VAPOURFLY_JUNK_PREVIEW_SCHEMA};

    /// Helper: build a minimal Game with the given parameters.
    fn make_game(
        app_id: u32,
        name: &str,
        playtime_minutes: Option<u32>,
        hltb_seconds: Option<u32>,
        rawg_rating: Option<f32>,
    ) -> Game {
        Game {
            app_id,
            name: name.into(),
            app_type: SteamAppType::Game,
            installed: false,
            install_dir: None,
            library_folder: None,
            playtime_minutes,
            playtime_2wks_minutes: None,
            playtime_disconnected_minutes: None,
            last_played_unix: None,
            steam_collections: vec![],
            is_hidden: false,
            is_junk: false,
            hltb: hltb_seconds.map(|s| HltbData {
                main_story_seconds: Some(s),
                main_extra_seconds: None,
                completionist_seconds: None,
                source: HltbSource::HltbScrape,
            }),
            igdb: None,
            rawg: rawg_rating.map(|r| RawgData {
                rawg_id: 0,
                rating_0_5: Some(r),
                ratings_count: Some(100),
                genres: vec![],
                tags: vec![],
                stores: vec![],
            }),
            protondb: None,
            pcgw: None,
        }
    }

    fn default_rules() -> JunkRules {
        JunkRules::default()
    }

    fn default_overrides() -> ManualOverrides {
        ManualOverrides::default()
    }

    // -- Test 1: Low playtime + low rating = Junk (Default mode) -------------

    #[test]
    fn low_playtime_plus_low_rating_is_junk() {
        let games = vec![make_game(1, "Bad Game", Some(5), None, Some(1.0))];
        let decisions = evaluate_junk(
            &games,
            &default_rules(),
            &JunkMode::Default,
            &default_overrides(),
        );
        assert_eq!(decisions.len(), 1);
        assert!(decisions[0].is_junk, "should be junk");
        assert_eq!(decisions[0].matched.len(), 2);
        assert!(
            decisions[0]
                .matched
                .iter()
                .any(|s| matches!(s, JunkSignal::LowPlaytime { .. }))
        );
        assert!(
            decisions[0]
                .matched
                .iter()
                .any(|s| matches!(s, JunkSignal::LowRating { .. }))
        );
    }

    // -- Test 2: Low playtime + missing rating + missing time = not junk -----
    //    (insufficient signals in Default mode)

    #[test]
    fn low_playtime_only_is_not_junk_default() {
        let games = vec![make_game(2, "Mystery", Some(5), None, None)];
        let decisions = evaluate_junk(
            &games,
            &default_rules(),
            &JunkMode::Default,
            &default_overrides(),
        );
        assert_eq!(decisions.len(), 1);
        assert!(!decisions[0].is_junk, "should NOT be junk — only 1 signal");
        assert_eq!(decisions[0].missing.len(), 2);
    }

    // -- Test 3: Strict mode requires all available signals to match ---------

    #[test]
    fn strict_mode_requires_all_signals() {
        // Game has low playtime, low rating, but HIGH completion time.
        let games = vec![make_game(3, "Long RPG", Some(10), Some(50_000), Some(1.0))];
        let decisions = evaluate_junk(
            &games,
            &default_rules(),
            &JunkMode::Strict,
            &default_overrides(),
        );
        assert_eq!(decisions.len(), 1);
        assert!(
            !decisions[0].is_junk,
            "should NOT be junk in strict mode — completion time is high"
        );
    }

    #[test]
    fn strict_mode_junk_when_all_match() {
        let games = vec![make_game(4, "Short Trash", Some(5), Some(1000), Some(0.5))];
        let decisions = evaluate_junk(
            &games,
            &default_rules(),
            &JunkMode::Strict,
            &default_overrides(),
        );
        assert_eq!(decisions.len(), 1);
        assert!(decisions[0].is_junk, "all three signals are low");
        assert_eq!(decisions[0].matched.len(), 3);
    }

    // -- Test 4: Aggressive mode includes with one negative signal -----------

    #[test]
    fn aggressive_mode_junk_with_one_extra_signal() {
        // Low playtime + low rating, but high completion time.
        let games = vec![make_game(5, "Mediocre", Some(10), Some(50_000), Some(1.0))];
        let decisions = evaluate_junk(
            &games,
            &default_rules(),
            &JunkMode::Aggressive,
            &default_overrides(),
        );
        assert_eq!(decisions.len(), 1);
        assert!(
            decisions[0].is_junk,
            "aggressive: playtime + one other signal is enough"
        );
    }

    // -- Test 5: Manual force-include / force-exclude -----------------------

    #[test]
    fn force_include_overrides_junk() {
        let games = vec![make_game(6, "Favorite", Some(2), Some(500), Some(0.5))];
        let mut overrides = default_overrides();
        overrides.force_include.insert(6);
        let decisions = evaluate_junk(&games, &default_rules(), &JunkMode::Strict, &overrides);
        assert_eq!(decisions.len(), 1);
        assert!(!decisions[0].is_junk, "force_include overrides everything");
    }

    #[test]
    fn force_exclude_overrides_non_junk() {
        let games = vec![make_game(
            7,
            "Actually Good",
            Some(500),
            Some(50_000),
            Some(4.5),
        )];
        let mut overrides = default_overrides();
        overrides.force_exclude.insert(7);
        let decisions = evaluate_junk(&games, &default_rules(), &JunkMode::Default, &overrides);
        assert_eq!(decisions.len(), 1);
        assert!(decisions[0].is_junk, "force_exclude overrides everything");
    }

    // -- Test 6: Zero candidates = empty result -----------------------------

    #[test]
    fn zero_candidates_is_empty() {
        let games: Vec<Game> = vec![];
        let decisions = evaluate_junk(
            &games,
            &default_rules(),
            &JunkMode::Default,
            &default_overrides(),
        );
        assert!(decisions.is_empty());
    }

    // -- Additional edge-case tests -----------------------------------------

    #[test]
    fn confidence_reflects_available_signals() {
        // All 3 signals available => confidence 1.0
        let games = vec![make_game(10, "Full Data", Some(5), Some(1000), Some(1.0))];
        let decisions = evaluate_junk(
            &games,
            &default_rules(),
            &JunkMode::Default,
            &default_overrides(),
        );
        assert!((decisions[0].confidence - 1.0).abs() < f32::EPSILON);

        // Only playtime available => confidence ~0.33
        let games = vec![make_game(11, "Sparse", Some(5), None, None)];
        let decisions = evaluate_junk(
            &games,
            &default_rules(),
            &JunkMode::Default,
            &default_overrides(),
        );
        assert!((decisions[0].confidence - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn high_playtime_game_is_not_junk() {
        let games = vec![make_game(12, "Main Game", Some(500), Some(500), Some(1.0))];
        let decisions = evaluate_junk(
            &games,
            &default_rules(),
            &JunkMode::Aggressive,
            &default_overrides(),
        );
        assert!(!decisions[0].is_junk, "high playtime means not junk");
    }

    #[test]
    fn aggressive_needs_at_least_two_signals() {
        // Only playtime available — no other signal to pair with.
        let games = vec![make_game(13, "Only Playtime", Some(5), None, None)];
        let decisions = evaluate_junk(
            &games,
            &default_rules(),
            &JunkMode::Aggressive,
            &default_overrides(),
        );
        assert!(
            !decisions[0].is_junk,
            "aggressive still needs playtime + at least one other"
        );
    }

    #[test]
    fn manual_overrides_supply_hltb_data() {
        let games = vec![make_game(14, "User Timed", Some(5), None, Some(1.0))];
        let mut overrides = default_overrides();
        overrides.manual_hltb.insert(14, 500);
        let decisions = evaluate_junk(&games, &default_rules(), &JunkMode::Strict, &overrides);
        assert_eq!(decisions[0].missing.len(), 0, "manual HLTB fills the gap");
        assert!(decisions[0].is_junk);
    }

    #[test]
    fn manual_overrides_supply_rating_data() {
        let games = vec![make_game(15, "User Rated", Some(5), Some(1000), None)];
        let mut overrides = default_overrides();
        overrides.manual_rating.insert(15, 0.5);
        let decisions = evaluate_junk(&games, &default_rules(), &JunkMode::Strict, &overrides);
        assert_eq!(decisions[0].missing.len(), 0, "manual rating fills the gap");
        assert!(decisions[0].is_junk);
    }

    #[test]
    fn load_manual_overrides_from_json() {
        let dir = std::env::temp_dir().join("vapourfly_junk_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("overrides.json");
        let json = r#"{
            "force_include": [1, 2],
            "force_exclude": [3],
            "manual_hltb": { "4": 3600 },
            "manual_rating": { "5": 3.5 }
        }"#;
        fs::write(&path, json).unwrap();

        let overrides = load_manual_overrides(&path).unwrap();
        assert!(overrides.force_include.contains(&1));
        assert!(overrides.force_include.contains(&2));
        assert!(overrides.force_exclude.contains(&3));
        assert_eq!(overrides.manual_hltb.get(&4), Some(&3600));
        assert!((overrides.manual_rating.get(&5).unwrap() - 3.5).abs() < f32::EPSILON);

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn junk_preview_result_schema() {
        let result = JunkPreviewResult {
            schema: VAPOURFLY_JUNK_PREVIEW_SCHEMA.into(),
            decisions: vec![],
            rules: default_rules(),
            mode: JunkMode::Default,
        };
        assert_eq!(result.schema, "vapourfly.junk_preview.v1");
    }
}
