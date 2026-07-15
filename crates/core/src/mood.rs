//! Editorial Moods — named, curated playlists with hidden selection criteria.
//!
//! Replaces the old tag-filter `Mood` dynamic template (ADR-0004). The user
//! sees evocative names ("Today's Biggest Hits", "Friday Party"); the
//! underlying logic is Vapourfly's editorial judgment, not a user-configured
//! filter. Each mood compiles to a Manual playlist (explicit AppID list)
//! evaluated against the current library.
//!
//! Criteria are computable from Vapourfly's available data: Steam Store,
//! IGDB, RAWG, ProtonDB, PCGW, HLTB, and local playtime. A game that lacks
//! the data needed by a mood's criteria is simply excluded — moods never
//! fail, they just produce shorter lists when data is sparse.

use crate::models::{Game, Playlist, PlaylistContent, PlaylistFile, VAPOURFLY_PLAYLIST_SCHEMA};
use crate::scoring;

// ---------------------------------------------------------------------------
// Mood catalogue
// ---------------------------------------------------------------------------

/// The seven canonical Editorial Moods.
///
/// Names are canonical English; localized display names are a UI/localization
/// concern, not part of the domain model. `parse` accepts the canonical id
/// (lowercase, hyphenated).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorialMood {
    TodaysBiggestHits,
    IndieRising,
    FridayParty,
    DeckGuardians,
    UnopenedTreasures,
    WeekendMarathon,
    QuickRound,
}

impl EditorialMood {
    /// Iterate over all canonical moods in catalogue order.
    pub fn all() -> &'static [EditorialMood] {
        &[
            EditorialMood::TodaysBiggestHits,
            EditorialMood::IndieRising,
            EditorialMood::FridayParty,
            EditorialMood::DeckGuardians,
            EditorialMood::UnopenedTreasures,
            EditorialMood::WeekendMarathon,
            EditorialMood::QuickRound,
        ]
    }

    /// Stable identifier used in CLI args, playlist IDs, and serialization.
    pub fn id(self) -> &'static str {
        match self {
            Self::TodaysBiggestHits => "todays-biggest-hits",
            Self::IndieRising => "indie-rising",
            Self::FridayParty => "friday-party",
            Self::DeckGuardians => "deck-guardians",
            Self::UnopenedTreasures => "unopened-treasures",
            Self::WeekendMarathon => "weekend-marathon",
            Self::QuickRound => "quick-round",
        }
    }

    /// Human-readable display name (canonical English).
    pub fn name(self) -> &'static str {
        match self {
            Self::TodaysBiggestHits => "Today's Biggest Hits",
            Self::IndieRising => "Indie Rising",
            Self::FridayParty => "Friday Party",
            Self::DeckGuardians => "Deck Guardians",
            Self::UnopenedTreasures => "Unopened Treasures",
            Self::WeekendMarathon => "Weekend Marathon",
            Self::QuickRound => "Quick Round",
        }
    }

    /// Short description shown alongside the name.
    pub fn description(self) -> &'static str {
        match self {
            Self::TodaysBiggestHits => "Owned games with a recent popularity surge",
            Self::IndieRising => "Indie games with high ratings and recent releases",
            Self::FridayParty => "Co-op, local multiplayer, and party games",
            Self::DeckGuardians => {
                "Steam Deck gold-or-better with full controller and short sessions"
            }
            Self::UnopenedTreasures => "Unplayed, highly rated, non-junk gems",
            Self::WeekendMarathon => "Unplayed, long, highly rated epics",
            Self::QuickRound => "Unplayed, short, non-junk picks",
        }
    }

    /// Parse a canonical id (case-insensitive, accepts `_` or `-` separators).
    pub fn parse(value: &str) -> Option<Self> {
        let normalized = value.to_ascii_lowercase().replace('_', "-");
        EditorialMood::all()
            .iter()
            .copied()
            .find(|m| m.id() == normalized)
    }
}

// ---------------------------------------------------------------------------
// Compilation
// ---------------------------------------------------------------------------

/// Compile an Editorial Mood into a Manual playlist evaluated against the
/// current library.
///
/// The resulting playlist has `created_by: "vapourfly"` and an id of the form
/// `mood-<canonical-id>`. AppIDs are sorted and deduplicated.
pub fn compile_editorial_mood(
    mood: EditorialMood,
    games: &[Game],
    max_count: usize,
) -> PlaylistFile {
    let mut app_ids: Vec<u32> = games
        .iter()
        .filter(|g| is_eligible(g))
        .filter(|g| matches_mood(mood, g))
        .map(|g| g.app_id)
        .collect();

    app_ids.sort_unstable();
    app_ids.dedup();
    app_ids.truncate(max_count);

    PlaylistFile {
        vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
        created_by: "vapourfly".into(),
        playlist: Playlist {
            id: format!("mood-{}", mood.id()),
            name: mood.name().into(),
            description: mood.description().into(),
            content: PlaylistContent::Manual { app_ids },
        },
    }
}

/// A game is eligible for any mood if it is a Game (not a Tool/DLC/Application),
/// not hidden, and not junk.
fn is_eligible(game: &Game) -> bool {
    crate::eligibility::is_generator_eligible(game)
}

/// Apply the hidden criteria for a specific mood.
fn matches_mood(mood: EditorialMood, game: &Game) -> bool {
    match mood {
        EditorialMood::TodaysBiggestHits => is_popularity_surge(game),
        EditorialMood::IndieRising => is_indie_rising(game),
        EditorialMood::FridayParty => is_party_game(game),
        EditorialMood::DeckGuardians => is_deck_guardian(game),
        EditorialMood::UnopenedTreasures => is_unopened_treasure(game),
        EditorialMood::WeekendMarathon => is_weekend_marathon(game),
        EditorialMood::QuickRound => is_quick_round(game),
    }
}

// ---------------------------------------------------------------------------
// Mood criteria
// ---------------------------------------------------------------------------

/// Today's Biggest Hits: on sale (discount_percent > 0).
///
/// "Rising player count" and "rising recent review activity" require data
/// sources Vapourfly does not currently cache; the on-sale signal is the
/// available proxy and is sufficient to produce a useful list. Games without
/// Steam Store price data are excluded.
fn is_popularity_surge(game: &Game) -> bool {
    game.steam_store
        .as_ref()
        .and_then(|s| s.price_overview.as_ref())
        .is_some_and(|p| p.discount_percent > 0)
}

/// Indie Rising: indie classification + high rating + recent release.
///
/// Indie is detected from IGDB themes/keywords containing "indie", or Steam
/// Store type "game" with "Indie" in genres. High rating is ≥ 4.0 on the
/// 0–5 scale (shared scoring primitive). "Recent" is approximated by
/// release_date being present and within the last 3 years; when no release
/// date is available we still accept the game if it is otherwise indie and
/// highly rated (degrade gracefully on missing data).
fn is_indie_rising(game: &Game) -> bool {
    if !is_indie(game) {
        return false;
    }
    if !scoring::is_high_rating(game) {
        return false;
    }
    is_recent_release(game)
}

/// Unplayed meaning shared with Discover (zero/unknown playtime).
fn is_unplayed(game: &Game) -> bool {
    crate::eligibility::is_unplayed(game)
}

/// Detect indie classification from IGDB themes/keywords or Steam Store genres.
fn is_indie(game: &Game) -> bool {
    if let Some(igdb) = &game.igdb {
        let indie_in_themes = igdb
            .themes
            .iter()
            .any(|t| t.to_ascii_lowercase().contains("indie"));
        let indie_in_keywords = igdb
            .keywords
            .iter()
            .any(|k| k.to_ascii_lowercase().contains("indie"));
        if indie_in_themes || indie_in_keywords {
            return true;
        }
    }
    if let Some(store) = &game.steam_store
        && store
            .genres
            .iter()
            .any(|g| g.to_ascii_lowercase().contains("indie"))
    {
        return true;
    }
    false
}

/// A release is "recent" if its Steam Store release_date parses to a year
/// within the last 3 years. When the date is missing or unparseable, we
/// accept the game (degrade gracefully — missing data should not exclude a
/// candidate that is otherwise indie and highly rated).
fn is_recent_release(game: &Game) -> bool {
    let Some(store) = &game.steam_store else {
        return true;
    };
    let Some(date_str) = &store.release_date else {
        return true;
    };
    match parse_release_year(date_str) {
        Some(year) => {
            let now_year = chrono::Utc::now()
                .format("%Y")
                .to_string()
                .parse::<i32>()
                .unwrap_or(0);
            now_year.saturating_sub(year) <= 3
        }
        None => true,
    }
}

/// Extract a 4-digit year from a Steam Store release date string.
///
/// Steam dates come in many formats ("1 Jan, 2020", "Jan 1, 2020", "2020",
/// "Q1 2020"). We just scan for the first 4-digit number in a plausible year
/// range.
fn parse_release_year(s: &str) -> Option<i32> {
    let mut digits = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if digits.len() == 4 {
            break;
        } else {
            digits.clear();
        }
    }
    if digits.len() == 4 {
        let year: i32 = digits.parse().ok()?;
        if (1970..=2100).contains(&year) {
            return Some(year);
        }
    }
    None
}

/// Friday Party: Steam Store categories include Co-op, Local Multiplayer, or
/// Party. Games without Steam Store data are excluded.
fn is_party_game(game: &Game) -> bool {
    let Some(store) = &game.steam_store else {
        return false;
    };
    let lower: Vec<String> = store
        .categories
        .iter()
        .map(|c| c.to_ascii_lowercase())
        .collect();
    lower.iter().any(|c| {
        c.contains("co-op")
            || c.contains("coop")
            || c.contains("local multiplayer")
            || c.contains("party")
            || c.contains("shared/split screen")
    })
}

/// Deck Guardians: ProtonDB Platinum or Gold + full controller support + short
/// HLTB main story (≤ 4 hours).
fn is_deck_guardian(game: &Game) -> bool {
    use crate::models::ProtonTier;
    let proton_ok = game
        .protondb
        .as_ref()
        .is_some_and(|p| matches!(p.tier, ProtonTier::Platinum | ProtonTier::Gold));
    if !proton_ok {
        return false;
    }
    use crate::models::ControllerSupport;
    let controller_ok = game
        .pcgw
        .as_ref()
        .is_some_and(|p| p.controller_support == ControllerSupport::Full);
    if !controller_ok {
        return false;
    }
    game.hltb
        .as_ref()
        .and_then(|h| h.main_story_seconds)
        .is_some_and(|secs| secs <= 4 * 3600)
}

/// Unopened Treasures: unplayed + high rating + not junk.
fn is_unopened_treasure(game: &Game) -> bool {
    is_unplayed(game) && scoring::is_high_rating(game)
}

/// Weekend Marathon: unplayed + long HLTB (≥ 20 hours) + high rating.
fn is_weekend_marathon(game: &Game) -> bool {
    if !is_unplayed(game) {
        return false;
    }
    if !scoring::is_high_rating(game) {
        return false;
    }
    game.hltb
        .as_ref()
        .and_then(|h| h.main_story_seconds)
        .is_some_and(|secs| secs >= 20 * 3600)
}

/// Quick Round: unplayed + short HLTB (≤ 4 hours) + not junk.
fn is_quick_round(game: &Game) -> bool {
    if !is_unplayed(game) {
        return false;
    }
    game.hltb
        .as_ref()
        .and_then(|h| h.main_story_seconds)
        .is_some_and(|secs| secs <= 4 * 3600)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        ControllerSupport, HltbData, HltbSource, IgdbData, PcgwData, PriceOverview, ProtonDbData,
        ProtonTier, RawgData, SteamAppType, SteamStoreDetails, SteamStorePlatforms,
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
            steam_store: None,
        }
    }

    fn igdb_with(theme: &str, rating: Option<f32>) -> IgdbData {
        IgdbData {
            igdb_id: 1,
            name: "X".into(),
            slug: None,
            rating_0_100: rating,
            total_rating_0_100: None,
            genres: vec![],
            themes: vec![theme.into()],
            keywords: vec![],
            similar_game_ids: vec![],
            steam_app_id_confirmed: false,
            time_to_beat: None,
        }
    }

    fn rawg_with(rating: Option<f32>) -> RawgData {
        RawgData {
            rawg_id: 1,
            rating_0_5: rating,
            ratings_count: Some(10),
            genres: vec![],
            tags: vec![],
            stores: vec![],
        }
    }

    fn store_with(
        categories: Vec<&str>,
        genres: Vec<&str>,
        discount: u32,
        date: Option<&str>,
    ) -> SteamStoreDetails {
        SteamStoreDetails {
            app_id: 1,
            name: "X".into(),
            steam_store_type: "game".into(),
            is_free: false,
            short_description: None,
            header_image: None,
            developers: vec![],
            publishers: vec![],
            genres: genres.into_iter().map(String::from).collect(),
            categories: categories.into_iter().map(String::from).collect(),
            release_date: date.map(String::from),
            metacritic_score: None,
            platforms: SteamStorePlatforms {
                windows: true,
                mac: false,
                linux: false,
            },
            coming_soon: false,
            price_overview: Some(PriceOverview {
                currency: "USD".into(),
                initial_price_cents: 1999,
                final_price_cents: 1999,
                discount_percent: discount,
            }),
        }
    }

    #[test]
    fn parse_accepts_canonical_and_underscore_ids() {
        assert_eq!(
            EditorialMood::parse("todays-biggest-hits"),
            Some(EditorialMood::TodaysBiggestHits)
        );
        assert_eq!(
            EditorialMood::parse("todays_biggest_hits"),
            Some(EditorialMood::TodaysBiggestHits)
        );
        assert_eq!(
            EditorialMood::parse("Quick-Round"),
            Some(EditorialMood::QuickRound)
        );
        assert_eq!(EditorialMood::parse("unknown"), None);
    }

    #[test]
    fn all_returns_seven_moods() {
        assert_eq!(EditorialMood::all().len(), 7);
    }

    #[test]
    fn todays_biggest_hits_requires_discount() {
        let mut on_sale = make_game(1, "On Sale");
        on_sale.steam_store = Some(store_with(vec![], vec![], 50, None));
        let mut not_on_sale = make_game(2, "Full Price");
        not_on_sale.steam_store = Some(store_with(vec![], vec![], 0, None));
        let no_store = make_game(3, "No Store");

        let pf = compile_editorial_mood(
            EditorialMood::TodaysBiggestHits,
            &[on_sale, not_on_sale, no_store],
            25,
        );
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, vec![1]),
            _ => panic!("expected manual"),
        }
    }

    #[test]
    fn indie_rising_requires_indie_and_high_rating() {
        let mut indie_good = make_game(1, "Indie Gem");
        indie_good.igdb = Some(igdb_with("Indie", Some(85.0)));
        let mut indie_low = make_game(2, "Indie Meh");
        indie_low.igdb = Some(igdb_with("Indie", Some(50.0)));
        let mut aaa_good = make_game(3, "AAA Hit");
        aaa_good.igdb = Some(igdb_with("Action", Some(90.0)));

        let pf = compile_editorial_mood(
            EditorialMood::IndieRising,
            &[indie_good, indie_low, aaa_good],
            25,
        );
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, vec![1]),
            _ => panic!("expected manual"),
        }
    }

    #[test]
    fn friday_party_matches_coop_category() {
        let mut coop = make_game(1, "Coop");
        coop.steam_store = Some(store_with(vec!["Co-op"], vec![], 0, None));
        let mut party = make_game(2, "Party");
        party.steam_store = Some(store_with(vec!["Party"], vec![], 0, None));
        let mut solo = make_game(3, "Solo");
        solo.steam_store = Some(store_with(vec!["Single-player"], vec![], 0, None));

        let pf = compile_editorial_mood(EditorialMood::FridayParty, &[coop, party, solo], 25);
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, vec![1, 2]),
            _ => panic!("expected manual"),
        }
    }

    #[test]
    fn deck_guardians_requires_protondb_controller_and_short_hltb() {
        let mut good = make_game(1, "Deck Gem");
        good.protondb = Some(ProtonDbData {
            tier: ProtonTier::Gold,
            confidence: None,
            score: None,
        });
        good.pcgw = Some(PcgwData {
            page_name: None,
            controller_support: ControllerSupport::Full,
            steam_deck_notes: None,
            fixes_url: None,
        });
        good.hltb = Some(HltbData {
            main_story_seconds: Some(3 * 3600),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });

        let mut too_long = make_game(2, "Long");
        too_long.protondb = Some(ProtonDbData {
            tier: ProtonTier::Platinum,
            confidence: None,
            score: None,
        });
        too_long.pcgw = Some(PcgwData {
            page_name: None,
            controller_support: ControllerSupport::Full,
            steam_deck_notes: None,
            fixes_url: None,
        });
        too_long.hltb = Some(HltbData {
            main_story_seconds: Some(30 * 3600),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });

        let pf = compile_editorial_mood(EditorialMood::DeckGuardians, &[good, too_long], 25);
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, vec![1]),
            _ => panic!("expected manual"),
        }
    }

    #[test]
    fn unopened_treasures_requires_unplayed_high_rating() {
        let mut gem = make_game(1, "Gem");
        gem.rawg = Some(rawg_with(Some(4.5)));
        let mut played = make_game(2, "Played");
        played.playtime_minutes = Some(500);
        played.rawg = Some(rawg_with(Some(4.5)));
        let mut low = make_game(3, "Low");
        low.rawg = Some(rawg_with(Some(2.0)));

        let pf = compile_editorial_mood(EditorialMood::UnopenedTreasures, &[gem, played, low], 25);
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, vec![1]),
            _ => panic!("expected manual"),
        }
    }

    #[test]
    fn weekend_marathon_requires_long_hltb() {
        let mut epic = make_game(1, "Epic");
        epic.rawg = Some(rawg_with(Some(4.5)));
        epic.hltb = Some(HltbData {
            main_story_seconds: Some(40 * 3600),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });
        let mut short = make_game(2, "Short");
        short.rawg = Some(rawg_with(Some(4.5)));
        short.hltb = Some(HltbData {
            main_story_seconds: Some(2 * 3600),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });

        let pf = compile_editorial_mood(EditorialMood::WeekendMarathon, &[epic, short], 25);
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, vec![1]),
            _ => panic!("expected manual"),
        }
    }

    #[test]
    fn quick_round_requires_short_hltb() {
        let mut quick = make_game(1, "Quick");
        quick.hltb = Some(HltbData {
            main_story_seconds: Some(2 * 3600),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });
        let mut long = make_game(2, "Long");
        long.hltb = Some(HltbData {
            main_story_seconds: Some(40 * 3600),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });

        let pf = compile_editorial_mood(EditorialMood::QuickRound, &[quick, long], 25);
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, vec![1]),
            _ => panic!("expected manual"),
        }
    }

    #[test]
    fn hidden_and_junk_games_are_excluded() {
        let mut hidden = make_game(1, "Hidden");
        hidden.is_hidden = true;
        hidden.hltb = Some(HltbData {
            main_story_seconds: Some(3600),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });
        let mut junk = make_game(2, "Junk");
        junk.is_junk = true;
        junk.hltb = Some(HltbData {
            main_story_seconds: Some(3600),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });

        let pf = compile_editorial_mood(EditorialMood::QuickRound, &[hidden, junk], 25);
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => assert!(app_ids.is_empty()),
            _ => panic!("expected manual"),
        }
    }

    #[test]
    fn compile_truncates_to_max_count() {
        let games: Vec<Game> = (0..10)
            .map(|i| {
                let mut g = make_game(i, "G");
                g.hltb = Some(HltbData {
                    main_story_seconds: Some(3600),
                    main_extra_seconds: None,
                    completionist_seconds: None,
                    source: HltbSource::HltbScrape,
                });
                g
            })
            .collect();

        let pf = compile_editorial_mood(EditorialMood::QuickRound, &games, 3);
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids.len(), 3),
            _ => panic!("expected manual"),
        }
    }

    #[test]
    fn playlist_id_and_name_use_mood() {
        let pf = compile_editorial_mood(EditorialMood::QuickRound, &[], 25);
        assert_eq!(pf.playlist.id, "mood-quick-round");
        assert_eq!(pf.playlist.name, "Quick Round");
        assert_eq!(pf.created_by, "vapourfly");
    }

    #[test]
    fn parse_release_year_handles_common_formats() {
        assert_eq!(parse_release_year("1 Jan, 2020"), Some(2020));
        assert_eq!(parse_release_year("Jan 1, 2020"), Some(2020));
        assert_eq!(parse_release_year("2020"), Some(2020));
        assert_eq!(parse_release_year("Q1 2020"), Some(2020));
        assert_eq!(parse_release_year("not a date"), None);
    }
}
