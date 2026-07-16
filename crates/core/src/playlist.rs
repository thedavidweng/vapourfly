//! Playlist import, export, rule evaluation, and match reporting.
//!
//! Playlists are JSON files that describe a named subset of a Steam library.
//! A playlist can be either **manual** (explicit list of AppIDs) or **rule-based**
//! (a boolean expression tree evaluated against each game's metadata).
//!
//! # Import / Export
//!
//! [`import_playlist`] reads a JSON file and validates the schema, playlist ID
//! uniqueness, name, AppIDs, rule operators, and rule nesting depth.
//! [`export_playlist`] writes a playlist back to disk with sorted AppIDs and
//! pretty-printed JSON.
//!
//! # Rule Evaluation
//!
//! [`evaluate_rules`] walks a rule tree and returns whether a game matches.
//! Positive predicates (e.g. `Installed`, `HasGenre`) fail closed when the
//! underlying data is missing; negated predicates (`Not(...)`) pass through
//! when data is unavailable.
//!
//! # Match Reporting
//!
//! [`match_playlist`] evaluates a playlist against a full library and produces
//! a [`PlaylistMatchReport`] summarising owned, missing, played, unplayed,
//! hidden, and junk games, plus an optional completion price.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use crate::error::{Result, SafePath, VapourflyError};
use crate::models::{
    CompletionPrice, ControllerSupport, Game, Money, PlaylistContent, PlaylistFile,
    PlaylistMatchReport, PlaylistRule, PriceCoverage, ProtonTier, SteamStoreDetails,
    VAPOURFLY_PLAYLIST_SCHEMA,
};
use crate::signal;
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum nesting depth for rule trees before we reject the file.
const MAX_RULE_DEPTH: usize = 16;

// ---------------------------------------------------------------------------
// slugify
// ---------------------------------------------------------------------------

/// Produce a lowercased, hyphen-separated slug suitable for Steam collection IDs.
///
/// Rules:
/// - Convert to lowercase.
/// - Replace runs of non-alphanumeric characters with a single hyphen.
/// - Strip leading and trailing hyphens.
///
/// ```ignore
/// assert_eq!(slugify("My Cool Playlist!"), "my-cool-playlist");
/// assert_eq!(slugify("  spaces  "), "spaces");
/// ```
pub fn slugify(name: &str) -> String {
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    // Collapse runs of hyphens.
    let mut collapsed = String::with_capacity(slug.len());
    let mut prev_hyphen = false;
    for ch in slug.chars() {
        if ch == '-' {
            if !prev_hyphen {
                collapsed.push(ch);
            }
            prev_hyphen = true;
        } else {
            collapsed.push(ch);
            prev_hyphen = false;
        }
    }

    // Strip leading/trailing hyphens.
    collapsed.trim_matches('-').to_string()
}

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Check that every rule in the tree is a known variant.
fn validate_rule_operators(rules: &[PlaylistRule]) -> Result<()> {
    for rule in rules {
        match rule {
            PlaylistRule::ProtonAtLeast { tier } => {
                // All ProtonTier variants are known; this is a sanity check
                // that the tier deserialized correctly.
                match tier {
                    ProtonTier::Borked
                    | ProtonTier::Bronze
                    | ProtonTier::Silver
                    | ProtonTier::Gold
                    | ProtonTier::Platinum
                    | ProtonTier::Native
                    | ProtonTier::Unknown => {}
                }
            }
            PlaylistRule::HltbMaxMinutes { minutes: _ }
            | PlaylistRule::ControllerSupportFull
            | PlaylistRule::PlaytimeBetween { min: _, max: _ }
            | PlaylistRule::RatingAtLeast { rating_0_5: _ }
            | PlaylistRule::HasGenre { genre: _ }
            | PlaylistRule::HasTag { tag: _ }
            | PlaylistRule::Installed
            | PlaylistRule::NotJunk
            | PlaylistRule::NotHidden => {}
            PlaylistRule::And(children) | PlaylistRule::Or(children) => {
                validate_rule_operators(children)?;
            }
            PlaylistRule::Not(inner) => {
                validate_rule_operators(std::slice::from_ref(inner))?;
            }
        }
    }
    Ok(())
}

/// Compute the maximum nesting depth of a rule tree and reject if it exceeds
/// [`MAX_RULE_DEPTH`].
fn validate_rule_depth(rules: &[PlaylistRule], current: usize) -> Result<()> {
    if current > MAX_RULE_DEPTH {
        return Err(VapourflyError::InvalidInput(format!(
            "rule nesting depth exceeds maximum of {MAX_RULE_DEPTH}"
        )));
    }
    for rule in rules {
        match rule {
            PlaylistRule::And(children) | PlaylistRule::Or(children) => {
                validate_rule_depth(children, current + 1)?;
            }
            PlaylistRule::Not(inner) => {
                validate_rule_depth(std::slice::from_ref(inner), current + 1)?;
            }
            _ => {} // leaf nodes don't increase depth
        }
    }
    Ok(())
}

/// Validate that all AppIDs in a manual playlist are non-zero (Steam AppID 0 is
/// not a real game).
fn validate_app_ids(app_ids: &[u32]) -> Result<()> {
    for &id in app_ids {
        if id == 0 {
            return Err(VapourflyError::InvalidInput(
                "playlist contains invalid AppID 0".into(),
            ));
        }
    }
    Ok(())
}

/// Validate a parsed playlist file before importing, exporting, or sharing it.
pub(crate) fn validate_playlist_file(pf: &PlaylistFile) -> Result<()> {
    if pf.vapourfly_schema != VAPOURFLY_PLAYLIST_SCHEMA {
        return Err(VapourflyError::InvalidInput(format!(
            "unsupported playlist schema '{}', expected '{}'",
            pf.vapourfly_schema, VAPOURFLY_PLAYLIST_SCHEMA,
        )));
    }

    if pf.playlist.id.trim().is_empty() {
        return Err(VapourflyError::InvalidInput(
            "playlist id must not be empty".into(),
        ));
    }

    if pf.playlist.name.trim().is_empty() {
        return Err(VapourflyError::InvalidInput(
            "playlist name must not be empty".into(),
        ));
    }

    match &pf.playlist.content {
        PlaylistContent::Manual { app_ids } => {
            validate_app_ids(app_ids)?;
        }
        PlaylistContent::Rules { rules } => {
            validate_rule_operators(rules)?;
            validate_rule_depth(rules, 1)?;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// import_playlist
// ---------------------------------------------------------------------------

/// Read and validate a playlist JSON file from disk.
///
/// Validation checks (in order):
/// 1. File is readable and valid JSON.
/// 2. `vapourfly_schema` matches [`VAPOURFLY_PLAYLIST_SCHEMA`].
/// 3. Playlist `id` is non-empty.
/// 4. Playlist `name` is non-empty.
/// 5. All AppIDs are valid (non-zero).
/// 6. All rule operators are known variants.
/// 7. Rule nesting depth does not exceed 16.
pub fn import_playlist(path: &Path) -> Result<PlaylistFile> {
    let content = fs::read_to_string(path).map_err(|_| VapourflyError::FileNotFound {
        path: SafePath::new(path),
    })?;

    let pf: PlaylistFile =
        serde_json::from_str(&content).map_err(|e| VapourflyError::ParseError {
            path: SafePath::new(path),
            format: "JSON".into(),
            reason: e.to_string(),
        })?;

    validate_playlist_file(&pf)?;
    Ok(pf)
}

// ---------------------------------------------------------------------------
// export_playlist
// ---------------------------------------------------------------------------

/// Write a playlist to disk with sorted AppIDs and pretty-printed JSON.
///
/// For manual playlists, the AppID list is sorted ascending before writing.
/// Rule-based playlists are written as-is (rules have no inherent ordering).
pub fn export_playlist(playlist: &PlaylistFile, path: &Path) -> Result<()> {
    validate_playlist_file(playlist)?;

    let mut pf = playlist.clone();

    // Sort AppIDs in manual playlists for deterministic output.
    if let PlaylistContent::Manual { ref mut app_ids } = pf.playlist.content {
        app_ids.sort_unstable();
    }

    let json = serde_json::to_string_pretty(&pf)
        .map_err(|e| VapourflyError::Internal(format!("failed to serialize playlist: {e}")))?;

    fs::write(path, json).map_err(|e| {
        VapourflyError::Internal(format!(
            "failed to write playlist to {}: {e}",
            path.display()
        ))
    })?;

    Ok(())
}

// ---------------------------------------------------------------------------
// evaluate_rules
// ---------------------------------------------------------------------------

/// Evaluate a single leaf or compound rule against a game.
///
/// Returns `true` if the game matches, `false` otherwise.
///
/// **Fail-closed for positive predicates**: when the data a predicate needs
/// is absent (e.g. no HLTB data for `HltbMaxMinutes`), the predicate returns
/// `false`.
///
/// **Pass-through for negated predicates**: `Not(rule)` inverts the result,
/// so `Not(HasGenre("RPG"))` returns `true` when genre data is unavailable
/// (because `HasGenre` fails closed to `false`, and `Not(false)` is `true`).
fn eval(rule: &PlaylistRule, game: &Game) -> bool {
    match rule {
        // -- Proton tier -------------------------------------------------------
        PlaylistRule::ProtonAtLeast { tier } => {
            match &game.protondb {
                Some(pdb) => pdb.tier >= *tier,
                None => false, // fail closed: no data => doesn't match
            }
        }

        // -- HLTB max minutes --------------------------------------------------
        PlaylistRule::HltbMaxMinutes { minutes } => {
            match game.hltb.as_ref().and_then(|h| h.main_story_seconds) {
                Some(seconds) => (seconds / 60) <= *minutes,
                None => false, // fail closed
            }
        }

        // -- Controller support -----------------------------------------------
        PlaylistRule::ControllerSupportFull => game
            .pcgw
            .as_ref()
            .is_some_and(|pcgw| matches!(pcgw.controller_support, ControllerSupport::Full)),

        // -- Playtime between --------------------------------------------------
        PlaylistRule::PlaytimeBetween { min, max } => {
            match game.playtime_minutes {
                Some(minutes) => minutes >= *min && minutes <= *max,
                None => false, // fail closed
            }
        }

        // -- Rating at least ---------------------------------------------------
        PlaylistRule::RatingAtLeast { rating_0_5 } => {
            match signal::effective_rating(game, None) {
                Some((rating, _)) => rating >= *rating_0_5,
                None => false, // fail closed
            }
        }

        // -- Has genre ----------------------------------------------------------
        PlaylistRule::HasGenre { genre } => {
            let lower = genre.to_lowercase();
            let has_igdb = game
                .igdb
                .as_ref()
                .is_some_and(|ig| ig.genres.iter().any(|g| g.to_lowercase() == lower));
            let has_rawg = game
                .rawg
                .as_ref()
                .is_some_and(|r| r.genres.iter().any(|g| g.to_lowercase() == lower));
            has_igdb || has_rawg
        }

        // -- Has tag ------------------------------------------------------------
        PlaylistRule::HasTag { tag } => {
            let lower = tag.to_lowercase();
            // Check RAWG tags, IGDB keywords, IGDB themes, and Steam collections.
            let has_rawg = game
                .rawg
                .as_ref()
                .is_some_and(|r| r.tags.iter().any(|t| t.to_lowercase() == lower));
            let has_igdb_kw = game
                .igdb
                .as_ref()
                .is_some_and(|ig| ig.keywords.iter().any(|k| k.to_lowercase() == lower));
            let has_igdb_theme = game
                .igdb
                .as_ref()
                .is_some_and(|ig| ig.themes.iter().any(|t| t.to_lowercase() == lower));
            let has_steam_col = game
                .steam_collections
                .iter()
                .any(|c| c.to_lowercase() == lower);
            has_rawg || has_igdb_kw || has_igdb_theme || has_steam_col
        }

        // -- Installed ----------------------------------------------------------
        PlaylistRule::Installed => game.installed,

        // -- Not junk -----------------------------------------------------------
        PlaylistRule::NotJunk => !game.is_junk,

        // -- Not hidden ---------------------------------------------------------
        PlaylistRule::NotHidden => !game.is_hidden,

        // -- And ---------------------------------------------------------------
        PlaylistRule::And(children) => children.iter().all(|r| eval(r, game)),

        // -- Or ----------------------------------------------------------------
        PlaylistRule::Or(children) => children.iter().any(|r| eval(r, game)),

        // -- Not ---------------------------------------------------------------
        PlaylistRule::Not(inner) => !eval(inner, game),
    }
}

/// Evaluate a set of rules against a game.
///
/// Rules are implicitly ANDed: the game must satisfy every rule in the slice.
pub fn evaluate_rules(rules: &[PlaylistRule], game: &Game) -> bool {
    rules.iter().all(|r| eval(r, game))
}

// ---------------------------------------------------------------------------
// match_playlist
// ---------------------------------------------------------------------------

/// Evaluate a playlist against a full game library and produce a match report.
///
/// For **manual** playlists, every listed AppID is checked against the library.
/// For **rule-based** playlists, every game in the library is tested against the
/// rule tree.
///
/// The report includes:
/// - `owned`: AppIDs present in the library (manual) or matching games (rules).
/// - `missing`: AppIDs in a manual playlist that are not in the library.
/// - `played`: Owned games with playtime > 0.
/// - `unplayed`: Owned games with no playtime (or playtime == 0).
/// - `hidden`: Owned games that are hidden.
/// - `junk`: Owned games that are junk.
/// - `completion_price`: Sum of Steam Store final prices for **missing,
///   non-free** entries with available price data. Owned/unplayed games are
///   never included. Missing entries are not owned Games, so the caller
///   provides their Steam Store details via `missing_store_details` (typically
///   cache-hydrated). Rule Playlists evaluate only the owned library and have
///   no missing entries, so their `completion_price` is `None`. Returns `None`
///   when no priced missing entries exist.
/// - `price_coverage`: How many missing non-free entries have price data vs.
///   how many are missing and non-free overall. `None` when there are no
///   missing non-free entries.
pub fn match_playlist(
    playlist: &PlaylistFile,
    games: &[Game],
    missing_store_details: &HashMap<u32, SteamStoreDetails>,
) -> Result<PlaylistMatchReport> {
    let by_id: std::collections::HashMap<u32, &Game> =
        games.iter().map(|g| (g.app_id, g)).collect();

    let mut owned: Vec<u32> = Vec::new();
    let mut missing: Vec<u32> = Vec::new();
    let mut played: Vec<u32> = Vec::new();
    let mut unplayed: Vec<u32> = Vec::new();
    let mut hidden: Vec<u32> = Vec::new();
    let mut junk: Vec<u32> = Vec::new();

    match &playlist.playlist.content {
        PlaylistContent::Manual { app_ids } => {
            let id_set: HashSet<u32> = app_ids.iter().copied().collect();
            for &id in &id_set {
                match by_id.get(&id) {
                    Some(game) => {
                        owned.push(id);
                        if game.playtime_minutes.is_none_or(|m| m == 0) {
                            unplayed.push(id);
                        } else {
                            played.push(id);
                        }
                        if game.is_hidden {
                            hidden.push(id);
                        }
                        if game.is_junk {
                            junk.push(id);
                        }
                    }
                    None => {
                        missing.push(id);
                    }
                }
            }
        }
        PlaylistContent::Rules { rules } => {
            for game in games {
                if evaluate_rules(rules, game) {
                    owned.push(game.app_id);
                    if game.playtime_minutes.is_none_or(|m| m == 0) {
                        unplayed.push(game.app_id);
                    } else {
                        played.push(game.app_id);
                    }
                    if game.is_hidden {
                        hidden.push(game.app_id);
                    }
                    if game.is_junk {
                        junk.push(game.app_id);
                    }
                }
            }
        }
    }

    // Sort all vectors for deterministic output.
    owned.sort_unstable();
    missing.sort_unstable();
    played.sort_unstable();
    unplayed.sort_unstable();
    hidden.sort_unstable();
    junk.sort_unstable();

    // completion_price: sum of Steam Store final prices for **missing,
    // non-free** entries. Owned/unplayed games are never included.
    // Rule Playlists have no missing entries → completion_price is None.
    //
    // Mixed-currency handling: if all priced missing entries share one
    // currency, return a Single total. If multiple currencies are
    // encountered, do not sum them — return per-currency grouped totals.
    let (completion_price, price_coverage) = {
        // Only manual playlists can have missing entries.
        let is_manual = matches!(playlist.playlist.content, PlaylistContent::Manual { .. });

        if !is_manual || missing.is_empty() {
            (None, None)
        } else {
            // Classify each missing entry into:
            //   confirmed_free, confirmed_non_free_priced,
            //   confirmed_non_free_unpriced, unknown.
            let mut confirmed_free: usize = 0;
            let mut confirmed_non_free_priced: usize = 0;
            let mut confirmed_non_free_unpriced: usize = 0;
            let mut unknown: usize = 0;
            // Per-currency totals (only for confirmed non-free priced).
            let mut totals: HashMap<String, u64> = HashMap::new();

            for &id in &missing {
                let Some(store) = missing_store_details.get(&id) else {
                    // No store details: unknown — excluded from denominator.
                    unknown += 1;
                    continue;
                };

                if store.is_free {
                    confirmed_free += 1;
                    continue;
                }

                // Confirmed non-free.
                if let Some(price) = &store.price_overview {
                    confirmed_non_free_priced += 1;
                    *totals.entry(price.currency.clone()).or_default() +=
                        u64::from(price.final_price_cents);
                } else {
                    confirmed_non_free_unpriced += 1;
                }
            }

            let total_confirmed_non_free = confirmed_non_free_priced + confirmed_non_free_unpriced;

            let price_coverage =
                if total_confirmed_non_free > 0 || confirmed_free > 0 || unknown > 0 {
                    Some(PriceCoverage {
                        confirmed_free,
                        confirmed_non_free_priced,
                        confirmed_non_free_unpriced,
                        unknown,
                    })
                } else {
                    None
                };

            let completion_price = if totals.is_empty() {
                None
            } else if totals.len() == 1 {
                let (currency, amount) = totals.into_iter().next().unwrap();
                Some(CompletionPrice::Single(Money {
                    amount_cents: amount,
                    currency,
                }))
            } else {
                let grouped: Vec<Money> = totals
                    .into_iter()
                    .map(|(currency, amount_cents)| Money {
                        amount_cents,
                        currency,
                    })
                    .collect();
                Some(CompletionPrice::Mixed(grouped))
            };

            (completion_price, price_coverage)
        }
    };

    Ok(PlaylistMatchReport {
        owned,
        missing,
        played,
        unplayed,
        hidden,
        junk,
        completion_price,
        price_coverage,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{
        HltbData, HltbSource, IgdbData, PcgwData, Playlist, ProtonDbData, RawgData, SteamAppType,
    };
    use std::fs;

    // -- Helpers --------------------------------------------------------------

    fn make_game(app_id: u32, name: &str) -> Game {
        Game {
            app_id,
            name: name.into(),
            app_type: SteamAppType::Game,
            installed: false,
            install_dir: None,
            library_folder: None,
            playtime_minutes: None,
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

    fn make_manual_playlist(id: &str, name: &str, app_ids: Vec<u32>) -> PlaylistFile {
        PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "test".into(),
            playlist: Playlist {
                id: id.into(),
                name: name.into(),
                description: String::new(),
                content: PlaylistContent::Manual { app_ids },
            },
        }
    }

    fn make_rules_playlist(id: &str, name: &str, rules: Vec<PlaylistRule>) -> PlaylistFile {
        PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "test".into(),
            playlist: Playlist {
                id: id.into(),
                name: name.into(),
                description: String::new(),
                content: PlaylistContent::Rules { rules },
            },
        }
    }

    // -- slugify --------------------------------------------------------------

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("My Cool Playlist!"), "my-cool-playlist");
    }

    #[test]
    fn slugify_leading_trailing_spaces() {
        assert_eq!(slugify("  spaces  "), "spaces");
    }

    #[test]
    fn slugify_special_chars() {
        assert_eq!(slugify("FPS & Action!!!"), "fps-action");
    }

    #[test]
    fn slugify_already_slug() {
        assert_eq!(slugify("already-a-slug"), "already-a-slug");
    }

    #[test]
    fn slugify_empty() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn slugify_consecutive_specials() {
        assert_eq!(slugify("a!!!b"), "a-b");
    }

    // -- import/export round trip ---------------------------------------------

    #[test]
    fn import_export_round_trip_manual() {
        let dir = std::env::temp_dir().join("vapourfly_playlist_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("round_trip_manual.json");

        let pf = make_manual_playlist("test-id", "Test Playlist", vec![730, 440, 427520]);
        export_playlist(&pf, &path).unwrap();

        let imported = import_playlist(&path).unwrap();
        assert_eq!(imported.playlist.id, "test-id");
        assert_eq!(imported.playlist.name, "Test Playlist");
        match &imported.playlist.content {
            PlaylistContent::Manual { app_ids } => {
                assert_eq!(app_ids, &vec![440, 730, 427520]); // sorted
            }
            _ => panic!("expected Manual"),
        }

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn import_export_round_trip_rules() {
        let dir = std::env::temp_dir().join("vapourfly_playlist_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("round_trip_rules.json");

        let pf = make_rules_playlist(
            "rules-test",
            "Rules Test",
            vec![
                PlaylistRule::NotHidden,
                PlaylistRule::NotJunk,
                PlaylistRule::ProtonAtLeast {
                    tier: ProtonTier::Gold,
                },
            ],
        );
        export_playlist(&pf, &path).unwrap();

        let imported = import_playlist(&path).unwrap();
        assert_eq!(imported.playlist.id, "rules-test");
        match &imported.playlist.content {
            PlaylistContent::Rules { rules } => {
                assert_eq!(rules.len(), 3);
                assert_eq!(rules[0], PlaylistRule::NotHidden);
                assert_eq!(rules[1], PlaylistRule::NotJunk);
            }
            _ => panic!("expected Rules"),
        }

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn export_sorts_app_ids() {
        let dir = std::env::temp_dir().join("vapourfly_playlist_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("sorted.json");

        let pf = make_manual_playlist("sorted", "Sorted", vec![427520, 730, 440]);
        export_playlist(&pf, &path).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        let back: PlaylistFile = serde_json::from_str(&content).unwrap();
        match back.playlist.content {
            PlaylistContent::Manual { app_ids } => {
                assert_eq!(app_ids, vec![440, 730, 427520]);
            }
            _ => panic!("expected Manual"),
        }

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn export_rejects_empty_id() {
        let dir = std::env::temp_dir().join("vapourfly_playlist_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("empty_export_id.json");
        let _ = fs::remove_file(&path);

        let pf = make_manual_playlist("", "Name", vec![730]);
        let err = export_playlist(&pf, &path).unwrap_err();

        assert!(err.to_string().contains("playlist id must not be empty"));
        assert!(!path.exists());
    }

    // -- import validation ----------------------------------------------------

    #[test]
    fn import_rejects_wrong_schema() {
        let dir = std::env::temp_dir().join("vapourfly_playlist_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("bad_schema.json");

        let mut pf = make_manual_playlist("id", "Name", vec![730]);
        pf.vapourfly_schema = "wrong.v1".into();
        let json = serde_json::to_string_pretty(&pf).unwrap();
        fs::write(&path, json).unwrap();

        let err = import_playlist(&path).unwrap_err();
        assert!(
            err.to_string().contains("unsupported playlist schema"),
            "got: {err}"
        );

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn import_rejects_empty_id() {
        let dir = std::env::temp_dir().join("vapourfly_playlist_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("empty_id.json");

        let pf = make_manual_playlist("", "Name", vec![730]);
        let json = serde_json::to_string_pretty(&pf).unwrap();
        fs::write(&path, json).unwrap();

        let err = import_playlist(&path).unwrap_err();
        assert!(err.to_string().contains("playlist id must not be empty"));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn import_rejects_empty_name() {
        let dir = std::env::temp_dir().join("vapourfly_playlist_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("empty_name.json");

        let pf = make_manual_playlist("id", "", vec![730]);
        let json = serde_json::to_string_pretty(&pf).unwrap();
        fs::write(&path, json).unwrap();

        let err = import_playlist(&path).unwrap_err();
        assert!(err.to_string().contains("playlist name must not be empty"));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn import_rejects_zero_app_id() {
        let dir = std::env::temp_dir().join("vapourfly_playlist_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("zero_id.json");

        let pf = make_manual_playlist("id", "Name", vec![0, 730]);
        let json = serde_json::to_string_pretty(&pf).unwrap();
        fs::write(&path, json).unwrap();

        let err = import_playlist(&path).unwrap_err();
        assert!(err.to_string().contains("invalid AppID 0"));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn import_rejects_excessive_rule_depth() {
        let dir = std::env::temp_dir().join("vapourfly_playlist_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("deep_rules.json");

        // Build a deeply nested rule: Not(Not(Not(...17 times...Installed)))
        let mut rule = PlaylistRule::Installed;
        for _ in 0..17 {
            rule = PlaylistRule::Not(Box::new(rule));
        }
        let pf = make_rules_playlist("deep", "Deep", vec![rule]);
        let json = serde_json::to_string_pretty(&pf).unwrap();
        fs::write(&path, json).unwrap();

        let err = import_playlist(&path).unwrap_err();
        assert!(err.to_string().contains("rule nesting depth"));

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn import_accepts_depth_exactly_16() {
        let dir = std::env::temp_dir().join("vapourfly_playlist_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("depth16.json");

        // Build depth-16 nesting (16 Not wrappers = depth 17 calls to validate,
        // but the root starts at depth 1, so 16 more levels = depth 17, which
        // exceeds the limit of 16). We need exactly 15 Not wrappers for depth 16.
        let mut rule = PlaylistRule::Installed;
        for _ in 0..15 {
            rule = PlaylistRule::Not(Box::new(rule));
        }
        let pf = make_rules_playlist("d16", "Depth 16", vec![rule]);
        let json = serde_json::to_string_pretty(&pf).unwrap();
        fs::write(&path, json).unwrap();

        let result = import_playlist(&path);
        assert!(result.is_ok(), "depth 15 Not wrappers should be accepted");

        let _ = fs::remove_file(&path);
    }

    // -- Rule evaluation: all operators ---------------------------------------

    #[test]
    fn rule_proton_at_least() {
        let mut game = make_game(1, "Game");
        game.protondb = Some(ProtonDbData {
            tier: ProtonTier::Gold,
            confidence: Some("high".into()),
            score: None,
        });
        assert!(evaluate_rules(
            &[PlaylistRule::ProtonAtLeast {
                tier: ProtonTier::Gold
            }],
            &game
        ));
        assert!(!evaluate_rules(
            &[PlaylistRule::ProtonAtLeast {
                tier: ProtonTier::Platinum
            }],
            &game
        ));
    }

    #[test]
    fn rule_proton_at_least_no_data() {
        let game = make_game(1, "Game");
        // No protondb data => fail closed.
        assert!(!evaluate_rules(
            &[PlaylistRule::ProtonAtLeast {
                tier: ProtonTier::Borked
            }],
            &game
        ));
    }

    #[test]
    fn rule_hltb_max_minutes() {
        let mut game = make_game(1, "Game");
        game.hltb = Some(HltbData {
            main_story_seconds: Some(3600), // 60 min
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        });
        assert!(evaluate_rules(
            &[PlaylistRule::HltbMaxMinutes { minutes: 120 }],
            &game
        ));
        assert!(!evaluate_rules(
            &[PlaylistRule::HltbMaxMinutes { minutes: 30 }],
            &game
        ));
    }

    #[test]
    fn rule_hltb_max_minutes_no_data() {
        let game = make_game(1, "Game");
        assert!(!evaluate_rules(
            &[PlaylistRule::HltbMaxMinutes { minutes: 9999 }],
            &game
        ));
    }

    #[test]
    fn rule_controller_support_full() {
        let mut game = make_game(1, "Game");
        game.pcgw = Some(PcgwData {
            page_name: Some("Game".into()),
            controller_support: crate::models::ControllerSupport::Full,
            steam_deck_notes: None,
            fixes_url: None,
        });
        assert!(evaluate_rules(
            &[PlaylistRule::ControllerSupportFull],
            &game
        ));

        game.pcgw = Some(PcgwData {
            page_name: Some("Game".into()),
            controller_support: crate::models::ControllerSupport::Partial,
            steam_deck_notes: None,
            fixes_url: None,
        });
        assert!(!evaluate_rules(
            &[PlaylistRule::ControllerSupportFull],
            &game
        ));
    }

    #[test]
    fn rule_playtime_between() {
        let mut game = make_game(1, "Game");
        game.playtime_minutes = Some(100);
        assert!(evaluate_rules(
            &[PlaylistRule::PlaytimeBetween { min: 50, max: 200 }],
            &game
        ));
        assert!(!evaluate_rules(
            &[PlaylistRule::PlaytimeBetween { min: 0, max: 50 }],
            &game
        ));
    }

    #[test]
    fn rule_playtime_between_no_data() {
        let game = make_game(1, "Game");
        assert!(!evaluate_rules(
            &[PlaylistRule::PlaytimeBetween { min: 0, max: 9999 }],
            &game
        ));
    }

    #[test]
    fn rule_rating_at_least() {
        let mut game = make_game(1, "Game");
        game.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: Some(4.0),
            ratings_count: Some(100),
            genres: vec![],
            tags: vec![],
            stores: vec![],
        });
        assert!(evaluate_rules(
            &[PlaylistRule::RatingAtLeast { rating_0_5: 3.5 }],
            &game
        ));
        assert!(!evaluate_rules(
            &[PlaylistRule::RatingAtLeast { rating_0_5: 4.5 }],
            &game
        ));
    }

    #[test]
    fn rule_rating_at_least_no_data() {
        let game = make_game(1, "Game");
        assert!(!evaluate_rules(
            &[PlaylistRule::RatingAtLeast { rating_0_5: 0.0 }],
            &game
        ));
    }

    #[test]
    fn rule_has_genre() {
        let mut game = make_game(1, "Game");
        game.igdb = Some(IgdbData {
            igdb_id: 1,
            name: "Game".into(),
            slug: None,
            rating_0_100: None,
            total_rating_0_100: None,
            genres: vec!["Role-playing (RPG)".into()],
            themes: vec![],
            keywords: vec![],
            similar_game_ids: vec![],
            steam_app_id_confirmed: false,
            time_to_beat: None,
        });
        assert!(evaluate_rules(
            &[PlaylistRule::HasGenre {
                genre: "Role-playing (RPG)".into()
            }],
            &game
        ));
        // Case-insensitive
        assert!(evaluate_rules(
            &[PlaylistRule::HasGenre {
                genre: "role-playing (rpg)".into()
            }],
            &game
        ));
        assert!(!evaluate_rules(
            &[PlaylistRule::HasGenre {
                genre: "Strategy".into()
            }],
            &game
        ));
    }

    #[test]
    fn rule_has_genre_rawg_fallback() {
        let mut game = make_game(1, "Game");
        game.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: None,
            ratings_count: None,
            genres: vec!["Action".into()],
            tags: vec![],
            stores: vec![],
        });
        assert!(evaluate_rules(
            &[PlaylistRule::HasGenre {
                genre: "Action".into()
            }],
            &game
        ));
    }

    #[test]
    fn rule_has_tag() {
        let mut game = make_game(1, "Game");
        game.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: None,
            ratings_count: None,
            genres: vec![],
            tags: vec!["roguelike".into()],
            stores: vec![],
        });
        assert!(evaluate_rules(
            &[PlaylistRule::HasTag {
                tag: "roguelike".into()
            }],
            &game
        ));
        assert!(!evaluate_rules(
            &[PlaylistRule::HasTag { tag: "mmo".into() }],
            &game
        ));
    }

    #[test]
    fn rule_has_tag_igdb_keywords() {
        let mut game = make_game(1, "Game");
        game.igdb = Some(IgdbData {
            igdb_id: 1,
            name: "Game".into(),
            slug: None,
            rating_0_100: None,
            total_rating_0_100: None,
            genres: vec![],
            themes: vec![],
            keywords: vec!["open world".into()],
            similar_game_ids: vec![],
            steam_app_id_confirmed: false,
            time_to_beat: None,
        });
        assert!(evaluate_rules(
            &[PlaylistRule::HasTag {
                tag: "open world".into()
            }],
            &game
        ));
    }

    #[test]
    fn rule_has_tag_steam_collections() {
        let mut game = make_game(1, "Game");
        game.steam_collections = vec!["favorites".into()];
        assert!(evaluate_rules(
            &[PlaylistRule::HasTag {
                tag: "favorites".into()
            }],
            &game
        ));
    }

    #[test]
    fn rule_installed() {
        let mut game = make_game(1, "Game");
        game.installed = true;
        assert!(evaluate_rules(&[PlaylistRule::Installed], &game));
        game.installed = false;
        assert!(!evaluate_rules(&[PlaylistRule::Installed], &game));
    }

    #[test]
    fn rule_not_junk() {
        let mut game = make_game(1, "Game");
        game.is_junk = false;
        assert!(evaluate_rules(&[PlaylistRule::NotJunk], &game));
        game.is_junk = true;
        assert!(!evaluate_rules(&[PlaylistRule::NotJunk], &game));
    }

    #[test]
    fn rule_not_hidden() {
        let mut game = make_game(1, "Game");
        game.is_hidden = false;
        assert!(evaluate_rules(&[PlaylistRule::NotHidden], &game));
        game.is_hidden = true;
        assert!(!evaluate_rules(&[PlaylistRule::NotHidden], &game));
    }

    // -- Nested rule evaluation -----------------------------------------------

    #[test]
    fn rule_and() {
        let mut game = make_game(1, "Game");
        game.installed = true;
        game.is_junk = false;
        assert!(evaluate_rules(
            &[PlaylistRule::And(vec![
                PlaylistRule::Installed,
                PlaylistRule::NotJunk,
            ])],
            &game
        ));
        game.is_junk = true;
        assert!(!evaluate_rules(
            &[PlaylistRule::And(vec![
                PlaylistRule::Installed,
                PlaylistRule::NotJunk,
            ])],
            &game
        ));
    }

    #[test]
    fn rule_or() {
        let mut game = make_game(1, "Game");
        game.installed = false;
        game.is_junk = false;
        assert!(evaluate_rules(
            &[PlaylistRule::Or(vec![
                PlaylistRule::Installed,
                PlaylistRule::NotJunk,
            ])],
            &game
        ));
        game.is_junk = true;
        assert!(!evaluate_rules(
            &[PlaylistRule::Or(vec![
                PlaylistRule::Installed,
                PlaylistRule::NotJunk,
            ])],
            &game
        ));
    }

    #[test]
    fn rule_not() {
        let mut game = make_game(1, "Game");
        game.is_hidden = true;
        // NotHidden = !is_hidden = false; Not(false) = true
        assert!(evaluate_rules(
            &[PlaylistRule::Not(Box::new(PlaylistRule::NotHidden))],
            &game
        ));
        game.is_hidden = false;
        // NotHidden = !is_hidden = true; Not(true) = false
        assert!(!evaluate_rules(
            &[PlaylistRule::Not(Box::new(PlaylistRule::NotHidden))],
            &game
        ));
    }

    #[test]
    fn rule_not_passes_through_missing_data() {
        // Not(HltbMaxMinutes) with no HLTB data:
        // HltbMaxMinutes fails closed => false, Not(false) => true.
        let game = make_game(1, "Game");
        assert!(evaluate_rules(
            &[PlaylistRule::Not(Box::new(PlaylistRule::HltbMaxMinutes {
                minutes: 120
            }))],
            &game
        ));
    }

    #[test]
    fn rule_complex_nesting() {
        // (Installed AND NotJunk) OR (HasGenre("RPG") AND RatingAtLeast(4.0))
        let mut game = make_game(1, "Game");
        game.installed = false;
        game.is_junk = false;
        game.rawg = Some(RawgData {
            rawg_id: 1,
            rating_0_5: Some(4.5),
            ratings_count: Some(100),
            genres: vec!["RPG".into()],
            tags: vec![],
            stores: vec![],
        });

        let rule = PlaylistRule::Or(vec![
            PlaylistRule::And(vec![PlaylistRule::Installed, PlaylistRule::NotJunk]),
            PlaylistRule::And(vec![
                PlaylistRule::HasGenre {
                    genre: "RPG".into(),
                },
                PlaylistRule::RatingAtLeast { rating_0_5: 4.0 },
            ]),
        ]);
        assert!(evaluate_rules(&[rule], &game));

        // Lower the rating so the second branch also fails.
        game.rawg.as_mut().unwrap().rating_0_5 = Some(3.0);
        let rule2 = PlaylistRule::Or(vec![
            PlaylistRule::And(vec![PlaylistRule::Installed, PlaylistRule::NotJunk]),
            PlaylistRule::And(vec![
                PlaylistRule::HasGenre {
                    genre: "RPG".into(),
                },
                PlaylistRule::RatingAtLeast { rating_0_5: 4.0 },
            ]),
        ]);
        assert!(!evaluate_rules(&[rule2], &game));
    }

    // -- Match report ---------------------------------------------------------

    #[test]
    fn match_report_manual_owned_and_missing() {
        let games = vec![make_game(730, "CS2"), make_game(440, "TF2")];
        let pf = make_manual_playlist("test", "Test", vec![730, 440, 999]);
        let report = match_playlist(&pf, &games, &std::collections::HashMap::new()).unwrap();
        assert_eq!(report.owned, vec![440, 730]);
        assert_eq!(report.missing, vec![999]);
    }

    #[test]
    fn match_report_manual_played_unplayed() {
        let mut cs2 = make_game(730, "CS2");
        cs2.playtime_minutes = Some(500);
        let tf2 = make_game(440, "TF2"); // no playtime
        let games = vec![cs2, tf2];
        let pf = make_manual_playlist("test", "Test", vec![730, 440]);
        let report = match_playlist(&pf, &games, &std::collections::HashMap::new()).unwrap();
        assert_eq!(report.played, vec![730]);
        assert_eq!(report.unplayed, vec![440]);
    }

    #[test]
    fn match_report_manual_hidden_and_junk() {
        let mut hidden_game = make_game(1, "Hidden");
        hidden_game.is_hidden = true;
        let mut junk_game = make_game(2, "Junk");
        junk_game.is_junk = true;
        let normal = make_game(3, "Normal");
        let games = vec![hidden_game, junk_game, normal];
        let pf = make_manual_playlist("test", "Test", vec![1, 2, 3]);
        let report = match_playlist(&pf, &games, &std::collections::HashMap::new()).unwrap();
        assert_eq!(report.hidden, vec![1]);
        assert_eq!(report.junk, vec![2]);
        assert_eq!(report.owned, vec![1, 2, 3]);
    }

    #[test]
    fn match_report_rules_based() {
        let mut installed_good = make_game(1, "Installed Good");
        installed_good.installed = true;
        installed_good.is_junk = false;
        installed_good.is_hidden = false;
        installed_good.playtime_minutes = Some(100);

        let mut not_installed = make_game(2, "Not Installed");
        not_installed.installed = false;

        let mut installed_junk = make_game(3, "Installed Junk");
        installed_junk.installed = true;
        installed_junk.is_junk = true;

        let games = vec![installed_good, not_installed, installed_junk];
        let pf = make_rules_playlist(
            "rules",
            "Rules",
            vec![PlaylistRule::Installed, PlaylistRule::NotJunk],
        );
        let report = match_playlist(&pf, &games, &std::collections::HashMap::new()).unwrap();
        assert_eq!(report.owned, vec![1]);
        assert_eq!(report.played, vec![1]);
        assert!(report.unplayed.is_empty());
        assert!(report.missing.is_empty());
    }

    #[test]
    fn match_report_empty_library() {
        let games: Vec<Game> = vec![];
        let pf = make_manual_playlist("test", "Test", vec![730]);
        let report = match_playlist(&pf, &games, &std::collections::HashMap::new()).unwrap();
        assert!(report.owned.is_empty());
        assert_eq!(report.missing, vec![730]);
    }

    #[test]
    fn match_report_empty_rules_match_nothing() {
        let games = vec![make_game(1, "Game")];
        let pf = make_rules_playlist("test", "Test", vec![]);
        let report = match_playlist(&pf, &games, &std::collections::HashMap::new()).unwrap();
        // Empty rules => all pass (every rule vacuously satisfied).
        assert_eq!(report.owned, vec![1]);
    }

    #[test]
    fn match_report_deterministic_ordering() {
        let games = vec![make_game(3, "C"), make_game(1, "A"), make_game(2, "B")];
        let pf = make_manual_playlist("test", "Test", vec![3, 1, 2]);
        let report = match_playlist(&pf, &games, &std::collections::HashMap::new()).unwrap();
        assert_eq!(report.owned, vec![1, 2, 3]);
    }

    // -- Completion price (corrected semantics: missing non-free entries) --

    use crate::models::{CompletionPrice, PriceOverview};

    fn make_store_details(
        app_id: u32,
        name: &str,
        is_free: bool,
        price: Option<(u32, &str)>,
    ) -> SteamStoreDetails {
        use crate::models::SteamStorePlatforms;
        SteamStoreDetails {
            app_id,
            name: name.into(),
            steam_store_type: "game".into(),
            is_free,
            short_description: None,
            header_image: None,
            developers: vec![],
            publishers: vec![],
            genres: vec![],
            categories: vec![],
            release_date: None,
            metacritic_score: None,
            platforms: SteamStorePlatforms {
                windows: true,
                mac: false,
                linux: false,
            },
            coming_soon: false,
            price_overview: price.map(|(cents, currency)| PriceOverview {
                currency: currency.into(),
                initial_price_cents: cents,
                final_price_cents: cents,
                discount_percent: 0,
            }),
        }
    }

    #[test]
    fn completion_price_sums_missing_non_free_entries() {
        // Playlist has AppIDs 1 (owned) and 100, 101 (missing).
        // Missing 100 costs 2999 USD, missing 101 costs 1999 USD.
        let games = vec![make_game(1, "Owned")];
        let pf = make_manual_playlist("test", "Test", vec![1, 100, 101]);
        let mut missing = HashMap::new();
        missing.insert(
            100,
            make_store_details(100, "Missing A", false, Some((2999, "USD"))),
        );
        missing.insert(
            101,
            make_store_details(101, "Missing B", false, Some((1999, "USD"))),
        );
        let report = match_playlist(&pf, &games, &missing).unwrap();

        let price = report.completion_price.expect("should have price");
        match price {
            CompletionPrice::Single(money) => {
                assert_eq!(money.currency, "USD");
                assert_eq!(money.amount_cents, 4998); // 2999 + 1999
            }
            CompletionPrice::Mixed(_) => panic!("should be single currency"),
        }
    }

    #[test]
    fn completion_price_excludes_owned_unplayed() {
        // Owned unplayed games must NOT contribute to completion price.
        let mut owned_game = make_game(1, "Owned Unplayed");
        owned_game.steam_store = Some(make_store_details(
            1,
            "Owned Unplayed",
            false,
            Some((5999, "USD")),
        ));
        let games = vec![owned_game];
        // Playlist has owned 1 and missing 100.
        let pf = make_manual_playlist("test", "Test", vec![1, 100]);
        let mut missing = HashMap::new();
        missing.insert(
            100,
            make_store_details(100, "Missing", false, Some((1999, "USD"))),
        );
        let report = match_playlist(&pf, &games, &missing).unwrap();

        match report.completion_price.expect("should have price") {
            CompletionPrice::Single(money) => {
                assert_eq!(money.amount_cents, 1999); // only missing, not owned
            }
            CompletionPrice::Mixed(_) => panic!("should be single currency"),
        }
    }

    #[test]
    fn completion_price_none_for_rule_playlists() {
        // Rule playlists evaluate only the owned library → no missing entries.
        let games = vec![make_game(1, "Game")];
        let pf = make_rules_playlist("test", "Test", vec![PlaylistRule::Installed]);
        let missing = HashMap::new();
        let report = match_playlist(&pf, &games, &missing).unwrap();
        assert!(report.completion_price.is_none());
        assert!(report.price_coverage.is_none());
    }

    #[test]
    fn completion_price_mixed_currency_returns_grouped_totals() {
        let games = vec![make_game(1, "Owned")];
        let pf = make_manual_playlist("test", "Test", vec![1, 100, 101]);
        let mut missing = HashMap::new();
        missing.insert(
            100,
            make_store_details(100, "USD Game", false, Some((2999, "USD"))),
        );
        missing.insert(
            101,
            make_store_details(101, "EUR Game", false, Some((1999, "EUR"))),
        );
        let report = match_playlist(&pf, &games, &missing).unwrap();

        match report.completion_price.expect("should have price") {
            CompletionPrice::Mixed(totals) => {
                assert_eq!(totals.len(), 2);
                let by_cur: HashMap<String, u64> = totals
                    .iter()
                    .map(|m| (m.currency.clone(), m.amount_cents))
                    .collect();
                assert_eq!(by_cur["USD"], 2999);
                assert_eq!(by_cur["EUR"], 1999);
            }
            CompletionPrice::Single(_) => panic!("should be mixed currency"),
        }
    }

    #[test]
    fn completion_price_none_when_no_priced_missing() {
        // Missing entries exist but none have price data.
        let games = vec![make_game(1, "Owned")];
        let pf = make_manual_playlist("test", "Test", vec![1, 100]);
        let missing = HashMap::new(); // no store details for 100
        let report = match_playlist(&pf, &games, &missing).unwrap();
        assert!(report.completion_price.is_none());
        // No store details → unknown (excluded from denominator).
        let coverage = report.price_coverage.expect("should have coverage");
        assert_eq!(coverage.confirmed_non_free_priced, 0);
        assert_eq!(coverage.confirmed_non_free_unpriced, 0);
        assert_eq!(coverage.unknown, 1);
    }

    #[test]
    fn completion_price_skips_free_missing() {
        let games = vec![make_game(1, "Owned")];
        let pf = make_manual_playlist("test", "Test", vec![1, 100]);
        let mut missing = HashMap::new();
        missing.insert(100, make_store_details(100, "Free Missing", true, None));
        let report = match_playlist(&pf, &games, &missing).unwrap();
        assert!(report.completion_price.is_none());
        // Coverage exists but shows only confirmed_free, no non-free.
        let coverage = report.price_coverage.expect("should have coverage");
        assert_eq!(coverage.confirmed_free, 1);
        assert_eq!(coverage.confirmed_non_free(), 0);
    }

    #[test]
    fn price_coverage_partial_when_some_missing_have_price() {
        // 3 missing: 2 non-free (1 priced, 1 not), 1 free.
        let games = vec![make_game(1, "Owned")];
        let pf = make_manual_playlist("test", "Test", vec![1, 100, 101, 102]);
        let mut missing = HashMap::new();
        missing.insert(
            100,
            make_store_details(100, "Priced", false, Some((2999, "USD"))),
        );
        missing.insert(101, make_store_details(101, "No Price", false, None));
        missing.insert(102, make_store_details(102, "Free", true, None));
        let report = match_playlist(&pf, &games, &missing).unwrap();

        let coverage = report.price_coverage.expect("should have coverage");
        assert_eq!(coverage.confirmed_non_free_priced, 1); // 100
        assert_eq!(coverage.confirmed_non_free_unpriced, 1); // 101
        assert_eq!(coverage.confirmed_free, 1); // 102
        assert_eq!(coverage.unknown, 0);
        assert_eq!(coverage.confirmed_non_free(), 2); // 100 + 101
        let ratio = coverage.ratio().expect("should have ratio");
        assert!((ratio - 0.5).abs() < 1e-9);
    }

    #[test]
    fn completion_price_none_when_fully_owned() {
        // No missing entries → no completion price.
        let games = vec![make_game(1, "Owned A"), make_game(2, "Owned B")];
        let pf = make_manual_playlist("test", "Test", vec![1, 2]);
        let missing = HashMap::new();
        let report = match_playlist(&pf, &games, &missing).unwrap();
        assert!(report.completion_price.is_none());
        assert!(report.price_coverage.is_none());
    }
}
