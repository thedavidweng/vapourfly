//! Discover playlist generation from taste similarity and IGDB metadata.

use std::collections::{BTreeMap, HashSet};

use crate::models::{Game, Playlist, PlaylistContent, PlaylistFile, VAPOURFLY_PLAYLIST_SCHEMA};
use crate::{preferences, progress, scoring, taste};

/// Options for generating a Discover playlist.
#[derive(Clone, Debug)]
pub struct DiscoverOptions {
    /// Optional seed game. When set, IGDB `similar_game_ids` are preferred.
    pub seed_app_id: Option<u32>,
    /// Maximum number of games to include.
    pub count: usize,
}

impl Default for DiscoverOptions {
    fn default() -> Self {
        Self {
            seed_app_id: None,
            count: 20,
        }
    }
}

/// One scored Discover candidate with human-readable reason codes.
///
/// Used by the GUI Discover page to show results on-page (scores + reasons)
/// without re-running opaque scoring in the UI layer.
#[derive(Clone, Debug, PartialEq)]
pub struct DiscoverPick {
    pub app_id: u32,
    pub name: String,
    pub score: f32,
    pub reasons: Vec<DiscoverReason>,
}

/// The same signed attribution schema as Recommendations, without planner inputs.
pub type DiscoverReason = crate::models::RecommendReason;

/// Identity-free convenience API. Account callers supply their cached profile/progress.
pub fn rank_discover_picks(games: &[Game], options: &DiscoverOptions) -> Vec<DiscoverPick> {
    rank_discover_with_preferences(
        games,
        options,
        &preferences::Preferences::new(),
        chrono::Utc::now().timestamp(),
    )
}

pub fn rank_discover_with_preferences(
    games: &[Game],
    options: &DiscoverOptions,
    prefs: &preferences::Preferences,
    now: i64,
) -> Vec<DiscoverPick> {
    let profile = taste::derive(games, prefs, &BTreeMap::new(), now);
    rank_discover_personalized(games, options, prefs, &profile, &BTreeMap::new(), now)
}

/// Existing owned/unplayed eligibility is unchanged. Missing activity is not evidence of zero activity.
pub fn rank_discover_personalized(
    games: &[Game],
    options: &DiscoverOptions,
    prefs: &preferences::Preferences,
    profile: &taste::TasteProfile,
    progress: &BTreeMap<u32, progress::Summary>,
    now: i64,
) -> Vec<DiscoverPick> {
    let similar = seed_similar_ids(games, options.seed_app_id);
    let eligible: Vec<_> = games
        .iter()
        .filter(|g| crate::eligibility::is_discover_eligible(g))
        .filter(|g| !preferences::excludes(prefs, g.app_id, now))
        .collect();
    let tags: BTreeMap<_, _> = eligible
        .iter()
        .map(|g| (g.app_id, taste::canonical_tags(g)))
        .collect();
    let candidates: Vec<_> = eligible.iter().map(|game| {
        let mut reasons = scoring::personal_reasons(game, profile, &tags[&game.app_id], progress.get(&game.app_id), prefs, now);
        if game.igdb.as_ref().is_some_and(|i| i.steam_app_id_confirmed && i.igdb_id != 0 && similar.contains(&i.igdb_id)) {
            scoring::contribution(&mut reasons, "SEED_SIMILAR", "Validated cached IGDB Steam mappings and seed similar link (bounded; not population taste)".into(), 0.75);
        } else if options.seed_app_id.is_some() {
            scoring::contribution(&mut reasons, "seed_evidence_missing", "No validated cached IGDB seed link to this candidate; similarity not assumed".into(), 0.0);
        }
        match scoring::known_playtime(profile, game.app_id) {
            Some(minutes) => scoring::contribution(&mut reasons, "novelty", format!("Bounded novelty from known lifetime activity ({minutes} min); not story completion"), 0.25 / (1.0 + minutes as f32 / 120.0)),
            None => scoring::contribution(&mut reasons, "activity_unknown", "Lifetime activity unavailable; legacy local-unplayed eligibility is not an assertion of zero playtime".into(), 0.0),
        }
        DiscoverPick { app_id: game.app_id, name: game.name.clone(), score: reasons.iter().map(|r| r.weight).sum(), reasons }
    }).collect();
    scoring::diverse_order(
        &candidates
            .iter()
            .map(|c| (c.app_id, c.score))
            .collect::<Vec<_>>(),
        &tags,
        options.count,
        None,
    )
    .into_iter()
    .map(|(index, amount)| {
        let mut selected = candidates[index].clone();
        if amount > 0.0 {
            selected.score -= amount;
            scoring::contribution(
                &mut selected.reasons,
                "diversity",
                "Bounded canonical-concept overlap with an earlier pick".into(),
                -amount,
            );
        }
        selected
    })
    .collect()
}

pub fn explanation(pick: &DiscoverPick) -> String {
    pick.reasons
        .iter()
        .map(|r| format!("{:+.2} {}", r.weight, r.description))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Build a manual Discover playlist from the user's library and cached metadata.
///
/// Uses the shared canonical profile and bounded validated seed relatedness.
/// Keeps useful eligible picks even with sparse or absent metadata.
///
/// Prefer [`rank_discover_picks`] + [`playlist_from_discover_picks`] when the
/// caller also needs scores/reasons, so ranking runs only once.
pub fn generate_discover_playlist(games: &[Game], options: &DiscoverOptions) -> PlaylistFile {
    let picks = rank_discover_picks(games, options);
    playlist_from_discover_picks(games, options, &picks)
}

/// Build a Discover [`PlaylistFile`] from already-ranked picks (no re-score).
pub fn playlist_from_discover_picks(
    games: &[Game],
    options: &DiscoverOptions,
    picks: &[DiscoverPick],
) -> PlaylistFile {
    let app_ids: Vec<u32> = picks.iter().map(|p| p.app_id).collect();

    let (id, name, description) = if let Some(seed_id) = options.seed_app_id {
        let seed_name = games
            .iter()
            .find(|g| g.app_id == seed_id)
            .map_or("selected game", |g| g.name.as_str());
        (
            format!("discover-{seed_id}"),
            format!("Discover: {seed_name}"),
            format!("Similar unplayed picks inspired by {seed_name}"),
        )
    } else {
        (
            "discover-taste".into(),
            "Discover".into(),
            "Unplayed picks that match your taste profile".into(),
        )
    };

    PlaylistFile {
        vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
        created_by: "vapourfly".into(),
        playlist: Playlist {
            id,
            name,
            description,
            content: PlaylistContent::Manual { app_ids },
        },
    }
}

fn seed_similar_ids(games: &[Game], seed_app_id: Option<u32>) -> HashSet<u64> {
    let mut seed_similar: HashSet<u64> = HashSet::new();
    if let Some(seed_id) = seed_app_id
        && let Some(seed_game) = games.iter().find(|g| g.app_id == seed_id)
        && let Some(igdb) = &seed_game.igdb
        && igdb.steam_app_id_confirmed
        && igdb.igdb_id != 0
    {
        seed_similar.extend(igdb.similar_game_ids.iter().copied());
    }
    seed_similar
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SteamAppType;
    use crate::models::{IgdbData, RawgData};

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

    fn mapping(id: u64, confirmed: bool, similar: Vec<u64>) -> IgdbData {
        IgdbData {
            igdb_id: id,
            name: "Mapping".into(),
            slug: None,
            rating_0_100: None,
            total_rating_0_100: None,
            genres: vec![],
            themes: vec![],
            keywords: vec![],
            similar_game_ids: similar,
            steam_app_id_confirmed: confirmed,
            time_to_beat: None,
        }
    }

    #[test]
    fn personal_taste_is_continuous_and_validated_seed_cannot_erase_it() {
        let mut engaged = tagged(100, &["Strategy", "Co-op"]);
        engaged.playtime_minutes = Some(600);
        engaged.igdb = Some(mapping(1000, true, vec![3000]));
        let mut related = tagged(3, &["Puzzle"]);
        related.igdb = Some(mapping(3000, true, vec![]));
        let games = vec![
            engaged,
            tagged(1, &["strategy", "cooperative"]),
            tagged(2, &["Strategy"]),
            related,
        ];
        let options = DiscoverOptions {
            seed_app_id: Some(100),
            count: 10,
        };
        let picks = rank_discover_picks(&games, &options);
        assert_eq!(picks[0].app_id, 1);
        let weight = |id| {
            picks
                .iter()
                .find(|p| p.app_id == id)
                .unwrap()
                .reasons
                .iter()
                .find(|r| r.code == "taste_similarity")
                .unwrap()
                .weight
        };
        assert_eq!(weight(1), 3.0);
        assert_eq!(weight(2), 1.5);
        assert_eq!(
            picks
                .iter()
                .find(|p| p.app_id == 3)
                .unwrap()
                .reasons
                .iter()
                .find(|r| r.code == "SEED_SIMILAR")
                .unwrap()
                .weight,
            0.75
        );
        let mut reversed = games;
        reversed.reverse();
        assert_eq!(picks, rank_discover_picks(&reversed, &options));
    }

    #[test]
    fn seed_links_require_both_confirmed_identities() {
        for (seed_valid, candidate_valid) in
            [(false, false), (true, false), (false, true), (true, true)]
        {
            let mut seed = make_game(1, "Seed");
            seed.playtime_minutes = Some(600);
            seed.igdb = Some(mapping(100, seed_valid, vec![200]));
            let mut candidate = make_game(2, "Candidate");
            candidate.igdb = Some(mapping(200, candidate_valid, vec![]));
            let picks = rank_discover_picks(
                &[seed, candidate],
                &DiscoverOptions {
                    seed_app_id: Some(1),
                    count: 10,
                },
            );
            assert_eq!(picks.len(), 1);
            assert_eq!(
                picks[0].reasons.iter().any(|r| r.code == "SEED_SIMILAR"),
                seed_valid && candidate_valid
            );
            assert_eq!(
                picks[0]
                    .reasons
                    .iter()
                    .any(|r| r.code == "seed_evidence_missing"),
                !(seed_valid && candidate_valid)
            );
        }
    }

    #[test]
    fn cold_unknown_library_is_useful_honest_and_playlist_compatible() {
        let mut unknown = make_game(2, "Unknown");
        unknown.playtime_minutes = None;
        let options = DiscoverOptions::default();
        let games = vec![unknown, make_game(1, "Known zero")];
        let picks = rank_discover_picks(&games, &options);
        assert_eq!(
            picks.iter().map(|p| p.app_id).collect::<Vec<_>>(),
            vec![1, 2]
        );
        let why = explanation(&picks[1]);
        assert!(why.contains("not an assertion of zero"));
        assert!(why.contains("Cache/local only"));
        assert!(!why.contains("SEED_SIMILAR"));
        assert_eq!(picks[1].score, 0.0);
        let pf = playlist_from_discover_picks(&games, &options, &picks);
        let code = crate::share_code::encode_share_code(&pf).unwrap();
        let decoded = crate::share_code::decode_share_code(&code).unwrap();
        let PlaylistContent::Manual { app_ids } = decoded.playlist.content else {
            panic!("manual playlist");
        };
        assert_eq!(app_ids, vec![1, 2]);
        assert_eq!(pf.playlist.id, "discover-taste");
        assert_eq!(decoded.playlist.id, "discover");
    }

    #[test]
    fn diversity_and_progress_remain_bounded_signed_evidence() {
        let games = vec![
            tagged(1, &["Strategy"]),
            tagged(2, &["Strategy"]),
            tagged(3, &["Puzzle"]),
        ];
        let prefs = preferences::Preferences::new();
        let profile = taste::derive(&games, &prefs, &BTreeMap::new(), 100);
        let options = DiscoverOptions::default();
        let picks =
            rank_discover_personalized(&games, &options, &prefs, &profile, &BTreeMap::new(), 100);
        assert_eq!(
            picks.iter().map(|p| p.app_id).collect::<Vec<_>>(),
            vec![1, 3, 2]
        );
        assert!(
            picks[2]
                .reasons
                .iter()
                .any(|r| r.code == "diversity" && r.weight < 0.0)
        );
        let mut progress = BTreeMap::new();
        progress.insert(1, progress::Summary::new(Some(0), true, None));
        progress.insert(
            2,
            progress::Summary::new(
                Some(0),
                false,
                Some(progress::AchievementEvidence {
                    unlocked: 10,
                    total: 10,
                    stale: true,
                }),
            ),
        );
        let picks = rank_discover_personalized(&games, &options, &prefs, &profile, &progress, 100);
        assert_ne!(picks[0].app_id, 1);
        assert!(
            !picks
                .iter()
                .find(|p| p.app_id == 2)
                .unwrap()
                .reasons
                .iter()
                .any(|r| r.code == "perfected")
        );
        for pick in picks {
            assert!((pick.score - pick.reasons.iter().map(|r| r.weight).sum::<f32>()).abs() < 1e-5);
        }
    }

    #[test]
    fn exclusions_precede_scoring_and_not_now_expiry_is_exact() {
        let games = vec![
            make_game(1, "Excluded"),
            make_game(2, "Later"),
            make_game(3, "Interested"),
        ];
        let prefs: preferences::Preferences = serde_json::from_value(serde_json::json!({
            "1": { "version": 1, "choice": "not_interested", "expires_at": null },
            "2": { "version": 1, "choice": "not_now", "expires_at": 100 },
            "3": { "version": 1, "choice": "interested", "expires_at": null }
        }))
        .unwrap();
        let options = DiscoverOptions::default();
        assert_eq!(
            rank_discover_with_preferences(&games, &options, &prefs, 99)
                .iter()
                .map(|p| p.app_id)
                .collect::<Vec<_>>(),
            vec![3]
        );
        assert_eq!(
            rank_discover_with_preferences(&games, &options, &prefs, 100)
                .iter()
                .map(|p| p.app_id)
                .collect::<Vec<_>>(),
            vec![3, 2]
        );
    }

    #[test]
    fn discover_prefers_seed_similarity() {
        let mut seed = make_game(1, "Seed");
        seed.igdb = Some(IgdbData {
            igdb_id: 100,
            name: "Seed".into(),
            slug: None,
            rating_0_100: None,
            total_rating_0_100: None,
            genres: vec![],
            themes: vec![],
            keywords: vec![],
            similar_game_ids: vec![200],
            steam_app_id_confirmed: true,
            time_to_beat: None,
        });

        let mut similar = make_game(2, "Similar");
        similar.igdb = Some(IgdbData {
            igdb_id: 200,
            name: "Similar".into(),
            slug: None,
            rating_0_100: None,
            total_rating_0_100: None,
            genres: vec![],
            themes: vec![],
            keywords: vec![],
            similar_game_ids: vec![],
            steam_app_id_confirmed: true,
            time_to_beat: None,
        });

        let other = make_game(3, "Other");
        let games = vec![seed, similar, other];

        let pf = generate_discover_playlist(
            &games,
            &DiscoverOptions {
                seed_app_id: Some(1),
                count: 5,
            },
        );

        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => {
                assert_eq!(app_ids.first().copied(), Some(2));
            }
            _ => panic!("expected manual playlist"),
        }
    }

    #[test]
    fn discover_excludes_played_games() {
        let mut played = make_game(1, "Played");
        played.playtime_minutes = Some(500);
        let mut unplayed = make_game(2, "Unplayed");
        unplayed.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: Some(4.5),
            ratings_count: Some(10),
            genres: vec!["Action".into()],
            tags: vec![],
            stores: vec![],
        });

        let pf = generate_discover_playlist(&[played, unplayed], &DiscoverOptions::default());
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => {
                assert_eq!(app_ids, vec![2]);
            }
            _ => panic!("expected manual playlist"),
        }
    }

    #[test]
    fn rank_discover_picks_includes_scores_and_seed_reason() {
        let mut seed = make_game(1, "Seed");
        seed.playtime_minutes = Some(600);
        seed.igdb = Some(IgdbData {
            igdb_id: 100,
            name: "Seed".into(),
            slug: None,
            rating_0_100: None,
            total_rating_0_100: None,
            genres: vec![],
            themes: vec![],
            keywords: vec![],
            similar_game_ids: vec![200],
            steam_app_id_confirmed: true,
            time_to_beat: None,
        });

        let mut similar = make_game(2, "Similar");
        similar.igdb = Some(IgdbData {
            igdb_id: 200,
            name: "Similar".into(),
            slug: None,
            rating_0_100: None,
            total_rating_0_100: None,
            genres: vec![],
            themes: vec![],
            keywords: vec![],
            similar_game_ids: vec![],
            steam_app_id_confirmed: true,
            time_to_beat: None,
        });

        let picks = rank_discover_picks(
            &[seed, similar],
            &DiscoverOptions {
                seed_app_id: Some(1),
                count: 5,
            },
        );

        assert_eq!(picks.len(), 1);
        assert_eq!(picks[0].app_id, 2);
        assert_eq!(picks[0].name, "Similar");
        assert!(picks[0].score >= 0.75);
        assert!(
            picks[0]
                .reasons
                .iter()
                .any(|r| r.code == "SEED_SIMILAR" && (r.weight - 0.75).abs() < f32::EPSILON),
            "expected SEED_SIMILAR reason: {:?}",
            picks[0].reasons
        );
    }

    #[test]
    fn rank_discover_picks_high_rating_contributes_reason() {
        let mut unplayed = make_game(10, "Rated");
        unplayed.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: Some(4.8),
            ratings_count: Some(100),
            genres: vec!["Indie".into()],
            tags: vec![],
            stores: vec![],
        });

        let picks = rank_discover_picks(&[unplayed], &DiscoverOptions::default());
        assert_eq!(picks.len(), 1);
        assert!(
            picks[0].reasons.iter().any(|r| r.code == "high_rating"),
            "expected HIGH_RATING: {:?}",
            picks[0].reasons
        );
    }

    #[test]
    fn generate_playlist_app_ids_match_ranked_picks() {
        let mut a = make_game(1, "A");
        a.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: Some(4.9),
            ratings_count: Some(50),
            genres: vec!["Action".into()],
            tags: vec![],
            stores: vec![],
        });
        let mut b = make_game(2, "B");
        b.rawg = Some(RawgData {
            rawg_id: 2,
            rating_0_5: Some(4.0),
            ratings_count: Some(10),
            genres: vec!["RPG".into()],
            tags: vec![],
            stores: vec![],
        });

        let options = DiscoverOptions {
            seed_app_id: None,
            count: 10,
        };
        let picks = rank_discover_picks(&[a.clone(), b.clone()], &options);
        let pf = generate_discover_playlist(&[a, b], &options);
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => {
                let pick_ids: Vec<u32> = picks.iter().map(|p| p.app_id).collect();
                assert_eq!(app_ids, pick_ids);
            }
            _ => panic!("expected manual"),
        }
    }
}
