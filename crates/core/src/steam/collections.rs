//! Parse `cloud-storage-namespace-1.json` for user collections and hidden state.
//!
//! The file is a JSON array of `[outer_key, CloudEntry]` pairs.
//! Collections are keyed `user-collections.{id}`.

use std::fs;
use std::path::Path;

use crate::error::{Result, VapourflyError};
use crate::models::{CloudStorageFile, CollectionValue, SteamCollection};

/// Read the raw cloud storage file.
pub fn read_cloud_storage(path: &Path) -> Result<CloudStorageFile> {
    let content = fs::read_to_string(path).map_err(|_| VapourflyError::FileNotFound {
        path: crate::SafePath::new(path),
    })?;

    serde_json::from_str(&content).map_err(|e| VapourflyError::ParseError {
        path: crate::SafePath::new(path),
        format: "JSON".into(),
        reason: e.to_string(),
    })
}

/// Extract user collections from a parsed cloud storage file.
///
/// - Filters keys starting with `user-collections.`
/// - Skips entries where `is_deleted == true`
/// - Parses `entry.value` as `CollectionValue`
/// - Computes effective AppIDs as `added - removed`
/// - Identifies hidden collection
/// - Sorts by name then id
pub fn read_user_collections(cloud: &CloudStorageFile) -> Result<Vec<SteamCollection>> {
    let mut collections = Vec::new();

    for (outer_key, entry) in cloud {
        // Only user-collections
        if !outer_key.starts_with("user-collections.") {
            continue;
        }

        // Skip deleted
        if entry.is_deleted == Some(true) {
            continue;
        }

        // Must have a value
        let value_str = match &entry.value {
            Some(v) => v,
            None => continue,
        };

        // Parse collection value
        let cv: CollectionValue = match serde_json::from_str(value_str) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(
                    key = %outer_key,
                    error = %e,
                    "skipping unparseable collection entry"
                );
                continue;
            }
        };

        // Compute effective AppIDs = added - removed
        let removed_set: std::collections::HashSet<u32> = cv.removed.iter().copied().collect();
        let mut app_ids: Vec<u32> = cv
            .added
            .iter()
            .copied()
            .filter(|id| !removed_set.contains(id))
            .collect();
        app_ids.sort_unstable();
        app_ids.dedup();

        let is_hidden = cv.id == "hidden" || outer_key == "user-collections.hidden";

        collections.push(SteamCollection {
            id: cv.id,
            name: cv.name,
            app_ids,
            removed_app_ids: cv.removed,
            is_hidden_collection: is_hidden,
        });
    }

    // Sort by name then id
    collections.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));

    Ok(collections)
}

/// Find the hidden collection in a list.
pub fn find_hidden_collection(collections: &[SteamCollection]) -> Option<&SteamCollection> {
    collections.iter().find(|c| c.is_hidden_collection)
}

/// Get all AppIDs from the hidden collection.
pub fn get_all_hidden_app_ids(collections: &[SteamCollection]) -> Vec<u32> {
    find_hidden_collection(collections)
        .map(|c| c.app_ids.clone())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_fixture_cloud_storage() {
        let path = Path::new(
            "../../data/fixtures/steam_minimal/userdata/76561198000000000/config/cloudstorage/cloud-storage-namespace-1.json",
        );
        let cloud = read_cloud_storage(path).unwrap();
        let collections = read_user_collections(&cloud).unwrap();

        // Should have: favorite, hidden, from-tag-Indie (deleted-one is skipped, sc-version is not a collection)
        let names: Vec<&str> = collections.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"Favorites"));
        assert!(names.contains(&"Hidden"));
        assert!(names.contains(&"Indie"));

        // Deleted collection should be skipped
        assert!(!names.contains(&"deleted-one"));
    }

    #[test]
    fn favorite_collection_has_correct_apps() {
        let path = Path::new(
            "../../data/fixtures/steam_minimal/userdata/76561198000000000/config/cloudstorage/cloud-storage-namespace-1.json",
        );
        let cloud = read_cloud_storage(path).unwrap();
        let collections = read_user_collections(&cloud).unwrap();

        let fav = collections.iter().find(|c| c.id == "favorite").unwrap();
        assert_eq!(fav.app_ids, vec![730, 427520]);
        assert!(!fav.is_hidden_collection);
    }

    #[test]
    fn hidden_collection_detected() {
        let path = Path::new(
            "../../data/fixtures/steam_minimal/userdata/76561198000000000/config/cloudstorage/cloud-storage-namespace-1.json",
        );
        let cloud = read_cloud_storage(path).unwrap();
        let collections = read_user_collections(&cloud).unwrap();

        let hidden = find_hidden_collection(&collections).unwrap();
        assert!(hidden.is_hidden_collection);
        assert_eq!(hidden.id, "hidden");
        assert!(hidden.app_ids.is_empty());
    }

    #[test]
    fn empty_cloud_storage() {
        let path = Path::new(
            "../../data/fixtures/empty_cloudstorage/userdata/76561198000000000/config/cloudstorage/cloud-storage-namespace-1.json",
        );
        let cloud = read_cloud_storage(path).unwrap();
        let collections = read_user_collections(&cloud).unwrap();
        assert!(collections.is_empty());
    }

    #[test]
    fn malformed_cloud_storage_fails() {
        let path = Path::new(
            "../../data/fixtures/malformed_cloudstorage/userdata/76561198000000000/config/cloudstorage/cloud-storage-namespace-1.json",
        );
        let result = read_cloud_storage(path);
        assert!(result.is_err());
    }
}
