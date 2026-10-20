//! Original, descriptive rules; not personality scores or population comparisons.

use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::{
    progress::Summary,
    taste::{TagKind, TasteProfile},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Example {
    pub app_id: u32,
    pub name: String,
    pub evidence: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Trait {
    pub label: String,
    pub supported: bool,
    pub rule: String,
    pub supporting: Vec<Example>,
    pub contrary: Vec<Example>,
    pub unknown: Vec<Example>,
}

/// Uses the same eligible titles, max-source minutes and canonical genres as Top Tags.
/// An absent story mark is unknown, never proof of an unfinished story.
pub fn derive(taste: &TasteProfile, progress: &BTreeMap<u32, Summary>) -> Vec<Trait> {
    let mut completion = Trait {
        label: "Completion-focused".into(), supported: false,
        rule: "At least 3 completed titles and at least 60% of titles with known completion evidence. Completed = local story mark or fresh authoritative perfected achievements. Missing story marks and stale/missing achievements are unknown, not unfinished stories.".into(),
        supporting: Vec::new(), contrary: Vec::new(), unknown: Vec::new(),
    };
    let mut deep = Trait {
        label: "Deep diver".into(), supported: false,
        rule: "At least 2 titles with 1200+ lifetime minutes, making up at least 50% of played titles with known lifetime minutes. Unknown playtime is excluded, not zero. Hours describe engagement, never completion duration.".into(),
        supporting: Vec::new(), contrary: Vec::new(), unknown: Vec::new(),
    };
    let mut explorer = Trait {
        label: "Explorer".into(), supported: false,
        rule: "At least 5 titles with 60+ lifetime minutes spanning at least 3 canonical broad genres. Metadata missing is unknown; ownership and Interested alone do not count as play.".into(),
        supporting: Vec::new(), contrary: Vec::new(), unknown: Vec::new(),
    };
    let mut genres = BTreeSet::new();
    for title in &taste.titles {
        let example = |evidence: String| Example {
            app_id: title.app_id,
            name: title.name.clone(),
            evidence,
        };
        let p = progress.get(&title.app_id);
        let played = title.lifetime_minutes > 0 || title.recent_minutes > 0;
        if p.is_some_and(|p| p.story_completed) {
            completion.supporting.push(example(
                "Local story completion mark; not a Steam achievement.".into(),
            ));
        } else if let Some(e) = p
            .and_then(|p| p.achievements)
            .filter(|e| !e.stale && e.total > 0)
        {
            let item = example(format!(
                "Fresh cached achievement counts: {}/{}; story unmarked/unknown.",
                e.unlocked, e.total
            ));
            if e.unlocked == e.total {
                completion.supporting.push(item);
            } else {
                completion.contrary.push(item);
            }
        } else {
            completion.unknown.push(example("Story unmarked; fresh achievement denominator unavailable. No completion inference.".into()));
        }
        let known_lifetime = title.activity.iter().any(|a| a.lifetime_minutes.is_some());
        if !known_lifetime {
            deep.unknown
                .push(example("Lifetime minutes unknown.".into()));
        } else if played {
            let item = example(format!(
                "{} lifetime minutes; max across local/cached sources, not summed.",
                title.lifetime_minutes
            ));
            if title.lifetime_minutes >= 1200 {
                deep.supporting.push(item);
            } else {
                deep.contrary.push(item);
            }
        } else {
            deep.unknown.push(example(
                "No positive play evidence; ownership does not support a trait.".into(),
            ));
        }
        let title_genres: BTreeSet<_> = taste
            .long_term
            .iter()
            .filter(|t| {
                t.kind == TagKind::Genre
                    && t.supporting_titles.iter().any(|s| s.app_id == title.app_id)
            })
            .map(|t| t.tag.clone())
            .collect();
        if title.lifetime_minutes >= 60 && !title_genres.is_empty() {
            genres.extend(title_genres.clone());
            explorer.supporting.push(example(format!(
                "{} lifetime minutes; canonical genres {:?}; metadata sources {:?}.",
                title.lifetime_minutes, title_genres, title.metadata_sources
            )));
        } else if known_lifetime && title.lifetime_minutes < 60 {
            explorer.contrary.push(example(format!("{} lifetime minutes, below 60-minute engagement threshold; not evidence of dislike.", title.lifetime_minutes)));
        } else {
            explorer.unknown.push(example(
                "Lifetime minutes or broad genre metadata missing; excluded from genre coverage."
                    .into(),
            ));
        }
    }
    let completed = completion.supporting.len();
    completion.supported =
        completed >= 3 && completed * 100 >= (completed + completion.contrary.len()) * 60;
    let deep_count = deep.supporting.len();
    deep.supported = deep_count >= 2 && deep_count * 2 >= deep_count + deep.contrary.len();
    explorer.supported = explorer.supporting.len() >= 5 && genres.len() >= 3;
    explorer
        .rule
        .push_str(&format!(" Observed qualifying genres: {}.", genres.len()));
    vec![completion, deep, explorer]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{models::Game, preferences::Preferences, progress::AchievementEvidence, taste};

    fn game(id: u32, minutes: Option<u32>, genre: &str) -> Game {
        serde_json::from_value(serde_json::json!({
            "app_id": id, "name": format!("Game {id}"), "app_type": "Game",
            "installed": false, "steam_collections": [], "is_hidden": false, "is_junk": false,
            "playtime_minutes": minutes,
            "rawg": {"rawg_id": id, "genres": [genre], "tags": [], "stores": []}
        }))
        .unwrap()
    }

    #[test]
    fn grounded_completion_explorer_and_deep_diver() {
        let games: Vec<_> = (1..=5)
            .map(|id| {
                game(
                    id,
                    Some(if id <= 3 { 1500 } else { 60 }),
                    ["RPG", "Strategy", "Puzzle"][(id % 3) as usize],
                )
            })
            .collect();
        let progress = (1..=5)
            .map(|id| {
                (
                    id,
                    Summary::new(
                        Some(100),
                        id <= 3,
                        Some(AchievementEvidence {
                            unlocked: 1,
                            total: 10,
                            stale: false,
                        }),
                    ),
                )
            })
            .collect();
        let taste = taste::derive(&games, &Preferences::new(), &BTreeMap::new(), 100);
        let traits = derive(&taste, &progress);
        assert!(traits.iter().all(|t| t.supported));
        assert_eq!(traits[0].supporting.len(), 3);
        assert_eq!(traits[0].contrary.len(), 2);
        assert!(traits[0].supporting[0].evidence.contains("Local story"));
        assert!(traits[2].supporting[0].evidence.contains("rawg.genres"));
        assert!(traits[2].rule.contains("genres: 3"));
    }

    #[test]
    fn ownership_and_unknown_playtime_never_invent_traits() {
        let games = vec![game(1, Some(0), "RPG"), game(2, None, "Strategy")];
        let taste = taste::derive(&games, &Preferences::new(), &BTreeMap::new(), 100);
        let traits = derive(&taste, &BTreeMap::new());
        assert!(traits.iter().all(|t| !t.supported));
        assert_eq!(traits[0].unknown.len(), 2);
        assert_eq!(traits[1].unknown.len(), 2);
        assert_eq!(traits[2].contrary.len(), 1);
        assert_eq!(traits[2].unknown.len(), 1);
    }

    #[test]
    fn stale_achievements_are_unknown_not_completion_and_story_is_independent() {
        let games: Vec<_> = (1..=3).map(|id| game(id, Some(60), "RPG")).collect();
        let mut progress: BTreeMap<_, _> = (1..=3)
            .map(|id| {
                (
                    id,
                    Summary::new(
                        Some(60),
                        false,
                        Some(AchievementEvidence {
                            unlocked: 10,
                            total: 10,
                            stale: true,
                        }),
                    ),
                )
            })
            .collect();
        let taste = taste::derive(&games, &Preferences::new(), &BTreeMap::new(), 100);
        assert!(!derive(&taste, &progress)[0].supported);
        assert_eq!(derive(&taste, &progress)[0].unknown.len(), 3);
        for value in progress.values_mut() {
            value.story_completed = true;
        }
        assert!(derive(&taste, &progress)[0].supported);
    }

    #[test]
    fn deterministic_coverage_excludes_hidden_junk_and_unknown_metadata() {
        let mut games = vec![
            game(1, Some(1200), "RPG"),
            game(2, Some(1500), "Strategy"),
            game(3, Some(60), "Puzzle"),
        ];
        games[1].is_hidden = true;
        games[2].rawg = None;
        let derive_games = |games: &[Game]| {
            derive(
                &taste::derive(games, &Preferences::new(), &BTreeMap::new(), 100),
                &BTreeMap::new(),
            )
        };
        let first = derive_games(&games);
        games.reverse();
        assert_eq!(first, derive_games(&games));
        assert!(!first[1].supported);
        assert_eq!(first[0].unknown.len(), 2);
        assert_eq!(first[2].unknown.len(), 1);
        assert!(first.iter().all(|t| !t.rule.is_empty()));
    }
}
