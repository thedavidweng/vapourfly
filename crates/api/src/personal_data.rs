//! Account-scoped inventory and explicit, exclusively owned cleanup previews.

use std::path::Path;

use serde::Serialize;

use crate::{activity, cache::DiskCache};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryError {
    InvalidIdentity,
    Storage,
    Busy,
    Clear,
}

impl std::fmt::Display for InventoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidIdentity => "Invalid personal-storage identity",
            Self::Storage => "Cannot inspect personal storage; check local file access",
            Self::Busy => "Personal records are busy or cannot be locked; finish other operations and retry",
            Self::Clear => "Personal cleanup failed; some records may have been removed. Reopen inventory before retrying",
        })
    }
}

impl std::error::Error for InventoryError {}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct InventoryRow {
    pub kind: &'static str,
    pub records: usize,
    /// Filesystem write time, not provider freshness or play time.
    pub last_local_write: Option<i64>,
    pub evidence: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearScope {
    Cached,
    Authored,
}

impl ClearScope {
    pub fn warning(self) -> &'static str {
        match self {
            Self::Cached => {
                "Clear this account's cached activity and achievement progress only. Provider data is unchanged; authorized explicit refresh may fetch it again. Story marks and preferences remain."
            }
            Self::Authored => {
                "Permanently clear this account's local story marks and preferences, including expired, invalid and temporary authored records. This cannot be undone. Remote caches remain."
            }
        }
    }
}

enum ClearTarget {
    File(std::path::PathBuf),
    Directory(std::path::PathBuf),
}

/// An explicit preview capability. Exclusive account locks remain held until
/// confirmation consumes this plan or cancellation drops it. No network calls.
pub struct ClearPlan {
    id: String,
    pub inventory: Inventory,
    pub scope: ClearScope,
    targets: Vec<ClearTarget>,
    _owners: Vec<vapourfly_core::personal_store::AccountLock>,
}

impl ClearPlan {
    pub fn is_for(&self, id: &str) -> bool {
        self.id == id
    }

    pub fn clear(self) -> Result<(), InventoryError> {
        for target in &self.targets {
            let result = match target {
                ClearTarget::File(path) => std::fs::remove_file(path),
                ClearTarget::Directory(path) => std::fs::remove_dir_all(path),
            };
            result.map_err(|_| InventoryError::Clear)?;
        }
        Ok(())
    }
}

fn add_target(
    targets: &mut Vec<ClearTarget>,
    path: std::path::PathBuf,
    directory: bool,
) -> Result<(), InventoryError> {
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if (directory && metadata.is_dir()) || (!directory && metadata.is_file()) => {
            targets.push(if directory {
                ClearTarget::Directory(path)
            } else {
                ClearTarget::File(path)
            });
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(InventoryError::Storage),
    }
}

pub fn prepare_clear(
    cache: &DiskCache,
    story_root: &Path,
    preferences_root: &Path,
    id: &str,
    scope: ClearScope,
) -> Result<ClearPlan, InventoryError> {
    activity::validate_steam_id64(id).map_err(|_| InventoryError::InvalidIdentity)?;
    let activity_path = cache.path_for(activity::SOURCE, id);
    let achievement_path = cache.path_for(crate::achievements::SOURCE, id);
    let roots = match scope {
        ClearScope::Cached => [
            activity_path.parent().expect("activity cache"),
            achievement_path.parent().expect("achievement cache"),
        ],
        ClearScope::Authored => [story_root, preferences_root],
    };
    let mut owners = Vec::new();
    for root in roots {
        owners.push(
            vapourfly_core::personal_store::lock_account(root, id)
                .map_err(|_| InventoryError::Busy)?,
        );
    }
    let inventory = inspect(cache, story_root, preferences_root, id)?;
    let mut targets = Vec::new();
    match scope {
        ClearScope::Cached => {
            add_target(&mut targets, activity_path.clone(), false)?;
            add_target(
                &mut targets,
                activity_path.with_extension("json.tmp"),
                false,
            )?;
            let prefix = format!("{id}-");
            let entries = std::fs::read_dir(roots[1]).map_err(|_| InventoryError::Storage)?;
            for entry in entries {
                let path = entry.map_err(|_| InventoryError::Storage)?.path();
                if path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .and_then(|s| s.strip_prefix(&prefix))
                    .and_then(|s| {
                        s.strip_suffix(".json.tmp")
                            .or_else(|| s.strip_suffix(".json"))
                    })
                    .is_some_and(|s| s.parse::<u32>().is_ok_and(|id| id > 0))
                {
                    add_target(&mut targets, path, false)?;
                }
            }
        }
        ClearScope::Authored => {
            add_target(&mut targets, story_root.join(id), true)?;
            add_target(&mut targets, preferences_root.join(id), true)?;
        }
    }
    Ok(ClearPlan {
        id: id.into(),
        inventory,
        scope,
        targets,
        _owners: owners,
    })
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Inventory {
    pub rows: Vec<InventoryRow>,
}

impl Inventory {
    pub fn summary(&self) -> Vec<String> {
        self.rows
            .iter()
            .map(|row| {
                format!(
                    "{}: {} on-disk records; last local write {}; {}",
                    row.kind,
                    row.records,
                    row.last_local_write
                        .map_or_else(|| "unknown".into(), |t| t.to_string()),
                    row.evidence,
                )
            })
            .collect()
    }
}

fn include_record(row: &mut InventoryRow, path: &Path) -> Result<(), InventoryError> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| InventoryError::Storage)?;
    if !metadata.is_file() {
        return Err(InventoryError::Storage);
    }
    row.records += 1;
    if let Ok(time) = metadata.modified() {
        let seconds = time
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .and_then(|d| i64::try_from(d.as_secs()).ok());
        row.last_local_write = row.last_local_write.max(seconds);
    }
    Ok(())
}

fn directory_records(
    directory: &Path,
    prefix: &str,
    row: &mut InventoryRow,
) -> Result<(), InventoryError> {
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(InventoryError::Storage),
    };
    for entry in entries {
        let entry = entry.map_err(|_| InventoryError::Storage)?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        // Count only committed positive-AppID JSON records, never locks/temp files.
        if name
            .strip_prefix(prefix)
            .and_then(|s| s.strip_suffix(".json"))
            .is_some_and(|s| s.parse::<u32>().is_ok_and(|id| id > 0))
        {
            include_record(row, &path)?;
        }
    }
    Ok(())
}

/// Inspect exactly one identity, including records for titles no longer in the
/// local Library. Counts are storage coverage, never ownership or visibility.
pub fn inspect(
    cache: &DiskCache,
    story_root: &Path,
    preferences_root: &Path,
    id: &str,
) -> Result<Inventory, InventoryError> {
    activity::validate_steam_id64(id).map_err(|_| InventoryError::InvalidIdentity)?;
    let mut remote = InventoryRow {
        kind: "Cached activity",
        records: 0,
        last_local_write: None,
        evidence: "No readable snapshot; remote ownership and visibility unknown".into(),
    };
    let activity_path = cache.path_for(activity::SOURCE, id);
    match std::fs::symlink_metadata(&activity_path) {
        Ok(_) => include_record(&mut remote, &activity_path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(InventoryError::Storage),
    }
    match activity::load(cache, id) {
        Ok(Some(snapshot)) => remote.evidence = snapshot.summary(),
        Ok(None) => {}
        Err(_) => return Err(InventoryError::Storage),
    }
    let mut achievements = InventoryRow {
        kind: "Cached achievements",
        records: 0,
        last_local_write: None,
        evidence: "Per-game freshness/visibility require progress inspection; not inferred from file count".into(),
    };
    let achievement_path = cache.path_for(crate::achievements::SOURCE, id);
    directory_records(
        achievement_path.parent().expect("cache directory"),
        &format!("{id}-"),
        &mut achievements,
    )?;
    let mut story = InventoryRow {
        kind: "Local story marks",
        records: 0,
        last_local_write: None,
        evidence: "User-created local marks, not Steam achievements; record validity not inferred"
            .into(),
    };
    directory_records(&story_root.join(id), "", &mut story)?;
    let mut feedback = InventoryRow {
        kind: "Local preferences",
        records: 0,
        last_local_write: None,
        evidence: "User-created choices, including expired Not now; record validity not inferred"
            .into(),
    };
    directory_records(&preferences_root.join(id), "", &mut feedback)?;
    Ok(Inventory {
        rows: vec![remote, achievements, story, feedback],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "76561198000000000";
    const OTHER: &str = "76561198000000001";

    struct NoRequests;

    impl crate::http::HttpBackend for NoRequests {
        fn execute(
            &self,
            _: &crate::http::HttpRequest,
        ) -> vapourfly_core::Result<crate::http::HttpResponse> {
            panic!("cleanup ownership and offline/no-key reads must not request a provider")
        }
    }

    #[test]
    fn cached_preview_cancel_and_confirm_preserve_authored_shared_and_other_records() {
        let root = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(root.path().join("cache"));
        let story = root.path().join("story");
        let prefs = root.path().join("prefs");
        let removed = [
            cache.path_for(activity::SOURCE, ID),
            cache
                .path_for(activity::SOURCE, ID)
                .with_extension("json.tmp"),
            cache.path_for(crate::achievements::SOURCE, &format!("{ID}-999999")),
            cache
                .path_for(crate::achievements::SOURCE, &format!("{ID}-999999"))
                .with_extension("json.tmp"),
            cache
                .path_for(crate::achievements::SOURCE, &format!("{ID}-1234"))
                .with_extension("json.tmp"),
        ];
        let retained = [
            cache.path_for(activity::SOURCE, OTHER),
            cache
                .path_for(activity::SOURCE, OTHER)
                .with_extension("json.tmp"),
            cache.path_for(crate::achievements::SOURCE, &format!("{OTHER}-10")),
            cache
                .path_for(crate::achievements::SOURCE, &format!("{OTHER}-10"))
                .with_extension("json.tmp"),
            cache.path_for("steam-achievement-schema", "10"),
            story.join(ID).join("10.json"),
            prefs.join(ID).join("10.json"),
            root.path().join("playlists/manual.json"),
            root.path().join("config.toml"),
            root.path().join("Steam/localconfig.vdf"),
        ];
        for path in removed.iter().chain(&retained) {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"private-marker").unwrap();
        }
        let plan = prepare_clear(&cache, &story, &prefs, ID, ClearScope::Cached).unwrap();
        assert!(plan.is_for(ID));
        assert!(!plan.is_for(OTHER));
        assert_eq!(plan.inventory.rows[1].records, 1);
        let http = crate::http::HttpClient::with_backend(Box::new(NoRequests));
        let control = activity::RefreshControl::default();
        assert!(activity::refresh(&cache, &http, Some("key"), ID, false, &control).is_err());
        assert!(
            crate::achievements::refresh(&cache, &http, Some("key"), ID, 10, false, &control)
                .is_err()
        );
        assert!(crate::achievements::batch_lock(&cache, ID).is_err());
        assert!(
            crate::achievements::refresh_batch(
                &cache,
                &http,
                Some("key"),
                ID,
                crate::achievements::BatchOptions {
                    app_ids: &[10],
                    limit: 1,
                    force: true,
                    offline: false,
                },
                &control
            )
            .is_err()
        );
        assert!(crate::achievements::batch_lock(&cache, OTHER).is_ok());
        // No-key/offline reads neither acquire another lock nor make requests.
        assert!(activity::refresh(&cache, &http, None, ID, false, &control).is_ok());
        assert!(
            crate::achievements::refresh(&cache, &http, Some("key"), ID, 10, true, &control)
                .is_ok()
        );
        drop(plan);
        assert!(removed.iter().all(|p| p.exists()));
        prepare_clear(&cache, &story, &prefs, ID, ClearScope::Cached)
            .unwrap()
            .clear()
            .unwrap();
        assert!(removed.iter().all(|p| !p.exists()));
        for path in retained {
            assert_eq!(std::fs::read(path).unwrap(), b"private-marker");
        }
        assert_eq!(
            inspect(&cache, &story, &prefs, ID).unwrap().rows[1].records,
            0
        );
        assert!(crate::achievements::batch_lock(&cache, ID).is_ok());
    }

    #[test]
    fn authored_clear_owns_writers_and_removes_invalid_and_temporary_records() {
        let root = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(root.path().join("cache"));
        let story = root.path().join("story");
        let prefs = root.path().join("prefs");
        for dir in [&story, &prefs] {
            for id in [ID, OTHER] {
                std::fs::create_dir_all(dir.join(id)).unwrap();
                std::fs::write(dir.join(id).join("10.json"), b"invalid-private-marker").unwrap();
                std::fs::write(dir.join(id).join("10.json.tmp"), b"pending").unwrap();
            }
        }
        let plan = prepare_clear(&cache, &story, &prefs, ID, ClearScope::Authored).unwrap();
        assert!(plan.scope.warning().contains("cannot be undone"));
        assert!(vapourfly_core::progress::set_story_completed(&story, ID, 10, true).is_err());
        assert!(vapourfly_core::preferences::set(&prefs, ID, 10, None, 0).is_err());
        assert!(vapourfly_core::preferences::reset_all(&prefs, ID).is_err());
        plan.clear().unwrap();
        for dir in [&story, &prefs] {
            assert!(!dir.join(ID).exists());
            assert_eq!(
                std::fs::read(dir.join(OTHER).join("10.json")).unwrap(),
                b"invalid-private-marker"
            );
            assert!(dir.join(format!("{ID}.lock")).exists());
        }
        assert_eq!(
            vapourfly_core::preferences::load(&prefs, ID).unwrap().len(),
            0
        );
        vapourfly_core::progress::set_story_completed(&story, ID, 10, true).unwrap();
    }

    #[test]
    fn partial_clear_is_an_explicit_redacted_failure_and_releases_ownership() {
        let root = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(root.path().join("cache"));
        let story = root.path().join("story");
        let prefs = root.path().join("prefs");
        for dir in [&story, &prefs] {
            std::fs::create_dir_all(dir.join(ID)).unwrap();
            std::fs::write(dir.join(ID).join("10.json"), b"private-marker").unwrap();
        }
        let plan = prepare_clear(&cache, &story, &prefs, ID, ClearScope::Authored).unwrap();
        // A non-cooperating process removes a target after the preview.
        std::fs::remove_dir_all(prefs.join(ID)).unwrap();
        let error = plan.clear().unwrap_err();
        assert_eq!(error, InventoryError::Clear);
        assert!(
            error
                .to_string()
                .contains("some records may have been removed")
        );
        assert!(!error.to_string().contains(ID));
        assert!(!story.join(ID).exists());
        assert!(prepare_clear(&cache, &story, &prefs, ID, ClearScope::Authored).is_ok());
    }

    #[test]
    fn inventory_is_read_only_scoped_and_includes_orphan_records_without_payloads() {
        let root = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(root.path().join("cache"));
        let story = root.path().join("story");
        let prefs = root.path().join("prefs");
        let private = b"private-persona-per-game-marker";
        for path in [
            cache.path_for(activity::SOURCE, ID),
            cache.path_for(crate::achievements::SOURCE, &format!("{ID}-10")),
            cache.path_for(crate::achievements::SOURCE, &format!("{OTHER}-10")),
            cache.path_for("steam-achievement-schema", "10"),
            story.join(ID).join("999999.json"),
            story.join(OTHER).join("10.json"),
            prefs.join(ID).join("20.json"),
            prefs.join(ID).join("21.json.tmp"),
        ] {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, private).unwrap();
        }
        let inventory = inspect(&cache, &story, &prefs, ID).unwrap();
        assert_eq!(
            inventory.rows.iter().map(|r| r.records).collect::<Vec<_>>(),
            vec![1, 1, 1, 1]
        );
        assert!(inventory.rows.iter().all(|r| r.last_local_write.is_some()));
        let export = serde_json::to_string(&inventory).unwrap();
        assert!(!export.contains("private-persona-per-game-marker"));
        assert!(!export.contains(ID));
        assert!(!export.contains(OTHER));
        assert!(inventory.rows[0].evidence.contains("unknown"));
        assert_eq!(
            std::fs::read(story.join(ID).join("999999.json")).unwrap(),
            private
        );
        assert_eq!(
            inspect(&cache, &story, &prefs, OTHER).unwrap().rows[2].records,
            1
        );
    }

    #[test]
    fn empty_inventory_is_unknown_not_an_empty_remote_library_and_rejects_bad_identity() {
        let root = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(root.path().join("cache"));
        let inventory = inspect(&cache, root.path(), root.path(), ID).unwrap();
        assert!(inventory.rows.iter().all(|r| r.records == 0));
        assert!(inventory.rows[0].evidence.contains("visibility unknown"));
        assert_eq!(
            inspect(&cache, root.path(), root.path(), "../private-marker"),
            Err(InventoryError::InvalidIdentity)
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    #[test]
    fn inaccessible_storage_is_a_redacted_failure_not_an_empty_inventory() {
        let root = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(root.path().join("cache"));
        let story = root.path().join("private-marker");
        std::fs::create_dir_all(&story).unwrap();
        std::fs::write(story.join(ID), b"not a directory").unwrap();
        let error = inspect(&cache, &story, root.path(), ID).unwrap_err();
        assert_eq!(error, InventoryError::Storage);
        assert!(!format!("{error:?}: {error}").contains("private-marker"));
    }
}
