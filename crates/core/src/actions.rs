//! Act-half workflow verbs — from evaluated inputs to a previewed write plan.
//!
//! Deep module: one verb per Steam-write workflow (Junk apply, Junk hide,
//! Recommendation collection, Playlist sync). Each verb owns the
//! disposition → cloud read → preview assembly that CLI and GUI previously
//! wired independently, including the rule-Playlist → owned-AppID resolution
//! for sync. The read half of a workflow lives in `vapourfly_api::workflow`;
//! these verbs are the act half.
//!
//! Every verb returns a [`PreviewedPlan`] (or a wrapper around one) — the
//! confirmation gate: frontends show its diff, obtain consent, and pass the
//! same value to [`write::commit`] / [`write::commit_with_retention`].
//! ADR-0001: all resulting ops target cloud storage only.

use std::collections::HashMap;
use std::path::Path;

use crate::error::{Result, VapourflyError};
use crate::models::{Game, PlaylistContent, PlaylistFile, WriteOp};
use crate::write::{self, PreviewedPlan};
use crate::{disposition, playlist, steam};

/// Read the cloud storage at `cloud_path` and preview `ops` against it.
fn preview_ops(ops: Vec<WriteOp>, cloud_path: &Path) -> Result<PreviewedPlan> {
    let cloud = steam::read_cloud_storage(cloud_path)?;
    write::preview(&cloud, ops, cloud_path.to_path_buf())
}

/// Preview upserting junk games into a named Steam Collection.
///
/// Errors on an empty id set (`disposition::junk_apply` rejects it) —
/// callers decide upstream whether "nothing to do" is an error or a notice.
pub fn preview_junk_apply(
    collection: &str,
    junk_app_ids: Vec<u32>,
    cloud_path: &Path,
) -> Result<PreviewedPlan> {
    let op = disposition::junk_apply(collection, junk_app_ids)?;
    preview_ops(vec![op], cloud_path)
}

/// Preview adding junk games to Steam's hidden collection.
pub fn preview_junk_hide(junk_app_ids: Vec<u32>, cloud_path: &Path) -> Result<PreviewedPlan> {
    let op = disposition::junk_hide(junk_app_ids)?;
    preview_ops(vec![op], cloud_path)
}

/// Preview writing recommendations to the temporary
/// [`disposition::RECOMMEND_COLLECTION_ID`] Steam Collection.
pub fn preview_recommend_collection(app_ids: Vec<u32>, cloud_path: &Path) -> Result<PreviewedPlan> {
    let op = disposition::recommend_to_collection(app_ids)?;
    preview_ops(vec![op], cloud_path)
}

/// A previewed Playlist → Steam Collection sync, with the resolved identity
/// frontends display alongside the diff.
#[derive(Debug)]
pub struct SyncPreview {
    pub plan: PreviewedPlan,
    /// The Steam Collection id the sync targets (slugified playlist id).
    pub collection_id: String,
    /// The AppIDs that will be synced (resolved and normalized).
    pub app_ids: Vec<u32>,
}

/// Preview syncing a Playlist to its Steam Collection.
///
/// Owns the rule-Playlist resolution: a Rules playlist is matched against
/// `library` and its owned AppIDs become the sync set. Manual playlists use
/// their AppID list directly and do not consult `library` (callers may pass
/// `None` to skip preparing one).
///
/// Returns `Ok(None)` when the playlist resolves to zero AppIDs — the
/// caller decides whether that is a notice (CLI) or an error (GUI).
pub fn preview_playlist_sync(
    pf: &PlaylistFile,
    library: Option<&[Game]>,
    cloud_path: &Path,
) -> Result<Option<SyncPreview>> {
    let resolved_owned = match &pf.playlist.content {
        PlaylistContent::Manual { .. } => None,
        PlaylistContent::Rules { .. } => {
            let games = library.ok_or_else(|| {
                VapourflyError::InvalidInput(
                    "rule Playlist sync requires a prepared library".into(),
                )
            })?;
            let report = playlist::match_playlist(pf, games, &HashMap::new())?;
            Some(report.owned)
        }
    };
    let app_ids = disposition::playlist_sync_app_ids(pf, resolved_owned)?;
    if app_ids.is_empty() {
        return Ok(None);
    }
    let op = disposition::playlist_sync(pf, app_ids.clone())?;
    let collection_id = match &op {
        WriteOp::UpsertCollection { id, .. } => id.clone(),
        _ => playlist::slugify(&pf.playlist.id),
    };
    let plan = preview_ops(vec![op], cloud_path)?;
    Ok(Some(SyncPreview {
        plan,
        collection_id,
        app_ids,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Playlist, PlaylistRule, SteamAppType, VAPOURFLY_PLAYLIST_SCHEMA};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn empty_cloud_file(dir: &Path) -> PathBuf {
        let path = dir.join("cloud-storage-namespace-1.json");
        fs::write(&path, "[]").unwrap();
        path
    }

    fn game(app_id: u32, installed: bool) -> Game {
        Game {
            app_id,
            name: format!("g{app_id}"),
            app_type: SteamAppType::Game,
            installed,
            install_dir: None,
            library_folder: None,
            playtime_minutes: Some(0),
            playtime_2wks_minutes: None,
            playtime_disconnected_minutes: None,
            last_played_unix: None,
            steam_collections: vec![],
            is_hidden: false,
            is_junk: false,
            hltb: None,
            igdb: None,
            rawg: None,
            protondb: None,
            pcgw: None,
            steam_store: None,
        }
    }

    fn manual_playlist(id: &str, app_ids: Vec<u32>) -> PlaylistFile {
        PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "test".into(),
            playlist: Playlist {
                id: id.into(),
                name: id.into(),
                description: String::new(),
                content: PlaylistContent::Manual { app_ids },
            },
        }
    }

    #[test]
    fn junk_apply_previews_collection_upsert() {
        let tmp = TempDir::new().unwrap();
        let cloud_path = empty_cloud_file(tmp.path());

        let plan = preview_junk_apply("junk", vec![3, 1, 1], &cloud_path).unwrap();
        assert_eq!(plan.diff.app_ids_added.len(), 2, "sorted + deduped");
        assert!(plan.diff.collections_changed.iter().any(|c| c.id == "junk"));

        assert!(preview_junk_apply("junk", vec![], &cloud_path).is_err());
    }

    #[test]
    fn junk_hide_previews_hidden_additions() {
        let tmp = TempDir::new().unwrap();
        let cloud_path = empty_cloud_file(tmp.path());

        let plan = preview_junk_hide(vec![730], &cloud_path).unwrap();
        assert_eq!(plan.diff.hidden_app_ids_added, vec![730]);
    }

    #[test]
    fn recommend_collection_uses_canonical_id() {
        let tmp = TempDir::new().unwrap();
        let cloud_path = empty_cloud_file(tmp.path());

        let plan = preview_recommend_collection(vec![10, 20], &cloud_path).unwrap();
        assert!(
            plan.diff
                .collections_changed
                .iter()
                .any(|c| c.id == disposition::RECOMMEND_COLLECTION_ID)
        );
    }

    #[test]
    fn manual_sync_does_not_need_a_library() {
        let tmp = TempDir::new().unwrap();
        let cloud_path = empty_cloud_file(tmp.path());
        let pf = manual_playlist("My List", vec![730, 440]);

        let sync = preview_playlist_sync(&pf, None, &cloud_path)
            .unwrap()
            .expect("non-empty manual playlist previews");
        assert_eq!(sync.collection_id, "my-list");
        assert_eq!(sync.app_ids, vec![440, 730]);
    }

    #[test]
    fn rule_sync_resolves_owned_app_ids_against_library() {
        let tmp = TempDir::new().unwrap();
        let cloud_path = empty_cloud_file(tmp.path());

        let pf = PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "test".into(),
            playlist: Playlist {
                id: "installed-only".into(),
                name: "Installed".into(),
                description: String::new(),
                content: PlaylistContent::Rules {
                    rules: vec![PlaylistRule::Installed],
                },
            },
        };
        let library = vec![game(1, true), game(2, false), game(3, true)];

        let sync = preview_playlist_sync(&pf, Some(&library), &cloud_path)
            .unwrap()
            .expect("matching rules preview");
        assert_eq!(sync.app_ids, vec![1, 3], "only installed games sync");

        // A rules playlist without a library is a caller error, not a panic.
        assert!(preview_playlist_sync(&pf, None, &cloud_path).is_err());
    }

    #[test]
    fn empty_resolution_returns_none_not_error() {
        let tmp = TempDir::new().unwrap();
        let cloud_path = empty_cloud_file(tmp.path());

        let pf = manual_playlist("empty", vec![]);
        assert!(
            preview_playlist_sync(&pf, None, &cloud_path)
                .unwrap()
                .is_none()
        );
    }
}
