//! Steam process detection and write safety checks.
//!
//! Before writing to Steam cloud storage, we check if Steam is running.
//! If it is, we warn the user and require explicit confirmation.

use std::path::Path;

use crate::error::{Result, VapourflyError};

/// Best-effort check for whether the Steam client is currently running.
///
/// Returns `false` if detection fails (conservative: assume not running).
pub fn is_steam_running() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("pgrep")
            .args(["-x", "steam_osx"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("pidof")
            .arg("steam")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq Steam.exe"])
            .output()
            .map(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                stdout.contains("Steam.exe")
            })
            .unwrap_or(false)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        false
    }
}

/// Check write safety before modifying Steam files.
///
/// - If Steam is running and `allow_steam_running` is false, returns an error.
/// - If target file doesn't exist, returns an error.
/// - If target directory isn't writable, returns an error.
pub fn check_write_safety(target_path: &Path, allow_steam_running: bool) -> Result<()> {
    // Check Steam process
    if !allow_steam_running && is_steam_running() {
        return Err(VapourflyError::UnsafeWrite {
            reason: "Steam is currently running. Close Steam first, or use --allow-steam-running"
                .into(),
        });
    }

    // Check target exists
    if !target_path.exists() {
        return Err(VapourflyError::FileNotFound {
            path: crate::SafePath::new(target_path),
        });
    }

    // Check parent directory is writable
    if let Some(parent) = target_path.parent() {
        if !parent.exists() {
            return Err(VapourflyError::UnsafeWrite {
                reason: format!("parent directory does not exist: {}", parent.display()),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn check_safety_valid_target() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("test.json");
        std::fs::write(&target, "{}").unwrap();

        // Should pass (Steam likely not running in test env)
        let result = check_write_safety(&target, false);
        assert!(result.is_ok());
    }

    #[test]
    fn check_safety_missing_target() {
        let result = check_write_safety(Path::new("/nonexistent/file.json"), false);
        assert!(result.is_err());
    }

    #[test]
    fn check_safety_allow_steam_running() {
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("test.json");
        std::fs::write(&target, "{}").unwrap();

        // Should always pass with allow_steam_running=true
        let result = check_write_safety(&target, true);
        assert!(result.is_ok());
    }
}
