//! Parse Steam's `librarycache.json` for name fallback.
//!
//! Path: `{steam}/userdata/{uid}/config/librarycache/librarycache.json`
//!
//! The file is a JSON array of `{"appid": N, "name": "..."}` objects produced
//! by Steam's library cache.  When an installed app's name is missing from its
//! `appmanifest_*.acf` the library cache provides a fallback.

use std::collections::HashMap;
use std::path::Path;

use crate::error::{Result, VapourflyError};

/// Parse a `librarycache.json` file and return a map of `appid -> name`.
///
/// Returns an empty `HashMap` if the file does not exist or contains
/// unparseable JSON. Returns `Err` only on I/O errors other than "not found".
pub fn parse_librarycache(path: &Path) -> Result<HashMap<u32, String>> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => {
            return Err(VapourflyError::InvalidInput(format!(
                "failed to read {}: {}",
                path.display(),
                e
            )));
        }
    };

    parse_librarycache_json(&content)
}

/// Parse library cache JSON content into an app-id-to-name map.
fn parse_librarycache_json(content: &str) -> Result<HashMap<u32, String>> {
    #[derive(serde::Deserialize)]
    struct CacheEntry {
        appid: u32,
        name: String,
    }

    let entries: Vec<CacheEntry> = match serde_json::from_str(content) {
        Ok(v) => v,
        Err(_) => return Ok(HashMap::new()),
    };

    let mut map = HashMap::new();
    for entry in entries {
        map.insert(entry.appid, entry.name);
    }
    Ok(map)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Path to the minimal fixture Steam directory.
    fn fixture_steam_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal")
    }

    fn fixture_cache_path() -> PathBuf {
        fixture_steam_dir().join("userdata/76561198000000000/config/librarycache/librarycache.json")
    }

    #[test]
    fn parses_fixture() {
        let cache = parse_librarycache(&fixture_cache_path()).unwrap();
        assert_eq!(cache.len(), 2);
        assert_eq!(
            cache.get(&730).map(|s| s.as_str()),
            Some("Counter-Strike 2")
        );
        assert_eq!(cache.get(&223850).map(|s| s.as_str()), Some("Factorio"));
    }

    #[test]
    fn missing_file_returns_empty() {
        let cache = parse_librarycache(Path::new("/nonexistent/cache.json")).unwrap();
        assert!(cache.is_empty());
    }

    #[test]
    fn empty_array_returns_empty() {
        let cache = parse_librarycache_json("[]").unwrap();
        assert!(cache.is_empty());
    }

    #[test]
    fn invalid_json_returns_empty() {
        let cache = parse_librarycache_json("not json at all").unwrap();
        assert!(cache.is_empty());
    }

    #[test]
    fn single_entry() {
        let json = r#"[{"appid": 440, "name": "Team Fortress 2"}]"#;
        let cache = parse_librarycache_json(json).unwrap();
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.get(&440).map(|s| s.as_str()), Some("Team Fortress 2"));
    }

    #[test]
    fn duplicate_appid_last_wins() {
        let json = r#"[
            {"appid": 730, "name": "CS:GO"},
            {"appid": 730, "name": "Counter-Strike 2"}
        ]"#;
        let cache = parse_librarycache_json(json).unwrap();
        assert_eq!(cache.len(), 1);
        assert_eq!(
            cache.get(&730).map(|s| s.as_str()),
            Some("Counter-Strike 2")
        );
    }
}
