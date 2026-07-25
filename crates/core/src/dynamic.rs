//! Dynamic collection templates that compile to Vapourfly playlists.

use crate::models::{
    Game, Playlist, PlaylistContent, PlaylistFile, PlaylistRule, ProtonTier,
    VAPOURFLY_PLAYLIST_SCHEMA,
};

/// Built-in dynamic collection templates.
///
/// `PlaylistRadio` was removed (ADR-0005): Discover-with-seed covers the
/// entire "seed-based similar picks" surface and does it better. `Mood` was
/// replaced by Editorial Moods (ADR-0004); see [`crate::mood`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicTemplate {
    DeckSession,
    FinishIt,
}

impl DynamicTemplate {
    pub fn id(self) -> &'static str {
        match self {
            Self::DeckSession => "deck-session",
            Self::FinishIt => "finish-it",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::DeckSession => "Deck Session",
            Self::FinishIt => "Finish It",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "deck-session" | "deck_session" | "decksession" => Some(Self::DeckSession),
            "finish-it" | "finish_it" | "finishit" => Some(Self::FinishIt),
            _ => None,
        }
    }
}

/// Parameters for compiling a dynamic template.
#[derive(Clone, Debug, Default)]
pub struct DynamicTemplateOptions {
    /// Session length for Deck Session (minutes).
    pub session_minutes: u32,
    /// Maximum games for manual templates.
    pub count: usize,
}

impl DynamicTemplateOptions {
    pub fn with_defaults() -> Self {
        Self {
            session_minutes: 90,
            count: 25,
        }
    }
}

/// Compile a dynamic template into a playlist file.
///
/// `DeckSession` returns a rule playlist; `FinishIt` evaluates the current
/// library and returns an explicit AppID list.
pub fn compile_dynamic_template(
    template: DynamicTemplate,
    games: &[Game],
    options: &DynamicTemplateOptions,
) -> PlaylistFile {
    match template {
        DynamicTemplate::DeckSession => deck_session_playlist(options),
        DynamicTemplate::FinishIt => finish_it_playlist(games, options),
    }
}

fn deck_session_playlist(options: &DynamicTemplateOptions) -> PlaylistFile {
    let minutes = options.session_minutes.max(15);
    PlaylistFile {
        vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
        created_by: "vapourfly".into(),
        playlist: Playlist {
            id: DynamicTemplate::DeckSession.id().into(),
            name: format!("Deck Session ({minutes}m)"),
            description: format!(
                "Installed Steam Deck-friendly games that fit a {minutes}-minute session"
            ),
            content: PlaylistContent::Rules {
                rules: vec![
                    PlaylistRule::Installed,
                    PlaylistRule::NotHidden,
                    PlaylistRule::NotJunk,
                    PlaylistRule::ProtonAtLeast {
                        tier: ProtonTier::Gold,
                    },
                    PlaylistRule::ControllerSupportFull,
                    PlaylistRule::HltbMaxMinutes { minutes },
                ],
            },
        },
    }
}

fn finish_it_playlist(games: &[Game], options: &DynamicTemplateOptions) -> PlaylistFile {
    let mut app_ids: Vec<u32> = games
        .iter()
        .filter(|g| crate::eligibility::is_generator_eligible(g))
        .filter_map(|game| {
            let playtime = game.playtime_minutes?;
            if playtime == 0 {
                return None;
            }
            let main_secs = crate::signal::main_story_seconds(game)?;
            let main_mins = main_secs / 60;
            if main_mins == 0 {
                return None;
            }
            let ratio = playtime as f32 / main_mins as f32;
            if (0.5..=1.25).contains(&ratio) {
                Some(game.app_id)
            } else {
                None
            }
        })
        .collect();

    app_ids.sort_unstable();
    app_ids.truncate(options.count);

    PlaylistFile {
        vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
        created_by: "vapourfly".into(),
        playlist: Playlist {
            id: DynamicTemplate::FinishIt.id().into(),
            name: "Finish It".into(),
            description: "Started games that look close to the main-story finish line".into(),
            content: PlaylistContent::Manual { app_ids },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{HltbData, HltbSource};

    fn make_game(app_id: u32, name: &str) -> Game {
        Game {
            app_id,
            name: name.into(),
            app_type: crate::models::SteamAppType::Game,
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
    fn deck_session_is_rule_playlist() {
        let pf = compile_dynamic_template(
            DynamicTemplate::DeckSession,
            &[],
            &DynamicTemplateOptions {
                session_minutes: 60,
                ..DynamicTemplateOptions::with_defaults()
            },
        );
        match pf.playlist.content {
            PlaylistContent::Rules { rules } => {
                assert!(rules.contains(&PlaylistRule::ControllerSupportFull));
            }
            _ => panic!("expected rules"),
        }
    }

    #[test]
    fn finish_it_selects_near_complete_games() {
        let mut almost_done = make_game(1, "Almost");
        almost_done.playtime_minutes = Some(90);
        almost_done.hltb = Some(HltbData {
            main_story_seconds: Some(6000),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });

        let fresh = make_game(2, "Fresh");

        let pf = compile_dynamic_template(
            DynamicTemplate::FinishIt,
            &[almost_done, fresh],
            &DynamicTemplateOptions::with_defaults(),
        );

        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, vec![1]),
            _ => panic!("expected manual"),
        }
    }
}
