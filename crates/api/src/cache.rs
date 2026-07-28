//! On-disk cache for Vapourfly external API responses.
//!
//! Responses are stored under `{cache_root}/vapourfly/cache/{source}/{key}.json`.
//! Each file contains a JSON-serialized [`CacheRecord<T>`](crate::http::CacheRecord).
//!
//! The cache supports:
//! - TTL-based freshness checking
//! - Stale-while-revalidate semantics (return stale data when network fails)
//! - ETag preservation for conditional requests

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::http::CacheRecord;
use vapourfly_core::error::{Result, VapourflyError};

/// On-disk cache for API responses.
///
/// Each cached record is stored as a JSON file at:
/// `{cache_root}/vapourfly/cache/{source}/{key}.json`
pub struct DiskCache {
    /// Root of the cache tree, typically `{app_data}/vapourfly/cache/`.
    root: PathBuf,
}

impl DiskCache {
    /// Create a new disk cache rooted at the given directory.
    ///
    /// The directory is created on first write if it doesn't exist.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Return the filesystem path for a given source + key.
    pub fn path_for(&self, source: &str, key: &str) -> PathBuf {
        let safe_key = sanitize_key(key);
        self.root.join(source).join(format!("{safe_key}.json"))
    }

    /// Read a cached record from disk.
    ///
    /// Returns `Ok(None)` when the file doesn't exist or can't be deserialized.
    /// The `stale` flag on the returned record is set based on TTL comparison.
    pub fn get<T>(&self, source: &str, key: &str) -> Result<Option<CacheRecord<T>>>
    where
        T: for<'de> Deserialize<'de>,
    {
        let path = self.path_for(source, key);
        if !path.exists() {
            return Ok(None);
        }

        let bytes = std::fs::read(&path).map_err(|_e| VapourflyError::FileNotFound {
            path: vapourfly_core::error::SafePath::new(&path),
        })?;

        let mut record: CacheRecord<T> =
            serde_json::from_slice(&bytes).map_err(|e| VapourflyError::ParseError {
                path: vapourfly_core::error::SafePath::new(&path),
                format: "JSON".to_string(),
                reason: e.to_string(),
            })?;

        record.stale = record.is_expired();
        Ok(Some(record))
    }

    /// Write a record to disk, creating parent directories as needed.
    ///
    /// The write is atomic: data is written to a temporary file in the same
    /// directory and then renamed over the target. This prevents cache
    /// corruption if the process is interrupted mid-write.
    pub fn put<T: Serialize>(&self, record: &CacheRecord<T>) -> Result<()> {
        let path = self.path_for(&record.source, &record.key);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                VapourflyError::Internal(format!(
                    "failed to create cache directory {}: {e}",
                    parent.display()
                ))
            })?;
        }

        let json = serde_json::to_vec_pretty(record).map_err(|e| {
            VapourflyError::Internal(format!("failed to serialize cache record: {e}"))
        })?;

        let tmp_path = path.with_extension("json.tmp");
        std::fs::write(&tmp_path, &json).map_err(|e| {
            VapourflyError::Internal(format!(
                "failed to write cache tmp file {}: {e}",
                tmp_path.display()
            ))
        })?;
        std::fs::rename(&tmp_path, &path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            VapourflyError::Internal(format!(
                "failed to rename cache tmp file {} -> {}: {e}",
                tmp_path.display(),
                path.display()
            ))
        })?;

        Ok(())
    }
}

/// Sanitize a cache key for use as a filename.
///
/// Replaces `/` and `\` with `_` to flatten path-like keys (e.g. "app/292030/details")
/// into safe filenames.
fn sanitize_key(key: &str) -> String {
    key.replace(['/', '\\', ':'], "_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tempfile::TempDir;

    fn make_cache() -> (DiskCache, TempDir) {
        let dir = TempDir::new().unwrap();
        let cache = DiskCache::new(dir.path());
        (cache, dir)
    }

    fn make_record<T>(source: &str, key: &str, data: T) -> CacheRecord<T> {
        CacheRecord {
            source: source.to_string(),
            key: key.to_string(),
            fetched_at: chrono::Utc::now(),
            ttl: Duration::from_secs(3600),
            data,
            stale: false,
            etag: None,
        }
    }

    #[test]
    fn put_then_get_roundtrip() {
        let (cache, _dir) = make_cache();
        let record = make_record("igdb", "game/12345", "witcher data".to_string());

        cache.put(&record).unwrap();
        let loaded: Option<CacheRecord<String>> = cache.get("igdb", "game/12345").unwrap();

        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.source, "igdb");
        assert_eq!(loaded.key, "game/12345");
        assert_eq!(loaded.data, "witcher data");
        assert!(!loaded.stale); // fresh, within TTL
    }

    #[test]
    fn get_returns_none_for_missing_key() {
        let (cache, _dir) = make_cache();
        let result: Option<CacheRecord<String>> = cache.get("igdb", "nonexistent").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn stale_flag_set_when_expired() {
        let (cache, _dir) = make_cache();
        let mut record = make_record("steam", "app/292030", vec![1, 2, 3]);
        record.fetched_at = chrono::Utc::now() - chrono::Duration::hours(25);
        record.ttl = Duration::from_secs(86400); // 24h

        cache.put(&record).unwrap();
        let loaded: Option<CacheRecord<Vec<i32>>> = cache.get("steam", "app/292030").unwrap();

        let loaded = loaded.unwrap();
        assert!(loaded.stale, "record older than TTL should be marked stale");
    }

    #[test]
    fn path_for_sanitizes_slashes() {
        let (cache, dir) = make_cache();
        let path = cache.path_for("igdb", "game/12345/details");
        let file_name = path.file_name().unwrap().to_str().unwrap();
        assert_eq!(file_name, "game_12345_details.json");
        // The source directory itself is preserved.
        assert!(path.starts_with(dir.path().join("igdb")));
    }

    #[test]
    fn nested_keys_create_parent_directories() {
        let (cache, _dir) = make_cache();
        let record = make_record("steam-store", "app/292030/details", "data".to_string());
        cache.put(&record).unwrap();

        let loaded: Option<CacheRecord<String>> =
            cache.get("steam-store", "app/292030/details").unwrap();
        assert!(loaded.is_some());
    }

    #[test]
    fn etag_preserved_through_roundtrip() {
        let (cache, _dir) = make_cache();
        let mut record = make_record("steam", "app/292030", "data".to_string());
        record.etag = Some("W/\"abc123\"".to_string());

        cache.put(&record).unwrap();
        let loaded: Option<CacheRecord<String>> = cache.get("steam", "app/292030").unwrap();
        assert_eq!(loaded.unwrap().etag, Some("W/\"abc123\"".to_string()));
    }

    #[test]
    fn sanitize_key_preserves_simple_keys() {
        assert_eq!(sanitize_key("app_292030"), "app_292030");
        assert_eq!(sanitize_key("simple"), "simple");
    }

    #[test]
    fn sanitize_key_replaces_special_chars() {
        assert_eq!(sanitize_key("a/b:c\\d"), "a_b_c_d");
    }
}
