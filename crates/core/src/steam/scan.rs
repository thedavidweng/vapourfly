//! Scan aggregation: merge all Steam data sources into Game records.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

use crate::error::Result;
use crate::models::{Game, ScanResult, ScanWarning, SteamAppType};
use crate::steam::account::select_account;
use crate::steam::collections::{
    get_all_hidden_app_ids, read_cloud_storage, read_user_collections,
};
use crate::steam::librarycache::parse_librarycache;
use crate::steam::localconfig::parse_localconfig;
use crate::steam::paths::{
    detect_accounts, detect_library_folders, detect_steam_dirs, parse_appmanifests,
};

/// Options for a library scan.
#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Override Steam directory (or fixtures root).
    pub steam_dir: Option<PathBuf>,
    /// Override account selection.
    pub account: Option<String>,
    /// Use fixtures directory instead of real Steam.
    pub fixtures: Option<PathBuf>,
}

/// Scan the Steam library and return aggregated Game records.
pub fn scan_library(opts: &ScanOptions) -> Result<ScanResult> {
    let mut warnings = Vec::new();

    // Determine steam dir
    let steam_root = if let Some(fix) = &opts.fixtures {
        fix.clone()
    } else if let Some(dir) = &opts.steam_dir {
        dir.clone()
    } else {
        detect_steam_dirs(None)
            .into_iter()
            .next()
            .unwrap_or_else(|| PathBuf::from("."))
    };

    // Detect accounts
    let accounts = detect_accounts(&steam_root).unwrap_or_default();
    let account = select_account(&accounts, opts.account.as_deref());
    let (account_name, user_id) = match account {
        Ok(acc) => (acc.account_name.clone(), acc.steam_id64.clone()),
        Err(_) => {
            warnings.push(ScanWarning {
                code: "no_account".into(),
                message: "no Steam account detected".into(),
            });
            ("unknown".into(), "0".into())
        }
    };

    // Detect library folders
    let library_folders = detect_library_folders(&steam_root).unwrap_or_else(|_| {
        warnings.push(ScanWarning {
            code: "no_library_folders".into(),
            message: "could not detect library folders".into(),
        });
        vec![steam_root.clone()]
    });

    // Parse appmanifests from all library folders
    let mut all_manifests = Vec::new();
    for folder in &library_folders {
        match parse_appmanifests(folder) {
            Ok(manifests) => all_manifests.extend(manifests),
            Err(e) => {
                warnings.push(ScanWarning {
                    code: "manifest_parse_error".into(),
                    message: format!("failed to parse manifests in {}: {e}", folder.display()),
                });
            }
        }
    }

    // Parse localconfig
    let localconfig_path = steam_root
        .join("userdata")
        .join(&user_id)
        .join("config/localconfig.vdf");
    let local_apps = parse_localconfig(&localconfig_path).unwrap_or_else(|_| {
        warnings.push(ScanWarning {
            code: "localconfig_missing".into(),
            message: "localconfig.vdf not found or unparseable".into(),
        });
        BTreeMap::new()
    });

    // Parse librarycache
    let cache_path = steam_root
        .join("userdata")
        .join(&user_id)
        .join("config/librarycache/librarycache.json");
    let library_cache = parse_librarycache(&cache_path).unwrap_or_default();

    // Parse cloud storage for collections
    let cloud_path = steam_root
        .join("userdata")
        .join(&user_id)
        .join("config/cloudstorage/cloud-storage-namespace-1.json");
    let (collections, hidden_ids) = match read_cloud_storage(&cloud_path) {
        Ok(cloud) => match read_user_collections(&cloud) {
            Ok(cols) => {
                let hidden = get_all_hidden_app_ids(&cols);
                (cols, hidden)
            }
            Err(e) => {
                warnings.push(ScanWarning {
                    code: "collections_parse_error".into(),
                    message: format!("failed to parse collections: {e}"),
                });
                (Vec::new(), Vec::new())
            }
        },
        Err(_) => {
            warnings.push(ScanWarning {
                code: "cloudstorage_missing".into(),
                message: "cloud-storage-namespace-1.json not found".into(),
            });
            (Vec::new(), Vec::new())
        }
    };

    // Build collection membership map
    let mut collection_map: HashMap<u32, Vec<String>> = HashMap::new();
    for col in &collections {
        if col.is_hidden_collection {
            continue;
        }
        for &app_id in &col.app_ids {
            collection_map
                .entry(app_id)
                .or_default()
                .push(col.name.clone());
        }
    }

    let hidden_set: std::collections::HashSet<u32> = hidden_ids.iter().copied().collect();

    // Build Game records from manifests (installed games)
    let mut games_map: BTreeMap<u32, Game> = BTreeMap::new();
    for manifest in &all_manifests {
        let app_id = manifest.app_id;
        let local = local_apps.get(&app_id);
        let name = manifest.name.clone();
        let steam_collections = collection_map.get(&app_id).cloned().unwrap_or_default();

        let game = Game {
            app_id,
            name,
            app_type: classify_app_type(app_id, manifest.state_flags),
            installed: true,
            install_dir: Some(PathBuf::from(&manifest.installdir)),
            library_folder: Some(manifest.library_folder.clone()),
            playtime_minutes: local.and_then(|l| l.playtime_minutes),
            playtime_2wks_minutes: local.and_then(|l| l.playtime_2wks_minutes),
            playtime_disconnected_minutes: local.and_then(|l| l.playtime_disconnected_minutes),
            last_played_unix: local.and_then(|l| l.last_played_unix),
            steam_collections,
            is_hidden: hidden_set.contains(&app_id),
            is_junk: false,
            hltb: None,
            igdb: None,
            rawg: None,
            protondb: None,
            pcgw: None,
        };
        games_map.insert(app_id, game);
    }

    // Add non-installed games from collections/localconfig where name is resolvable
    let all_known_ids: Vec<u32> = collections
        .iter()
        .flat_map(|c| c.app_ids.clone())
        .chain(local_apps.keys().copied())
        .collect();

    for app_id in all_known_ids {
        if games_map.contains_key(&app_id) {
            continue;
        }

        let name = library_cache
            .get(&app_id)
            .cloned()
            .unwrap_or_else(|| format!("App {app_id}"));

        let local = local_apps.get(&app_id);
        let steam_collections = collection_map.get(&app_id).cloned().unwrap_or_default();

        let game = Game {
            app_id,
            name,
            app_type: SteamAppType::Unknown("uninstalled".into()),
            installed: false,
            install_dir: None,
            library_folder: None,
            playtime_minutes: local.and_then(|l| l.playtime_minutes),
            playtime_2wks_minutes: local.and_then(|l| l.playtime_2wks_minutes),
            playtime_disconnected_minutes: local.and_then(|l| l.playtime_disconnected_minutes),
            last_played_unix: local.and_then(|l| l.last_played_unix),
            steam_collections,
            is_hidden: hidden_set.contains(&app_id),
            is_junk: false,
            hltb: None,
            igdb: None,
            rawg: None,
            protondb: None,
            pcgw: None,
        };
        games_map.insert(app_id, game);
    }

    let games: Vec<Game> = games_map.into_values().collect();

    Ok(ScanResult {
        games,
        warnings,
        steam_dir: steam_root.display().to_string(),
        account: account_name,
    })
}

/// Classify app type from state flags (simplified).
fn classify_app_type(_app_id: u32, state_flags: u32) -> SteamAppType {
    // StateFlags bit 2 = UpdateRunning, bit 4 = FullyInstalled
    // For now, treat everything with state_flags as Game
    if state_flags & 4 != 0 {
        SteamAppType::Game
    } else {
        SteamAppType::Unknown(format!("state:{state_flags}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_fixture_library() {
        let opts = ScanOptions {
            steam_dir: None,
            account: None,
            fixtures: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal"),
            ),
        };
        let result = scan_library(&opts).unwrap();

        // Should have games from manifests + collections
        assert!(!result.games.is_empty());

        // CS2 should be installed
        let cs2 = result.games.iter().find(|g| g.app_id == 730).unwrap();
        assert!(cs2.installed);
        assert_eq!(cs2.playtime_minutes, Some(418));
        assert!(cs2.steam_collections.contains(&"Favorites".to_string()));
        assert!(!cs2.is_hidden);
    }

    #[test]
    fn scan_deterministic() {
        let opts = ScanOptions {
            steam_dir: None,
            account: None,
            fixtures: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal"),
            ),
        };
        let r1 = scan_library(&opts).unwrap();
        let r2 = scan_library(&opts).unwrap();

        let j1 = serde_json::to_string(&r1).unwrap();
        let j2 = serde_json::to_string(&r2).unwrap();
        assert_eq!(j1, j2);
    }

    #[test]
    fn scan_empty_cloudstorage() {
        let opts = ScanOptions {
            steam_dir: None,
            account: None,
            fixtures: Some(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("../../data/fixtures/empty_cloudstorage"),
            ),
        };
        let result = scan_library(&opts).unwrap();
        // Should still have games from manifests
        assert!(!result.games.is_empty());
        // All games should have empty collections since cloud storage is empty
        assert!(result.games.iter().all(|g| g.steam_collections.is_empty()));
    }
}
