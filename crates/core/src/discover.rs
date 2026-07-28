//! Discover playlist generation from taste similarity and IGDB metadata.

use std::collections::{HashMap, HashSet};

use crate::models::{Game, Playlist, PlaylistContent, PlaylistFile, VAPOURFLY_PLAYLIST_SCHEMA};
use crate::scoring;

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

/// Explainable contribution to a Discover pick's score.
#[derive(Clone, Debug, PartialEq)]
pub struct DiscoverReason {
    pub code: &'static str,
    pub description: &'static str,
    pub weight: f32,
}

/// Rank Discover candidates with scores and reason codes (no store write).
///
/// Ranking matches [`generate_discover_playlist`]: seed IGDB similarity,
/// taste-vector overlap, and a high-rating bonus. Only candidates with a
/// positive score are returned, sorted highest-first and truncated to
/// `options.count`.
pub fn rank_discover_picks(games: &[Game], options: &DiscoverOptions) -> Vec<DiscoverPick> {
    let taste_vector = scoring::build_taste_vector(games);
    let seed_similar = seed_similar_ids(games, options.seed_app_id);

    let mut picks: Vec<DiscoverPick> = games
        .iter()
        .filter(|g| crate::eligibility::is_discover_eligible(g))
        .filter_map(|game| {
            let (score, reasons) = score_candidate(game, &taste_vector, &seed_similar);
            if score > 0.0 {
                Some(DiscoverPick {
                    app_id: game.app_id,
                    name: game.name.clone(),
                    score,
                    reasons,
                })
            } else {
                None
            }
        })
        .collect();

    picks.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.app_id.cmp(&b.app_id))
    });
    picks.truncate(options.count);
    picks
}

/// Build a manual Discover playlist from the user's library and cached metadata.
///
/// When `seed_app_id` is provided and that game has IGDB similar-game IDs,
/// owned unplayed candidates from that similarity list are ranked first.
/// Otherwise, candidates are ranked by overlap with the taste vector built
/// from meaningful lifetime playtime.
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
    {
        seed_similar.extend(igdb.similar_game_ids.iter().copied());
    }
    seed_similar
}

fn score_candidate(
    game: &Game,
    taste_vector: &HashMap<String, f32>,
    seed_similar: &HashSet<u64>,
) -> (f32, Vec<DiscoverReason>) {
    let mut score = 0.0;
    let mut reasons = Vec::new();

    if let Some(igdb) = &game.igdb
        && seed_similar.contains(&igdb.igdb_id)
    {
        let weight = 5.0;
        score += weight;
        reasons.push(DiscoverReason {
            code: "SEED_SIMILAR",
            description: "Similar to seed game (IGDB)",
            weight,
        });
    }

    if !taste_vector.is_empty() {
        let overlap = scoring::taste_overlap(game, taste_vector);
        if overlap > 0.0 {
            score += overlap;
            reasons.push(DiscoverReason {
                code: "TASTE",
                description: "Overlaps your taste profile",
                weight: overlap,
            });
        }
    }

    if scoring::is_high_rating(game) {
        let weight = 0.25;
        score += weight;
        reasons.push(DiscoverReason {
            code: "HIGH_RATING",
            description: "High community rating",
            weight,
        });
    }

    (score, reasons)
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
            steam_app_id_confirmed: false,
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
            steam_app_id_confirmed: false,
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
            steam_app_id_confirmed: false,
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
            steam_app_id_confirmed: false,
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
        assert!(picks[0].score >= 5.0);
        assert!(
            picks[0]
                .reasons
                .iter()
                .any(|r| r.code == "SEED_SIMILAR" && (r.weight - 5.0).abs() < f32::EPSILON),
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
            picks[0].reasons.iter().any(|r| r.code == "HIGH_RATING"),
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
