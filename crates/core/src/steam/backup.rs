//! Backup, atomic write, and rollback for Vapourfly.
//!
//! Provides crash-safe file mutation through a write-ahead pattern:
//!
//! 1. **Confirm** the target file still matches the expected pre-write hash.
//! 2. **Backup** the target before any mutation.
//! 3. **Write** to a temporary file in the same directory.
//! 4. **fsync** the temporary file and rename it atomically over the target.
//! 5. **fsync** the parent directory (best-effort, platform-dependent).
//! 6. **Verify** the written file by re-reading and checking its hash.
//! 7. **Prune** old backups, keeping only the most recent N.
//!
//! If any step after backup creation fails, an automatic restore is attempted.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeZone, Utc};

use crate::error::{Result, VapourflyError};
use crate::models::{CloudStorageFile, WritePlan};
use crate::steam::write_plan::compute_sha256;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Metadata for a discovered backup file.
#[derive(Clone, Debug)]
pub struct BackupInfo {
    pub path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub sha256: String,
}

// ---------------------------------------------------------------------------
// Naming helpers
// ---------------------------------------------------------------------------

/// The tag inserted into backup filenames.
const BACKUP_TAG: &str = "vapourfly-backup";

/// Build a backup filename for `target_name` at the given UTC time with the
/// given source SHA-256 prefix.
///
/// Format: `{target_name}.vapourfly-backup-{YYYYMMDDTHHMMSSZ}-{short_sha}.json`
fn backup_filename(target_name: &str, now: DateTime<Utc>, source_sha: &str) -> String {
    let ts = now.format("%Y%m%dT%H%M%SZ");
    let short = &source_sha[..8.min(source_sha.len())];
    format!("{target_name}.{BACKUP_TAG}-{ts}-{short}.json")
}

/// Build a temporary filename for an in-progress atomic write.
///
/// Format: `.{target_name}.vapourfly.tmp-{pid}`
fn tmp_filename(target_name: &str) -> String {
    let pid = std::process::id();
    format!(".{target_name}.vapourfly.tmp-{pid}")
}

/// Parse a backup filename into its creation timestamp.
///
/// Returns `None` if the name doesn't match the expected pattern.
fn parse_backup_timestamp(target_name: &str, filename: &str) -> Option<DateTime<Utc>> {
    let prefix = format!("{target_name}.{BACKUP_TAG}-");
    let rest = filename.strip_prefix(&prefix)?;
    let rest = rest.strip_suffix(".json")?;
    // rest is now "{YYYYMMDDTHHMMSSZ}-{short_sha}"
    let ts_str = rest.split('-').next()?;
    if ts_str.len() != 16 || !ts_str.ends_with('Z') {
        return None;
    }
    let year: i32 = ts_str[0..4].parse().ok()?;
    let month: u32 = ts_str[4..6].parse().ok()?;
    let day: u32 = ts_str[6..8].parse().ok()?;
    let hour: u32 = ts_str[9..11].parse().ok()?;
    let min: u32 = ts_str[11..13].parse().ok()?;
    let sec: u32 = ts_str[13..15].parse().ok()?;
    Utc.with_ymd_and_hms(year, month, day, hour, min, sec)
        .single()
}

// ---------------------------------------------------------------------------
// 1. create_backup
// ---------------------------------------------------------------------------

/// Create a backup of the file at `target_path`.
///
/// The backup is placed in the same directory as the target with a name
/// incorporating the current UTC timestamp and a short SHA-256 of the source
/// content. Returns the path to the newly created backup file.
///
/// `retention_count` is accepted for API compatibility but not applied here;
/// call [`prune_old_backups`] separately if you need retention enforcement.
pub fn create_backup(target_path: &Path, _retention_count: u32) -> Result<PathBuf> {
    let source_bytes = fs::read(target_path).map_err(|_| VapourflyError::FileNotFound {
        path: crate::SafePath::new(target_path),
    })?;
    let source_sha = compute_sha256(&source_bytes);

    let target_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("cloud-storage-namespace-1.json");
    let parent = target_path.parent().unwrap_or(Path::new("."));

    let now = Utc::now();
    let bk_name = backup_filename(target_name, now, &source_sha);
    let backup_path = parent.join(&bk_name);

    fs::copy(target_path, &backup_path).map_err(|e| {
        VapourflyError::Internal(format!(
            "failed to create backup {}: {e}",
            backup_path.display()
        ))
    })?;

    tracing::debug!(
        source = %target_path.display(),
        backup = %backup_path.display(),
        sha256 = %source_sha,
        "backup created"
    );

    Ok(backup_path)
}

// ---------------------------------------------------------------------------
// 2. execute_write_plan
// ---------------------------------------------------------------------------

/// Execute an atomic write described by a [`WritePlan`].
///
/// `retention_count` controls how many old backups to keep after a successful
/// write. The plan's `after_content` bytes are used as the source data.
///
/// The sequence:
///
/// 1. Reads the target and confirms it still matches `before_sha256`.
/// 2. Creates a backup of the current target.
/// 3. Writes `after_content` to a temporary file in the same directory.
/// 4. Flushes and `fsync`s the temporary file.
/// 5. Atomically renames the temporary file over the target.
/// 6. `fsync`s the parent directory (best-effort, platform-dependent).
/// 7. Re-reads the target and verifies the SHA-256 matches `after_sha256`.
/// 8. Parses the re-read bytes as `CloudStorageFile` (semantic postcondition).
/// 9. Prunes old backups to `retention_count`.
///
/// If a failure occurs after backup creation, an automatic restore is attempted.
pub fn execute_write_plan(plan: &WritePlan, retention_count: u32) -> Result<()> {
    // -- Step 1: confirm target still matches ---------------------------------
    let current_bytes = fs::read(&plan.target_path).map_err(|_| VapourflyError::FileNotFound {
        path: crate::SafePath::new(&plan.target_path),
    })?;
    let current_hash = compute_sha256(&current_bytes);
    if current_hash != plan.before_sha256 {
        return Err(VapourflyError::UnsafeWrite {
            reason: format!(
                "target has been modified since the plan was generated \
                 (expected {}, found {})",
                &plan.before_sha256[..16],
                &current_hash[..16]
            ),
        });
    }

    // -- Step 2: create backup ------------------------------------------------
    let backup_path = create_backup(&plan.target_path, retention_count)?;

    // -- Steps 3-9: perform the atomic write, restoring on failure ------------
    match atomic_write_inner(plan, retention_count) {
        Ok(()) => Ok(()),
        Err(write_err) => {
            tracing::warn!(
                error = %write_err,
                backup = %backup_path.display(),
                "write failed, attempting restore from backup"
            );
            match restore_backup(&backup_path, &plan.target_path) {
                Ok(()) => Err(VapourflyError::Internal(format!(
                    "write failed and was rolled back: {write_err}"
                ))),
                Err(restore_err) => Err(VapourflyError::Internal(format!(
                    "write failed ({write_err}) and rollback also failed: {restore_err}"
                ))),
            }
        }
    }
}

/// The inner atomic write sequence, separated so the caller can wrap failures
/// with restore logic.
fn atomic_write_inner(plan: &WritePlan, retention_count: u32) -> Result<()> {
    // -- Step 3: write tmp file -----------------------------------------------
    let target_name = plan
        .target_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("cloud-storage-namespace-1.json");
    let parent = plan.target_path.parent().unwrap_or(Path::new("."));
    let tmp_path = parent.join(tmp_filename(target_name));

    {
        let mut tmp_file = fs::File::create(&tmp_path).map_err(|e| {
            VapourflyError::Internal(format!(
                "failed to create tmp file {}: {e}",
                tmp_path.display()
            ))
        })?;
        tmp_file
            .write_all(&plan.after_content)
            .map_err(|e| VapourflyError::Internal(format!("failed to write tmp file: {e}")))?;

        // -- Step 4: flush and fsync tmp --------------------------------------
        tmp_file
            .flush()
            .map_err(|e| VapourflyError::Internal(format!("failed to flush tmp file: {e}")))?;
        tmp_file
            .sync_all()
            .map_err(|e| VapourflyError::Internal(format!("failed to fsync tmp file: {e}")))?;
    } // drop closes the file handle before rename

    // -- Step 5: rename tmp over target ---------------------------------------
    fs::rename(&tmp_path, &plan.target_path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        VapourflyError::Internal(format!(
            "failed to rename {} -> {}: {e}",
            tmp_path.display(),
            plan.target_path.display()
        ))
    })?;

    // -- Step 6: fsync parent directory (best-effort) -------------------------
    fsync_parent(parent)?;

    // -- Step 7: re-read and verify hash --------------------------------------
    let written_bytes = fs::read(&plan.target_path).map_err(|e| {
        VapourflyError::Internal(format!("failed to re-read target after write: {e}"))
    })?;
    let written_hash = compute_sha256(&written_bytes);
    if written_hash != plan.after_sha256 {
        return Err(VapourflyError::UnsafeWrite {
            reason: format!(
                "post-write verification failed: expected {}, found {}",
                &plan.after_sha256[..16],
                &written_hash[..16]
            ),
        });
    }

    // -- Step 8: semantic verification ----------------------------------------
    let _: CloudStorageFile = serde_json::from_slice(&written_bytes).map_err(|e| {
        VapourflyError::Internal(format!(
            "written file is not valid CloudStorageFile JSON: {e}"
        ))
    })?;

    // -- Step 9: prune old backups --------------------------------------------
    let _ = prune_old_backups(&plan.target_path, retention_count);

    tracing::info!(
        target = %plan.target_path.display(),
        sha256 = %written_hash,
        "atomic write completed successfully"
    );

    Ok(())
}

/// `fsync` the parent directory. This is a best-effort operation: on Unix it
/// opens the directory and calls `sync_all`; on platforms where this fails it
/// is logged and silently skipped.
fn fsync_parent(parent: &Path) -> Result<()> {
    match fs::File::open(parent) {
        Ok(dir_file) => {
            if let Err(e) = dir_file.sync_all() {
                tracing::warn!(
                    dir = %parent.display(),
                    error = %e,
                    "fsync on parent directory failed (best-effort)"
                );
            }
        }
        Err(e) => {
            tracing::warn!(
                dir = %parent.display(),
                error = %e,
                "could not open parent dir for fsync (best-effort)"
            );
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 3. restore_backup
// ---------------------------------------------------------------------------

/// Restore a file at `target_path` from `backup_path`.
///
/// As a safety measure, the *current* target is backed up before the restore
/// overwrites it. The restored file is verified to be valid JSON before the
/// function returns.
pub fn restore_backup(backup_path: &Path, target_path: &Path) -> Result<()> {
    let bytes = fs::read(backup_path).map_err(|_| VapourflyError::FileNotFound {
        path: crate::SafePath::new(backup_path),
    })?;

    // Verify the backup is valid CloudStorageFile JSON before writing it.
    let _: CloudStorageFile =
        serde_json::from_slice(&bytes).map_err(|e| VapourflyError::ParseError {
            path: crate::SafePath::new(backup_path),
            format: "JSON".into(),
            reason: format!("backup file is not valid CloudStorageFile JSON: {e}"),
        })?;

    // Back up the current target before overwriting (if it exists).
    if target_path.exists() {
        let _ = create_backup(target_path, 0);
    }

    // Atomic restore: write to tmp then rename.
    let parent = target_path.parent().unwrap_or(Path::new("."));
    let target_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("target");
    let tmp = parent.join(format!(".{target_name}.vapourfly.restore.tmp"));

    {
        let mut tmp_file = fs::File::create(&tmp).map_err(|e| {
            VapourflyError::Internal(format!("failed to create restore tmp file: {e}"))
        })?;
        tmp_file.write_all(&bytes).map_err(|e| {
            VapourflyError::Internal(format!("failed to write restore tmp file: {e}"))
        })?;
        tmp_file.flush().map_err(|e| {
            VapourflyError::Internal(format!("failed to flush restore tmp file: {e}"))
        })?;
        tmp_file.sync_all().map_err(|e| {
            VapourflyError::Internal(format!("failed to fsync restore tmp file: {e}"))
        })?;
    }

    fs::rename(&tmp, target_path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        VapourflyError::Internal(format!("failed to rename restore tmp over target: {e}"))
    })?;

    tracing::info!(
        backup = %backup_path.display(),
        target = %target_path.display(),
        "backup restored"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// 4. list_backups
// ---------------------------------------------------------------------------

/// List all backups for the file at `target_path`, sorted by creation time
/// descending (most recent first).
pub fn list_backups(target_path: &Path) -> Result<Vec<BackupInfo>> {
    let target_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("cloud-storage-namespace-1.json");
    let parent = target_path.parent().unwrap_or(Path::new("."));

    let dir_entries = fs::read_dir(parent).map_err(|e| {
        VapourflyError::Internal(format!(
            "failed to read directory {}: {e}",
            parent.display()
        ))
    })?;

    let prefix_pattern = format!("{target_name}.{BACKUP_TAG}-");
    let mut backups: Vec<BackupInfo> = Vec::new();

    for entry in dir_entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let file_name = match entry.file_name().to_str().map(|s| s.to_owned()) {
            Some(n) => n,
            None => continue,
        };
        if !file_name.starts_with(&prefix_pattern) || !file_name.ends_with(".json") {
            continue;
        }
        let created_at = match parse_backup_timestamp(target_name, &file_name) {
            Some(t) => t,
            None => continue,
        };
        let path = entry.path();
        let sha256 = match fs::read(&path) {
            Ok(bytes) => compute_sha256(&bytes),
            Err(_) => continue,
        };
        backups.push(BackupInfo {
            path,
            created_at,
            sha256,
        });
    }

    backups.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.sha256.cmp(&a.sha256))
            .then_with(|| b.path.cmp(&a.path))
    });
    Ok(backups)
}

// ---------------------------------------------------------------------------
// 5. prune_old_backups
// ---------------------------------------------------------------------------

/// Delete all but the `keep_count` most recent backups for `target_path`.
///
/// Backups are identified by the naming convention used in [`create_backup`].
/// Deletion is best-effort; individual removal failures are logged but do not
/// stop the function from processing the remaining candidates.
pub fn prune_old_backups(target_path: &Path, keep_count: u32) -> Result<()> {
    let mut backups = list_backups(target_path)?;
    if backups.len() <= keep_count as usize {
        return Ok(());
    }

    // Backups are already sorted newest-first; remove the tail.
    let to_remove = backups.split_off(keep_count as usize);
    let mut removed = 0u32;

    for info in &to_remove {
        match fs::remove_file(&info.path) {
            Ok(()) => {
                removed += 1;
                tracing::debug!(backup = %info.path.display(), "pruned old backup");
            }
            Err(e) => {
                tracing::warn!(
                    backup = %info.path.display(),
                    error = %e,
                    "failed to prune backup"
                );
            }
        }
    }

    tracing::info!(
        target = %target_path.display(),
        removed,
        kept = keep_count,
        "backup pruning complete"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{CloudEntry, CloudStorageFile, WriteOp};
    use std::collections::BTreeMap;

    // -- Helpers --------------------------------------------------------------

    fn empty_cloud() -> CloudStorageFile {
        vec![]
    }

    fn cloud_with_collection(id: &str, app_ids: Vec<u32>) -> CloudStorageFile {
        let cv = serde_json::to_string(&crate::models::CollectionValue {
            id: id.to_owned(),
            name: id.to_owned(),
            added: app_ids,
            removed: vec![],
            extra: BTreeMap::new(),
        })
        .unwrap();
        vec![(
            format!("user-collections.{id}"),
            CloudEntry {
                key: format!("user-collections.{id}"),
                timestamp: Some(1700000000),
                value: Some(cv),
                version: Some("1".into()),
                is_deleted: None,
                conflict_resolution_method: None,
                str_method_id: None,
                extra: BTreeMap::new(),
            },
        )]
    }

    /// Write a cloud storage file to disk and return the path.
    fn write_cloud_to_file(cloud: &CloudStorageFile, dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let json = serde_json::to_string_pretty(cloud).unwrap();
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(json.as_bytes()).unwrap();
        f.flush().unwrap();
        f.sync_all().unwrap();
        path
    }

    /// Generate a valid WritePlan from a cloud file on disk.
    fn make_plan(target: PathBuf, cloud: &CloudStorageFile, ops: Vec<WriteOp>) -> WritePlan {
        crate::steam::write_plan::generate_write_plan(cloud, ops, target).unwrap()
    }

    // -- create_backup tests --------------------------------------------------

    #[test]
    fn backup_creation() {
        let tmp = tempfile::tempdir().unwrap();
        let cloud = cloud_with_collection("fav", vec![730, 440]);
        let target = write_cloud_to_file(&cloud, tmp.path(), "cloud-storage-namespace-1.json");

        let backup_path = create_backup(&target, 5).unwrap();

        assert!(backup_path.exists(), "backup file should exist");
        assert!(
            backup_path.to_string_lossy().contains(BACKUP_TAG),
            "backup filename should contain the backup tag"
        );

        // Contents should be identical.
        let original = fs::read(&target).unwrap();
        let backed_up = fs::read(&backup_path).unwrap();
        assert_eq!(original, backed_up);
    }

    #[test]
    fn backup_filename_contains_timestamp_and_sha() {
        let tmp = tempfile::tempdir().unwrap();
        let cloud = empty_cloud();
        let target = write_cloud_to_file(&cloud, tmp.path(), "data.json");

        let backup_path = create_backup(&target, 5).unwrap();
        let fname = backup_path.file_name().unwrap().to_str().unwrap();

        // Pattern: data.json.vapourfly-backup-{ts}-{short_sha}.json
        assert!(fname.starts_with("data.json.vapourfly-backup-"));
        assert!(fname.ends_with(".json"));

        let ts_part = fname
            .strip_prefix("data.json.vapourfly-backup-")
            .unwrap()
            .strip_suffix(".json")
            .unwrap();
        let ts_str = ts_part.split('-').next().unwrap();
        assert_eq!(ts_str.len(), 16, "timestamp should be YYYYMMDDTHHMMSSZ");
        assert!(ts_str.ends_with('Z'));
    }

    #[test]
    fn backup_not_found_for_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("does-not-exist.json");
        let result = create_backup(&missing, 5);
        assert!(result.is_err());
    }

    // -- list_backups tests ---------------------------------------------------

    #[test]
    fn list_returns_empty_when_no_backups() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("cloud.json");
        fs::write(&target, "[]").unwrap();

        let backups = list_backups(&target).unwrap();
        assert!(backups.is_empty());
    }

    #[test]
    fn list_returns_backups_sorted_descending() {
        let tmp = tempfile::tempdir().unwrap();
        let cloud = cloud_with_collection("a", vec![1]);
        let target = write_cloud_to_file(&cloud, tmp.path(), "cloud.json");

        let b1 = create_backup(&target, 5).unwrap();
        // Modify target so the hash changes (different short_sha).
        let cloud2 = cloud_with_collection("a", vec![1, 2]);
        let json2 = serde_json::to_string_pretty(&cloud2).unwrap();
        fs::write(&target, &json2).unwrap();
        let b2 = create_backup(&target, 5).unwrap();

        let backups = list_backups(&target).unwrap();
        assert_eq!(backups.len(), 2);
        // Both backups should be present.
        let paths: Vec<PathBuf> = backups.iter().map(|b| b.path.clone()).collect();
        assert!(paths.contains(&b1));
        assert!(paths.contains(&b2));
        assert!(backups[0].created_at >= backups[1].created_at);
        // sha256 is full 64-char hex.
        assert_eq!(backups[0].sha256.len(), 64);
    }

    // -- prune_old_backups tests ----------------------------------------------

    #[test]
    fn prune_keeps_specified_count() {
        let tmp = tempfile::tempdir().unwrap();
        let target = write_cloud_to_file(&empty_cloud(), tmp.path(), "c.json");

        for i in 0..3u32 {
            let c = cloud_with_collection("x", vec![i]);
            let json = serde_json::to_string_pretty(&c).unwrap();
            fs::write(&target, &json).unwrap();
            let _ = create_backup(&target, 10).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let before = list_backups(&target).unwrap();
        assert_eq!(before.len(), 3);

        prune_old_backups(&target, 1).unwrap();

        let after = list_backups(&target).unwrap();
        assert_eq!(after.len(), 1);
    }

    #[test]
    fn prune_noop_when_under_limit() {
        let tmp = tempfile::tempdir().unwrap();
        let target = write_cloud_to_file(&empty_cloud(), tmp.path(), "d.json");
        let _ = create_backup(&target, 10).unwrap();

        prune_old_backups(&target, 5).unwrap();

        let backups = list_backups(&target).unwrap();
        assert_eq!(backups.len(), 1, "should not have removed anything");
    }

    // -- execute_write_plan tests ---------------------------------------------

    #[test]
    fn execute_applies_operations_atomically() {
        let tmp = tempfile::tempdir().unwrap();
        let cloud = cloud_with_collection("fav", vec![730]);
        let target = write_cloud_to_file(&cloud, tmp.path(), "cloud.json");

        let plan = make_plan(
            target.clone(),
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "fav".into(),
                added: vec![730, 440],
                removed: vec![],
            }],
        );

        execute_write_plan(&plan, 3).unwrap();

        // Verify the file was updated.
        let written: CloudStorageFile =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        let (_, entry) = written
            .iter()
            .find(|(k, _)| k == "user-collections.fav")
            .unwrap();
        let cv: crate::models::CollectionValue =
            serde_json::from_str(entry.value.as_ref().unwrap()).unwrap();
        assert_eq!(cv.added, vec![440, 730]);
    }

    #[test]
    fn execute_rejects_stale_target() {
        let tmp = tempfile::tempdir().unwrap();
        let cloud = cloud_with_collection("fav", vec![730]);
        let target = write_cloud_to_file(&cloud, tmp.path(), "cloud.json");

        let plan = make_plan(
            target.clone(),
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "fav".into(),
                added: vec![730, 440],
                removed: vec![],
            }],
        );

        // Modify the target after plan generation (simulating concurrent edit).
        fs::write(&target, "corrupted").unwrap();

        let result = execute_write_plan(&plan, 3);
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("modified"),
            "error should indicate stale target: {msg}"
        );
    }

    #[test]
    fn execute_creates_backup_before_writing() {
        let tmp = tempfile::tempdir().unwrap();
        let cloud = cloud_with_collection("fav", vec![730]);
        let target = write_cloud_to_file(&cloud, tmp.path(), "cloud.json");

        let plan = make_plan(
            target.clone(),
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "fav".into(),
                added: vec![730, 440],
                removed: vec![],
            }],
        );

        execute_write_plan(&plan, 3).unwrap();

        let backups = list_backups(&target).unwrap();
        assert!(!backups.is_empty(), "at least one backup should exist");
    }

    #[test]
    fn execute_prunes_old_backups() {
        let tmp = tempfile::tempdir().unwrap();
        let cloud = cloud_with_collection("fav", vec![730]);
        let target = write_cloud_to_file(&cloud, tmp.path(), "cloud.json");

        // Run the plan multiple times to accumulate backups.
        for _ in 0..5 {
            let bytes = fs::read(&target).unwrap();
            let current: CloudStorageFile = serde_json::from_slice(&bytes).unwrap();
            let plan = make_plan(
                target.clone(),
                &current,
                vec![WriteOp::AddToHidden { app_ids: vec![999] }],
            );
            execute_write_plan(&plan, 2).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(50));
        }

        let backups = list_backups(&target).unwrap();
        assert!(
            backups.len() <= 2,
            "should have pruned to retention limit, found {}",
            backups.len()
        );
    }

    // -- restore_backup tests -------------------------------------------------

    #[test]
    fn restore_recovers_original_content() {
        let tmp = tempfile::tempdir().unwrap();
        let original_cloud = cloud_with_collection("fav", vec![730]);
        let target = write_cloud_to_file(&original_cloud, tmp.path(), "cloud.json");

        let backup_path = create_backup(&target, 5).unwrap();

        // Corrupt the target.
        fs::write(&target, "{}").unwrap();

        restore_backup(&backup_path, &target).unwrap();

        let restored: CloudStorageFile =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].0, "user-collections.fav");
    }

    #[test]
    fn restore_rejects_invalid_json_backup() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("cloud.json");
        fs::write(&target, "[]").unwrap();

        let bad_backup = tmp.path().join("bad-backup.json");
        fs::write(&bad_backup, "not json at all {{{").unwrap();

        let result = restore_backup(&bad_backup, &target);
        assert!(result.is_err());
    }

    #[test]
    fn restore_creates_safety_backup_of_current_target() {
        let tmp = tempfile::tempdir().unwrap();
        let cloud_a = cloud_with_collection("a", vec![1]);
        let cloud_b = cloud_with_collection("b", vec![2]);
        let target = write_cloud_to_file(&cloud_a, tmp.path(), "cloud.json");
        let backup_path = create_backup(&target, 5).unwrap();

        // Overwrite target with different content.
        let json_b = serde_json::to_string_pretty(&cloud_b).unwrap();
        fs::write(&target, &json_b).unwrap();

        let before_count = list_backups(&target).unwrap().len();

        restore_backup(&backup_path, &target).unwrap();

        // Should have one more backup (the safety backup of the overwritten content).
        let after_count = list_backups(&target).unwrap().len();
        assert_eq!(after_count, before_count + 1);

        // Restored content should match cloud_a.
        let restored: CloudStorageFile =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        assert_eq!(restored[0].0, "user-collections.a");
    }

    // -- Full round-trip: write then restore ----------------------------------

    #[test]
    fn backup_contains_original_content_before_write() {
        let tmp = tempfile::tempdir().unwrap();
        let cloud = cloud_with_collection("fav", vec![730]);
        let target = write_cloud_to_file(&cloud, tmp.path(), "cloud.json");

        let plan = make_plan(
            target.clone(),
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "fav".into(),
                added: vec![730, 440],
                removed: vec![],
            }],
        );

        execute_write_plan(&plan, 3).unwrap();

        // Verify the backup contains the original content (not the updated content).
        let backups = list_backups(&target).unwrap();
        assert!(!backups.is_empty());
        let backup_content: CloudStorageFile =
            serde_json::from_str(&fs::read_to_string(&backups[0].path).unwrap()).unwrap();
        let (_, entry) = backup_content
            .iter()
            .find(|(k, _)| k == "user-collections.fav")
            .unwrap();
        let cv: crate::models::CollectionValue =
            serde_json::from_str(entry.value.as_ref().unwrap()).unwrap();
        assert_eq!(
            cv.added,
            vec![730],
            "backup should contain original content"
        );
    }

    #[test]
    fn full_write_then_restore_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let cloud = cloud_with_collection("fav", vec![730]);
        let target = write_cloud_to_file(&cloud, tmp.path(), "cloud.json");

        // Execute a write plan.
        let plan = make_plan(
            target.clone(),
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "fav".into(),
                added: vec![730, 440, 223850],
                removed: vec![],
            }],
        );
        execute_write_plan(&plan, 5).unwrap();

        // Verify the write happened.
        let after: CloudStorageFile =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        let (_, entry) = after
            .iter()
            .find(|(k, _)| k == "user-collections.fav")
            .unwrap();
        let cv: crate::models::CollectionValue =
            serde_json::from_str(entry.value.as_ref().unwrap()).unwrap();
        assert_eq!(cv.added, vec![440, 730, 223850]);

        // Restore from the backup created during execute_write_plan.
        let backups = list_backups(&target).unwrap();
        assert!(!backups.is_empty());
        restore_backup(&backups[0].path, &target).unwrap();

        // Verify original content is back.
        let restored: CloudStorageFile =
            serde_json::from_str(&fs::read_to_string(&target).unwrap()).unwrap();
        let (_, entry) = restored
            .iter()
            .find(|(k, _)| k == "user-collections.fav")
            .unwrap();
        let cv: crate::models::CollectionValue =
            serde_json::from_str(entry.value.as_ref().unwrap()).unwrap();
        assert_eq!(cv.added, vec![730], "should be restored to original");
    }

    // -- BackupInfo parsing tests ---------------------------------------------

    #[test]
    fn parse_backup_timestamp_round_trip() {
        let name = "cloud.json.vapourfly-backup-20260624T123045Z-abcd1234.json";
        let dt = parse_backup_timestamp("cloud.json", name).unwrap();
        assert_eq!(dt.format("%Y%m%dT%H%M%SZ").to_string(), "20260624T123045Z");
    }

    #[test]
    fn parse_backup_timestamp_returns_none_for_malformed() {
        assert!(parse_backup_timestamp("x.json", "garbage.json").is_none());
        assert!(parse_backup_timestamp("x.json", "x.json.vapourfly-backup-bad.json").is_none());
    }
}
