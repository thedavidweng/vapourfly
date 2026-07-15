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

/// Resolve the path for a playlist id under `store_dir`.
pub fn playlist_path(store_dir: &Path, id: &str) -> PathBuf {
    store_dir.join(format!("{id}.json"))
}

/// Persist a playlist under `store_dir` as `{playlist.id}.json`.
///
/// Creates `store_dir` if missing. Returns the written path.
pub fn put(store_dir: &Path, playlist: &PlaylistFile) -> Result<PathBuf> {
    fs::create_dir_all(store_dir).map_err(|e| {
        VapourflyError::Internal(format!(
            "failed to create playlist store {}: {e}",
            store_dir.display()
        ))
    })?;
    let path = playlist_path(store_dir, &playlist.playlist.id);
    export_playlist(playlist, &path)?;
    Ok(path)
}

/// Load a playlist by id from `store_dir`.
pub fn get(store_dir: &Path, id: &str) -> Result<PlaylistFile> {
    let path = playlist_path(store_dir, id);
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

/// Put using the platform default playlists directory.
pub fn put_default(playlist: &PlaylistFile) -> Result<PathBuf> {
    put(&crate::config::default_playlists_dir(), playlist)
}

/// Get using the platform default playlists directory.
pub fn get_default(id: &str) -> Result<PlaylistFile> {
    get(&crate::config::default_playlists_dir(), id)
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
}
