//! Dynamic collection templates that compile to Vapourfly playlists.

use crate::models::{
    Game, Playlist, PlaylistContent, PlaylistFile, PlaylistRule, ProtonTier,
    VAPOURFLY_PLAYLIST_SCHEMA,
};

/// Built-in dynamic collection templates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicTemplate {
    DeckSession,
    FinishIt,
    Mood,
    PlaylistRadio,
}

impl DynamicTemplate {
    pub fn id(self) -> &'static str {
        match self {
            Self::DeckSession => "deck-session",
            Self::FinishIt => "finish-it",
            Self::Mood => "mood",
            Self::PlaylistRadio => "playlist-radio",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::DeckSession => "Deck Session",
            Self::FinishIt => "Finish It",
            Self::Mood => "Mood",
            Self::PlaylistRadio => "Playlist Radio",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "deck-session" | "deck_session" | "decksession" => Some(Self::DeckSession),
            "finish-it" | "finish_it" | "finishit" => Some(Self::FinishIt),
            "mood" => Some(Self::Mood),
            "playlist-radio" | "playlist_radio" | "playlistradio" | "radio" => {
                Some(Self::PlaylistRadio)
            }
            _ => None,
        }
    }
}

/// Parameters for compiling a dynamic template.
#[derive(Clone, Debug, Default)]
pub struct DynamicTemplateOptions {
    /// Session length for Deck Session (minutes).
    pub session_minutes: u32,
    /// Genre or tag filter for Mood templates.
    pub mood: Option<String>,
    /// Seed AppID for Playlist Radio.
    pub seed_app_id: Option<u32>,
    /// Maximum games for manual templates.
    pub count: usize,
}

impl DynamicTemplateOptions {
    pub fn with_defaults() -> Self {
        Self {
            session_minutes: 90,
            mood: None,
            seed_app_id: None,
            count: 25,
        }
    }
}

/// Compile a dynamic template into a playlist file.
///
/// Rule-based templates (`DeckSession`, `Mood`) return rule playlists.
/// Manual templates (`FinishIt`, `PlaylistRadio`) evaluate the current
/// library and return explicit AppID lists.
pub fn compile_dynamic_template(
    template: DynamicTemplate,
    games: &[Game],
    options: &DynamicTemplateOptions,
) -> PlaylistFile {
    match template {
        DynamicTemplate::DeckSession => deck_session_playlist(options),
        DynamicTemplate::FinishIt => finish_it_playlist(games, options),
        DynamicTemplate::Mood => mood_playlist(options),
        DynamicTemplate::PlaylistRadio => playlist_radio_playlist(games, options),
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

fn mood_playlist(options: &DynamicTemplateOptions) -> PlaylistFile {
    let mood = options
        .mood
        .clone()
        .filter(|m| !m.trim().is_empty())
        .unwrap_or_else(|| "Relaxing".into());

    PlaylistFile {
        vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
        created_by: "vapourfly".into(),
        playlist: Playlist {
            id: format!("mood-{}", crate::playlist::slugify(&mood)),
            name: format!("Mood: {mood}"),
            description: format!("Unplayed or low-playtime games tagged with '{mood}'"),
            content: PlaylistContent::Rules {
                rules: vec![
                    PlaylistRule::NotHidden,
                    PlaylistRule::NotJunk,
                    PlaylistRule::Or(vec![
                        PlaylistRule::HasGenre {
                            genre: mood.clone(),
                        },
                        PlaylistRule::HasTag { tag: mood },
                    ]),
                    PlaylistRule::PlaytimeBetween { min: 0, max: 120 },
                ],
            },
        },
    }
}

fn finish_it_playlist(games: &[Game], options: &DynamicTemplateOptions) -> PlaylistFile {
    let mut app_ids: Vec<u32> = games
        .iter()
        .filter(|g| !g.is_hidden && !g.is_junk)
        .filter_map(|game| {
            let playtime = game.playtime_minutes?;
            if playtime == 0 {
                return None;
            }
            let main_secs = game.hltb.as_ref().and_then(|h| h.main_story_seconds)?;
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

fn playlist_radio_playlist(games: &[Game], options: &DynamicTemplateOptions) -> PlaylistFile {
    let seed_id = options.seed_app_id.unwrap_or(0);
    let seed_name = games
        .iter()
        .find(|g| g.app_id == seed_id)
        .map_or("your library", |g| g.name.as_str());

    let similar_ids: std::collections::HashSet<u64> = games
        .iter()
        .find(|g| g.app_id == seed_id)
        .and_then(|g| g.igdb.as_ref())
        .map(|igdb| igdb.similar_game_ids.iter().copied().collect())
        .unwrap_or_default();

    let mut app_ids: Vec<u32> = games
        .iter()
        .filter(|g| !g.is_hidden && !g.is_junk)
        .filter(|g| g.playtime_minutes.is_none_or(|m| m == 0))
        .filter(|g| {
            g.igdb
                .as_ref()
                .is_some_and(|igdb| similar_ids.contains(&igdb.igdb_id))
        })
        .map(|g| g.app_id)
        .collect();

    if app_ids.is_empty() && seed_id != 0 {
        app_ids = games
            .iter()
            .filter(|g| !g.is_hidden && !g.is_junk)
            .filter(|g| g.playtime_minutes.is_none_or(|m| m == 0))
            .filter(|g| g.app_id != seed_id)
            .take(options.count)
            .map(|g| g.app_id)
            .collect();
    }

    app_ids.sort_unstable();
    app_ids.truncate(options.count);

    PlaylistFile {
        vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
        created_by: "vapourfly".into(),
        playlist: Playlist {
            id: format!("playlist-radio-{seed_id}"),
            name: format!("Playlist Radio: {seed_name}"),
            description: format!("Unplayed picks in the orbit of {seed_name}"),
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

    #[test]
    fn mood_template_parses_slug() {
        let pf = compile_dynamic_template(
            DynamicTemplate::Mood,
            &[],
            &DynamicTemplateOptions {
                mood: Some("Sci-Fi".into()),
                ..DynamicTemplateOptions::with_defaults()
            },
        );
        assert_eq!(pf.playlist.id, "mood-sci-fi");
    }
}
