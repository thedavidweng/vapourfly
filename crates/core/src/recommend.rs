//! Recommendation engine.
//!
//! Scores games from a user's library based on multiple weighted signals and
//! returns the top-N recommendations with human-readable reason codes.
//! Designed to be deterministic when a seed is supplied.

use std::collections::HashMap;

use crate::models::{Game, ProtonTier, RecommendReason, RecommendRequest, Recommendation};
use crate::scoring;

const LOW_PLAYTIME_THRESHOLD_MINUTES: u32 = 120;
const RECENTLY_PLAYED_DAYS: i64 = 14;
const LIKELY_FINISHED_MULTIPLIER: f32 = 1.5;

// Score weights
const W_LOW_PLAYTIME: f32 = 2.0;
const W_DECK_NATIVE: f32 = 2.0;
const W_DECK_PLATINUM: f32 = 1.5;
const W_DECK_GOLD: f32 = 1.0;
const W_TIME_MATCH: f32 = 1.5;
const W_HIGH_RATING: f32 = 1.0;
const W_TASTE_SIMILARITY: f32 = 1.0;
const W_RECENTLY_PLAYED: f32 = -1.0;
const W_LIKELY_FINISHED: f32 = -0.5;

/// Minimal SplitMix64 PRNG for deterministic perturbation.
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Return a float in [0.0, 1.0).
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
    }
}

/// Score a single candidate game and produce reasons.
fn score_game(
    game: &Game,
    request: &RecommendRequest,
    taste_vector: &HashMap<String, f32>,
    now_unix: i64,
) -> (f32, Vec<RecommendReason>) {
    let mut score: f32 = 0.0;
    let mut reasons: Vec<RecommendReason> = Vec::new();

    let playtime = game.playtime_minutes.unwrap_or(0);
    if playtime < LOW_PLAYTIME_THRESHOLD_MINUTES {
        let desc = if playtime == 0 {
            "Never played".to_string()
        } else {
            format!("Low playtime ({playtime} min)")
        };
        score += W_LOW_PLAYTIME;
        reasons.push(RecommendReason {
            code: "low_playtime".into(),
            description: desc,
            weight: W_LOW_PLAYTIME,
        });
    }

    if request.deck_mode {
        let deck_score = game.protondb.as_ref().and_then(|pdb| match pdb.tier {
            ProtonTier::Native => Some((W_DECK_NATIVE, "Native Steam Deck support")),
            ProtonTier::Platinum => Some((W_DECK_PLATINUM, "Platinum on Steam Deck")),
            ProtonTier::Gold => Some((W_DECK_GOLD, "Gold on Steam Deck")),
            _ => None,
        });
        if let Some((w, desc)) = deck_score {
            score += w;
            reasons.push(RecommendReason {
                code: "deck_compatible".into(),
                description: desc.into(),
                weight: w,
            });
        }
    }

    if request.available_minutes > 0 {
        // A game is a time match when its known main-story completion time
        // fits inside the available window (PRD: "HLTB 主线 ≤ 可用时长").
        // HLTB main story is preferred; IGDB time-to-beat (normally) is the
        // fallback. Games with no known completion time get no time_match.
        let main_secs = crate::signal::main_story_seconds(game).or_else(|| {
            game.igdb
                .as_ref()
                .and_then(|i| i.time_to_beat.as_ref())
                .and_then(|t| t.normally_seconds)
        });
        if let Some(secs) = main_secs
            && secs / 60 <= request.available_minutes
        {
            score += W_TIME_MATCH;
            reasons.push(RecommendReason {
                code: "time_match".into(),
                description: format!("Fits in {} minutes", request.available_minutes),
                weight: W_TIME_MATCH,
            });
        }
    }

    if scoring::is_high_rating(game) {
        score += W_HIGH_RATING;
        reasons.push(RecommendReason {
            code: "high_rating".into(),
            description: "Highly rated".into(),
            weight: W_HIGH_RATING,
        });
    }

    if !taste_vector.is_empty() {
        let similarity = scoring::taste_overlap(game, taste_vector);
        if similarity > 0.05 {
            score += W_TASTE_SIMILARITY;
            reasons.push(RecommendReason {
                code: "taste_similarity".into(),
                description: "Matches your taste profile".into(),
                weight: W_TASTE_SIMILARITY,
            });
        }
    }

    if let Some(last_played) = game.last_played_unix {
        let days_since = (now_unix - last_played) / 86400;
        if days_since < RECENTLY_PLAYED_DAYS {
            let desc = match days_since {
                0 => "Played today".to_string(),
                1 => "Played 1 day ago".to_string(),
                n => format!("Played {n} days ago"),
            };
            score += W_RECENTLY_PLAYED;
            reasons.push(RecommendReason {
                code: "recently_played_penalty".into(),
                description: desc,
                weight: W_RECENTLY_PLAYED,
            });
        }
    }

    if let Some(main_secs) = game.hltb.as_ref().and_then(|h| h.main_story_seconds) {
        let main_mins = main_secs as f32 / 60.0;
        if main_mins > 0.0 && playtime as f32 > main_mins * LIKELY_FINISHED_MULTIPLIER {
            score += W_LIKELY_FINISHED;
            reasons.push(RecommendReason {
                code: "likely_finished_penalty".into(),
                description: "Likely finished based on playtime".into(),
                weight: W_LIKELY_FINISHED,
            });
        }
    }

    (score, reasons)
}

/// Recommend games from a library.
///
/// Filtering rules:
/// - Hidden games are always excluded.
/// - Junk games are always excluded.
/// - Non-game app types (Application, Tool, Dlc) are excluded by default.
/// - If `include_installed_only` is set, only installed games pass.
/// - Games in any of the `exclude_collections` are filtered out.
///
/// Scoring uses a weighted sum of several signals (see reason codes).
/// A random perturbation in [0.0, 0.25) is added to break ties; when `seed`
/// is provided the perturbation is deterministic.
pub fn recommend(games: &[Game], request: &RecommendRequest) -> Vec<Recommendation> {
    use crate::eligibility::{EligibilityOptions, is_eligible};

    let taste_vector = scoring::build_taste_vector(games);
    let now_unix = chrono::Utc::now().timestamp();
    let mut rng = request.seed.map(SplitMix64::new);

    let mut candidates: Vec<Recommendation> = games
        .iter()
        .filter(|g| {
            is_eligible(
                g,
                EligibilityOptions {
                    unplayed_only: false,
                    installed_only: request.include_installed_only,
                },
            ) && !g
                .steam_collections
                .iter()
                .any(|c| request.exclude_collections.contains(c))
        })
        .map(|game| {
            let (base_score, reasons) = score_game(game, request, &taste_vector, now_unix);
            let perturbation: f32 = match &mut rng {
                Some(r) => (r.next_f64() as f32) * 0.25,
                None => 0.0, // no seed => no perturbation (purely score-based)
            };

            Recommendation {
                app_id: game.app_id,
                name: game.name.clone(),
                score: base_score + perturbation,
                reasons,
            }
        })
        .collect();

    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.app_id.cmp(&b.app_id))
    });

    candidates.truncate(request.count);
    candidates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{HltbData, HltbSource, IgdbData, ProtonDbData, RawgData, SteamAppType};

    fn make_game(app_id: u32, name: &str) -> Game {
        Game {
            app_id,
            name: name.into(),
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

    fn default_request() -> RecommendRequest {
        RecommendRequest {
            available_minutes: 120,
            count: 5,
            deck_mode: false,
            include_installed_only: false,
            seed: Some(42),
            exclude_collections: vec![],
        }
    }

    #[test]
    fn hidden_games_excluded() {
        let mut game = make_game(1, "Hidden Game");
        game.is_hidden = true;
        let games = vec![game];
        let req = default_request();
        let result = recommend(&games, &req);
        assert!(result.is_empty(), "hidden games should be excluded");
    }

    #[test]
    fn junk_games_excluded() {
        let mut game = make_game(2, "Junk Game");
        game.is_junk = true;
        let games = vec![game];
        let req = default_request();
        let result = recommend(&games, &req);
        assert!(result.is_empty(), "junk games should be excluded");
    }

    #[test]
    fn deterministic_with_seed() {
        let games = vec![
            make_game(10, "Game A"),
            make_game(20, "Game B"),
            make_game(30, "Game C"),
        ];
        let req = default_request(); // seed = Some(42)
        let result1 = recommend(&games, &req);
        let result2 = recommend(&games, &req);
        assert_eq!(result1.len(), result2.len());
        for (a, b) in result1.iter().zip(result2.iter()) {
            assert_eq!(a.app_id, b.app_id, "order must be deterministic");
            assert!(
                (a.score - b.score).abs() < f32::EPSILON,
                "scores must be deterministic"
            );
        }
    }

    #[test]
    fn no_seed_no_perturbation() {
        let games = vec![make_game(10, "Game A")];
        let mut req = default_request();
        req.seed = None;
        let result = recommend(&games, &req);
        assert_eq!(result.len(), 1);
        // Score should be exactly the sum of applied weights with no perturbation.
        // low_playtime (2.0) only — no completion-time data, so no time_match.
        assert!(
            (result[0].score - 2.0).abs() < f32::EPSILON,
            "score without seed should have no perturbation, got {}",
            result[0].score
        );
    }

    #[test]
    fn low_playtime_higher_score() {
        let mut low = make_game(1, "Unplayed");
        low.playtime_minutes = Some(0);
        low.last_played_unix = None;

        let mut high = make_game(2, "Overplayed");
        high.playtime_minutes = Some(5000);
        high.last_played_unix = None;

        let games = vec![low, high];
        let mut req = default_request();
        req.seed = None; // no perturbation for clean comparison
        let result = recommend(&games, &req);

        assert_eq!(result.len(), 2);
        // The unplayed game should score higher (low_playtime = +2.0).
        assert!(
            result[0].score > result[1].score,
            "unplayed game should score higher: {} vs {}",
            result[0].score,
            result[1].score
        );
        assert!(result[0].reasons.iter().any(|r| r.code == "low_playtime"));
    }

    #[test]
    fn time_matching_works() {
        // Game with a 60-minute main story — fits in 120 minutes.
        let mut fits = make_game(1, "Short Game");
        fits.hltb = Some(HltbData {
            main_story_seconds: Some(3600), // 60 min
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });

        // Game with a 300-minute main story — does not fit in 120 minutes.
        let mut long = make_game(2, "Long Game");
        long.hltb = Some(HltbData {
            main_story_seconds: Some(18000), // 300 min
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });

        let games = vec![fits, long];
        let mut req = default_request();
        req.seed = None;
        let result = recommend(&games, &req);

        assert_eq!(result.len(), 2);
        let short_rec = result.iter().find(|r| r.app_id == 1).unwrap();
        let long_rec = result.iter().find(|r| r.app_id == 2).unwrap();

        assert!(
            short_rec.reasons.iter().any(|r| r.code == "time_match"),
            "short game should get time_match"
        );
        assert!(
            !long_rec.reasons.iter().any(|r| r.code == "time_match"),
            "long game should NOT get time_match"
        );
    }

    // time_match requires a known completion time (PRD: HLTB 主线 ≤ 可用时长).

    #[test]
    fn time_match_requires_known_completion_time() {
        // No HLTB and no IGDB time-to-beat: no time_match regardless of window.
        let unknown = make_game(1, "Unknown Length");

        // No HLTB, but IGDB time-to-beat fits: time_match via fallback.
        let mut igdb_fallback = make_game(2, "IGDB Timed");
        igdb_fallback.igdb = Some(IgdbData {
            igdb_id: 2,
            name: "IGDB Timed".into(),
            slug: None,
            rating_0_100: None,
            total_rating_0_100: None,
            genres: vec![],
            themes: vec![],
            keywords: vec![],
            similar_game_ids: vec![],
            steam_app_id_confirmed: false,
            time_to_beat: Some(crate::models::IgdbTimeToBeat {
                hastily_seconds: None,
                normally_seconds: Some(3600), // 60 min
                completely_seconds: None,
                submission_count: None,
            }),
        });

        let games = vec![unknown, igdb_fallback];
        let mut req = default_request(); // available_minutes = 120
        req.seed = None;
        let result = recommend(&games, &req);

        let unknown_rec = result.iter().find(|r| r.app_id == 1).unwrap();
        let fallback_rec = result.iter().find(|r| r.app_id == 2).unwrap();
        assert!(
            !unknown_rec.reasons.iter().any(|r| r.code == "time_match"),
            "game without completion-time data must not get time_match"
        );
        assert!(
            fallback_rec.reasons.iter().any(|r| r.code == "time_match"),
            "IGDB time-to-beat fallback should grant time_match"
        );
    }

    #[test]
    fn time_match_has_no_minimum_window_floor() {
        // A 5-minute game fits a 10-minute window; no hidden 15-minute floor.
        let mut tiny = make_game(1, "Tiny Game");
        tiny.hltb = Some(HltbData {
            main_story_seconds: Some(300), // 5 min
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });
        let games = vec![tiny];
        let mut req = default_request();
        req.available_minutes = 10;
        req.seed = None;
        let result = recommend(&games, &req);
        assert!(
            result[0].reasons.iter().any(|r| r.code == "time_match"),
            "a game fitting a sub-15-minute window should still get time_match"
        );
    }

    #[test]
    fn recently_played_reason_reports_actual_days() {
        let mut game = make_game(1, "Fresh Game");
        // Played 3 days ago.
        game.last_played_unix = Some(chrono::Utc::now().timestamp() - 3 * 86400);
        let games = vec![game];
        let mut req = default_request();
        req.seed = None;
        let result = recommend(&games, &req);
        let reason = result[0]
            .reasons
            .iter()
            .find(|r| r.code == "recently_played_penalty")
            .expect("recently played game should carry the penalty");
        assert_eq!(reason.description, "Played 3 days ago");
    }

    #[test]
    fn empty_library_returns_empty() {
        let games: Vec<Game> = vec![];
        let req = default_request();
        let result = recommend(&games, &req);
        assert!(result.is_empty());
    }

    // -- Additional tests ----------------------------------------------------

    #[test]
    fn unsupported_types_excluded() {
        let mut app = make_game(100, "Steamworks Common");
        app.app_type = SteamAppType::Application;
        let mut tool = make_game(200, "Valve Authoring");
        tool.app_type = SteamAppType::Tool;
        let games = vec![app, tool];
        let req = default_request();
        let result = recommend(&games, &req);
        assert!(
            result.is_empty(),
            "Application/Tool types should be excluded"
        );
    }

    #[test]
    fn installed_only_filter() {
        let mut installed = make_game(1, "Installed");
        installed.installed = true;
        let mut not_installed = make_game(2, "Not Installed");
        not_installed.installed = false;

        let games = vec![installed, not_installed];
        let mut req = default_request();
        req.include_installed_only = true;
        req.seed = None;
        let result = recommend(&games, &req);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].app_id, 1);
    }

    #[test]
    fn exclude_collections_filter() {
        let mut g1 = make_game(1, "In Collection");
        g1.steam_collections = vec!["ignored".into()];
        let mut g2 = make_game(2, "Not In Collection");
        g2.steam_collections = vec![];

        let games = vec![g1, g2];
        let mut req = default_request();
        req.exclude_collections = vec!["ignored".into()];
        req.seed = None;
        let result = recommend(&games, &req);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].app_id, 2);
    }

    #[test]
    fn deck_mode_scores_deck_compatible() {
        let mut native = make_game(1, "Native");
        native.protondb = Some(ProtonDbData {
            tier: ProtonTier::Native,
            confidence: Some("high".into()),
            score: None,
        });
        let mut borked = make_game(2, "Borked");
        borked.protondb = Some(ProtonDbData {
            tier: ProtonTier::Borked,
            confidence: Some("high".into()),
            score: None,
        });

        let games = vec![native, borked];
        let mut req = default_request();
        req.deck_mode = true;
        req.seed = None;
        let result = recommend(&games, &req);

        let native_rec = result.iter().find(|r| r.app_id == 1).unwrap();
        let borked_rec = result.iter().find(|r| r.app_id == 2).unwrap();

        assert!(
            native_rec
                .reasons
                .iter()
                .any(|r| r.code == "deck_compatible"),
            "native should get deck_compatible"
        );
        assert!(
            !borked_rec
                .reasons
                .iter()
                .any(|r| r.code == "deck_compatible"),
            "borked should not get deck_compatible"
        );
    }

    #[test]
    fn recently_played_penalty_applied() {
        // Set last_played to "now" (within 14 days).
        let now = chrono::Utc::now().timestamp();
        let mut recent = make_game(1, "Recently Played");
        recent.last_played_unix = Some(now - 3600); // 1 hour ago

        let mut old = make_game(2, "Old Game");
        old.last_played_unix = Some(now - 86400 * 30); // 30 days ago

        let games = vec![recent, old];
        let mut req = default_request();
        req.seed = None;
        let result = recommend(&games, &req);

        let recent_rec = result.iter().find(|r| r.app_id == 1).unwrap();
        assert!(
            recent_rec
                .reasons
                .iter()
                .any(|r| r.code == "recently_played_penalty"),
            "recently played game should get penalty"
        );
    }

    #[test]
    fn likely_finished_penalty_applied() {
        // Played 2x the main story length.
        let mut finished = make_game(1, "Finished");
        finished.playtime_minutes = Some(600);
        finished.hltb = Some(HltbData {
            main_story_seconds: Some(18000), // 300 min, 600 > 300 * 1.5
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });

        let games = vec![finished];
        let mut req = default_request();
        req.seed = None;
        let result = recommend(&games, &req);

        assert!(
            result[0]
                .reasons
                .iter()
                .any(|r| r.code == "likely_finished_penalty"),
            "likely finished game should get penalty"
        );
    }

    #[test]
    fn high_rating_detected() {
        let mut rawg_game = make_game(1, "RAWG High");
        rawg_game.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: Some(4.5),
            ratings_count: Some(1000),
            genres: vec![],
            tags: vec![],
            stores: vec![],
        });

        let mut igdb_game = make_game(2, "IGDB High");
        igdb_game.igdb = Some(IgdbData {
            igdb_id: 1,
            name: "IGDB High".into(),
            slug: None,
            rating_0_100: Some(85.0),
            total_rating_0_100: None,
            genres: vec![],
            themes: vec![],
            keywords: vec![],
            similar_game_ids: vec![],
            steam_app_id_confirmed: false,
            time_to_beat: None,
        });

        let games = vec![rawg_game, igdb_game];
        let mut req = default_request();
        req.seed = None;
        let result = recommend(&games, &req);

        for rec in &result {
            assert!(
                rec.reasons.iter().any(|r| r.code == "high_rating"),
                "{} should get high_rating",
                rec.name
            );
        }
    }

    // high_rating is an independent OR: a low RAWG rating must not mask a
    // high IGDB rating (PRD: RAWG ≥4.0 或 IGDB ≥80).

    #[test]
    fn high_rating_or_semantics_low_rawg_high_igdb() {
        let mut game = make_game(1, "Divisive Gem");
        game.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: Some(3.0), // below the RAWG threshold
            ratings_count: Some(1000),
            genres: vec![],
            tags: vec![],
            stores: vec![],
        });
        game.igdb = Some(IgdbData {
            igdb_id: 1,
            name: "Divisive Gem".into(),
            slug: None,
            rating_0_100: Some(85.0), // above the IGDB threshold
            total_rating_0_100: None,
            genres: vec![],
            themes: vec![],
            keywords: vec![],
            similar_game_ids: vec![],
            steam_app_id_confirmed: false,
            time_to_beat: None,
        });

        let games = vec![game];
        let mut req = default_request();
        req.seed = None;
        let result = recommend(&games, &req);
        assert!(
            result[0].reasons.iter().any(|r| r.code == "high_rating"),
            "IGDB ≥ 80 must grant high_rating even when RAWG < 4.0"
        );
    }

    #[test]
    fn taste_vector_prefers_igdb() {
        let mut game = make_game(1, "Taste Game");
        game.playtime_minutes = Some(500);
        game.igdb = Some(IgdbData {
            igdb_id: 1,
            name: "Taste Game".into(),
            slug: None,
            rating_0_100: None,
            total_rating_0_100: None,
            genres: vec!["Role-playing (RPG)".into()],
            themes: vec!["Fantasy".into()],
            keywords: vec!["open world".into()],
            similar_game_ids: vec![],
            steam_app_id_confirmed: false,
            time_to_beat: None,
        });
        game.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: None,
            ratings_count: None,
            genres: vec!["Action".into()],
            tags: vec!["fps".into()],
            stores: vec![],
        });

        let games = vec![game];
        let vector = scoring::build_taste_vector(&games);

        // IGDB keywords should be present
        assert!(vector.contains_key("open world"));
        assert!(vector.contains_key("fantasy"));
        assert!(vector.contains_key("role-playing (rpg)"));
        // RAWG should NOT be used since IGDB was available
        assert!(!vector.contains_key("action"));
        assert!(!vector.contains_key("fps"));
    }

    #[test]
    fn taste_vector_falls_back_to_rawg() {
        let mut game = make_game(1, "RAWG Only");
        game.playtime_minutes = Some(500);
        game.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: None,
            ratings_count: None,
            genres: vec!["Strategy".into()],
            tags: vec!["turn-based".into()],
            stores: vec![],
        });

        let games = vec![game];
        let vector = scoring::build_taste_vector(&games);

        assert!(vector.contains_key("strategy"));
        assert!(vector.contains_key("turn-based"));
    }

    #[test]
    fn build_taste_vector_excludes_hidden_and_junk() {
        let mut hidden = make_game(1, "Hidden");
        hidden.is_hidden = true;
        hidden.playtime_minutes = Some(500);

        let mut junk = make_game(2, "Junk");
        junk.is_junk = true;
        junk.playtime_minutes = Some(500);

        let games = vec![hidden, junk];
        let vector = scoring::build_taste_vector(&games);
        assert!(vector.is_empty());
    }

    #[test]
    fn build_taste_vector_excludes_low_playtime() {
        let mut game = make_game(1, "No Playtime");
        game.playtime_minutes = Some(10); // below 60 min threshold
        game.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: None,
            ratings_count: None,
            genres: vec!["Action".into()],
            tags: vec![],
            stores: vec![],
        });

        let games = vec![game];
        let vector = scoring::build_taste_vector(&games);
        assert!(vector.is_empty());
    }

    #[test]
    fn different_seeds_give_different_scores() {
        // Create a game with identical base scores. With different seeds
        // the perturbation values should differ, giving different final scores.
        let games: Vec<Game> = vec![make_game(100, "Game A")];

        let mut req1 = default_request();
        req1.seed = Some(1);
        let result1 = recommend(&games, &req1);

        let mut req2 = default_request();
        req2.seed = Some(999);
        let result2 = recommend(&games, &req2);

        assert_eq!(result1.len(), 1);
        assert_eq!(result2.len(), 1);
        assert_ne!(
            result1[0].score, result2[0].score,
            "different seeds should produce different scores"
        );
    }

    #[test]
    fn count_limits_results() {
        let games: Vec<Game> = (0..10)
            .map(|id| make_game(id, &format!("Game {id}")))
            .collect();
        let mut req = default_request();
        req.count = 3;
        let result = recommend(&games, &req);
        assert_eq!(result.len(), 3);
    }
}
