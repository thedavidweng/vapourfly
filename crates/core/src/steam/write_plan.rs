//! Write plan generation for Vapourfly.
//!
//! A [`WritePlan`] describes a set of mutations to apply to a cloud storage
//! file.  It carries before/after SHA-256 hashes, the serialised result, and a
//! human-readable diff so that callers can preview changes before committing.

use std::collections::BTreeMap;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

use crate::error::{Result, VapourflyError};
use crate::models::{
    CloudEntry, CloudStorageFile, CollectionChange, CollectionValue, WriteOp, WritePlan,
    WritePlanDiff,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Generate a [`WritePlan`] by applying `operations` to a copy of `cloud`.
///
/// 1. Reads the file at `target_path` and computes `before_sha256`.
/// 2. Applies each operation to a deep copy of the cloud storage.
/// 3. Serialises the result and computes `after_sha256`.
/// 4. Builds a human-readable diff.
/// 5. Validates the generated JSON can be round-tripped.
/// 6. Derives `backup_path` and `tmp_path` from `target_path`.
pub fn generate_write_plan(
    cloud: &CloudStorageFile,
    operations: Vec<WriteOp>,
    target_path: PathBuf,
) -> Result<WritePlan> {
    // 1. Read original bytes for before hash
    let original_bytes = std::fs::read(&target_path).map_err(|_| VapourflyError::FileNotFound {
        path: crate::SafePath::new(&target_path),
    })?;
    let before_sha256 = hex_sha256(&original_bytes);

    // 2. Snapshot the original state for diff computation
    let original_snapshot = cloud.clone();

    // 3. Apply operations to a mutable copy
    let mut modified = cloud.clone();
    let now_unix = chrono::Utc::now().timestamp();

    for op in &operations {
        match op {
            WriteOp::UpsertCollection { id, added, .. } => {
                // Use the collection id as the display name; the caller can
                // override the name via a separate operation if needed.
                upsert_collection(&mut modified, id, id, added.clone(), now_unix)?;
            }
            WriteOp::AddToHidden { app_ids } => {
                merge_hidden(&mut modified, app_ids, now_unix)?;
            }
        }
    }

    // 4. Serialise and compute after hash
    let after_json =
        serde_json::to_string_pretty(&modified).map_err(|e| VapourflyError::ParseError {
            path: crate::SafePath::new(&target_path),
            format: "JSON".into(),
            reason: format!("serialisation failed: {e}"),
        })?;
    let after_sha256 = hex_sha256(after_json.as_bytes());

    // 5. Validate round-trip
    let _: CloudStorageFile =
        serde_json::from_str(&after_json).map_err(|e| VapourflyError::ParseError {
            path: crate::SafePath::new(&target_path),
            format: "JSON".into(),
            reason: format!("round-trip validation failed: {e}"),
        })?;

    // 6. Generate diff
    let diff = compute_diff(&original_snapshot, &modified, &operations);

    // 7. Derive backup and tmp paths
    let file_name = target_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("cloud-storage-namespace-1.json");
    let parent = target_path.parent().unwrap_or(std::path::Path::new("."));
    let backup_path = parent.join(format!("{file_name}.vapourfly-backup-1.json"));
    let tmp_path = parent.join(format!(".{file_name}.vapourfly.tmp"));

    Ok(WritePlan {
        target_path,
        backup_path,
        tmp_path,
        before_sha256,
        after_sha256,
        after_content: after_json.into_bytes(),
        operations,
        diff,
    })
}

/// Insert or update a collection in the cloud storage.
///
/// * Outer key = `user-collections.{collection_id}`
/// * `entry.key` = same full key
/// * `CollectionValue.id` = the short `collection_id`
/// * `removed` = `[]` (full-set writes)
/// * Preserves existing entry metadata (`version`, `conflictResolutionMethod`,
///   `strMethodId`, `extra`)
/// * Deduplicates and sorts AppIDs
/// * Rejects collection IDs with whitespace, slash, backslash, control chars,
///   or `..`
pub fn upsert_collection(
    cloud: &mut CloudStorageFile,
    collection_id: &str,
    name: &str,
    app_ids: Vec<u32>,
    now_unix: i64,
) -> Result<()> {
    validate_collection_id(collection_id)?;

    let outer_key = format!("user-collections.{collection_id}");

    // Deduplicate and sort AppIDs
    let mut app_ids = app_ids;
    app_ids.sort_unstable();
    app_ids.dedup();

    // Build the value JSON — removed is always [] for full-set writes
    let cv = CollectionValue {
        id: collection_id.to_owned(),
        name: name.to_owned(),
        added: app_ids,
        removed: vec![],
        extra: BTreeMap::new(),
    };
    let value_json = serde_json::to_string(&cv).map_err(|e| {
        VapourflyError::Internal(format!("failed to serialise CollectionValue: {e}"))
    })?;

    // Find existing entry or create new one
    if let Some((_, entry)) = cloud.iter_mut().find(|(k, _)| k == &outer_key) {
        // Preserve existing metadata, update mutable fields
        entry.key = outer_key;
        entry.timestamp = Some(now_unix);
        entry.value = Some(value_json);
        entry.is_deleted = Some(false);
        // version, conflictResolutionMethod, strMethodId, extra are untouched
    } else {
        cloud.push((
            outer_key.clone(),
            CloudEntry {
                key: outer_key,
                timestamp: Some(now_unix),
                value: Some(value_json),
                version: None,
                is_deleted: Some(false),
                conflict_resolution_method: None,
                str_method_id: None,
                extra: BTreeMap::new(),
            },
        ));
    }

    Ok(())
}

/// Merge new AppIDs into the hidden collection.
///
/// Never removes existing hidden AppIDs.  If the hidden entry does not exist
/// it is created with the name "Hidden".
pub fn merge_hidden(
    cloud: &mut CloudStorageFile,
    new_app_ids: &[u32],
    now_unix: i64,
) -> Result<()> {
    let outer_key = "user-collections.hidden";

    // Collect existing hidden AppIDs
    let mut merged: Vec<u32> = cloud
        .iter()
        .find(|(k, _)| k == outer_key)
        .and_then(|(_, entry)| entry.value.as_deref())
        .and_then(|v| serde_json::from_str::<CollectionValue>(v).ok())
        .map(|cv| cv.added)
        .unwrap_or_default();

    // Merge — additive only, never remove
    merged.extend_from_slice(new_app_ids);
    merged.sort_unstable();
    merged.dedup();

    // Build value
    let cv = CollectionValue {
        id: "hidden".to_owned(),
        name: "Hidden".to_owned(),
        added: merged,
        removed: vec![],
        extra: BTreeMap::new(),
    };
    let value_json = serde_json::to_string(&cv).map_err(|e| {
        VapourflyError::Internal(format!("failed to serialise hidden CollectionValue: {e}"))
    })?;

    if let Some((_, entry)) = cloud.iter_mut().find(|(k, _)| k == outer_key) {
        entry.key = outer_key.to_owned();
        entry.timestamp = Some(now_unix);
        entry.value = Some(value_json);
        entry.is_deleted = Some(false);
    } else {
        cloud.push((
            outer_key.to_owned(),
            CloudEntry {
                key: outer_key.to_owned(),
                timestamp: Some(now_unix),
                value: Some(value_json),
                version: None,
                is_deleted: Some(false),
                conflict_resolution_method: None,
                str_method_id: None,
                extra: BTreeMap::new(),
            },
        ));
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Reject collection IDs that contain whitespace, slashes, backslashes,
/// control characters, or `..` path traversal sequences.
fn validate_collection_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(VapourflyError::InvalidInput(
            "collection ID must not be empty".into(),
        ));
    }
    if id.contains("..") {
        return Err(VapourflyError::InvalidInput(format!(
            "collection ID must not contain '..': {id:?}"
        )));
    }
    if id.contains(|c: char| c.is_whitespace()) {
        return Err(VapourflyError::InvalidInput(format!(
            "collection ID must not contain whitespace: {id:?}"
        )));
    }
    if id.contains('/') || id.contains('\\') {
        return Err(VapourflyError::InvalidInput(format!(
            "collection ID must not contain slashes: {id:?}"
        )));
    }
    if id.contains(|c: char| c.is_control()) {
        return Err(VapourflyError::InvalidInput(format!(
            "collection ID must not contain control characters: {id:?}"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Diff computation
// ---------------------------------------------------------------------------

fn compute_diff(
    before: &CloudStorageFile,
    _after: &CloudStorageFile,
    operations: &[WriteOp],
) -> WritePlanDiff {
    let mut diff = WritePlanDiff::default();

    // Index the "before" state by outer key
    let before_map: BTreeMap<&str, &CloudEntry> =
        before.iter().map(|(k, e)| (k.as_str(), e)).collect();

    // Walk operations to find which collections changed
    for op in operations {
        match op {
            WriteOp::UpsertCollection { id, added, removed } => {
                let outer_key = format!("user-collections.{id}");
                let action = if before_map.contains_key(outer_key.as_str())
                    && !is_deleted(before_map[outer_key.as_str()])
                {
                    "updated"
                } else {
                    "created"
                };
                diff.collections_changed.push(CollectionChange {
                    id: id.clone(),
                    action: action.to_owned(),
                });

                // Compute effective AppID changes for this collection
                let old_ids = parse_entry_app_ids(before_map.get(outer_key.as_str()).copied());
                let new_ids_set: std::collections::HashSet<u32> = added.iter().copied().collect();
                let removed_set: std::collections::HashSet<u32> = removed.iter().copied().collect();

                for aid in added {
                    if !old_ids.contains(aid) && !removed_set.contains(aid) {
                        diff.app_ids_added.push(*aid);
                    }
                }
                for aid in &old_ids {
                    if removed_set.contains(aid)
                        || (!new_ids_set.is_empty() && !new_ids_set.contains(aid))
                    {
                        diff.app_ids_removed.push(*aid);
                    }
                }
            }
            WriteOp::AddToHidden { app_ids } => {
                let old_ids =
                    parse_entry_app_ids(before_map.get("user-collections.hidden").copied());
                for aid in app_ids {
                    if !old_ids.contains(aid) {
                        diff.hidden_app_ids_added.push(*aid);
                    }
                }
            }
        }
    }

    diff.app_ids_added.sort_unstable();
    diff.app_ids_added.dedup();
    diff.app_ids_removed.sort_unstable();
    diff.app_ids_removed.dedup();
    diff.hidden_app_ids_added.sort_unstable();
    diff.hidden_app_ids_added.dedup();

    // Count unchanged and skipped-deleted entries
    let mut changed_keys: std::collections::HashSet<String> = std::collections::HashSet::new();
    for c in &diff.collections_changed {
        changed_keys.insert(format!("user-collections.{}", c.id));
    }
    if !diff.hidden_app_ids_added.is_empty() {
        changed_keys.insert("user-collections.hidden".to_owned());
    }

    for (key, entry) in before {
        if !key.starts_with("user-collections.") {
            continue;
        }
        if changed_keys.contains(key.as_str()) {
            continue;
        }
        if is_deleted(entry) {
            diff.skipped_deleted_count += 1;
        } else {
            diff.unchanged_count += 1;
        }
    }

    diff
}

fn is_deleted(entry: &CloudEntry) -> bool {
    entry.is_deleted == Some(true)
}

/// Parse the `added` list from an entry's value JSON, returning an empty set
/// if the entry is missing, deleted, or unparseable.
fn parse_entry_app_ids(entry: Option<&CloudEntry>) -> std::collections::HashSet<u32> {
    let entry = match entry {
        Some(e) => e,
        None => return std::collections::HashSet::new(),
    };
    if is_deleted(entry) {
        return std::collections::HashSet::new();
    }
    let value_str = match &entry.value {
        Some(v) => v,
        None => return std::collections::HashSet::new(),
    };
    let cv: CollectionValue = match serde_json::from_str(value_str) {
        Ok(v) => v,
        Err(_) => return std::collections::HashSet::new(),
    };
    cv.added.into_iter().collect()
}

/// Compute SHA-256 hash of data and return as hex string.
pub fn compute_sha256(data: &[u8]) -> String {
    hex_sha256(data)
}

fn hex_sha256(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

// Minimal hex encode (avoids pulling in the `hex` crate)
mod hex {
    pub fn encode(bytes: impl IntoIterator<Item = u8>) -> String {
        let mut s = String::with_capacity(64);
        for b in bytes {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
        }
        s
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn empty_cloud() -> CloudStorageFile {
        vec![]
    }

    fn cloud_with_favorite() -> CloudStorageFile {
        vec![(
            "user-collections.favorite".into(),
            CloudEntry {
                key: "user-collections.favorite".into(),
                timestamp: Some(1774229861),
                value: Some(
                    r#"{"id":"favorite","name":"Favorites","added":[730,427520],"removed":[]}"#
                        .into(),
                ),
                version: Some("1880".into()),
                is_deleted: None,
                conflict_resolution_method: None,
                str_method_id: None,
                extra: BTreeMap::new(),
            },
        )]
    }

    fn cloud_with_hidden() -> CloudStorageFile {
        vec![(
            "user-collections.hidden".into(),
            CloudEntry {
                key: "user-collections.hidden".into(),
                timestamp: Some(1737614952),
                value: Some(r#"{"id":"hidden","name":"Hidden","added":[440],"removed":[]}"#.into()),
                version: Some("1674".into()),
                is_deleted: None,
                conflict_resolution_method: None,
                str_method_id: None,
                extra: BTreeMap::new(),
            },
        )]
    }

    fn write_temp_cloud(cloud: &CloudStorageFile) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        let json = serde_json::to_string_pretty(cloud).unwrap();
        f.write_all(json.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    // -- upsert_collection tests --------------------------------------------

    #[test]
    fn upsert_new_collection() {
        let mut cloud = empty_cloud();
        let now = 1700000000;
        upsert_collection(&mut cloud, "my-games", "My Games", vec![730, 440], now).unwrap();

        assert_eq!(cloud.len(), 1);
        let (key, entry) = &cloud[0];
        assert_eq!(key, "user-collections.my-games");
        assert_eq!(entry.key, "user-collections.my-games");
        assert_eq!(entry.timestamp, Some(now));
        assert_eq!(entry.is_deleted, Some(false));
        assert!(entry.version.is_none());

        let cv: CollectionValue = serde_json::from_str(entry.value.as_ref().unwrap()).unwrap();
        assert_eq!(cv.id, "my-games");
        assert_eq!(cv.name, "My Games");
        assert_eq!(cv.added, vec![440, 730]); // sorted
        assert!(cv.removed.is_empty());
    }

    #[test]
    fn upsert_existing_collection_preserves_metadata() {
        let mut cloud = cloud_with_favorite();
        let now = 1800000000;
        upsert_collection(&mut cloud, "favorite", "Favorites", vec![730, 440], now).unwrap();

        assert_eq!(cloud.len(), 1);
        let (_, entry) = &cloud[0];
        // Metadata preserved
        assert_eq!(entry.version, Some("1880".into()));
        assert_eq!(entry.timestamp, Some(now));
        assert_eq!(entry.is_deleted, Some(false));

        let cv: CollectionValue = serde_json::from_str(entry.value.as_ref().unwrap()).unwrap();
        assert_eq!(cv.added, vec![440, 730]);
        // removed is always [] for full-set writes
        assert!(cv.removed.is_empty());
    }

    #[test]
    fn upsert_deduplicates_app_ids() {
        let mut cloud = empty_cloud();
        upsert_collection(&mut cloud, "dup", "Dup", vec![730, 730, 440, 440], 0).unwrap();

        let cv: CollectionValue = serde_json::from_str(cloud[0].1.value.as_ref().unwrap()).unwrap();
        assert_eq!(cv.added, vec![440, 730]);
    }

    // -- merge_hidden tests -------------------------------------------------

    #[test]
    fn merge_hidden_additive() {
        let mut cloud = cloud_with_hidden(); // hidden has [440]
        merge_hidden(&mut cloud, &[730, 427520], 1800000000).unwrap();

        let (_, entry) = cloud
            .iter()
            .find(|(k, _)| k == "user-collections.hidden")
            .unwrap();
        let cv: CollectionValue = serde_json::from_str(entry.value.as_ref().unwrap()).unwrap();
        assert_eq!(cv.added, vec![440, 730, 427520]);
        assert_eq!(cv.name, "Hidden");
    }

    #[test]
    fn merge_hidden_creates_when_absent() {
        let mut cloud = empty_cloud();
        merge_hidden(&mut cloud, &[730], 1800000000).unwrap();

        assert_eq!(cloud.len(), 1);
        let (key, entry) = &cloud[0];
        assert_eq!(key, "user-collections.hidden");
        let cv: CollectionValue = serde_json::from_str(entry.value.as_ref().unwrap()).unwrap();
        assert_eq!(cv.id, "hidden");
        assert_eq!(cv.name, "Hidden");
        assert_eq!(cv.added, vec![730]);
    }

    #[test]
    fn merge_hidden_deduplicates() {
        let mut cloud = cloud_with_hidden(); // has [440]
        merge_hidden(&mut cloud, &[440, 730], 1800000000).unwrap();

        let (_, entry) = cloud
            .iter()
            .find(|(k, _)| k == "user-collections.hidden")
            .unwrap();
        let cv: CollectionValue = serde_json::from_str(entry.value.as_ref().unwrap()).unwrap();
        assert_eq!(cv.added, vec![440, 730]);
    }

    // -- collection ID validation -------------------------------------------

    #[test]
    fn invalid_collection_id_whitespace() {
        let mut cloud = empty_cloud();
        let result = upsert_collection(&mut cloud, "my games", "X", vec![], 0);
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("whitespace"));
    }

    #[test]
    fn invalid_collection_id_slash() {
        let mut cloud = empty_cloud();
        let result = upsert_collection(&mut cloud, "a/b", "X", vec![], 0);
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("slashes"));
    }

    #[test]
    fn invalid_collection_id_backslash() {
        let mut cloud = empty_cloud();
        let result = upsert_collection(&mut cloud, "a\\b", "X", vec![], 0);
        assert!(result.is_err());
    }

    #[test]
    fn invalid_collection_id_dotdot() {
        let mut cloud = empty_cloud();
        let result = upsert_collection(&mut cloud, "..", "X", vec![], 0);
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains(".."));
    }

    #[test]
    fn invalid_collection_id_control_char() {
        let mut cloud = empty_cloud();
        let result = upsert_collection(&mut cloud, "bad\x01id", "X", vec![], 0);
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("control"));
    }

    #[test]
    fn invalid_collection_id_empty() {
        let mut cloud = empty_cloud();
        let result = upsert_collection(&mut cloud, "", "X", vec![], 0);
        assert!(result.is_err());
        assert!(format!("{}", result.unwrap_err()).contains("empty"));
    }

    #[test]
    fn validate_id_allows_hyphens_and_underscores() {
        let mut cloud = empty_cloud();
        // Should not error
        upsert_collection(&mut cloud, "from-tag-Indie", "Indie", vec![], 0).unwrap();
        upsert_collection(&mut cloud, "my_collection", "Mine", vec![], 0).unwrap();
    }

    // -- generate_write_plan tests ------------------------------------------

    #[test]
    fn generate_write_plan_upsert_new() {
        let cloud = cloud_with_favorite();
        let tmp = write_temp_cloud(&cloud);
        let path = tmp.path().to_path_buf();

        let plan = generate_write_plan(
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "rpg".into(),
                added: vec![220, 440],
                removed: vec![],
            }],
            path,
        )
        .unwrap();

        assert_eq!(plan.before_sha256.len(), 64);
        assert_eq!(plan.after_sha256.len(), 64);
        assert_ne!(plan.before_sha256, plan.after_sha256);
        assert_eq!(plan.diff.collections_changed.len(), 1);
        assert_eq!(plan.diff.collections_changed[0].id, "rpg");
        assert_eq!(plan.diff.collections_changed[0].action, "created");
        assert_eq!(plan.diff.app_ids_added, vec![220, 440]);
        assert!(plan.diff.app_ids_removed.is_empty());
        assert_eq!(plan.diff.unchanged_count, 1); // favorite untouched
    }

    #[test]
    fn generate_write_plan_upsert_existing() {
        let cloud = cloud_with_favorite();
        let tmp = write_temp_cloud(&cloud);
        let path = tmp.path().to_path_buf();

        let plan = generate_write_plan(
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "favorite".into(),
                added: vec![730, 427520, 440],
                removed: vec![],
            }],
            path,
        )
        .unwrap();

        assert_eq!(plan.diff.collections_changed[0].action, "updated");
        assert_eq!(plan.diff.app_ids_added, vec![440]);
        assert!(plan.diff.app_ids_removed.is_empty());
    }

    #[test]
    fn generate_write_plan_hidden_add() {
        let cloud = cloud_with_favorite();
        let tmp = write_temp_cloud(&cloud);
        let path = tmp.path().to_path_buf();

        let plan = generate_write_plan(
            &cloud,
            vec![WriteOp::AddToHidden {
                app_ids: vec![730, 999],
            }],
            path,
        )
        .unwrap();

        // No hidden entry exists, so both are new
        assert_eq!(plan.diff.hidden_app_ids_added, vec![730, 999]);
        assert!(plan.diff.collections_changed.is_empty());
        assert_eq!(plan.diff.unchanged_count, 1); // favorite untouched
    }

    #[test]
    fn generate_write_plan_diff_preserves_existing_hidden() {
        let mut cloud = cloud_with_hidden(); // hidden has [440]
        cloud.push((
            "sc-version".into(),
            CloudEntry {
                key: "sc-version".into(),
                timestamp: Some(100),
                value: Some("6".into()),
                version: Some("1".into()),
                is_deleted: None,
                conflict_resolution_method: None,
                str_method_id: None,
                extra: BTreeMap::new(),
            },
        ));
        let tmp = write_temp_cloud(&cloud);
        let path = tmp.path().to_path_buf();

        let plan = generate_write_plan(
            &cloud,
            vec![WriteOp::AddToHidden {
                app_ids: vec![440, 730],
            }],
            path,
        )
        .unwrap();

        // 440 already existed, only 730 is new
        assert_eq!(plan.diff.hidden_app_ids_added, vec![730]);
        // hidden is changed, sc-version is not a collection
        assert_eq!(plan.diff.unchanged_count, 0);
    }

    #[test]
    fn generate_write_plan_skipped_deleted() {
        let mut cloud = cloud_with_favorite();
        cloud.push((
            "user-collections.old".into(),
            CloudEntry {
                key: "user-collections.old".into(),
                timestamp: Some(100),
                value: None,
                version: Some("1".into()),
                is_deleted: Some(true),
                conflict_resolution_method: None,
                str_method_id: None,
                extra: BTreeMap::new(),
            },
        ));
        let tmp = write_temp_cloud(&cloud);
        let path = tmp.path().to_path_buf();

        let plan = generate_write_plan(
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "new".into(),
                added: vec![1],
                removed: vec![],
            }],
            path,
        )
        .unwrap();

        assert_eq!(plan.diff.skipped_deleted_count, 1);
        assert_eq!(plan.diff.unchanged_count, 1); // favorite
    }

    #[test]
    fn generate_write_plan_invalid_id_returns_error() {
        let cloud = empty_cloud();
        let tmp = write_temp_cloud(&cloud);
        let path = tmp.path().to_path_buf();

        let result = generate_write_plan(
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "bad id".into(),
                added: vec![],
                removed: vec![],
            }],
            path,
        );
        assert!(result.is_err());
    }

    #[test]
    fn generate_write_plan_json_is_valid() {
        let cloud = cloud_with_favorite();
        let tmp = write_temp_cloud(&cloud);
        let path = tmp.path().to_path_buf();

        let plan = generate_write_plan(
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "test".into(),
                added: vec![1, 2, 3],
                removed: vec![],
            }],
            path,
        )
        .unwrap();

        // The plan was generated without error, meaning round-trip validation passed.
        assert_eq!(plan.after_sha256.len(), 64);
    }

    #[test]
    fn plan_paths_are_derived_correctly() {
        let cloud = empty_cloud();
        let tmp = write_temp_cloud(&cloud);
        let path = tmp.path().to_path_buf();

        let plan = generate_write_plan(&cloud, vec![], path.clone()).unwrap();

        assert_eq!(plan.target_path, path);
        let parent = path.parent().unwrap();
        let fname = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(
            plan.backup_path,
            parent.join(format!("{fname}.vapourfly-backup-1.json"))
        );
        assert_eq!(
            plan.tmp_path,
            parent.join(format!(".{fname}.vapourfly.tmp"))
        );
    }

    #[test]
    fn empty_operations_produces_unchanged_diff() {
        let cloud = cloud_with_favorite();
        let tmp = write_temp_cloud(&cloud);
        let path = tmp.path().to_path_buf();

        let plan = generate_write_plan(&cloud, vec![], path).unwrap();

        assert_eq!(plan.before_sha256, plan.after_sha256);
        assert!(plan.diff.collections_changed.is_empty());
        assert!(plan.diff.app_ids_added.is_empty());
        assert!(plan.diff.app_ids_removed.is_empty());
        assert!(plan.diff.hidden_app_ids_added.is_empty());
        assert_eq!(plan.diff.unchanged_count, 1); // favorite
        assert_eq!(plan.diff.skipped_deleted_count, 0);
    }
}
