//! Steam platform path detection and file discovery.
//!
//! Detects Steam installation directories, user accounts, library folders,
//! and installed app manifests across macOS, Linux (including Steam Deck),
//! and Windows.

use std::path::{Path, PathBuf};

use crate::error::{Result, VapourflyError};
use crate::models::VdfNode;
use crate::steam::parse_text_vdf;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// A Steam user account parsed from `loginusers.vdf`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SteamAccount {
    /// 64-bit Steam ID (e.g. `76561198000000000`).
    pub steam_id64: String,
    /// Login/account name (e.g. `vapourfly_fixture_user`).
    pub account_name: String,
    /// Display/persona name (e.g. `Vapourfly Fixture`).
    pub persona_name: String,
    /// Whether this was the most recently logged-in account.
    pub most_recent: bool,
}

/// An installed application parsed from an `appmanifest_*.acf` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppManifest {
    /// Steam application ID.
    pub app_id: u32,
    /// Display name of the application.
    pub name: String,
    /// Installation directory name (relative to the library folder's
    /// `steamapps/common/`).
    pub installdir: String,
    /// Bitmask state flags (4 = fully installed).
    pub state_flags: u32,
    /// The library folder this app belongs to.
    pub library_folder: PathBuf,
}

// ---------------------------------------------------------------------------
// Platform Steam directory detection
// ---------------------------------------------------------------------------

/// Detect all candidate Steam installation directories for the current platform.
///
/// Returns a list of well-known paths. Callers should check `.exists()` on
/// each entry to filter to actual installations. On unsupported platforms the
/// returned `Vec` is empty.
///
/// When `fixtures_root` is `Some`, platform detection is bypassed entirely and
/// the given root is returned as the sole entry. This is used by `--fixtures`.
pub fn detect_steam_dirs(fixtures_root: Option<&Path>) -> Vec<PathBuf> {
    if let Some(root) = fixtures_root {
        return vec![root.to_path_buf()];
    }

    let mut dirs = Vec::new();

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Library/Application Support/Steam"));
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(home) = dirs::home_dir() {
            // Flatpak Steam
            dirs.push(home.join(".var/app/com.valvesoftware.Steam/data/Steam"));
            // Standard symlink
            dirs.push(home.join(".steam/steam"));
            // Direct path (also used on Steam Deck)
            dirs.push(home.join(".local/share/Steam"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Try registry first
        if let Some(path) = windows_registry_steam_path() {
            dirs.push(path);
        }
        // Fallback to Program Files
        if let Some(pf) = std::env::var_os("ProgramFiles(x86)") {
            dirs.push(PathBuf::from(pf).join("Steam"));
        }
        dirs.push(PathBuf::from("C:\\Program Files (x86)\\Steam"));
    }

    dirs
}

// ---------------------------------------------------------------------------
// Account detection
// ---------------------------------------------------------------------------

/// Parse `loginusers.vdf` inside `steam_dir` and return all known accounts.
///
/// Returns an empty `Vec` if the file does not exist or contains no user
/// entries.
pub fn detect_accounts(steam_dir: &Path) -> Result<Vec<SteamAccount>> {
    let path = steam_dir.join("config").join("loginusers.vdf");
    let content = std::fs::read_to_string(&path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            VapourflyError::FileNotFound {
                path: crate::error::SafePath::new(&path),
            }
        } else {
            VapourflyError::InvalidInput(format!("failed to read {}: {}", path.display(), e))
        }
    })?;

    let root = parse_text_vdf(&content)?;
    let users = root.child_object(&["users"]).ok_or_else(|| {
        VapourflyError::InvalidInput("loginusers.vdf: missing \"users\" top-level key".into())
    })?;

    let entries = match users {
        VdfNode::Object(e) => e,
        _ => {
            return Err(VapourflyError::InvalidInput(
                "loginusers.vdf: \"users\" is not an object".into(),
            ));
        }
    };

    let mut accounts = Vec::new();
    for (steam_id, user_node) in entries {
        if let VdfNode::Object(_) = user_node {
            let account_name = user_node
                .first_string("AccountName")
                .unwrap_or("")
                .to_string();
            let persona_name = user_node
                .first_string("PersonaName")
                .unwrap_or("")
                .to_string();
            let most_recent = user_node.first_string("MostRecent") == Some("1");

            accounts.push(SteamAccount {
                steam_id64: steam_id.clone(),
                account_name,
                persona_name,
                most_recent,
            });
        }
    }

    Ok(accounts)
}

// ---------------------------------------------------------------------------
// Library folder detection
// ---------------------------------------------------------------------------

/// Parse `libraryfolders.vdf` inside `steam_dir` and return all library
/// folder paths.
///
/// The primary Steam directory itself is always included as a library folder
/// (it is the implicit library 0). Additional entries come from the VDF file.
pub fn detect_library_folders(steam_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut folders = vec![steam_dir.to_path_buf()];

    let path = steam_dir.join("config").join("libraryfolders.vdf");
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(folders),
        Err(e) => {
            return Err(VapourflyError::InvalidInput(format!(
                "failed to read {}: {}",
                path.display(),
                e
            )));
        }
    };

    let root = parse_text_vdf(&content)?;

    // Navigate to the "LibraryFolders" object.
    let lib_folders = match root.child_object(&["LibraryFolders"]) {
        Some(obj) => obj,
        None => return Ok(folders),
    };

    if let VdfNode::Object(entries) = lib_folders {
        for (key, value) in entries {
            // Library folder entries are keyed by numeric index ("0", "1", ...)
            // and contain a "path" sub-key. Skip metadata keys like
            // "TimeNextStatsReport" and "ContentStatsID".
            if key.chars().all(|c| c.is_ascii_digit()) {
                if let Some(path_val) = value.first_string("path") {
                    let p = PathBuf::from(path_val);
                    if p.exists() && !folders.contains(&p) {
                        folders.push(p);
                    }
                }
            }
        }
    }

    Ok(folders)
}

/// Select the best account from a list.
///
/// Priority: preferred (by name/id) > most_recent > single account > error
pub fn select_account<'a>(
    accounts: &'a [SteamAccount],
    preferred: Option<&str>,
) -> Result<&'a SteamAccount> {
    if accounts.is_empty() {
        return Err(VapourflyError::InvalidInput("no accounts found".into()));
    }

    // 1. Try preferred
    if let Some(pref) = preferred {
        if let Some(acc) = accounts
            .iter()
            .find(|a| a.account_name == pref || a.steam_id64 == pref || a.persona_name == pref)
        {
            return Ok(acc);
        }
    }

    // 2. Try most_recent
    if let Some(acc) = accounts.iter().find(|a| a.most_recent) {
        return Ok(acc);
    }

    // 3. Single account
    if accounts.len() == 1 {
        return Ok(&accounts[0]);
    }

    // 4. Ambiguous
    Err(VapourflyError::AmbiguousAccount {
        count: accounts.len(),
    })
}

/// Parse librarycache JSON for name fallback.
pub fn parse_librarycache(path: &Path) -> Result<std::collections::HashMap<u32, String>> {
    let mut map = std::collections::HashMap::new();
    if !path.exists() {
        return Ok(map);
    }

    let content = std::fs::read_to_string(path).map_err(|_| VapourflyError::FileNotFound {
        path: crate::SafePath::new(path),
    })?;

    // librarycache.json is a simple JSON object: {"730": "Counter-Strike 2", ...}
    if let Ok(obj) = serde_json::from_str::<std::collections::HashMap<String, String>>(&content) {
        for (k, v) in obj {
            if let Ok(app_id) = k.parse::<u32>() {
                map.insert(app_id, v);
            }
        }
    }

    Ok(map)
}

// ---------------------------------------------------------------------------
// Path redaction
// ---------------------------------------------------------------------------

/// Redact a path for safe display: returns only the file-name component,
/// with the directory portion replaced by `[REDACTED]`.
///
/// If the path has no file-name component (e.g. it is `/`), returns
/// `[REDACTED]`.
pub fn redact_path(path: &Path) -> String {
    match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => format!("[REDACTED]/{name}"),
        None => "[REDACTED]".to_string(),
    }
}

// ---------------------------------------------------------------------------
// App manifest parsing
// ---------------------------------------------------------------------------

/// Parse all `appmanifest_*.acf` files in a library folder's `steamapps/`
/// directory.
///
/// Returns an empty `Vec` if the `steamapps/` directory does not exist.
pub fn parse_appmanifests(library_folder: &Path) -> Result<Vec<AppManifest>> {
    let steamapps = library_folder.join("steamapps");
    if !steamapps.is_dir() {
        return Ok(Vec::new());
    }

    let mut manifests = Vec::new();
    let entries = std::fs::read_dir(&steamapps).map_err(|e| {
        VapourflyError::InvalidInput(format!(
            "failed to read directory {}: {}",
            steamapps.display(),
            e
        ))
    })?;

    for entry in entries {
        let entry = entry
            .map_err(|e| VapourflyError::InvalidInput(format!("failed to read dir entry: {e}")))?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if !name_str.starts_with("appmanifest_") || !name_str.ends_with(".acf") {
            continue;
        }

        let content = std::fs::read_to_string(entry.path()).map_err(|e| {
            VapourflyError::InvalidInput(format!(
                "failed to read {}: {}",
                entry.path().display(),
                e
            ))
        })?;

        let root = parse_text_vdf(&content)?;

        // ACF manifests wrap everything under an "AppState" object.
        let state = root.child_object(&["AppState"]).unwrap_or(&root);

        let app_id_str = state.first_string("appid").ok_or_else(|| {
            VapourflyError::InvalidInput(format!("{name_str}: missing \"appid\"",))
        })?;
        let app_id: u32 = app_id_str.parse().map_err(|_| {
            VapourflyError::InvalidInput(format!("{name_str}: invalid appid \"{app_id_str}\"",))
        })?;

        let name = state.first_string("name").unwrap_or("").to_string();
        let installdir = state.first_string("installdir").unwrap_or("").to_string();
        let state_flags: u32 = state
            .first_string("StateFlags")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        manifests.push(AppManifest {
            app_id,
            name,
            installdir,
            state_flags,
            library_folder: library_folder.to_path_buf(),
        });
    }

    Ok(manifests)
}

// ---------------------------------------------------------------------------
// Windows registry helper
// ---------------------------------------------------------------------------

/// On Windows, attempt to read the Steam install path from the registry key
/// `HKCU\Software\Valve\Steam\SteamPath`.
///
/// Returns `None` on non-Windows platforms or if the key is missing.
#[cfg(target_os = "windows")]
fn windows_registry_steam_path() -> Option<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey("Software\\Valve\\Steam").ok()?;
    let val: String = key.get_value("SteamPath").ok()?;
    Some(PathBuf::from(val))
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

    // -- detect_steam_dirs ---------------------------------------------------

    #[test]
    fn detect_steam_dirs_with_fixtures_returns_root() {
        let root = fixture_steam_dir();
        let dirs = detect_steam_dirs(Some(&root));
        assert_eq!(dirs.len(), 1);
        assert_eq!(dirs[0], root);
    }

    #[test]
    fn detect_steam_dirs_platform_returns_nonempty() {
        let dirs = detect_steam_dirs(None);
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(
            !dirs.is_empty(),
            "should find at least one candidate on macOS/Linux"
        );
    }

    #[test]
    fn detect_steam_dirs_none_fixture_returns_multiple_on_linux() {
        // On Linux there are at least 3 candidate paths (flatpak, symlink, direct).
        #[cfg(target_os = "linux")]
        {
            let dirs = detect_steam_dirs(None);
            assert!(
                dirs.len() >= 3,
                "expected at least 3 candidates, got {}",
                dirs.len()
            );
        }
    }

    // -- detect_accounts ----------------------------------------------------

    #[test]
    fn detect_accounts_parses_fixture() {
        let accounts = detect_accounts(&fixture_steam_dir()).unwrap();
        assert_eq!(accounts.len(), 1);

        let acct = &accounts[0];
        assert_eq!(acct.steam_id64, "76561198000000000");
        assert_eq!(acct.account_name, "vapourfly_fixture_user");
        assert_eq!(acct.persona_name, "Vapourfly Fixture");
        assert!(acct.most_recent);
    }

    #[test]
    fn detect_accounts_missing_file_returns_error() {
        let result = detect_accounts(Path::new("/nonexistent/steam"));
        assert!(result.is_err());
    }

    // -- detect_library_folders ---------------------------------------------

    #[test]
    fn detect_library_folders_parses_fixture() {
        let folders = detect_library_folders(&fixture_steam_dir()).unwrap();
        // Should contain the primary steam_dir plus the library from the VDF.
        // The VDF has "path" = "./" which resolves relative; the primary dir
        // is always present.
        assert!(
            folders.iter().any(|f| *f == fixture_steam_dir()),
            "should include the primary steam_dir, got {folders:?}",
        );
    }

    #[test]
    fn detect_library_folders_missing_file_returns_primary_only() {
        let folders = detect_library_folders(Path::new("/nonexistent/steam")).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0], PathBuf::from("/nonexistent/steam"));
    }

    // -- redact_path --------------------------------------------------------

    #[test]
    fn redact_path_shows_filename_only() {
        let redacted = redact_path(Path::new("/Users/alice/.steam/config/config.vdf"));
        assert_eq!(redacted, "[REDACTED]/config.vdf");
    }

    #[test]
    fn redact_path_handles_root() {
        let redacted = redact_path(Path::new("/"));
        assert_eq!(redacted, "[REDACTED]");
    }

    #[test]
    fn redact_path_handles_single_component() {
        let redacted = redact_path(Path::new("myfile.txt"));
        assert_eq!(redacted, "[REDACTED]/myfile.txt");
    }

    // -- parse_appmanifests -------------------------------------------------

    #[test]
    fn parse_appmanifests_parses_fixture() {
        let manifests = parse_appmanifests(&fixture_steam_dir()).unwrap();
        assert_eq!(manifests.len(), 2);

        // Sort by app_id for deterministic assertions.
        let mut sorted = manifests.clone();
        sorted.sort_by_key(|m| m.app_id);

        assert_eq!(sorted[0].app_id, 730);
        assert_eq!(sorted[0].name, "Counter-Strike 2");
        assert_eq!(sorted[0].installdir, "cs2");
        assert_eq!(sorted[0].state_flags, 4);
        assert_eq!(sorted[0].library_folder, fixture_steam_dir());

        assert_eq!(sorted[1].app_id, 223850);
        assert_eq!(sorted[1].name, "Factorio");
        assert_eq!(sorted[1].installdir, "Factorio");
        assert_eq!(sorted[1].state_flags, 4);
        assert_eq!(sorted[1].library_folder, fixture_steam_dir());
    }

    #[test]
    fn parse_appmanifests_no_steamapps_returns_empty() {
        let manifests = parse_appmanifests(Path::new("/nonexistent")).unwrap();
        assert!(manifests.is_empty());
    }

    // -- round-trip: detect_accounts -> parse_appmanifests -------------------

    #[test]
    fn full_fixture_pipeline() {
        let dir = fixture_steam_dir();

        // Accounts
        let accounts = detect_accounts(&dir).unwrap();
        let primary = accounts.iter().find(|a| a.most_recent).unwrap();
        assert_eq!(primary.steam_id64, "76561198000000000");

        // Library folders
        let folders = detect_library_folders(&dir).unwrap();
        assert!(!folders.is_empty());

        // App manifests from primary library
        let manifests = parse_appmanifests(&folders[0]).unwrap();
        assert_eq!(manifests.len(), 2);

        // Redaction
        let redacted = redact_path(&dir.join("config").join("loginusers.vdf"));
        assert!(
            !redacted.contains("fixtures"),
            "redacted path should not leak directory: {redacted}"
        );
        assert!(redacted.contains("loginusers.vdf"));
    }
}
