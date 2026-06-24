//! Configuration model for Vapourfly.
//!
//! Loading precedence:
//! 1. CLI flags
//! 2. Environment variables
//! 3. Config file
//! 4. Platform defaults

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Vapourfly configuration.  Secrets are never stored — only credential
/// *presence* is tracked.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VapourflyConfig {
    /// Override for the Steam installation directory.
    pub steam_dir: Option<PathBuf>,

    /// Override for the Steam account identifier.
    pub account: Option<String>,

    /// Root for cache files (default: platform-specific app data dir).
    pub cache_root: PathBuf,

    /// Root for app data (default: platform-specific config dir).
    pub app_data_root: PathBuf,

    /// Whether IGDB credentials are available.
    pub has_igdb_credentials: bool,

    /// Whether RAWG credentials are available.
    pub has_rawg_credentials: bool,

    /// Country code for Steam Store price queries (default: "US").
    pub cc: String,

    /// Language for Steam Store queries (default: "english").
    pub lang: String,

    /// Number of backups to retain (default: 5).
    pub backup_retention_count: u32,
}

impl VapourflyConfig {
    /// Build a configuration from CLI overrides and environment variables.
    ///
    /// CLI flags take highest priority, then env vars, then platform defaults.
    pub fn from_cli_and_env(
        cli_steam_dir: Option<PathBuf>,
        cli_account: Option<String>,
    ) -> Result<Self> {
        let steam_dir =
            cli_steam_dir.or_else(|| std::env::var("VAPOURFLY_STEAM_DIR").ok().map(PathBuf::from));

        let account = cli_account.or_else(|| std::env::var("VAPOURFLY_ACCOUNT").ok());

        let cache_root = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vapourfly");

        let app_data_root = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vapourfly");

        let has_igdb_credentials = std::env::var("VAPOURFLY_IGDB_CLIENT_ID").is_ok()
            && std::env::var("VAPOURFLY_IGDB_CLIENT_SECRET").is_ok();

        let has_rawg_credentials = std::env::var("VAPOURFLY_RAWG_KEY").is_ok();

        let cc = std::env::var("VAPOURFLY_CC").unwrap_or_else(|_| "US".into());
        let lang = std::env::var("VAPOURFLY_LANG").unwrap_or_else(|_| "english".into());

        Ok(Self {
            steam_dir,
            account,
            cache_root,
            app_data_root,
            has_igdb_credentials,
            has_rawg_credentials,
            cc,
            lang,
            backup_retention_count: 5,
        })
    }

    /// Return the cache directory for a given source.
    pub fn cache_dir(&self, source: &str) -> PathBuf {
        self.cache_root.join("cache").join(source)
    }

    /// Return the backup directory for cloud storage writes.
    pub fn backup_dir(&self) -> PathBuf {
        self.cache_root.join("backups")
    }

    /// Detect the Steam installation directory using platform-specific paths.
    pub fn detect_steam_dir() -> Option<PathBuf> {
        #[cfg(target_os = "macos")]
        {
            dirs::home_dir().map(|h| h.join("Library/Application Support/Steam"))
        }
        #[cfg(target_os = "linux")]
        {
            // Try ~/.steam/steam first (symlink), then ~/.local/share/Steam
            let home = dirs::home_dir()?;
            let p1 = home.join(".steam/steam");
            if p1.exists() {
                return Some(p1);
            }
            let p2 = home.join(".local/share/Steam");
            if p2.exists() {
                return Some(p2);
            }
            Some(p1) // fallback to default even if not present
        }
        #[cfg(target_os = "windows")]
        {
            // Try registry, then fallback
            // For now just use the common default
            Some(PathBuf::from("C:\\Program Files (x86)\\Steam"))
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let cfg = VapourflyConfig::from_cli_and_env(None, None).unwrap();
        assert_eq!(cfg.cc, "US");
        assert_eq!(cfg.lang, "english");
        assert_eq!(cfg.backup_retention_count, 5);
    }

    #[test]
    fn config_cli_overrides() {
        let cfg = VapourflyConfig::from_cli_and_env(
            Some(PathBuf::from("/custom/steam")),
            Some("myaccount".into()),
        )
        .unwrap();
        assert_eq!(cfg.steam_dir, Some(PathBuf::from("/custom/steam")));
        assert_eq!(cfg.account, Some("myaccount".into()));
    }

    #[test]
    fn cache_dir_structure() {
        let cfg = VapourflyConfig::from_cli_and_env(None, None).unwrap();
        let igdb_cache = cfg.cache_dir("igdb");
        assert!(igdb_cache.ends_with("vapourfly/cache/igdb"));
    }
}
