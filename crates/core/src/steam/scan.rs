//! Scan aggregation: merge all Steam data sources into Game records.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use crate::error::Result;
use crate::models::{Game, ScanResult, ScanWarning, SteamAppType};
use crate::steam::account::select_account;
use crate::steam::appinfo::lookup_appinfo_names;
use crate::steam::collections::{
    get_all_hidden_app_ids, read_cloud_storage, read_user_collections,
};
use crate::steam::librarycache::parse_librarycache;
use crate::steam::localconfig::parse_localconfig;
use crate::steam::paths::{
    detect_accounts, detect_library_folders, parse_appmanifests, resolve_userdata_dir,
};

/// Options for a library scan.
#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Resolved Steam directory path.
    pub steam_dir: PathBuf,
    /// Override account selection (matched against account name, persona name,
    /// or steam id).
    pub account: Option<String>,
    /// Use a fixtures directory instead of the real Steam directory. When set,
    /// overrides `steam_dir`.
    pub fixtures: Option<PathBuf>,
}

/// Scan the Steam library and return aggregated [`Game`] records.
///
/// Pipeline:
///
/// 1. Detect steam_dir (or use fixtures path)
/// 2. Detect accounts and select one
/// 3. Detect library folders
/// 4. Parse appmanifests from all library folders
/// 5. Parse localconfig for playtime data
/// 6. Parse librarycache for name fallback
/// 7. Parse cloud storage for collections and hidden status
/// 8. Merge into Game records
/// 9. Resolve remaining placeholder names from `appcache/appinfo.vdf`
/// 10. Sort games by name then app_id
pub fn scan_library(opts: &ScanOptions) -> Result<ScanResult> {
    let mut warnings = Vec::new();

    // -- 1. Resolve steam directory -------------------------------------------
    let steam_root = if let Some(fix) = &opts.fixtures {
        fix.clone()
    } else {
        opts.steam_dir.clone()
    };

    // -- 2. Detect accounts and select one ------------------------------------
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

    // -- 3. Detect library folders --------------------------------------------
    let library_folders = match detect_library_folders(&steam_root) {
        Ok(folders) if !folders.is_empty() => folders,
        Ok(_) => {
            warnings.push(ScanWarning {
                code: "no_library_folders".into(),
                message: "no library folders found".into(),
            });
            vec![steam_root.clone()]
        }
        Err(e) => {
            warnings.push(ScanWarning {
                code: "library_folders_error".into(),
                message: format!("could not detect library folders: {e}"),
            });
            vec![steam_root.clone()]
        }
    };

    // -- 4. Parse appmanifests from all library folders -----------------------
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

    let userdata_dir = resolve_userdata_dir(&steam_root, &user_id);

    // -- 5. Parse localconfig for playtime data -------------------------------
    let localconfig_path = userdata_dir.join("config/localconfig.vdf");
    let local_apps = match parse_localconfig(&localconfig_path) {
        Ok(apps) => apps,
        Err(_) => {
            warnings.push(ScanWarning {
                code: "localconfig_missing".into(),
                message: "localconfig.vdf not found or unparseable".into(),
            });
            BTreeMap::new()
        }
    };

    // -- 6. Parse librarycache for name fallback ------------------------------
    let cache_path = userdata_dir.join("config/librarycache");
    let library_cache = match parse_librarycache(&cache_path) {
        Ok(cache) => cache,
        Err(e) => {
            warnings.push(ScanWarning {
                code: "librarycache_error".into(),
                message: format!("librarycache.json parse error: {e}"),
            });
            HashMap::new()
        }
    };

    // -- 7. Parse cloud storage for collections and hidden status -------------
    let cloud_path = userdata_dir.join("config/cloudstorage/cloud-storage-namespace-1.json");
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
                message: "cloud-storage-namespace-1.json not found or unparseable".into(),
            });
            (Vec::new(), Vec::new())
        }
    };

    // -- 8. Merge into Game records -------------------------------------------

    // Build collection membership map (excluding hidden collection itself).
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

    let hidden_set: HashSet<u32> = hidden_ids.iter().copied().collect();

    // Installed games from appmanifests.
    let mut games_map: BTreeMap<u32, Game> = BTreeMap::new();
    for manifest in &all_manifests {
        let app_id = manifest.app_id;
        let local = local_apps.get(&app_id);

        // Name: appmanifest -> librarycache -> "App {appid}"
        let name = if !manifest.name.is_empty() {
            manifest.name.clone()
        } else {
            library_cache
                .get(&app_id)
                .cloned()
                .unwrap_or_else(|| format!("App {app_id}"))
        };

        let steam_collections = collection_map.get(&app_id).cloned().unwrap_or_default();

        games_map.insert(
            app_id,
            Game {
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
                steam_store: None,
            },
        );
    }

    // Non-installed games from collections and localconfig.
    // Include if name is resolvable (librarycache hit or generic fallback).
    let all_known_ids: HashSet<u32> = collections
        .iter()
        .flat_map(|c| c.app_ids.iter().copied())
        .chain(local_apps.keys().copied())
        .collect();

    for app_id in all_known_ids {
        if games_map.contains_key(&app_id) {
            continue;
        }

        // Name: librarycache -> "App {appid}"
        let name = library_cache
            .get(&app_id)
            .cloned()
            .unwrap_or_else(|| format!("App {app_id}"));

        let local = local_apps.get(&app_id);
        let steam_collections = collection_map.get(&app_id).cloned().unwrap_or_default();

        games_map.insert(
            app_id,
            Game {
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
                steam_store: None,
            },
        );
    }

    // -- 9. Resolve remaining names from appinfo.vdf --------------------------
    let unresolved: HashSet<u32> = games_map
        .iter()
        .filter(|(app_id, game)| game.name == placeholder_name(**app_id))
        .map(|(app_id, _)| *app_id)
        .collect();

    if !unresolved.is_empty() {
        match lookup_appinfo_names(&steam_root, &unresolved) {
            Ok(appinfo_names) => {
                for (app_id, name) in appinfo_names {
                    if let Some(game) = games_map.get_mut(&app_id) {
                        game.name = name;
                    }
                }
            }
            Err(e) => {
                warnings.push(ScanWarning {
                    code: "appinfo_error".into(),
                    message: format!("appinfo.vdf lookup failed: {e}"),
                });
            }
        }
    }

    // -- 10. Sort games by name then app_id -----------------------------------
    let mut games: Vec<Game> = games_map.into_values().collect();
    games.sort_by(|a, b| a.name.cmp(&b.name).then(a.app_id.cmp(&b.app_id)));

    Ok(ScanResult {
        games,
        warnings,
        steam_dir: steam_root.display().to_string(),
        account: account_name,
    })
}

fn placeholder_name(app_id: u32) -> String {
    format!("App {app_id}")
}

/// Classify app type from state flags (simplified).
fn classify_app_type(_app_id: u32, state_flags: u32) -> SteamAppType {
    // StateFlags bit 2 (value 4) = FullyInstalled.
    if state_flags & 4 != 0 {
        SteamAppType::Game
    } else {
        SteamAppType::Unknown(format!("state:{state_flags}"))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Path to the minimal fixture Steam directory.
    fn fixture_steam_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal")
    }

    /// Path to the empty-cloudstorage fixture.
    fn fixture_empty_cloud_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/empty_cloudstorage")
    }

    /// Default ScanOptions pointing at the minimal fixture.
    fn fixture_opts() -> ScanOptions {
        ScanOptions {
            steam_dir: fixture_steam_dir(),
            account: None,
            fixtures: None,
        }
    }

    // -- Basic scan -----------------------------------------------------------

    #[test]
    fn scan_fixture_library_returns_games() {
        let result = scan_library(&fixture_opts()).unwrap();
        assert!(!result.games.is_empty());
    }

    #[test]
    fn scan_fixture_account_name() {
        let result = scan_library(&fixture_opts()).unwrap();
        assert_eq!(result.account, "vapourfly_fixture_user");
    }

    #[test]
    fn scan_fixture_steam_dir_reported() {
        let result = scan_library(&fixture_opts()).unwrap();
        assert_eq!(result.steam_dir, fixture_steam_dir().display().to_string());
    }

    // -- Installed games (from appmanifests) ----------------------------------

    #[test]
    fn scan_cs2_installed() {
        let result = scan_library(&fixture_opts()).unwrap();
        let cs2 = result.games.iter().find(|g| g.app_id == 730).unwrap();

        assert!(cs2.installed);
        assert_eq!(cs2.name, "Counter-Strike 2");
        assert_eq!(cs2.playtime_minutes, Some(418));
        assert_eq!(cs2.playtime_2wks_minutes, Some(213));
        assert_eq!(cs2.playtime_disconnected_minutes, Some(3));
        assert_eq!(cs2.last_played_unix, Some(1628871494));
        assert!(cs2.install_dir.is_some());
        assert!(cs2.library_folder.is_some());
    }

    #[test]
    fn scan_factorio_installed() {
        let result = scan_library(&fixture_opts()).unwrap();
        let factorio = result.games.iter().find(|g| g.app_id == 427520).unwrap();

        assert!(factorio.installed);
        assert_eq!(factorio.name, "Factorio");
        assert_eq!(factorio.playtime_minutes, Some(1038));
        assert_eq!(factorio.playtime_2wks_minutes, Some(0));
    }

    #[test]
    fn scan_all_manifest_games_present() {
        let result = scan_library(&fixture_opts()).unwrap();
        let installed_ids: Vec<u32> = result
            .games
            .iter()
            .filter(|g| g.installed)
            .map(|g| g.app_id)
            .collect();
        assert!(installed_ids.contains(&730), "CS2 should be installed");
        assert!(
            installed_ids.contains(&427520),
            "Factorio should be installed"
        );
    }

    // -- Collections ----------------------------------------------------------

    #[test]
    fn scan_cs2_collections() {
        let result = scan_library(&fixture_opts()).unwrap();
        let cs2 = result.games.iter().find(|g| g.app_id == 730).unwrap();

        assert!(
            cs2.steam_collections.contains(&"Favorites".to_string()),
            "CS2 should be in Favorites"
        );
        assert!(
            cs2.steam_collections.contains(&"Indie".to_string()),
            "CS2 should be in Indie"
        );
    }

    #[test]
    fn scan_factorio_collections() {
        let result = scan_library(&fixture_opts()).unwrap();
        let factorio = result.games.iter().find(|g| g.app_id == 427520).unwrap();

        assert!(
            factorio
                .steam_collections
                .contains(&"Favorites".to_string()),
            "Factorio should be in Favorites"
        );
        assert!(
            !factorio.steam_collections.contains(&"Indie".to_string()),
            "Factorio should not be in Indie"
        );
    }

    // -- Hidden ---------------------------------------------------------------

    #[test]
    fn scan_no_hidden_games_in_fixture() {
        // The fixture hidden collection is empty, so no game should be hidden.
        let result = scan_library(&fixture_opts()).unwrap();
        assert!(
            result.games.iter().all(|g| !g.is_hidden),
            "no games should be hidden with an empty hidden collection"
        );
    }

    // -- Non-installed games --------------------------------------------------

    #[test]
    fn scan_non_installed_from_localconfig() {
        let result = scan_library(&fixture_opts()).unwrap();

        // App 999 is in localconfig (playtime 5) and in the Indie collection
        // but not installed. It should appear as a non-installed game.
        let app999 = result.games.iter().find(|g| g.app_id == 999);
        assert!(
            app999.is_some(),
            "app 999 should be included from localconfig/collections"
        );
        let app999 = app999.unwrap();
        assert!(!app999.installed);
        assert_eq!(app999.playtime_minutes, Some(5));
        assert!(
            app999.steam_collections.contains(&"Indie".to_string()),
            "app 999 should be in Indie collection"
        );
    }

    // -- Name fallback --------------------------------------------------------

    #[test]
    fn scan_name_fallback_librarycache() {
        // App 999 has no manifest and no librarycache entry -> "App 999".
        let result = scan_library(&fixture_opts()).unwrap();
        let app999 = result.games.iter().find(|g| g.app_id == 999).unwrap();
        assert_eq!(app999.name, "App 999");
    }

    #[test]
    fn scan_name_from_manifest() {
        // Both 730 and 427520 have names in their manifests.
        let result = scan_library(&fixture_opts()).unwrap();
        assert_eq!(
            result.games.iter().find(|g| g.app_id == 730).unwrap().name,
            "Counter-Strike 2"
        );
        assert_eq!(
            result
                .games
                .iter()
                .find(|g| g.app_id == 427520)
                .unwrap()
                .name,
            "Factorio"
        );
    }

    // -- Sorting --------------------------------------------------------------

    #[test]
    fn scan_sorted_by_name_then_app_id() {
        let result = scan_library(&fixture_opts()).unwrap();

        for window in result.games.windows(2) {
            let a = &window[0];
            let b = &window[1];
            assert!(
                (a.name.as_str(), a.app_id) <= (b.name.as_str(), b.app_id),
                "games not sorted by (name, app_id): \
                 ({:?}, {}) should come before ({:?}, {})",
                a.name,
                a.app_id,
                b.name,
                b.app_id,
            );
        }
    }

    // -- Determinism ----------------------------------------------------------

    #[test]
    fn scan_deterministic() {
        let r1 = scan_library(&fixture_opts()).unwrap();
        let r2 = scan_library(&fixture_opts()).unwrap();

        let j1 = serde_json::to_string(&r1).unwrap();
        let j2 = serde_json::to_string(&r2).unwrap();
        assert_eq!(j1, j2);
    }

    // -- Empty / missing optional files --------------------------------------

    #[test]
    fn scan_empty_cloudstorage() {
        let opts = ScanOptions {
            steam_dir: fixture_empty_cloud_dir(),
            account: None,
            fixtures: None,
        };
        let result = scan_library(&opts).unwrap();

        // Should still have installed games from manifests.
        assert!(!result.games.is_empty());
        // All games should have empty collections (file is valid but empty).
        assert!(result.games.iter().all(|g| g.steam_collections.is_empty()));
        // No warning: the file exists and parses fine, it just has no entries.
    }

    // -- Fixtures override ----------------------------------------------------

    #[test]
    fn scan_fixtures_override_steam_dir() {
        let opts = ScanOptions {
            steam_dir: PathBuf::from("/nonexistent"),
            account: None,
            fixtures: Some(fixture_steam_dir()),
        };
        let result = scan_library(&opts).unwrap();

        // fixtures should take precedence over steam_dir.
        assert_eq!(result.account, "vapourfly_fixture_user");
        assert!(!result.games.is_empty());
    }

    // -- Account override -----------------------------------------------------

    #[test]
    fn scan_with_account_override() {
        let opts = ScanOptions {
            steam_dir: fixture_steam_dir(),
            account: Some("vapourfly_fixture_user".into()),
            fixtures: None,
        };
        let result = scan_library(&opts).unwrap();
        assert_eq!(result.account, "vapourfly_fixture_user");
    }
}
