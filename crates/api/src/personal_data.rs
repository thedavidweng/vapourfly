//! Read-only personal-storage inventory. No network, credential values or titles.

use std::path::Path;

use serde::Serialize;

use crate::{activity, cache::DiskCache};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InventoryError {
    InvalidIdentity,
    Storage,
}

impl std::fmt::Display for InventoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::InvalidIdentity => "Invalid personal-storage identity",
            Self::Storage => "Cannot inspect personal storage; check local file access",
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
