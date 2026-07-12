//! Discover playlist generation from taste similarity and IGDB metadata.

use std::collections::{HashMap, HashSet};

use crate::models::{
    Game, Playlist, PlaylistContent, PlaylistFile, SteamAppType, VAPOURFLY_PLAYLIST_SCHEMA,
};
use crate::recommend::build_taste_vector;

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

/// Build a manual Discover playlist from the user's library and cached metadata.
///
/// When `seed_app_id` is provided and that game has IGDB similar-game IDs,
/// owned unplayed candidates from that similarity list are ranked first.
/// Otherwise, candidates are ranked by overlap with the taste vector built
/// from meaningful lifetime playtime.
pub fn generate_discover_playlist(games: &[Game], options: &DiscoverOptions) -> PlaylistFile {
    let taste_vector = build_taste_vector(games);
    let mut seed_similar: HashSet<u64> = HashSet::new();
    if let Some(seed_id) = options.seed_app_id {
        if let Some(seed_game) = games.iter().find(|g| g.app_id == seed_id) {
            if let Some(igdb) = &seed_game.igdb {
                seed_similar.extend(igdb.similar_game_ids.iter().copied());
            }
        }
    }

    let mut scored: Vec<(u32, f32)> = games
        .iter()
        .filter(|g| is_discover_candidate(g))
        .map(|game| {
            (
                game.app_id,
                score_candidate(game, &taste_vector, &seed_similar),
            )
        })
        .filter(|(_, score)| *score > 0.0)
        .collect();

    scored.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    scored.truncate(options.count);

    let app_ids: Vec<u32> = scored.into_iter().map(|(id, _)| id).collect();

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

fn is_discover_candidate(game: &Game) -> bool {
    if game.is_hidden || game.is_junk {
        return false;
    }
    if matches!(
        game.app_type,
        SteamAppType::Application | SteamAppType::Tool | SteamAppType::Dlc
    ) {
        return false;
    }
    game.playtime_minutes.is_none_or(|m| m == 0)
}

fn score_candidate(
    game: &Game,
    taste_vector: &HashMap<String, f32>,
    seed_similar: &HashSet<u64>,
) -> f32 {
    let mut score = 0.0;

    if let Some(igdb) = &game.igdb {
        if seed_similar.contains(&igdb.igdb_id) {
            score += 5.0;
        }
    }

    if !taste_vector.is_empty() {
        let keywords = game.keywords_lower();
        let total_taste: f32 = taste_vector.values().sum();
        if total_taste > 0.0 {
            let overlap: f32 = keywords.iter().filter_map(|kw| taste_vector.get(kw)).sum();
            score += overlap / total_taste;
        }
    }

    if let Some(rawg) = &game.rawg {
        if rawg.rating_0_5.is_some_and(|r| r >= 4.0) {
            score += 0.25;
        }
    } else if let Some(igdb) = &game.igdb {
        if igdb.rating_0_100.is_some_and(|r| r >= 80.0)
            || igdb.total_rating_0_100.is_some_and(|r| r >= 80.0)
        {
            score += 0.25;
        }
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
