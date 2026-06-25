//! Recommendation engine.
//!
//! Scores games from a user's library based on multiple weighted signals and
//! returns the top-N recommendations with human-readable reason codes.
//! Designed to be deterministic when a seed is supplied.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::models::{
    Game, ProtonTier, RecommendReason, RecommendRequest, Recommendation, SteamAppType,
};

// ---------------------------------------------------------------------------
// Public result type
// ---------------------------------------------------------------------------

/// The full result of a recommendation evaluation, including schema version
/// for forward compatibility.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecommendResult {
    pub schema: String,
    pub recommendations: Vec<Recommendation>,
    pub request: RecommendRequest,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const LOW_PLAYTIME_THRESHOLD_MINUTES: u32 = 120;
#[allow(dead_code)]
const HIGH_PLAYTIME_FOR_TASTE_MINUTES: u32 = 300;
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

// ---------------------------------------------------------------------------
// Deterministic PRNG (SplitMix64)
// ---------------------------------------------------------------------------

/// Minimal SplitMix64 PRNG for deterministic perturbation.
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }

    /// Return a float in [0.0, 1.0).
    fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / ((1u64 << 53) as f64)
    }
}

// ---------------------------------------------------------------------------
// Taste vector
// ---------------------------------------------------------------------------

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
        // Skip hidden and junk
        if game.is_hidden || game.is_junk {
            continue;
        }

        let playtime = match game.playtime_minutes {
            Some(m) if m >= 60 => m as f32,
            _ => continue, // skip games with no meaningful playtime
        };

        let weight = (1.0 + playtime).ln();

        // Prefer IGDB keywords, then themes, then genres
        let mut keywords: Vec<String> = Vec::new();
        if let Some(igdb) = &game.igdb {
            keywords.extend(igdb.keywords.iter().cloned());
            keywords.extend(igdb.themes.iter().cloned());
            keywords.extend(igdb.genres.iter().cloned());
        }

        // Fall back to RAWG if IGDB is absent
        if keywords.is_empty() {
            if let Some(rawg) = &game.rawg {
                keywords.extend(rawg.tags.iter().cloned());
                keywords.extend(rawg.genres.iter().cloned());
            }
        }

        // Deduplicate per game so a game with genre+tag overlap doesn't
        // double-count a keyword.
        let mut seen = std::collections::HashSet::new();
        for kw in keywords {
            let lower = kw.to_lowercase();
            if seen.insert(lower.clone()) {
                *vector.entry(lower).or_insert(0.0) += weight;
            }
        }
    }

    vector
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

/// Score a single candidate game and produce reasons.
fn score_game(
    game: &Game,
    request: &RecommendRequest,
    taste_vector: &HashMap<String, f32>,
    now_unix: i64,
) -> (f32, Vec<RecommendReason>) {
    let mut score: f32 = 0.0;
    let mut reasons: Vec<RecommendReason> = Vec::new();

    // -- low_playtime --------------------------------------------------------
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

    // -- deck_compatible (only when --deck) -----------------------------------
    if request.deck_mode {
        let deck_score = match &game.protondb {
            Some(pdb) => match pdb.tier {
                ProtonTier::Native => Some((W_DECK_NATIVE, "Native Steam Deck support")),
                ProtonTier::Platinum => Some((W_DECK_PLATINUM, "Platinum on Steam Deck")),
                ProtonTier::Gold => Some((W_DECK_GOLD, "Gold on Steam Deck")),
                _ => None,
            },
            None => None,
        };
        if let Some((w, desc)) = deck_score {
            score += w;
            reasons.push(RecommendReason {
                code: "deck_compatible".into(),
                description: desc.into(),
                weight: w,
            });
        }
    }

    // -- time_match -----------------------------------------------------------
    if request.available_minutes > 0 {
        // We consider a game a good time match if the available window could
        // meaningfully cover at least a short session (15+ min) and the HLTB
        // main story is within the available time (or no HLTB data at all).
        let fits = match &game.hltb {
            Some(hltb) => match hltb.main_story_seconds {
                Some(main_secs) => {
                    let main_mins = main_secs / 60;
                    main_mins <= request.available_minutes && request.available_minutes >= 15
                }
                None => request.available_minutes >= 15,
            },
            None => request.available_minutes >= 15,
        };
        if fits {
            score += W_TIME_MATCH;
            reasons.push(RecommendReason {
                code: "time_match".into(),
                description: format!("Fits in {} minutes", request.available_minutes),
                weight: W_TIME_MATCH,
            });
        }
    }

    // -- high_rating ----------------------------------------------------------
    let is_high_rating = if let Some(rawg) = &game.rawg {
        rawg.rating_0_5.is_some_and(|r| r >= 4.0)
    } else if let Some(igdb) = &game.igdb {
        igdb.rating_0_100.is_some_and(|r| r >= 80.0)
            || igdb.total_rating_0_100.is_some_and(|r| r >= 80.0)
    } else {
        false
    };
    if is_high_rating {
        score += W_HIGH_RATING;
        reasons.push(RecommendReason {
            code: "high_rating".into(),
            description: "Highly rated".into(),
            weight: W_HIGH_RATING,
        });
    }

    // -- taste_similarity -----------------------------------------------------
    if !taste_vector.is_empty() {
        let game_keywords = game_keywords_lower(game);
        if !game_keywords.is_empty() {
            let mut overlap: f32 = 0.0;
            for kw in &game_keywords {
                if let Some(&w) = taste_vector.get(kw) {
                    overlap += w;
                }
            }
            // Normalize: divide by the max possible overlap (sum of taste vector).
            // Only award the signal if there is meaningful overlap.
            let total_taste: f32 = taste_vector.values().sum();
            if total_taste > 0.0 {
                let similarity = overlap / total_taste;
                if similarity > 0.05 {
                    score += W_TASTE_SIMILARITY;
                    reasons.push(RecommendReason {
                        code: "taste_similarity".into(),
                        description: "Matches your taste profile".into(),
                        weight: W_TASTE_SIMILARITY,
                    });
                }
            }
        }
    }

    // -- recently_played_penalty ----------------------------------------------
    if let Some(last_played) = game.last_played_unix {
        let days_since = (now_unix - last_played) / 86400;
        if days_since < RECENTLY_PLAYED_DAYS {
            score += W_RECENTLY_PLAYED;
            reasons.push(RecommendReason {
                code: "recently_played_penalty".into(),
                description: format!("Played {} days ago", RECENTLY_PLAYED_DAYS - days_since),
                weight: W_RECENTLY_PLAYED,
            });
        }
    }

    // -- likely_finished_penalty ----------------------------------------------
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

/// Collect lowercase keywords (IGDB preferred, RAWG fallback) for a game.
fn game_keywords_lower(game: &Game) -> Vec<String> {
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

// ---------------------------------------------------------------------------
// Filtering helpers
// ---------------------------------------------------------------------------

/// Returns true if the game type should be excluded by default.
///
/// Non-game app types (Application, Tool, Dlc, Demo) are excluded unless they
/// have been explicitly curated by the user (i.e., they are not junk and not
/// hidden).
fn is_unsupported_type(game: &Game) -> bool {
    matches!(
        game.app_type,
        SteamAppType::Application | SteamAppType::Tool | SteamAppType::Dlc
    )
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

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
    let taste_vector = build_taste_vector(games);

    // "now" — derive from chrono for testability; in production this is wall clock.
    let now_unix = chrono::Utc::now().timestamp();

    let mut rng = request.seed.map(SplitMix64::new);

    let mut candidates: Vec<Recommendation> = games
        .iter()
        .filter(|g| {
            // -- Filtering ----------------------------------------------------
            if g.is_hidden || g.is_junk {
                return false;
            }
            if is_unsupported_type(g) {
                return false;
            }
            if request.include_installed_only && !g.installed {
                return false;
            }
            // Exclude games belonging to any excluded collection.
            if !request.exclude_collections.is_empty() {
                let excluded = g
                    .steam_collections
                    .iter()
                    .any(|c| request.exclude_collections.iter().any(|ec| ec == c));
                if excluded {
                    return false;
                }
            }
            true
        })
        .map(|game| {
            let (base_score, reasons) = score_game(game, request, &taste_vector, now_unix);

            // Deterministic (or random) perturbation in [0.0, 0.25).
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

    // Sort descending by score; for equal scores, lower app_id wins (stable).
    candidates.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.app_id.cmp(&b.app_id))
    });

    candidates.truncate(request.count);
    candidates
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        HltbData, HltbSource, IgdbData, ProtonDbData, RawgData, SteamAppType,
        VAPOURFLY_RECOMMENDATIONS_SCHEMA,
    };

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

    // Test 1: Hidden games are excluded.

    #[test]
    fn hidden_games_excluded() {
        let mut game = make_game(1, "Hidden Game");
        game.is_hidden = true;
        let games = vec![game];
        let req = default_request();
        let result = recommend(&games, &req);
        assert!(result.is_empty(), "hidden games should be excluded");
    }

    // Test 2: Junk games are excluded.

    #[test]
    fn junk_games_excluded() {
        let mut game = make_game(2, "Junk Game");
        game.is_junk = true;
        let games = vec![game];
        let req = default_request();
        let result = recommend(&games, &req);
        assert!(result.is_empty(), "junk games should be excluded");
    }

    // Test 3: Deterministic with seed.

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

    // Test 4: Without seed, perturbation is zero (still deterministic).

    #[test]
    fn no_seed_no_perturbation() {
        let games = vec![make_game(10, "Game A")];
        let mut req = default_request();
        req.seed = None;
        let result = recommend(&games, &req);
        assert_eq!(result.len(), 1);
        // Score should be exactly the sum of applied weights with no perturbation.
        // low_playtime (2.0) + time_match (1.5) = 3.5
        assert!(
            (result[0].score - 3.5).abs() < f32::EPSILON,
            "score without seed should have no perturbation, got {}",
            result[0].score
        );
    }

    // Test 5: Low playtime gets a higher score than high playtime.

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

    // Test 6: Time matching works.

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

    // Test 7: Empty library returns empty.

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
        let vector = build_taste_vector(&games);

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
        let vector = build_taste_vector(&games);

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
        let vector = build_taste_vector(&games);
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
        let vector = build_taste_vector(&games);
        assert!(vector.is_empty());
    }

    #[test]
    fn result_schema_is_correct() {
        let games: Vec<Game> = vec![];
        let req = default_request();
        let recommendations = recommend(&games, &req);
        let result = RecommendResult {
            schema: VAPOURFLY_RECOMMENDATIONS_SCHEMA.into(),
            recommendations,
            request: req.clone(),
        };
        assert_eq!(result.schema, "vapourfly.recommendations.v1");
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
