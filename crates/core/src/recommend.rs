//! Cache-only owned-library ranking with attributable, bounded contributions.

use std::collections::{BTreeMap, BTreeSet};

use crate::models::{Game, ProtonTier, RecommendReason, RecommendRequest, Recommendation};
use crate::{preferences, progress, scoring, taste};

const RECENT_SECONDS: i64 = 14 * 86400;
const DIVERSITY_WEIGHT: f32 = 0.35;

fn tie_key(seed: Option<u64>, app_id: u32) -> u64 {
    let Some(seed) = seed else {
        return u64::from(app_id);
    };
    let mut z = (seed ^ u64::from(app_id)).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn contribution(reasons: &mut Vec<RecommendReason>, code: &str, description: String, weight: f32) {
    reasons.push(RecommendReason {
        code: code.into(),
        description,
        weight,
    });
}

fn score_game(
    game: &Game,
    request: &RecommendRequest,
    profile: &taste::TasteProfile,
    concepts: &BTreeSet<String>,
    progress: Option<&progress::Summary>,
    prefs: &preferences::Preferences,
    now: i64,
) -> Recommendation {
    let mut reasons = Vec::new();
    // The shared derivation emits title evidence in AppID order.
    let title = profile
        .titles
        .binary_search_by_key(&game.app_id, |t| t.app_id)
        .ok()
        .map(|index| &profile.titles[index]);
    let playtime = title.and_then(|t| t.activity.iter().filter_map(|a| a.lifetime_minutes).max());
    if let Some(minutes) = playtime.filter(|m| *m < 120) {
        contribution(
            &mut reasons,
            "low_playtime",
            format!("Known low playtime ({minutes} min)"),
            2.0,
        );
    } else if playtime.is_none() {
        contribution(
            &mut reasons,
            "activity_unknown",
            "Lifetime activity unknown, not zero or unplayed".into(),
            0.0,
        );
    }

    if request.deck_mode {
        let deck = game.protondb.as_ref().and_then(|p| match p.tier {
            ProtonTier::Native => Some((2.0, "Native Steam Deck support")),
            ProtonTier::Platinum => Some((1.5, "Platinum on Steam Deck")),
            ProtonTier::Gold => Some((1.0, "Gold on Steam Deck")),
            _ => None,
        });
        if let Some((weight, label)) = deck {
            contribution(&mut reasons, "deck_compatible", label.into(), weight);
        }
    }
    if request.available_minutes > 0 {
        if let Some(secs) = crate::signal::main_story_seconds(game)
            && secs / 60 <= request.available_minutes
        {
            contribution(
                &mut reasons,
                "time_match",
                format!(
                    "Cached whole-story estimate fits {} min; not remaining time or session evidence",
                    request.available_minutes
                ),
                1.5,
            );
        }
    }
    if let Some(weight) = scoring::quality(game) {
        contribution(
            &mut reasons,
            "high_rating",
            "Above-midpoint cached game quality (bounded 0..1; low/missing neutral; not personal completion)".into(),
            weight,
        );
    }
    for (code, tags, weight) in [
        ("taste_similarity", &profile.long_term, 3.0),
        ("recent_taste", &profile.recent, 0.5),
    ] {
        let affinity = scoring::profile_affinity(concepts, tags);
        if affinity > 0.0 {
            let matches = tags
                .iter()
                .filter(|t| concepts.contains(&t.tag))
                .map(|t| t.tag.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            contribution(
                &mut reasons,
                code,
                format!("Cached canonical profile affinity {affinity:.3}: {matches}"),
                weight * affinity,
            );
        }
    }
    contribution(
        &mut reasons,
        "evidence_limits",
        format!(
            "Cache/local only; descriptors {}/{} eligible titles; absent matches are not dislike; fit is not probability or player percentile",
            profile.coverage.metadata_titles, profile.coverage.eligible_titles
        ),
        0.0,
    );

    if let Some(last) = title.and_then(|t| t.last_played_at) {
        let age = now - last;
        if (0..RECENT_SECONDS).contains(&age) {
            let days = age / 86400;
            let label = match days {
                0 => "Played today".into(),
                1 => "Played 1 day ago".into(),
                n => format!("Played {n} days ago"),
            };
            contribution(
                &mut reasons,
                "recently_played_penalty",
                label,
                -(1.0 - age as f32 / RECENT_SECONDS as f32),
            );
        }
    }
    match progress {
        Some(p) if p.story_completed => contribution(&mut reasons, "story_completed", "Story completed (your local mark); achievements are separate".into(), -1.25),
        Some(p) if p.achievements.is_some_and(|e| !e.stale && e.total > 0 && e.unlocked == e.total) => contribution(&mut reasons, "perfected", "Fresh authoritative achievements: all unlocked; not a story inference".into(), -1.0),
        Some(p) if p.achievements.is_some_and(|e| !e.stale && e.total > 0 && e.unlocked > 0 && e.unlocked < e.total) => {
            let e = p.achievements.unwrap();
            contribution(&mut reasons, "achievement_progress", format!("Fresh authoritative achievements {}/{}; incomplete achievements do not imply unfinished story", e.unlocked, e.total), 0.5 * e.unlocked as f32 / e.total as f32);
        }
        _ => contribution(&mut reasons, "progress_unknown", "Story unmarked; achievement completion missing/stale/zero; no playtime or duration completion guess".into(), 0.0),
    }
    if preferences::interested(prefs, game.app_id, now) {
        contribution(
            &mut reasons,
            "local_interested",
            "Interested (your local preference)".into(),
            preferences::INTERESTED_WEIGHT,
        );
    }
    if game
        .steam_collections
        .iter()
        .any(|c| c.eq_ignore_ascii_case("favorites"))
    {
        contribution(
            &mut reasons,
            "favorite",
            "Steam Favorites (explicit choice, not a game tag)".into(),
            0.25,
        );
    }
    Recommendation {
        app_id: game.app_id,
        name: game.name.clone(),
        score: reasons.iter().map(|r| r.weight).sum(),
        reasons,
    }
}

/// Complete signed contributions and limitations, shared by human CLI/GUI output.
pub fn explanation(rec: &Recommendation) -> String {
    rec.reasons
        .iter()
        .map(|r| format!("{:+.2} {}", r.weight, r.description))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Identity-free local-library convenience API; account callers provide their prepared profile/progress.
pub fn recommend(games: &[Game], request: &RecommendRequest) -> Vec<Recommendation> {
    recommend_with_preferences(
        games,
        request,
        &preferences::Preferences::new(),
        chrono::Utc::now().timestamp(),
    )
}

pub fn recommend_with_preferences(
    games: &[Game],
    request: &RecommendRequest,
    prefs: &preferences::Preferences,
    now: i64,
) -> Vec<Recommendation> {
    let profile = taste::derive(games, prefs, &BTreeMap::new(), now);
    recommend_personalized(games, request, prefs, &profile, &BTreeMap::new(), now)
}

/// Eligibility precedes scoring. Seed affects ties only, never relevance weights.
pub fn recommend_personalized(
    games: &[Game],
    request: &RecommendRequest,
    prefs: &preferences::Preferences,
    profile: &taste::TasteProfile,
    progress: &BTreeMap<u32, progress::Summary>,
    now: i64,
) -> Vec<Recommendation> {
    use crate::eligibility::{EligibilityOptions, is_eligible};
    let eligible: Vec<_> = games
        .iter()
        .filter(|g| {
            is_eligible(
                g,
                EligibilityOptions {
                    unplayed_only: false,
                    installed_only: request.include_installed_only,
                },
            ) && !preferences::excludes(prefs, g.app_id, now)
                && !g
                    .steam_collections
                    .iter()
                    .any(|c| request.exclude_collections.contains(c))
        })
        .collect();
    let tags: BTreeMap<_, _> = eligible
        .iter()
        .map(|g| (g.app_id, taste::canonical_tags(g)))
        .collect();
    let mut candidates: Vec<_> = eligible
        .iter()
        .map(|g| {
            score_game(
                g,
                request,
                profile,
                &tags[&g.app_id],
                progress.get(&g.app_id),
                prefs,
                now,
            )
        })
        .collect();
    let mut result: Vec<Recommendation> = Vec::new();
    while !candidates.is_empty() && result.len() < request.count {
        let penalty = |candidate: &Recommendation| {
            result
                .iter()
                .map(|picked| {
                    let a = &tags[&candidate.app_id];
                    let b = &tags[&picked.app_id];
                    let union = a.union(b).count();
                    if union == 0 {
                        0.0
                    } else {
                        DIVERSITY_WEIGHT * a.intersection(b).count() as f32 / union as f32
                    }
                })
                .fold(0.0, f32::max)
        };
        let (index, amount) = candidates
            .iter()
            .enumerate()
            .map(|(i, c)| (i, penalty(c)))
            .max_by(|(ai, ap), (bi, bp)| {
                let a = &candidates[*ai];
                let b = &candidates[*bi];
                (a.score - ap)
                    .total_cmp(&(b.score - bp))
                    .then_with(|| {
                        tie_key(request.seed, b.app_id).cmp(&tie_key(request.seed, a.app_id))
                    })
                    .then_with(|| b.app_id.cmp(&a.app_id))
            })
            .unwrap();
        let mut selected = candidates.swap_remove(index);
        if amount > 0.0 {
            selected.score -= amount;
            contribution(
                &mut selected.reasons,
                "diversity",
                "Bounded canonical-concept overlap with an earlier pick".into(),
                -amount,
            );
        }
        result.push(selected);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{HltbData, HltbSource, IgdbData, ProtonDbData, RawgData, SteamAppType};

    fn tagged(id: u32, tags: &[&str]) -> Game {
        let mut game = make_game(id, "Candidate");
        game.rawg = Some(RawgData {
            rawg_id: u64::from(id),
            rating_0_5: None,
            ratings_count: None,
            genres: vec![],
            tags: tags.iter().map(|t| (*t).into()).collect(),
            stores: vec![],
        });
        game
    }

    #[test]
    fn continuous_affinity_diversity_and_signed_explanations_are_stable() {
        let mut engaged = tagged(100, &["Strategy", "Co-op"]);
        engaged.playtime_minutes = Some(600);
        let games = vec![
            engaged,
            tagged(1, &["strategy", "cooperative"]),
            tagged(2, &["Strategy"]),
            tagged(3, &["Puzzle"]),
        ];
        let prefs = preferences::Preferences::new();
        let profile = taste::derive(&games, &prefs, &BTreeMap::new(), 100);
        let req = default_request();
        let result = recommend_personalized(&games, &req, &prefs, &profile, &BTreeMap::new(), 100);
        assert_eq!(result[0].app_id, 1);
        let weight = |id| {
            result
                .iter()
                .find(|r| r.app_id == id)
                .unwrap()
                .reasons
                .iter()
                .find(|r| r.code == "taste_similarity")
                .unwrap()
                .weight
        };
        assert_eq!(weight(1), 3.0);
        assert_eq!(weight(2), 1.5);
        for rec in &result {
            assert!((rec.score - rec.reasons.iter().map(|r| r.weight).sum::<f32>()).abs() < 1e-5);
            let text = explanation(rec);
            assert!(text.contains("not probability or player percentile"));
            for reason in &rec.reasons {
                assert!(text.contains(&format!("{:+.2}", reason.weight)));
            }
        }
        let mut reversed = games.clone();
        reversed.reverse();
        let reversed_profile = taste::derive(&reversed, &prefs, &BTreeMap::new(), 100);
        assert_eq!(
            serde_json::to_value(&result).unwrap(),
            serde_json::to_value(recommend_personalized(
                &reversed,
                &req,
                &prefs,
                &reversed_profile,
                &BTreeMap::new(),
                100
            ))
            .unwrap()
        );
        // With no engagement, variety should beat an exact duplicate, not relevance.
        let cold = vec![
            tagged(1, &["Strategy"]),
            tagged(2, &["Strategy"]),
            tagged(3, &["Puzzle"]),
        ];
        let mut request = default_request();
        request.seed = None;
        let picks = recommend_with_preferences(&cold, &request, &prefs, 100);
        assert_eq!(
            picks.iter().map(|r| r.app_id).collect::<Vec<_>>(),
            vec![1, 3, 2]
        );
        assert!(
            picks[2]
                .reasons
                .iter()
                .any(|r| r.code == "diversity" && r.weight == -0.35)
        );
    }

    #[test]
    fn progress_uses_only_local_story_or_fresh_authoritative_counts() {
        let games: Vec<_> = (1..=5).map(|id| make_game(id, "Progress")).collect();
        let prefs = preferences::Preferences::new();
        let profile = taste::derive(&games, &prefs, &BTreeMap::new(), 100);
        let counts = |unlocked, total, stale| {
            Some(progress::AchievementEvidence {
                unlocked,
                total,
                stale,
            })
        };
        let progress = BTreeMap::from([
            (1, progress::Summary::new(Some(0), true, None)),
            (
                2,
                progress::Summary::new(Some(0), false, counts(10, 10, false)),
            ),
            (
                3,
                progress::Summary::new(Some(0), false, counts(5, 10, false)),
            ),
            (
                4,
                progress::Summary::new(Some(0), false, counts(10, 10, true)),
            ),
            (
                5,
                progress::Summary::new(Some(0), false, counts(0, 0, false)),
            ),
        ]);
        let result =
            recommend_personalized(&games, &default_request(), &prefs, &profile, &progress, 100);
        for (id, code, weight) in [
            (1, "story_completed", -1.25),
            (2, "perfected", -1.0),
            (3, "achievement_progress", 0.25),
            (4, "progress_unknown", 0.0),
            (5, "progress_unknown", 0.0),
        ] {
            assert!(
                result
                    .iter()
                    .find(|r| r.app_id == id)
                    .unwrap()
                    .reasons
                    .iter()
                    .any(|r| r.code == code && r.weight == weight)
            );
        }
    }

    #[test]
    fn unknown_activity_feedback_expiry_and_favorites_remain_distinct() {
        let temp = tempfile::tempdir().unwrap();
        let id = "76561198000000000";
        for (app, choice) in [
            (1, preferences::Choice::NotInterested),
            (2, preferences::Choice::NotNow),
            (3, preferences::Choice::Interested),
        ] {
            preferences::set(temp.path(), id, app, Some(choice), 100).unwrap();
        }
        let prefs = preferences::load(temp.path(), id).unwrap();
        let mut games: Vec<_> = (1..=4).map(|id| make_game(id, "Feedback")).collect();
        games[2].steam_collections.push("Favorites".into());
        games[3].playtime_minutes = None;
        games[3].last_played_unix = Some(200); // future activity is not recent evidence
        let picks = recommend_with_preferences(&games, &default_request(), &prefs, 100);
        assert_eq!(
            picks.iter().map(|r| r.app_id).collect::<Vec<_>>(),
            vec![3, 4]
        );
        assert!(
            picks[0]
                .reasons
                .iter()
                .any(|r| r.code == "local_interested" && r.weight == 0.5)
        );
        assert!(
            picks[0]
                .reasons
                .iter()
                .any(|r| r.code == "favorite" && r.weight == 0.25)
        );
        assert!(
            picks[1]
                .reasons
                .iter()
                .any(|r| r.code == "activity_unknown")
        );
        assert_eq!(picks[1].score, 0.0);
        let expired = recommend_with_preferences(
            &games,
            &default_request(),
            &prefs,
            100 + preferences::NOT_NOW_SECONDS,
        );
        assert!(expired.iter().any(|r| r.app_id == 2));
        assert!(expired.iter().all(|r| r.app_id != 1));
    }

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

    // time_match requires a known effective completion time.

    #[test]
    fn time_match_requires_known_completion_time() {
        // No completion time: no time_match regardless of window.
        let unknown = make_game(1, "Unknown Length");

        // IGDB time-to-beat fits: time_match via the supported live source.
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
        assert!(result.is_empty(), "{result:?}");
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
    fn playtime_and_duration_do_not_infer_completion() {
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
                .all(|r| r.code != "likely_finished_penalty"),
            "playtime must not infer completion"
        );
    }

    #[test]
    fn low_quality_is_neutral_and_above_midpoint_quality_is_continuous() {
        let mut games: Vec<_> = (1..=5).map(|id| tagged(id, &[])).collect();
        for (game, rating) in
            games
                .iter_mut()
                .zip([Some(1.0), None, Some(2.5), Some(4.0), Some(5.0)])
        {
            game.rawg.as_mut().unwrap().rating_0_5 = rating;
        }
        let mut request = default_request();
        request.seed = None;
        let picks = recommend(&games, &request);
        assert_eq!(
            picks.iter().map(|r| r.app_id).collect::<Vec<_>>(),
            vec![5, 4, 1, 2, 3]
        );
        assert!((picks[0].score - picks[1].score - 0.4).abs() < 1e-5);
        assert_eq!(picks[2].score, picks[3].score);
        assert_eq!(picks[3].score, picks[4].score);
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
        assert!(vector.is_empty(), "{vector:?}");
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
        assert!(vector.is_empty(), "{vector:?}");
    }

    #[test]
    fn seeded_ties_are_input_order_independent_without_relevance_noise() {
        let games: Vec<Game> = (1..20).map(|id| make_game(id, "Same evidence")).collect();
        let req = default_request();
        let first = recommend(&games, &req);
        let mut reversed = games.clone();
        reversed.reverse();
        let second = recommend(&reversed, &req);
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&second).unwrap()
        );
        let other = recommend(
            &games,
            &RecommendRequest {
                seed: Some(999),
                ..req
            },
        );
        assert_ne!(
            first.iter().map(|r| r.app_id).collect::<Vec<_>>(),
            other.iter().map(|r| r.app_id).collect::<Vec<_>>()
        );
        assert!(first.iter().all(|r| r.score == 2.0));
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
