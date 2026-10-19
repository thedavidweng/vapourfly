//! Explicit local tastes, separate from community tags and Steam configuration.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Result, VapourflyError};

pub const NOT_NOW_SECONDS: i64 = 30 * 24 * 60 * 60;
pub const INTERESTED_WEIGHT: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Choice {
    Interested,
    NotInterested,
    NotNow,
}

impl Choice {
    pub const ALL: [Self; 3] = [Self::Interested, Self::NotInterested, Self::NotNow];

    pub fn label(self) -> &'static str {
        match self {
            Self::Interested => "Interested",
            Self::NotInterested => "Not interested",
            Self::NotNow => "Not now (30 days)",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Record {
    version: u8,
    pub choice: Choice,
    pub expires_at: Option<i64>,
}

impl Record {
    pub fn active(&self, now: i64) -> bool {
        self.expires_at.is_none_or(|expiry| now < expiry)
    }

    pub fn label(&self, now: i64) -> String {
        format!(
            "{}{}",
            self.choice.label(),
            if self.active(now) { "" } else { " (expired)" }
        )
    }
}

pub type Preferences = BTreeMap<u32, Record>;

pub fn excludes(preferences: &Preferences, app_id: u32, now: i64) -> bool {
    preferences.get(&app_id).is_some_and(|p| {
        p.active(now) && matches!(p.choice, Choice::NotInterested | Choice::NotNow)
    })
}

pub fn interested(preferences: &Preferences, app_id: u32, now: i64) -> bool {
    preferences
        .get(&app_id)
        .is_some_and(|p| p.active(now) && p.choice == Choice::Interested)
}

fn validate(id: &str) -> Result<()> {
    if id.len() != 17 || !id.bytes().all(|b| b.is_ascii_digit()) || id.parse::<u64>().is_err() {
        return Err(VapourflyError::InvalidInput(
            "Invalid preference identity".into(),
        ));
    }
    Ok(())
}

fn validate_app(app_id: u32) -> Result<()> {
    if app_id == 0 {
        return Err(VapourflyError::InvalidInput(
            "Invalid preference AppID".into(),
        ));
    }
    Ok(())
}

pub fn load(root: &Path, id: &str) -> Result<Preferences> {
    validate(id)?;
    let entries = match std::fs::read_dir(root.join(id)) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Preferences::new()),
        Err(_) => {
            return Err(VapourflyError::Internal(
                "Cannot read local preferences".into(),
            ));
        }
    };
    let mut result = Preferences::new();
    for entry in entries {
        let entry =
            entry.map_err(|_| VapourflyError::Internal("Cannot read local preferences".into()))?;
        let path = entry.path();
        // Atomic writer temporary files are not committed records.
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let app_id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| VapourflyError::InvalidInput("Invalid preference record name".into()))?;
        validate_app(app_id)?;
        let bytes = std::fs::read(path)
            .map_err(|_| VapourflyError::Internal("Cannot read local preference".into()))?;
        let record: Record = serde_json::from_slice(&bytes)
            .map_err(|_| VapourflyError::InvalidInput("Invalid preference record".into()))?;
        if record.version != 1 || (record.choice == Choice::NotNow) != record.expires_at.is_some() {
            return Err(VapourflyError::InvalidInput(
                "Invalid preference record policy".into(),
            ));
        }
        result.insert(app_id, record);
    }
    Ok(result)
}

/// Last explicit choice wins. Reset removes only this game's local choice.
pub fn set(
    root: &Path,
    id: &str,
    app_id: u32,
    choice: Option<Choice>,
    now: i64,
) -> Result<Option<Record>> {
    validate(id)?;
    validate_app(app_id)?;
    let _owner = crate::personal_store::lock_account(root, id)?;
    let directory = root.join(id);
    let path = directory.join(format!("{app_id}.json"));
    let Some(choice) = choice else {
        return match std::fs::remove_file(path) {
            Ok(()) => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(VapourflyError::Internal(
                "Cannot reset local preference".into(),
            )),
        };
    };
    let record =
        Record {
            version: 1,
            choice,
            expires_at: if choice == Choice::NotNow {
                Some(now.checked_add(NOT_NOW_SECONDS).ok_or_else(|| {
                    VapourflyError::InvalidInput("Invalid preference time".into())
                })?)
            } else {
                None
            },
        };
    let bytes = serde_json::to_vec(&record)
        .map_err(|_| VapourflyError::Internal("Cannot encode local preference".into()))?;
    let save = || -> std::io::Result<()> {
        std::fs::create_dir_all(&directory)?;
        let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
        temporary.write_all(&bytes)?;
        temporary.as_file().sync_all()?;
        temporary.persist(path).map_err(|e| e.error)?;
        Ok(())
    };
    save().map_err(|_| VapourflyError::Internal("Cannot save local preference".into()))?;
    Ok(Some(record))
}

/// Never touches story marks, caches, Playlists, Steam files, or another account.
pub fn reset_all(root: &Path, id: &str) -> Result<()> {
    validate(id)?;
    let _owner = crate::personal_store::lock_account(root, id)?;
    match std::fs::remove_dir_all(root.join(id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(VapourflyError::Internal(
            "Cannot reset account preferences; reload to inspect partial changes".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const A: &str = "76561198000000001";
    const B: &str = "76561198000000002";

    #[test]
    fn local_preferences_isolate_undo_expire_and_never_write_steam() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("personal/preferences");
        let steam = temp.path().join("Steam/config.vdf");
        std::fs::create_dir_all(steam.parent().unwrap()).unwrap();
        std::fs::write(&steam, "unchanged").unwrap();
        set(&root, A, 10, Some(Choice::NotNow), 100).unwrap();
        set(&root, B, 10, Some(Choice::NotInterested), 100).unwrap();
        let choices = load(&root, A).unwrap();
        assert!(excludes(&choices, 10, 100 + NOT_NOW_SECONDS - 1));
        assert!(!excludes(&choices, 10, 100 + NOT_NOW_SECONDS));
        set(&root, A, 10, Some(Choice::Interested), 101).unwrap();
        assert!(interested(&load(&root, A).unwrap(), 10, 101));
        set(&root, A, 20, Some(Choice::NotInterested), 101).unwrap();
        set(&root, A, 10, None, 101).unwrap();
        assert_eq!(load(&root, A).unwrap().len(), 1);
        reset_all(&root, A).unwrap();
        assert!(load(&root, A).unwrap().is_empty());
        assert!(excludes(&load(&root, B).unwrap(), 10, i64::MAX));
        assert_eq!(std::fs::read_to_string(steam).unwrap(), "unchanged");
    }

    #[test]
    fn local_preferences_reject_invalid_identity_and_corruption() {
        let temp = tempfile::tempdir().unwrap();
        assert!(set(temp.path(), "../escape", 10, Some(Choice::Interested), 0).is_err());
        assert!(set(temp.path(), A, 0, Some(Choice::Interested), 0).is_err());
        set(temp.path(), A, 10, Some(Choice::Interested), 0).unwrap();
        std::fs::write(temp.path().join(A).join("10.json"), "broken").unwrap();
        assert!(load(temp.path(), A).is_err());
        set(temp.path(), A, 10, None, 0).unwrap();
        assert!(load(temp.path(), A).unwrap().is_empty());
    }

    #[test]
    fn local_preferences_affect_both_rankers_without_overriding_eligibility() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/fixtures/steam_minimal");
        let scan = crate::steam::scan_library(&crate::steam::ScanOptions {
            steam_dir: fixture,
            account: None,
            fixtures: None,
        })
        .unwrap();
        let mut base = scan.games[0].clone();
        base.playtime_minutes = Some(0);
        base.app_type = crate::models::SteamAppType::Game;
        base.steam_collections.clear();
        base.is_hidden = false;
        base.is_junk = false;
        let mut games: Vec<_> = (1..=7)
            .map(|id| crate::models::Game {
                app_id: id,
                ..base.clone()
            })
            .collect();
        for id in [2, 5, 6, 7] {
            set(temp.path(), A, id, Some(Choice::Interested), 100).unwrap();
        }
        set(temp.path(), A, 3, Some(Choice::NotInterested), 100).unwrap();
        set(temp.path(), A, 4, Some(Choice::NotNow), 100).unwrap();
        games[4].is_hidden = true;
        games[5].is_junk = true;
        games[6].steam_collections = vec!["excluded".into()];
        let prefs = load(temp.path(), A).unwrap();
        let request = crate::models::RecommendRequest {
            available_minutes: 60,
            intent: crate::models::RecommendIntent::Session,
            count: 20,
            deck_mode: false,
            include_installed_only: false,
            seed: Some(5),
            exclude_collections: vec!["excluded".into()],
        };
        let recs = crate::recommend::recommend_with_preferences(&games, &request, &prefs, 100);
        assert_eq!(
            recs.iter().map(|r| r.app_id).collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert!(
            recs[0]
                .reasons
                .iter()
                .any(|r| r.code == "local_interested" && r.weight == INTERESTED_WEIGHT)
        );
        let options = crate::discover::DiscoverOptions {
            seed_app_id: None,
            count: 20,
        };
        let picks = crate::discover::rank_discover_with_preferences(&games, &options, &prefs, 100);
        assert!(
            picks
                .iter()
                .any(|p| p.app_id == 2 && p.reasons.iter().any(|r| r.code == "local_interested"))
        );
        assert!(picks.iter().all(|p| ![3, 4, 5, 6].contains(&p.app_id)));
        let later = crate::recommend::recommend_with_preferences(
            &games,
            &request,
            &prefs,
            100 + NOT_NOW_SECONDS,
        );
        assert!(later.iter().any(|r| r.app_id == 4));
        assert!(later.iter().all(|r| r.app_id != 3));
        let other = crate::recommend::recommend_with_preferences(
            &games,
            &request,
            &load(temp.path(), B).unwrap(),
            100,
        );
        assert!(other.iter().any(|r| r.app_id == 3));
    }
}
