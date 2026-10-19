//! Reproducible, evidence-only Top Tags. Scores describe this library, not players.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::models::{Game, SteamAppType};
use crate::preferences::{self, Preferences};

pub const RECENT_SECONDS: i64 = 14 * 24 * 3600;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ActivityEvidence {
    pub source: String,
    pub observed_at: Option<i64>,
    pub lifetime_minutes: Option<u32>,
    pub recent_minutes: Option<u32>,
    pub last_played_at: Option<i64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TagKind {
    Genre,
    Descriptor,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SupportingTitle {
    pub app_id: u32,
    pub name: String,
    pub lifetime_minutes: u32,
    pub recent_minutes: u32,
    pub last_played_at: Option<i64>,
    pub interested: bool,
    pub metadata_sources: BTreeSet<String>,
    pub activity: Vec<ActivityEvidence>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TagStrength {
    pub tag: String,
    pub kind: TagKind,
    /// Fraction of engaged title weight carrying this concept (non-exclusive).
    pub strength: f64,
    /// Strength / metadata-covered library frequency. NOT a population percentile.
    pub library_relative_lift: f64,
    pub supporting_titles: Vec<SupportingTitle>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Coverage {
    pub eligible_titles: usize,
    pub metadata_titles: usize,
    pub activity_titles: usize,
    pub interested_titles: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TasteProfile {
    pub as_of: i64,
    pub coverage: Coverage,
    pub long_term: Vec<TagStrength>,
    pub recent: Vec<TagStrength>,
    pub explanation: String,
}

/// Conservative aliases: related but distinct concepts (e.g. RPG/action RPG) stay distinct.
pub fn canonical_tag(value: &str) -> String {
    let normalized = value
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    match normalized.as_str() {
        "role playing" | "role playing game" | "roleplaying" | "role playing rpg" | "rpg" => {
            "rpg".into()
        }
        "co op" | "coop" | "cooperative" => "co op".into(),
        "single player" | "singleplayer" => "single player".into(),
        "multi player" | "multiplayer" => "multiplayer".into(),
        "sci fi" | "science fiction" => "science fiction".into(),
        _ => normalized,
    }
}

fn concepts(game: &Game) -> BTreeMap<String, (TagKind, BTreeSet<String>)> {
    let mut result = BTreeMap::new();
    let mut add = |values: &[String], kind, source: &str| {
        for value in values {
            let tag = canonical_tag(value);
            if tag.is_empty() {
                continue;
            }
            let entry = result.entry(tag).or_insert_with(|| (kind, BTreeSet::new()));
            if kind == TagKind::Genre {
                entry.0 = TagKind::Genre;
            }
            entry.1.insert(source.to_string());
        }
    };
    if let Some(data) = &game.steam_store {
        add(&data.genres, TagKind::Genre, "steam.genres");
        add(&data.categories, TagKind::Descriptor, "steam.categories");
    }
    if let Some(data) = &game.igdb {
        add(&data.genres, TagKind::Genre, "igdb.genres");
        add(&data.themes, TagKind::Descriptor, "igdb.themes");
        add(&data.keywords, TagKind::Descriptor, "igdb.keywords");
    }
    if let Some(data) = &game.rawg {
        add(&data.genres, TagKind::Genre, "rawg.genres");
        add(&data.tags, TagKind::Descriptor, "rawg.tags");
    }
    result
}

fn bounded(minutes: u32, scale: f64) -> f64 {
    f64::from(minutes) / (f64::from(minutes) + scale)
}

struct TagEvidence {
    kind: TagKind,
    library_titles: usize,
    engaged: Vec<(SupportingTitle, f64, f64)>,
}

/// Cached samples must already belong to the selected account. Taking maxima
/// avoids counting the same playtime again across local/owned/recent sources.
pub fn derive(
    games: &[Game],
    prefs: &Preferences,
    cached: &BTreeMap<u32, Vec<ActivityEvidence>>,
    now: i64,
) -> TasteProfile {
    let mut coverage = Coverage::default();
    let mut tags: BTreeMap<String, TagEvidence> = BTreeMap::new();
    let (mut total_long, mut total_recent) = (0.0, 0.0);
    // AppID ordering makes floating-point reductions and supporting evidence stable.
    let ordered: BTreeMap<_, _> = games.iter().map(|g| (g.app_id, g)).collect();
    for game in ordered.values().filter(|g| {
        !g.is_hidden
            && !g.is_junk
            && matches!(g.app_type, SteamAppType::Game | SteamAppType::Unknown(_))
    }) {
        coverage.eligible_titles += 1;
        let mut activity = vec![ActivityEvidence {
            source: "steam.local".into(),
            observed_at: None,
            lifetime_minutes: game.playtime_minutes,
            recent_minutes: game.playtime_2wks_minutes,
            last_played_at: game.last_played_unix,
        }];
        activity.extend(cached.get(&game.app_id).into_iter().flatten().cloned());
        activity.sort_by(|a, b| a.source.cmp(&b.source));
        let lifetime = activity
            .iter()
            .filter_map(|a| a.lifetime_minutes)
            .max()
            .unwrap_or(0);
        let recent = activity
            .iter()
            .filter(|a| {
                a.observed_at
                    .is_none_or(|t| t >= now - RECENT_SECONDS && t <= now)
                    && a.last_played_at
                        .is_none_or(|t| t >= now - RECENT_SECONDS && t <= now)
            })
            .filter_map(|a| a.recent_minutes)
            .max()
            .unwrap_or(0);
        let interested = preferences::interested(prefs, game.app_id, now);
        coverage.activity_titles += usize::from(lifetime > 0 || recent > 0);
        coverage.interested_titles += usize::from(interested);
        let concepts = concepts(game);
        if concepts.is_empty() {
            continue;
        }
        coverage.metadata_titles += 1;
        // Interested is modest, reversible and does not invent recent play.
        let long_weight = bounded(lifetime, 600.0) + if interested { 0.25 } else { 0.0 };
        let recent_weight = bounded(recent, 120.0);
        total_long += long_weight;
        total_recent += recent_weight;
        for (tag, (kind, sources)) in concepts {
            let entry = tags.entry(tag).or_insert_with(|| TagEvidence {
                kind,
                library_titles: 0,
                engaged: Vec::new(),
            });
            if kind == TagKind::Genre {
                entry.kind = kind;
            }
            entry.library_titles += 1;
            if long_weight > 0.0 || recent_weight > 0.0 {
                entry.engaged.push((
                    SupportingTitle {
                        app_id: game.app_id,
                        name: game.name.clone(),
                        lifetime_minutes: lifetime,
                        recent_minutes: recent,
                        last_played_at: activity.iter().filter_map(|a| a.last_played_at).max(),
                        interested,
                        metadata_sources: sources,
                        activity: activity.clone(),
                    },
                    long_weight,
                    recent_weight,
                ));
            }
        }
    }
    let strengths = |recent: bool, total: f64| {
        let mut values = Vec::new();
        for (tag, evidence) in &tags {
            let weight = |e: &(SupportingTitle, f64, f64)| if recent { e.2 } else { e.1 };
            let sum: f64 = evidence.engaged.iter().map(weight).sum();
            if sum == 0.0 {
                continue;
            }
            let strength = sum / total;
            values.push(TagStrength {
                tag: tag.clone(),
                kind: evidence.kind,
                strength,
                library_relative_lift: strength * coverage.metadata_titles as f64
                    / evidence.library_titles as f64,
                supporting_titles: evidence
                    .engaged
                    .iter()
                    .filter(|e| weight(e) > 0.0)
                    .map(|e| e.0.clone())
                    .collect(),
            });
        }
        values.sort_by(|a, b| {
            b.strength
                .total_cmp(&a.strength)
                .then_with(|| a.tag.cmp(&b.tag))
        });
        values
    };
    let long_term = strengths(false, total_long);
    let recent = strengths(true, total_recent);
    TasteProfile {
        as_of: now,
        explanation: if long_term.is_empty() && recent.is_empty() {
            "No eligible titles with both descriptors and playtime or Interested evidence.".into()
        } else {
            "Limited to cached/local evidence. Strengths are non-exclusive engagement shares; lift is relative to this metadata-covered library, never a player percentile. Recent means available two-week evidence; Interested affects long-term only. Local observations have no fetch timestamp.".into()
        },
        coverage,
        long_term,
        recent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const NOW: i64 = 1_800_000_000;

    fn game(id: u32, tag: &str, lifetime: u32, recent: u32) -> Game {
        serde_json::from_value(json!({
            "app_id": id, "name": format!("Title {id}"), "app_type": "Game",
            "installed": false, "steam_collections": [], "is_hidden": false, "is_junk": false,
            "playtime_minutes": lifetime, "playtime_2wks_minutes": recent,
            "last_played_unix": NOW,
            "rawg": {"rawg_id": id, "genres": [tag], "tags": [], "stores": []}
        }))
        .unwrap()
    }

    #[test]
    fn cross_provider_aliases_dedupe_preserving_sources_and_kind() {
        let mut g = game(1, "Role-playing (RPG)", 600, 0);
        g.rawg.as_mut().unwrap().tags = vec!["rpg".into(), "Sci-fi".into()];
        g.igdb = serde_json::from_value(json!({
            "igdb_id":1,"name":"Title 1","genres":["RPG"],"themes":["Science Fiction"],
            "keywords":["Cooperative"],"similar_game_ids":[],"steam_app_id_confirmed":true
        }))
        .unwrap();
        g.steam_store = serde_json::from_value(json!({
            "app_id":1,"name":"Title 1","steam_store_type":"game","is_free":false,
            "developers":[],"publishers":[],"genres":["Role playing"],"categories":["Co-op"],
            "platforms":{"windows":true,"mac":true,"linux":true},"coming_soon":false
        }))
        .unwrap();
        let p = derive(&[g], &Preferences::new(), &BTreeMap::new(), NOW);
        assert_eq!(p.long_term.len(), 3);
        let rpg = p.long_term.iter().find(|t| t.tag == "rpg").unwrap();
        assert_eq!(rpg.kind, TagKind::Genre);
        assert_eq!(rpg.strength, 1.0);
        assert_eq!(rpg.supporting_titles.len(), 1);
        assert_eq!(rpg.supporting_titles[0].metadata_sources.len(), 4);
        assert_eq!(
            p.long_term.iter().find(|t| t.tag == "co op").unwrap().kind,
            TagKind::Descriptor
        );
    }

    #[test]
    fn long_and_recent_differ_without_counting_cached_minutes_twice() {
        let games = vec![game(1, "Strategy", 20_000, 0), game(2, "RPG", 300, 120)];
        let mut samples = BTreeMap::new();
        samples.insert(
            2,
            vec![ActivityEvidence {
                source: "steam.owned".into(),
                observed_at: Some(NOW),
                lifetime_minutes: Some(300),
                recent_minutes: Some(120),
                last_played_at: Some(NOW),
            }],
        );
        let p = derive(&games, &Preferences::new(), &samples, NOW);
        assert_eq!(p.long_term[0].tag, "strategy");
        assert_eq!(p.recent[0].tag, "rpg");
        assert_eq!(p.recent[0].supporting_titles[0].lifetime_minutes, 300);
        assert_eq!(p.recent[0].supporting_titles[0].recent_minutes, 120);
        samples.get_mut(&2).unwrap()[0].observed_at = Some(NOW - RECENT_SECONDS - 1);
        let mut games = games;
        games[1].playtime_2wks_minutes = None;
        assert_eq!(
            derive(&games, &Preferences::new(), &samples, NOW).recent,
            [] as [TagStrength; 0]
        );
    }

    #[test]
    fn outliers_are_bounded_and_weights_are_continuous() {
        assert_eq!(bounded(0, 600.0), 0.0);
        assert!(bounded(1, 600.0) > 0.0);
        assert!(bounded(600, 600.0) < bounded(601, 600.0));
        let games = vec![
            game(1, "RPG", u32::MAX, 0),
            game(2, "Strategy", 600, 0),
            game(3, "Strategy", 600, 0),
            game(4, "Strategy", 600, 0),
        ];
        let p = derive(&games, &Preferences::new(), &BTreeMap::new(), NOW);
        assert_eq!(p.long_term[0].tag, "strategy");
        assert!(p.long_term[1].strength < 0.4);
    }

    #[test]
    fn recent_excludes_future_observation_or_last_played_time() {
        let mut g = game(1, "RPG", 600, 120);
        g.last_played_unix = Some(NOW + 1);
        let p = derive(&[g], &Preferences::new(), &BTreeMap::new(), NOW);
        assert_eq!(p.recent, [] as [TagStrength; 0]);
        assert_ne!(p.long_term, [] as [TagStrength; 0]);
        let mut g = game(1, "RPG", 0, 0);
        g.playtime_2wks_minutes = None;
        let samples = BTreeMap::from([(
            1,
            vec![ActivityEvidence {
                source: "steam.recent".into(),
                observed_at: Some(NOW + 1),
                lifetime_minutes: None,
                recent_minutes: Some(120),
                last_played_at: Some(NOW),
            }],
        )]);
        assert_eq!(
            derive(&[g], &Preferences::new(), &samples, NOW).recent,
            [] as [TagStrength; 0]
        );
    }

    #[test]
    fn ownership_alone_is_not_taste_and_interest_is_reversible() {
        let games = vec![game(1, "RPG", 0, 0), game(2, "Strategy", 600, 0)];
        let root = tempfile::tempdir().unwrap();
        let id = "76561198000000000";
        let empty = Preferences::new();
        assert_eq!(
            derive(&games[..1], &empty, &BTreeMap::new(), NOW).long_term,
            [] as [TagStrength; 0]
        );
        preferences::set(
            root.path(),
            id,
            1,
            Some(preferences::Choice::Interested),
            NOW,
        )
        .unwrap();
        let prefs = preferences::load(root.path(), id).unwrap();
        let p = derive(&games, &prefs, &BTreeMap::new(), NOW);
        assert_eq!(p.long_term[0].tag, "strategy");
        assert!((p.long_term[1].strength - 1.0 / 3.0).abs() < 1e-10);
        assert_eq!(p.recent, [] as [TagStrength; 0]);
        preferences::set(root.path(), id, 1, None, NOW).unwrap();
        assert_eq!(
            derive(
                &games,
                &preferences::load(root.path(), id).unwrap(),
                &BTreeMap::new(),
                NOW
            ),
            derive(&games, &empty, &BTreeMap::new(), NOW)
        );
    }

    #[test]
    fn determinism_coverage_and_hidden_junk_boundaries() {
        let mut games = vec![
            game(3, "rpg", 100, 50),
            game(1, "Strategy", 100, 25),
            game(2, "", 100, 0),
        ];
        games[2].rawg = None;
        let first = derive(&games, &Preferences::new(), &BTreeMap::new(), NOW);
        games.reverse();
        assert_eq!(
            serde_json::to_string(&first).unwrap(),
            serde_json::to_string(&derive(&games, &Preferences::new(), &BTreeMap::new(), NOW))
                .unwrap()
        );
        assert_eq!(first.coverage.metadata_titles, 2);
        assert_eq!(first.coverage.eligible_titles, 3);
        games[1].is_hidden = true;
        games[2].is_junk = true;
        assert_eq!(
            derive(&games, &Preferences::new(), &BTreeMap::new(), NOW).long_term,
            [] as [TagStrength; 0]
        );
    }
}
