//! Playlist Store — put / get / list playlists under the canonical store root.
//!
//! Deep module: owns the local persistence policy for Vapourfly-owned Playlist
//! artifacts. CLI and GUI call this instead of re-encoding `{id}.json` paths.
//! Import/export of arbitrary paths remains in [`crate::playlist`].

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Result, SafePath, VapourflyError};
use crate::models::PlaylistFile;
use crate::playlist::{export_playlist, import_playlist};

/// Validate a playlist id for safe filesystem use.
///
/// Rejects empty strings, path separators (`/`, `\`), path traversal
/// (`..`, `.`), control characters, and any character that is not
/// alphanumeric, hyphen, or underscore. This is the single source of
/// truth for id validation — `playlist_path`, `get`, and `put` all
/// call this before constructing a filesystem path.
pub fn validate_playlist_id(id: &str) -> std::result::Result<(), String> {
    if id.is_empty() {
        return Err("playlist id must not be empty".into());
    }
    if id == "." || id == ".." {
        return Err("playlist id must not be '.' or '..'".into());
    }
    for ch in id.chars() {
        if ch.is_control() {
            return Err(format!(
                "playlist id must not contain control characters (found U+{:04X})",
                ch as u32
            ));
        }
        if ch == '/' || ch == '\\' {
            return Err("playlist id must not contain path separators".into());
        }
        if !ch.is_ascii_alphanumeric() && ch != '-' && ch != '_' {
            return Err(format!(
                "playlist id must only contain alphanumeric, hyphen, or underscore (found '{ch}')"
            ));
        }
    }
    Ok(())
}

/// Resolve the path for a playlist id under `store_dir`.
///
/// Validates the id to prevent path traversal.
pub fn playlist_path(store_dir: &Path, id: &str) -> Result<PathBuf> {
    validate_playlist_id(id).map_err(VapourflyError::Internal)?;
    Ok(store_dir.join(format!("{id}.json")))
}

/// Persist a playlist under `store_dir` as `{playlist.id}.json`.
///
/// Creates `store_dir` if missing. Returns the written path.
/// Validates the playlist id to prevent path traversal.
pub fn put(store_dir: &Path, playlist: &PlaylistFile) -> Result<PathBuf> {
    fs::create_dir_all(store_dir).map_err(|e| {
        VapourflyError::Internal(format!(
            "failed to create playlist store {}: {e}",
            store_dir.display()
        ))
    })?;
    let path = playlist_path(store_dir, &playlist.playlist.id)?;
    export_playlist(playlist, &path)?;
    Ok(path)
}

/// Load a playlist by id from `store_dir`.
/// Validates the playlist id to prevent path traversal.
pub fn get(store_dir: &Path, id: &str) -> Result<PlaylistFile> {
    let path = playlist_path(store_dir, id)?;
    if !path.is_file() {
        return Err(VapourflyError::FileNotFound {
            path: SafePath::new(&path),
        });
    }
    import_playlist(&path)
}

/// List playlist ids present in `store_dir` (sorted).
///
/// Missing directory yields an empty list (not an error).
pub fn list_ids(store_dir: &Path) -> Result<Vec<String>> {
    if !store_dir.is_dir() {
        return Ok(vec![]);
    }
    let mut ids = Vec::new();
    for entry in fs::read_dir(store_dir).map_err(|e| {
        VapourflyError::Internal(format!(
            "failed to read playlist store {}: {e}",
            store_dir.display()
        ))
    })? {
        let entry = entry.map_err(|e| {
            VapourflyError::Internal(format!("failed to read playlist store entry: {e}"))
        })?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if let Some(id) = name.strip_suffix(".json")
            && !id.is_empty()
        {
            ids.push(id.to_string());
        }
    }
    ids.sort();
    Ok(ids)
}

/// Load all playlists from `store_dir`, returning `(id, Ok(PlaylistFile))`
/// for each readable file and `(id, Err(message))` for corrupt ones.
/// Missing directory yields an empty list. Sort order is by id.
pub fn list_all(
    store_dir: &Path,
) -> Result<Vec<(String, std::result::Result<PlaylistFile, String>)>> {
    let ids = list_ids(store_dir)?;
    let result = ids
        .into_iter()
        .map(|id| {
            let loaded = get(store_dir, &id).map_err(|e| e.to_string());
            (id, loaded)
        })
        .collect();
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Playlist, PlaylistContent, VAPOURFLY_PLAYLIST_SCHEMA};

    fn sample(id: &str) -> PlaylistFile {
        PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "test".into(),
            playlist: Playlist {
                id: id.into(),
                name: "Test".into(),
                description: String::new(),
                content: PlaylistContent::Manual {
                    app_ids: vec![730, 440],
                },
            },
        }
    }

    #[test]
    fn put_get_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let pf = sample("my-list");
        let path = put(dir.path(), &pf).unwrap();
        assert!(path.exists());
        assert_eq!(path.file_name().unwrap(), "my-list.json");
        let loaded = get(dir.path(), "my-list").unwrap();
        assert_eq!(loaded.playlist.id, "my-list");
        match loaded.playlist.content {
            PlaylistContent::Manual { app_ids } => {
                assert_eq!(app_ids, vec![440, 730]); // export sorts
            }
            PlaylistContent::Rules { .. } => panic!("expected manual"),
        }
    }

    #[test]
    fn list_ids_sorted() {
        let dir = tempfile::tempdir().unwrap();
        put(dir.path(), &sample("b")).unwrap();
        put(dir.path(), &sample("a")).unwrap();
        assert_eq!(list_ids(dir.path()).unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn get_missing_is_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let err = get(dir.path(), "nope").unwrap_err();
        assert!(matches!(err, VapourflyError::FileNotFound { .. }));
    }

    #[test]
    fn validate_playlist_id_rejects_path_traversal() {
        assert!(validate_playlist_id("../outside").is_err());
        assert!(validate_playlist_id("foo/bar").is_err());
        assert!(validate_playlist_id("foo\\bar").is_err());
        assert!(validate_playlist_id(".").is_err());
        assert!(validate_playlist_id("..").is_err());
        assert!(validate_playlist_id("").is_err());
    }

    #[test]
    fn validate_playlist_id_rejects_special_chars() {
        assert!(validate_playlist_id("my list").is_err()); // space
        assert!(validate_playlist_id("my.list").is_err()); // dot
        assert!(validate_playlist_id("my:list").is_err()); // colon
        assert!(validate_playlist_id("my+list").is_err()); // plus
    }

    #[test]
    fn validate_playlist_id_accepts_valid_ids() {
        assert!(validate_playlist_id("my-list").is_ok());
        assert!(validate_playlist_id("my_list").is_ok());
        assert!(validate_playlist_id("MyList123").is_ok());
        assert!(validate_playlist_id("a").is_ok());
        assert!(validate_playlist_id("dynamic-deck-session").is_ok());
    }

    #[test]
    fn put_rejects_path_traversal_id() {
        let dir = tempfile::tempdir().unwrap();
        let pf = sample("../outside");
        let err = put(dir.path(), &pf).unwrap_err();
        // Must not write outside the store dir.
        assert!(!dir.path().parent().unwrap().join("outside.json").exists());
        // Error should be an Internal error (from validate_playlist_id).
        assert!(matches!(err, VapourflyError::Internal(_)));
    }

    #[test]
    fn get_rejects_path_traversal_id() {
        let dir = tempfile::tempdir().unwrap();
        let err = get(dir.path(), "../outside").unwrap_err();
        assert!(matches!(err, VapourflyError::Internal(_)));
    }
}
