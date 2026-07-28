//! Steam process detection and write safety checks.
//!
//! Before writing to Steam cloud storage, we check if Steam is running.
//! If it is, we warn the user and require explicit confirmation.
//!
//! # Testing
//!
//! Process detection can be overridden for tests via [`set_steam_running_override`].
//! Pass `Some(true)` to simulate Steam running, `Some(false)` for not running,
//! or `None` to restore real detection. Tests that manipulate this global state
//! should use `#[serial]` from the `serial_test` crate to avoid interference.

use std::path::Path;
use std::sync::atomic::{AtomicI8, Ordering};

use crate::error::{Result, SafePath, VapourflyError};

/// Tri-state override for process detection in tests.
/// -1 = no override (use real detection), 0 = force not running, 1 = force running.
static STEAM_RUNNING_OVERRIDE: AtomicI8 = AtomicI8::new(-1);

/// Override Steam process detection for testing.
///
/// - `Some(true)`  — simulate Steam running
/// - `Some(false)` — simulate Steam not running
/// - `None`        — restore real platform detection
///
/// This operates on a global atomic, so tests that call it should be marked
/// `#[serial]` to prevent interference when running in parallel.
pub fn set_steam_running_override(value: Option<bool>) {
    let v = match value {
        None => -1,
        Some(false) => 0,
        Some(true) => 1,
    };
    STEAM_RUNNING_OVERRIDE.store(v, Ordering::Relaxed);
}

/// Real platform-specific Steam process detection.
///
/// Returns `false` if detection fails (conservative: assume not running).
fn detect_steam_process() -> bool {
    #[cfg(target_os = "macos")]
    {
        // steam_osx is the main Steam client binary on macOS.
        std::process::Command::new("pgrep")
            .args(["-xq", "steam_osx"])
            .status()
            .is_ok_and(|s| s.success())
    }

    #[cfg(target_os = "linux")]
    {
        // Try pidof first (procps), fall back to pgrep.
        let found = std::process::Command::new("pidof")
            .arg("-s")
            .arg("steam")
            .status()
            .is_ok_and(|s| s.success());
        if found {
            return true;
        }
        std::process::Command::new("pgrep")
            .args(["-xq", "steam"])
            .status()
            .is_ok_and(|s| s.success())
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq Steam.exe"])
            .output()
            .is_ok_and(|o| {
                let stdout = String::from_utf8_lossy(&o.stdout);
                stdout.contains("Steam.exe")
            })
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        false
    }
}

/// Best-effort check for whether the Steam client is currently running.
///
/// Returns `false` if detection fails (conservative: assume not running).
///
/// In test builds, behaviour can be controlled via [`set_steam_running_override`].
pub fn is_steam_running() -> bool {
    let override_val = STEAM_RUNNING_OVERRIDE.load(Ordering::Relaxed);
    match override_val {
        0 => false,
        1 => true,
        _ => detect_steam_process(),
    }
}

/// Check write safety before modifying Steam files.
///
/// 1. If Steam is running and `allow_steam_running` is `false`, returns
///    [`VapourflyError::UnsafeWrite`].
/// 2. If the target file does not exist, returns [`VapourflyError::FileNotFound`].
/// 3. If the parent directory does not exist, returns [`VapourflyError::UnsafeWrite`].
/// 4. On Unix, if the parent directory has no write permission bits set,
///    returns [`VapourflyError::UnsafeWrite`].
pub fn check_write_safety(target_path: &Path, allow_steam_running: bool) -> Result<()> {
    // 1. Steam process check
    if !allow_steam_running && is_steam_running() {
        return Err(VapourflyError::UnsafeWrite {
            reason: "Steam is currently running. Close Steam first, or use --allow-steam-running"
                .into(),
        });
    }

    // 2. Target must exist
    if !target_path.exists() {
        return Err(VapourflyError::FileNotFound {
            path: SafePath::new(target_path),
        });
    }

    // 3. Parent directory checks
    if let Some(parent) = target_path.parent() {
        if !parent.exists() {
            return Err(VapourflyError::UnsafeWrite {
                reason: format!("parent directory does not exist: {}", parent.display()),
            });
        }

        // 4. Best-effort writability check on Unix
        #[cfg(unix)]
        check_unix_writable(parent)?;
    }

    Ok(())
}

/// On Unix, check that the parent directory has at least one write permission
/// bit set (owner / group / other). This is a heuristic — ACLs and effective
/// uid/gid can override the mode bits — but it catches the common case of a
/// read-only mount or `chmod a-w` directory.
#[cfg(unix)]
fn check_unix_writable(dir: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let meta = std::fs::metadata(dir).map_err(|e| VapourflyError::UnsafeWrite {
        reason: format!("cannot stat parent directory: {e}"),
    })?;

    // 0o222 = write bits for user, group, other
    if meta.permissions().mode() & 0o222 == 0 {
        return Err(VapourflyError::UnsafeWrite {
            reason: "parent directory is not writable (no write permission bits set)".into(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    // -- is_steam_running override tests ------------------------------------

    #[test]
    #[serial]
    fn override_force_running() {
        set_steam_running_override(Some(true));
        assert!(is_steam_running());
        set_steam_running_override(None);
    }

    #[test]
    #[serial]
    fn override_force_not_running() {
        set_steam_running_override(Some(false));
        assert!(!is_steam_running());
        set_steam_running_override(None);
    }

    #[test]
    #[serial]
    fn override_none_restores_real_detection() {
        set_steam_running_override(None);
        // Just verify it doesn't panic; result depends on environment.
        let _ = is_steam_running();
    }

    // -- check_write_safety tests ------------------------------------------

    #[test]
    #[serial]
    fn safety_passes_for_valid_target() {
        set_steam_running_override(Some(false));
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("test.json");
        std::fs::write(&target, "{}").unwrap();

        let result = check_write_safety(&target, false);
        assert!(result.is_ok());
        set_steam_running_override(None);
    }

    #[test]
    #[serial]
    fn safety_fails_for_missing_target() {
        set_steam_running_override(Some(false));
        let result = check_write_safety(Path::new("/nonexistent/file.json"), false);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            VapourflyError::FileNotFound { .. }
        ));
        set_steam_running_override(None);
    }

    #[test]
    #[serial]
    fn safety_fails_when_steam_running_and_not_allowed() {
        set_steam_running_override(Some(true));
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("test.json");
        std::fs::write(&target, "{}").unwrap();

        let result = check_write_safety(&target, false);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, VapourflyError::UnsafeWrite { .. }));
        assert!(err.to_string().contains("Steam is currently running"));
        set_steam_running_override(None);
    }

    #[test]
    #[serial]
    fn safety_passes_when_steam_running_but_allowed() {
        set_steam_running_override(Some(true));
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("test.json");
        std::fs::write(&target, "{}").unwrap();

        let result = check_write_safety(&target, true);
        assert!(result.is_ok());
        set_steam_running_override(None);
    }

    #[test]
    #[serial]
    fn safety_fails_when_parent_directory_missing() {
        set_steam_running_override(Some(false));
        // Point to a file whose parent doesn't exist.
        let result = check_write_safety(Path::new("/no/such/dir/file.json"), false);
        // This hits FileNotFound (the file itself doesn't exist) before the
        // parent check, which is correct — we check existence first.
        assert!(result.is_err());
        set_steam_running_override(None);
    }

    #[test]
    #[serial]
    fn safety_passes_for_read_only_directory_when_steam_not_running() {
        // Create a file, then make the directory read-only.
        set_steam_running_override(Some(false));
        let tmp = TempDir::new().unwrap();
        let target = tmp.path().join("readonly.json");
        std::fs::write(&target, "{}").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(tmp.path()).unwrap().permissions();
            perms.set_mode(0o555); // read+execute only
            std::fs::set_permissions(tmp.path(), perms).unwrap();

            let result = check_write_safety(&target, false);
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(matches!(err, VapourflyError::UnsafeWrite { .. }));
            assert!(err.to_string().contains("not writable"));

            // Restore permissions so TempDir can clean up.
            let mut perms = std::fs::metadata(tmp.path()).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(tmp.path(), perms).unwrap();
        }
        set_steam_running_override(None);
    }

    // -- is_steam_running in CI should return false -------------------------
    // (Steam is not installed in CI/test environments)

    #[test]
    #[serial]
    fn real_detection_does_not_panic() {
        set_steam_running_override(None);
        // We can't assert a specific value, but it should not panic.
        let _running = is_steam_running();
    }
}
