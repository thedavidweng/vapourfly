//! Scoring primitives — taste vector, taste overlap, and high-rating check.
//!
//! Deep module: owns the shared scoring primitives that [`recommend`] and
//! [`discover`] both depend on. Each caller applies its own weights and
//! thresholds; the signal extraction and normalization live here.
//!
//! ## Primitives
//!
//! - [`build_taste_vector`]: maps keywords to log-scaled playtime weights.
//! - [`taste_overlap`]: normalized overlap between a game's keywords and a
//!   taste vector (0.0–1.0).
//! - [`is_high_rating`]: true when RAWG rates ≥ 4.0 or IGDB rates ≥ 80,
//!   evaluated as an independent OR over the two sources.

use std::collections::HashMap;

use crate::models::Game;
use crate::signal;

// ---------------------------------------------------------------------------
// Taste vector
// ---------------------------------------------------------------------------

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
        // Skip hidden and junk
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

// ---------------------------------------------------------------------------
// Taste overlap
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// High rating
// ---------------------------------------------------------------------------

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
