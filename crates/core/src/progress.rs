//! Local narrative completion and evidence-based Library progress. No Steam writes.

use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Result, VapourflyError};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    NotPlayed,
    InProgress,
    Unknown,
}

/// An authoritative achievement denominator, independent of narrative completion.
#[derive(Clone, Copy, Debug, Serialize)]
pub struct AchievementEvidence {
    pub unlocked: u32,
    pub total: u32,
    pub stale: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Summary {
    pub activity: Activity,
    pub story_completed: bool,
    pub achievements: Option<AchievementEvidence>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Unfinished,
    NearPerfect,
}

impl Filter {
    pub const ALL: [Self; 3] = [Self::All, Self::Unfinished, Self::NearPerfect];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All progress",
            Self::Unfinished => "Unfinished",
            Self::NearPerfect => "Near-perfect",
        }
    }
}

impl Summary {
    /// Missing playtime is not zero; manual completion never fabricates achievements.
    pub fn new(
        playtime: Option<u32>,
        story_completed: bool,
        achievements: Option<AchievementEvidence>,
    ) -> Self {
        let activity =
            if playtime.is_some_and(|n| n > 0) || achievements.is_some_and(|e| e.unlocked > 0) {
                Activity::InProgress
            } else if playtime == Some(0) || achievements.is_some() {
                Activity::NotPlayed
            } else {
                Activity::Unknown
            };
        Self {
            activity,
            story_completed,
            achievements,
        }
    }

    pub fn perfected(&self) -> bool {
        self.achievements
            .is_some_and(|e| e.total > 0 && e.unlocked == e.total)
    }

    /// Near-perfect means fresh, authoritative 90%..100% (exclusive of perfected).
    pub fn matches(&self, filter: Filter) -> bool {
        match filter {
            Filter::All => true,
            Filter::Unfinished => {
                self.activity != Activity::Unknown && !self.story_completed && !self.perfected()
            }
            Filter::NearPerfect => self.achievements.is_some_and(|e| {
                !e.stale
                    && e.total > 0
                    && e.unlocked < e.total
                    && u64::from(e.unlocked) * 100 >= u64::from(e.total) * 90
            }),
        }
    }

    pub fn label(&self) -> String {
        let activity = match self.activity {
            Activity::NotPlayed => "Not played",
            Activity::InProgress => "In progress",
            Activity::Unknown => "Unknown",
        };
        let story = if self.story_completed {
            "; Story completed (local)"
        } else {
            ""
        };
        let achievements = self.achievements.map_or_else(
            || "; achievement completion unknown".into(),
            |e| {
                format!(
                    "; {}{}/{}{}",
                    if self.perfected() { "Perfected; " } else { "" },
                    e.unlocked,
                    e.total,
                    if e.stale { " (cached/stale)" } else { "" }
                )
            },
        );
        format!("{activity}{story}{achievements}")
    }
}

fn validate(id: &str, app_id: u32) -> Result<()> {
    if id.len() != 17
        || !id.bytes().all(|b| b.is_ascii_digit())
        || id.parse::<u64>().is_err()
        || app_id == 0
    {
        return Err(VapourflyError::InvalidInput(
            "Invalid personal progress identity".into(),
        ));
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
struct StoryRecord {
    version: u8,
    completed: bool,
}

/// Missing means unmarked, not an assertion that the story is unfinished.
pub fn story_completed(root: &Path, id: &str, app_id: u32) -> Result<bool> {
    validate(id, app_id)?;
    let path = root.join(id).join(format!("{app_id}.json"));
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => {
            return Err(VapourflyError::Internal(
                "Cannot read story completion".into(),
            ));
        }
    };
    let record: StoryRecord = serde_json::from_slice(&bytes)
        .map_err(|_| VapourflyError::InvalidInput("Invalid story completion record".into()))?;
    if record.version != 1 {
        return Err(VapourflyError::UnsupportedFormat(
            "story completion version".into(),
        ));
    }
    Ok(record.completed)
}

/// Atomic per-account/AppID edits cannot clobber another game's mark. Undo removes it.
pub fn set_story_completed(root: &Path, id: &str, app_id: u32, completed: bool) -> Result<()> {
    validate(id, app_id)?;
    let directory = root.join(id);
    let path = directory.join(format!("{app_id}.json"));
    if !completed {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(VapourflyError::Internal(
                "Cannot undo story completion".into(),
            )),
        };
    }
    let save = || -> std::io::Result<()> {
        std::fs::create_dir_all(&directory)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
        temporary.write_all(br#"{"version":1,"completed":true}"#)?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|e| e.error)?;
        Ok(())
    };
    save().map_err(|_| VapourflyError::Internal("Cannot save story completion".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn story_progress_is_account_scoped_reversible_and_not_a_steam_write() {
        let temp = tempfile::tempdir().unwrap();
        let steam = temp.path().join("steam/localconfig.vdf");
        std::fs::create_dir_all(steam.parent().unwrap()).unwrap();
        std::fs::write(&steam, "unchanged").unwrap();
        let root = temp.path().join("personal/story");
        let id = "76561198000000001";
        assert!(!story_completed(&root, id, 10).unwrap());
        assert!(!root.exists());
        set_story_completed(&root, id, 10, true).unwrap();
        set_story_completed(&root, id, 11, true).unwrap();
        assert!(story_completed(&root, id, 10).unwrap());
        assert!(!story_completed(&root, "76561198000000002", 10).unwrap());
        set_story_completed(&root, id, 10, false).unwrap();
        assert!(!story_completed(&root, id, 10).unwrap());
        assert!(story_completed(&root, id, 11).unwrap());
        assert_eq!(std::fs::read_to_string(steam).unwrap(), "unchanged");
    }

    #[test]
    fn story_progress_rejects_invalid_identity_and_corruption_without_leaking_identity() {
        let root = tempfile::tempdir().unwrap();
        assert!(set_story_completed(root.path(), "../escape", 10, true).is_err());
        assert!(set_story_completed(root.path(), "76561198000000001", 0, true).is_err());
        let id = "76561198000000001";
        set_story_completed(root.path(), id, 10, true).unwrap();
        std::fs::write(root.path().join(id).join("10.json"), "corrupt").unwrap();
        let error = story_completed(root.path(), id, 10)
            .unwrap_err()
            .to_string();
        assert!(!error.contains(id));
        assert!(error.contains("Invalid story"));
    }

    #[test]
    fn story_progress_keeps_narrative_and_achievement_evidence_independent() {
        let unknown = Summary::new(None, false, None);
        assert_eq!(unknown.activity, Activity::Unknown);
        assert!(!unknown.matches(Filter::Unfinished));
        assert!(Summary::new(Some(0), false, None).matches(Filter::Unfinished));
        let marked = Summary::new(Some(1), true, None);
        assert!(!marked.perfected());
        assert!(!marked.matches(Filter::Unfinished));
        let evidence = AchievementEvidence {
            unlocked: 9,
            total: 10,
            stale: false,
        };
        assert!(Summary::new(None, true, Some(evidence)).matches(Filter::NearPerfect));
        assert!(
            !Summary::new(
                None,
                false,
                Some(AchievementEvidence {
                    stale: true,
                    ..evidence
                })
            )
            .matches(Filter::NearPerfect)
        );
        let perfect = Summary::new(
            None,
            false,
            Some(AchievementEvidence {
                unlocked: 10,
                ..evidence
            }),
        );
        assert!(perfect.perfected());
        assert!(!perfect.story_completed);
        assert!(!perfect.matches(Filter::Unfinished));
        assert!(!perfect.matches(Filter::NearPerfect));
    }
}
