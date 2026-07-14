//! Shared Game eligibility for recommendation and playlist generators.
//!
//! Deep module: owns the base library filter that Recommend, Discover,
//! Editorial Mood, and Dynamic Template (Finish It) all apply. Callers add
//! only their deltas (unplayed-only, installed-only, collection excludes).
//!
//! Base policy (CONTEXT.md): generators operate on owned games that are not
//! hidden, not junk, and not non-game app types (Application / Tool / DLC).

use crate::models::{Game, SteamAppType};

/// Options for the base eligibility filter.
#[derive(Clone, Copy, Debug, Default)]
pub struct EligibilityOptions {
    /// When true, only games with zero/unknown playtime pass.
    pub unplayed_only: bool,
    /// When true, only installed games pass.
    pub installed_only: bool,
}

/// True when the app type is not a playable game (Application, Tool, DLC).
pub fn is_non_game_type(game: &Game) -> bool {
    matches!(
        game.app_type,
        SteamAppType::Application | SteamAppType::Tool | SteamAppType::Dlc
    )
}

/// True when the game has no recorded playtime (or zero minutes).
pub fn is_unplayed(game: &Game) -> bool {
    game.playtime_minutes.is_none_or(|m| m == 0)
}

/// Base eligibility shared by Recommend, Discover, Editorial Mood, and Finish It.
///
/// Excludes hidden, junk, and non-game types. Optional flags apply unplayed /
/// installed-only constraints.
pub fn is_eligible(game: &Game, options: EligibilityOptions) -> bool {
    if game.is_hidden || game.is_junk {
        return false;
    }
    if is_non_game_type(game) {
        return false;
    }
    if options.unplayed_only && !is_unplayed(game) {
        return false;
    }
    if options.installed_only && !game.installed {
        return false;
    }
    true
}

/// Default eligibility for Recommend / Editorial Mood (all owned non-junk games).
pub fn is_generator_eligible(game: &Game) -> bool {
    is_eligible(game, EligibilityOptions::default())
}

/// Discover eligibility: base filter + unplayed only.
pub fn is_discover_eligible(game: &Game) -> bool {
    is_eligible(
        game,
        EligibilityOptions {
            unplayed_only: true,
            installed_only: false,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SteamAppType;
    use std::path::PathBuf;

    fn game(app_id: u32, app_type: SteamAppType) -> Game {
        Game {
            app_id,
            name: format!("g{app_id}"),
            app_type,
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
    fn base_excludes_non_game_types() {
        let _ = PathBuf::new(); // keep import quiet if unused after edits
        let tool = game(1, SteamAppType::Tool);
        let app = game(2, SteamAppType::Application);
        let dlc = game(3, SteamAppType::Dlc);
        let playable = game(4, SteamAppType::Game);
        assert!(!is_generator_eligible(&tool));
        assert!(!is_generator_eligible(&app));
        assert!(!is_generator_eligible(&dlc));
        assert!(is_generator_eligible(&playable));
    }

    #[test]
    fn base_excludes_hidden_and_junk() {
        let mut g = game(10, SteamAppType::Game);
        g.is_hidden = true;
        assert!(!is_generator_eligible(&g));
        g.is_hidden = false;
        g.is_junk = true;
        assert!(!is_generator_eligible(&g));
    }

    #[test]
    fn unplayed_and_installed_flags() {
        let mut g = game(20, SteamAppType::Game);
        g.playtime_minutes = Some(120);
        assert!(!is_discover_eligible(&g));
        g.playtime_minutes = Some(0);
        assert!(is_discover_eligible(&g));
        g.installed = false;
        assert!(!is_eligible(
            &g,
            EligibilityOptions {
                unplayed_only: false,
                installed_only: true,
            }
        ));
    }

    /// Generators must agree that Tool/DLC/Application are excluded.
    #[test]
    fn generators_agree_on_non_game_exclusions() {
        use crate::discover::generate_discover_playlist;
        use crate::dynamic::{DynamicTemplate, DynamicTemplateOptions, compile_dynamic_template};
        use crate::models::{HltbData, HltbSource, PlaylistContent, RecommendRequest};
        use crate::mood::{EditorialMood, compile_editorial_mood};
        use crate::recommend::recommend;

        let mut tool = game(9001, SteamAppType::Tool);
        tool.playtime_minutes = Some(60);
        tool.hltb = Some(HltbData {
            main_story_seconds: Some(3600),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });
        // Finish It needs playtime/HLTB ratio in range — still must exclude Tool.
        tool.playtime_minutes = Some(60);

        let games = vec![tool];
        let rec = recommend(
            &games,
            &RecommendRequest {
                available_minutes: 120,
                count: 10,
                deck_mode: false,
                include_installed_only: false,
                seed: Some(1),
                exclude_collections: vec![],
            },
        );
        assert!(rec.is_empty(), "recommend must exclude Tool");

        let disc = generate_discover_playlist(
            &games,
            &crate::discover::DiscoverOptions {
                seed_app_id: None,
                count: 10,
            },
        );
        match disc.playlist.content {
            PlaylistContent::Manual { app_ids } => {
                assert!(app_ids.is_empty(), "discover must exclude Tool");
            }
            _ => panic!("expected manual"),
        }

        let mood = compile_editorial_mood(EditorialMood::QuickRound, &games, 10);
        match mood.playlist.content {
            PlaylistContent::Manual { app_ids } => {
                assert!(app_ids.is_empty(), "mood must exclude Tool");
            }
            _ => panic!("expected manual"),
        }

        let finish = compile_dynamic_template(
            DynamicTemplate::FinishIt,
            &games,
            &DynamicTemplateOptions {
                session_minutes: 90,
                count: 25,
            },
        );
        match finish.playlist.content {
            PlaylistContent::Manual { app_ids } => {
                assert!(
                    app_ids.is_empty(),
                    "Finish It must exclude Tool (parity with other generators)"
                );
            }
            _ => panic!("expected manual"),
        }
    }
}
