//! Steam Collection write disposition — build WriteOps for product actions.
//!
//! Deep module: owns AppID extraction + WriteOp assembly for Junk apply/hide,
//! recommend-to-collection, and Playlist → Steam Collection sync. CLI and GUI
//! call this instead of re-sorting AppIDs and hard-coding collection IDs.
//!
//! ADR-0001: ops only target cloud-storage collections / hidden.

use crate::error::{Result, VapourflyError};
use crate::models::{Game, JunkDecision, PlaylistContent, PlaylistFile, WriteOp};
use crate::playlist;

/// Temporary Steam Collection id for recommendation writes.
pub const RECOMMEND_COLLECTION_ID: &str = "vapourfly-picks";

/// Sort and deduplicate AppIDs (canonical order for write ops).
pub fn normalize_app_ids(app_ids: impl IntoIterator<Item = u32>) -> Vec<u32> {
    let mut ids: Vec<u32> = app_ids.into_iter().collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// AppIDs marked junk from classified games (`Game.is_junk`).
pub fn junk_app_ids_from_games(games: &[Game]) -> Vec<u32> {
    normalize_app_ids(games.iter().filter(|g| g.is_junk).map(|g| g.app_id))
}

/// AppIDs marked junk from junk decisions.
pub fn junk_app_ids_from_decisions(decisions: &[JunkDecision]) -> Vec<u32> {
    normalize_app_ids(decisions.iter().filter(|d| d.is_junk).map(|d| d.app_id))
}

/// Upsert junk games into a named Steam Collection.
pub fn junk_apply(collection_id: impl Into<String>, app_ids: Vec<u32>) -> Result<WriteOp> {
    let added = normalize_app_ids(app_ids);
    if added.is_empty() {
        return Err(VapourflyError::InvalidInput(
            "no junk candidates to apply".into(),
        ));
    }
    Ok(WriteOp::UpsertCollection {
        id: collection_id.into(),
        added,
        removed: vec![],
    })
}

/// Add junk games to Steam's hidden collection.
pub fn junk_hide(app_ids: Vec<u32>) -> Result<WriteOp> {
    let app_ids = normalize_app_ids(app_ids);
    if app_ids.is_empty() {
        return Err(VapourflyError::InvalidInput(
            "no junk candidates to hide".into(),
        ));
    }
    Ok(WriteOp::AddToHidden { app_ids })
}

/// Upsert recommendations into the temporary `vapourfly-picks` collection.
pub fn recommend_to_collection(app_ids: Vec<u32>) -> Result<WriteOp> {
    let added = normalize_app_ids(app_ids);
    if added.is_empty() {
        return Err(VapourflyError::InvalidInput(
            "no recommendations to write".into(),
        ));
    }
    Ok(WriteOp::UpsertCollection {
        id: RECOMMEND_COLLECTION_ID.into(),
        added,
        removed: vec![],
    })
}

/// Resolve AppIDs from a playlist for Steam Collection sync.
///
/// Manual playlists use their AppID list. Rules playlists use `owned` from a
/// prior match against the library (caller supplies resolved AppIDs).
pub fn playlist_sync_app_ids(
    playlist: &PlaylistFile,
    resolved_owned: Option<Vec<u32>>,
) -> Result<Vec<u32>> {
    match &playlist.playlist.content {
        PlaylistContent::Manual { app_ids } => Ok(normalize_app_ids(app_ids.iter().copied())),
        PlaylistContent::Rules { .. } => {
            let owned = resolved_owned.ok_or_else(|| {
                VapourflyError::InvalidInput(
                    "rule-based playlist sync requires resolved owned AppIDs".into(),
                )
            })?;
            Ok(normalize_app_ids(owned))
        }
    }
}

/// Upsert a playlist's AppIDs into a Steam Collection (id = slugified playlist id).
pub fn playlist_sync(playlist: &PlaylistFile, app_ids: Vec<u32>) -> Result<WriteOp> {
    let collection_id = playlist::slugify(&playlist.playlist.id);
    if collection_id.is_empty() {
        return Err(VapourflyError::InvalidInput(
            "playlist id cannot produce a Steam collection id".into(),
        ));
    }
    let added = normalize_app_ids(app_ids);
    if added.is_empty() {
        return Err(VapourflyError::InvalidInput("no app IDs to sync".into()));
    }
    Ok(WriteOp::UpsertCollection {
        id: collection_id,
        added,
        removed: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{JunkDecision, Playlist, PlaylistContent, VAPOURFLY_PLAYLIST_SCHEMA};

    #[test]
    fn junk_apply_sorts_and_dedups() {
        let op = junk_apply("junk", vec![3, 1, 1, 2]).unwrap();
        match op {
            WriteOp::UpsertCollection { id, added, removed } => {
                assert_eq!(id, "junk");
                assert_eq!(added, vec![1, 2, 3]);
                assert!(removed.is_empty());
            }
            _ => panic!("expected upsert"),
        }
    }

    #[test]
    fn junk_hide_rejects_empty() {
        assert!(junk_hide(vec![]).is_err());
    }

    #[test]
    fn recommend_uses_canonical_collection_id() {
        let op = recommend_to_collection(vec![730]).unwrap();
        match op {
            WriteOp::UpsertCollection { id, .. } => {
                assert_eq!(id, RECOMMEND_COLLECTION_ID);
            }
            _ => panic!("expected upsert"),
        }
    }

    fn sample_game(app_id: u32, is_junk: bool) -> Game {
        Game {
            app_id,
            name: format!("g{app_id}"),
            app_type: crate::models::SteamAppType::Game,
            installed: true,
            install_dir: None,
            library_folder: None,
            playtime_minutes: None,
            playtime_2wks_minutes: None,
            playtime_disconnected_minutes: None,
            last_played_unix: None,
            steam_collections: vec![],
            is_hidden: false,
            is_junk,
            hltb: None,
            igdb: None,
            rawg: None,
            protondb: None,
            pcgw: None,
            steam_store: None,
        }
    }

    #[test]
    fn decisions_and_games_agree_on_junk_ids() {
        let games = vec![sample_game(1, true), sample_game(2, false)];
        let decisions = vec![
            JunkDecision {
                app_id: 1,
                name: "g1".into(),
                is_junk: true,
                confidence: 1.0,
                matched: vec![],
                missing: vec![],
                mode: crate::models::JunkMode::Default,
            },
            JunkDecision {
                app_id: 2,
                name: "g2".into(),
                is_junk: false,
                confidence: 0.0,
                matched: vec![],
                missing: vec![],
                mode: crate::models::JunkMode::Default,
            },
        ];
        assert_eq!(
            junk_app_ids_from_games(&games),
            junk_app_ids_from_decisions(&decisions)
        );
    }

    #[test]
    fn playlist_sync_slugifies_id() {
        let pf = PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "t".into(),
            playlist: Playlist {
                id: "My Cool List!".into(),
                name: "x".into(),
                description: String::new(),
                content: PlaylistContent::Manual { app_ids: vec![730] },
            },
        };
        let app_ids = playlist_sync_app_ids(&pf, None).unwrap();
        let op = playlist_sync(&pf, app_ids).unwrap();
        match op {
            WriteOp::UpsertCollection { id, added, .. } => {
                assert_eq!(id, "my-cool-list");
                assert_eq!(added, vec![730]);
            }
            _ => panic!("expected upsert"),
        }
    }
}
