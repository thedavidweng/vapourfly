//! Shared cache/local personal attribution, normalization and deterministic variety.
//!
//! Factories own eligibility and request-specific terms. Legacy public taste-vector
//! helpers remain for callers; personal ranking uses the canonical Taste Profile.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::models::{Game, RecommendReason};
use crate::signal;
use crate::{preferences, progress, taste};

const RECENT_SECONDS: i64 = 14 * 86400;
const DIVERSITY_WEIGHT: f32 = 0.35;

pub(crate) fn contribution(
    reasons: &mut Vec<RecommendReason>,
    code: &str,
    description: String,
    weight: f32,
) {
    reasons.push(RecommendReason {
        code: code.into(),
        description,
        weight,
    });
}

fn title(profile: &taste::TasteProfile, app_id: u32) -> Option<&taste::SupportingTitle> {
    // The shared derivation emits title evidence in AppID order.
    profile
        .titles
        .binary_search_by_key(&app_id, |t| t.app_id)
        .ok()
        .map(|i| &profile.titles[i])
}

pub(crate) fn known_playtime(profile: &taste::TasteProfile, app_id: u32) -> Option<u32> {
    title(profile, app_id).and_then(|t| t.activity.iter().filter_map(|a| a.lifetime_minutes).max())
}

/// Shared attribution only: each factory owns eligibility and its request-specific terms.
pub(crate) fn personal_reasons(
    game: &Game,
    profile: &taste::TasteProfile,
    concepts: &BTreeSet<String>,
    progress: Option<&progress::Summary>,
    prefs: &preferences::Preferences,
    now: i64,
) -> Vec<RecommendReason> {
    let mut reasons = Vec::new();
    let title = title(profile, game.app_id);
    if let Some(weight) = quality(game) {
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
        let affinity = profile_affinity(concepts, tags);
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
    reasons
}

fn tie_key(seed: Option<u64>, app_id: u32) -> u64 {
    let Some(seed) = seed else {
        return u64::from(app_id);
    };
    let mut z = (seed ^ u64::from(app_id)).wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Stable greedy relevance-minus-overlap ordering; returns source indices and signed-penalty magnitudes.
pub(crate) fn diverse_order(
    scores: &[(u32, f32)],
    tags: &BTreeMap<u32, BTreeSet<String>>,
    count: usize,
    seed: Option<u64>,
) -> Vec<(usize, f32)> {
    let mut remaining: Vec<_> = (0..scores.len()).collect();
    let mut result: Vec<(usize, f32)> = Vec::new();
    while !remaining.is_empty() && result.len() < count {
        let penalty = |i: usize| {
            result
                .iter()
                .map(|(picked, _)| {
                    let a = &tags[&scores[i].0];
                    let b = &tags[&scores[*picked].0];
                    let union = a.union(b).count();
                    if union == 0 {
                        0.0
                    } else {
                        DIVERSITY_WEIGHT * a.intersection(b).count() as f32 / union as f32
                    }
                })
                .fold(0.0, f32::max)
        };
        let (slot, amount) = remaining
            .iter()
            .enumerate()
            .map(|(slot, i)| (slot, penalty(*i)))
            .max_by(|(ai, ap), (bi, bp)| {
                let a = scores[remaining[*ai]];
                let b = scores[remaining[*bi]];
                (a.1 - ap)
                    .total_cmp(&(b.1 - bp))
                    .then_with(|| tie_key(seed, b.0).cmp(&tie_key(seed, a.0)))
                    .then_with(|| b.0.cmp(&a.0))
            })
            .unwrap();
        result.push((remaining.swap_remove(slot), amount));
    }
    result
}

/// L1-normalized profile mass covered by this game's canonical concepts.
pub fn profile_affinity(tags: &BTreeSet<String>, strengths: &[crate::taste::TagStrength]) -> f32 {
    let total: f64 = strengths.iter().map(|t| t.strength).sum();
    if total == 0.0 {
        return 0.0;
    }
    (strengths
        .iter()
        .filter(|t| tags.contains(&t.tag))
        .map(|t| t.strength)
        .sum::<f64>()
        / total) as f32
}

/// Above-midpoint cached quality contributes at most one point; low/missing is neutral.
pub fn quality(game: &Game) -> Option<f32> {
    [
        game.rawg
            .as_ref()
            .and_then(|r| r.rating_0_5)
            .map(|r| r / 5.0),
        game.igdb
            .as_ref()
            .and_then(|i| i.rating_0_100.or(i.total_rating_0_100))
            .map(|r| r / 100.0),
    ]
    .into_iter()
    .flatten()
    .filter(|r| r.is_finite())
    .map(|r| r.clamp(0.0, 1.0))
    .max_by(f32::total_cmp)
    .map(|q| ((q - 0.5) * 2.0).max(0.0))
}

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
        if game.is_hidden || game.is_junk {
            continue;
        }

        let playtime = match game.playtime_minutes {
            Some(m) if m >= 60 => m as f32,
            _ => continue, // skip games with no meaningful playtime
        };

        let weight = (1.0 + playtime).ln();

        let keywords = signal::keywords_lower(game);
        for kw in keywords {
            *vector.entry(kw).or_insert(0.0) += weight;
        }
    }

    vector
}

/// Compute the normalized taste overlap between a game and a taste vector.
///
/// Returns a value in [0.0, 1.0] representing the fraction of the taste
/// vector's total weight that the game's keywords cover. Returns 0.0 when
/// the taste vector is empty or the game has no matching keywords.
pub fn taste_overlap(game: &Game, taste_vector: &HashMap<String, f32>) -> f32 {
    if taste_vector.is_empty() {
        return 0.0;
    }
    let total_taste: f32 = taste_vector.values().sum();
    if total_taste <= 0.0 {
        return 0.0;
    }
    let keywords = signal::keywords_lower(game);
    if keywords.is_empty() {
        return 0.0;
    }
    let overlap: f32 = keywords.iter().filter_map(|kw| taste_vector.get(kw)).sum();
    overlap / total_taste
}

/// Check whether a game is highly rated: RAWG ≥ 4.0 **or** IGDB ≥ 80.
///
/// The two sources are evaluated independently (PRD: "RAWG ≥4.0 或 IGDB ≥80"),
/// so a game whose RAWG rating is mediocre still qualifies when IGDB rates it
/// highly. This differs from [`signal::effective_rating`], which resolves a
/// single rating by source precedence.
pub fn is_high_rating(game: &Game) -> bool {
    let rawg_high = game
        .rawg
        .as_ref()
        .and_then(|r| r.rating_0_5)
        .is_some_and(|r| r >= 4.0);
    let igdb_high = game
        .igdb
        .as_ref()
        .and_then(|i| i.rating_0_100.or(i.total_rating_0_100))
        .is_some_and(|r| r >= 80.0);
    rawg_high || igdb_high
}

/// A "short game" fits a single short session: effective completion time ≤ 4 hours.
pub const SHORT_GAME_MAX_SECONDS: u32 = 4 * 3600;

/// A "long game" earns a marathon: effective completion time ≥ 20 hours.
pub const LONG_GAME_MIN_SECONDS: u32 = 20 * 3600;

/// True when the game's main story fits a short session (≤ 4 hours).
/// Fails closed when completion-time data is missing.
pub fn is_short_game(game: &Game) -> bool {
    signal::main_story_seconds(game).is_some_and(|s| s <= SHORT_GAME_MAX_SECONDS)
}

/// True when the game's main story is marathon-length (≥ 20 hours).
/// Fails closed when completion-time data is missing.
pub fn is_long_game(game: &Game) -> bool {
    signal::main_story_seconds(game).is_some_and(|s| s >= LONG_GAME_MIN_SECONDS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{HltbData, HltbSource, IgdbData, RawgData, SteamAppType};

    fn game(app_id: u32) -> Game {
        Game {
            app_id,
            name: format!("g{app_id}"),
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

    fn igdb_with(genres: &[&str], rating: Option<f32>) -> IgdbData {
        IgdbData {
            igdb_id: 1,
            name: "g".into(),
            slug: None,
            rating_0_100: rating,
            total_rating_0_100: None,
            genres: genres.iter().map(|s| s.to_string()).collect(),
            themes: vec![],
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
            ratings_count: None,
            genres: vec![],
            tags: vec![],
            stores: vec![],
        }
    }

    fn hltb_secs(secs: u32) -> HltbData {
        HltbData {
            main_story_seconds: Some(secs),
            main_extra_seconds: None,
            completionist_seconds: None,
            source: HltbSource::HltbScrape,
        }
    }

    #[test]
    fn taste_vector_skips_hidden_junk_and_low_playtime() {
        let mut liked = game(1);
        liked.playtime_minutes = Some(600);
        liked.igdb = Some(igdb_with(&["Strategy"], None));

        let mut hidden = game(2);
        hidden.playtime_minutes = Some(600);
        hidden.is_hidden = true;
        hidden.igdb = Some(igdb_with(&["Racing"], None));

        let mut barely_played = game(3);
        barely_played.playtime_minutes = Some(30);
        barely_played.igdb = Some(igdb_with(&["Puzzle"], None));

        let vector = build_taste_vector(&[liked, hidden, barely_played]);
        assert!(vector.contains_key("strategy"));
        assert!(
            !vector.contains_key("racing"),
            "hidden games contribute nothing"
        );
        assert!(
            !vector.contains_key("puzzle"),
            "sub-hour playtime contributes nothing"
        );
    }

    #[test]
    fn taste_overlap_is_normalized_fraction() {
        let mut liked = game(1);
        liked.playtime_minutes = Some(600);
        liked.igdb = Some(igdb_with(&["Strategy", "Roguelike"], None));
        let vector = build_taste_vector(std::slice::from_ref(&liked));

        let mut half_match = game(2);
        half_match.igdb = Some(igdb_with(&["Strategy"], None));
        let overlap = taste_overlap(&half_match, &vector);
        assert!(
            (overlap - 0.5).abs() < 1e-6,
            "one of two equal-weight keywords = 0.5"
        );

        let no_keywords = game(3);
        assert_eq!(taste_overlap(&no_keywords, &vector), 0.0);
        assert_eq!(taste_overlap(&half_match, &HashMap::new()), 0.0);
    }

    #[test]
    fn high_rating_is_independent_or_over_sources() {
        let mut igdb_only = game(1);
        igdb_only.igdb = Some(igdb_with(&[], Some(85.0)));
        igdb_only.rawg = Some(rawg_with(Some(2.0))); // mediocre RAWG must not veto
        assert!(is_high_rating(&igdb_only));

        let mut rawg_only = game(2);
        rawg_only.rawg = Some(rawg_with(Some(4.5)));
        assert!(is_high_rating(&rawg_only));

        let mut low = game(3);
        low.rawg = Some(rawg_with(Some(3.9)));
        low.igdb = Some(igdb_with(&[], Some(79.0)));
        assert!(!is_high_rating(&low));

        assert!(!is_high_rating(&game(4)), "no data fails closed");
    }

    #[test]
    fn session_length_predicates_fail_closed_at_boundaries() {
        let mut short = game(1);
        short.hltb = Some(hltb_secs(SHORT_GAME_MAX_SECONDS));
        assert!(is_short_game(&short));
        assert!(!is_long_game(&short));

        let mut long = game(2);
        long.hltb = Some(hltb_secs(LONG_GAME_MIN_SECONDS));
        assert!(is_long_game(&long));
        assert!(!is_short_game(&long));

        let no_data = game(3);
        assert!(!is_short_game(&no_data));
        assert!(!is_long_game(&no_data));
    }
}
