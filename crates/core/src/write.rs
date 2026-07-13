//! Write workflow — preview and commit Steam cloud storage mutations.
//!
//! Deep module: owns the safety-check + execute sequence that CLI and GUI
//! both need. [`preview`] generates a write plan (delegates to
//! [`steam::generate_write_plan`]); [`commit`] runs the safety check and
//! atomic write in one call, returning the **real** backup path created on
//! disk. Frontends keep their own diff rendering — this module is about the
//! mutation sequence, not the presentation.
//!
//! ADR-0001: Vapourfly only modifies `cloud-storage-namespace-1.json`.
//! Every write crosses this seam.

use std::path::PathBuf;

use crate::error::Result;
use crate::models::{CloudStorageFile, WriteOp, WritePlan};
use crate::steam;

/// Default backup retention when config is unavailable.
///
/// Must match [`crate::config::VapourflyConfig`]'s default
/// `backup_retention_count` (5). Prefer [`commit_with_retention`] with the
/// resolved config value when available.
pub const DEFAULT_BACKUP_RETENTION: u32 = 5;

/// Generate a write plan without executing it.
///
/// Reads the current cloud storage, computes the diff, and returns a plan
/// that can be displayed to the user (dry-run) or passed to [`commit`].
///
/// `plan.backup_path` / `plan.tmp_path` are placeholders until commit; use
/// the [`PathBuf`] returned by [`commit`] for the real backup file path.
pub fn preview(
    cloud: &CloudStorageFile,
    ops: Vec<WriteOp>,
    cloud_path: PathBuf,
) -> Result<WritePlan> {
    steam::generate_write_plan(cloud, ops, cloud_path)
}

/// Commit a write plan with default backup retention.
///
/// Returns the absolute path of the backup file created on disk.
pub fn commit(plan: &WritePlan, allow_steam_running: bool) -> Result<PathBuf> {
    commit_with_retention(plan, allow_steam_running, DEFAULT_BACKUP_RETENTION)
}

/// Commit a write plan with an explicit retention count.
///
/// Returns the absolute path of the backup file created on disk. Retention
/// is the single source of truth for pruning after a successful write.
pub fn commit_with_retention(
    plan: &WritePlan,
    allow_steam_running: bool,
    retention_count: u32,
) -> Result<PathBuf> {
    steam::check_write_safety(&plan.target_path, allow_steam_running)?;
    steam::execute_write_plan(plan, retention_count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CloudEntry, CloudStorageFile, CollectionValue, WriteOp};
    use crate::steam::{self, set_steam_running_override};
    use serial_test::serial;
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::Write as _;
    use std::path::Path;

    fn empty_cloud() -> CloudStorageFile {
        vec![]
    }

    fn write_cloud_to_file(cloud: &CloudStorageFile, dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let json = serde_json::to_string_pretty(cloud).unwrap();
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(json.as_bytes()).unwrap();
        f.flush().unwrap();
        f.sync_all().unwrap();
        path
    }

    #[test]
    #[serial]
    fn commit_returns_real_backup_path_that_exists() {
        set_steam_running_override(Some(false));
        let tmp = tempfile::tempdir().unwrap();
        let target = write_cloud_to_file(&empty_cloud(), tmp.path(), "cloud-storage-namespace-1.json");

        let cloud = steam::read_cloud_storage(&target).unwrap();
        let plan = preview(
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "test".into(),
                added: vec![730],
                removed: vec![],
            }],
            target.clone(),
        )
        .unwrap();

        assert!(
            plan.backup_path.as_os_str().is_empty(),
            "plan must not invent a backup path before commit"
        );

        let backup = commit_with_retention(&plan, true, 5).unwrap();
        assert!(
            backup.exists(),
            "commit must return a path that exists: {}",
            backup.display()
        );
        let name = backup.file_name().unwrap().to_string_lossy();
        assert!(
            name.contains("vapourfly-backup-"),
            "backup name must use real pattern: {name}"
        );

        let after: CloudStorageFile =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert!(
            after
                .iter()
                .any(|(k, _)| k == "user-collections.test"),
            "target should contain new collection"
        );

        set_steam_running_override(None);
    }

    #[test]
    #[serial]
    fn retention_one_prunes_to_single_backup() {
        set_steam_running_override(Some(false));
        let tmp = tempfile::tempdir().unwrap();
        let target = write_cloud_to_file(&empty_cloud(), tmp.path(), "cloud.json");

        for i in 0..3 {
            let cloud = steam::read_cloud_storage(&target).unwrap();
            let plan = preview(
                &cloud,
                vec![WriteOp::UpsertCollection {
                    id: format!("c{i}"),
                    added: vec![100 + i as u32],
                    removed: vec![],
                }],
                target.clone(),
            )
            .unwrap();
            let _ = commit_with_retention(&plan, true, 1).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(1100));
        }

        let backups = steam::list_backups(&target).unwrap();
        assert_eq!(
            backups.len(),
            1,
            "retention=1 must leave a single backup, got {}",
            backups.len()
        );
        set_steam_running_override(None);
    }

    #[test]
    fn preview_builds_hidden_diff() {
        let tmp = tempfile::tempdir().unwrap();
        let cv = CollectionValue {
            id: "existing".into(),
            name: "existing".into(),
            added: vec![1],
            removed: vec![],
            extra: BTreeMap::new(),
        };
        let cloud: CloudStorageFile = vec![(
            "user-collections.existing".into(),
            CloudEntry {
                key: "user-collections.existing".into(),
                timestamp: Some(1),
                value: Some(serde_json::to_string(&cv).unwrap()),
                version: Some("1".into()),
                is_deleted: None,
                conflict_resolution_method: None,
                str_method_id: None,
                extra: BTreeMap::new(),
            },
        )];
        let target = write_cloud_to_file(&cloud, tmp.path(), "cloud.json");

        let plan = preview(
            &cloud,
            vec![WriteOp::AddToHidden {
                app_ids: vec![99],
            }],
            target,
        )
        .unwrap();
        assert!(!plan.after_content.is_empty());
        assert!(!plan.diff.hidden_app_ids_added.is_empty() || plan.after_sha256 != plan.before_sha256);
    }
}
