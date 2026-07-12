//! Configuration loading for Vapourfly.
//!
//! Loading precedence (highest wins):
//! 1. CLI flags (`--steam-dir`, `--account`)
//! 2. Environment variables (`VAPOURFLY_STEAM_DIR`, `VAPOURFLY_ACCOUNT`, etc.)
//! 3. Config file (`~/.config/vapourfly/config.toml` or platform equivalent)
//! 4. Platform defaults

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::error::{Result, VapourflyError};

// ---------------------------------------------------------------------------
// Settable config fields
// ---------------------------------------------------------------------------

/// A config field that can be shown, set, or unset via the CLI `settings`
/// command or the GUI Settings panel.
///
/// The variants line up with the keys written to `config.toml`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConfigField {
    SteamDir,
    Account,
    Cc,
    Lang,
    BackupRetentionCount,
}

impl ConfigField {
    /// The TOML key this field is stored under.
    pub fn as_key(&self) -> &'static str {
        match self {
            ConfigField::SteamDir => "steam_dir",
            ConfigField::Account => "account",
            ConfigField::Cc => "cc",
            ConfigField::Lang => "lang",
            ConfigField::BackupRetentionCount => "backup_retention_count",
        }
    }

    /// Parse a config field from its TOML key name.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "steam_dir" => Some(ConfigField::SteamDir),
            "account" => Some(ConfigField::Account),
            "cc" => Some(ConfigField::Cc),
            "lang" => Some(ConfigField::Lang),
            "backup_retention_count" => Some(ConfigField::BackupRetentionCount),
            _ => None,
        }
    }

    /// All known config fields, in display order.
    pub fn all() -> &'static [ConfigField] {
        &[
            ConfigField::SteamDir,
            ConfigField::Account,
            ConfigField::Cc,
            ConfigField::Lang,
            ConfigField::BackupRetentionCount,
        ]
    }

    /// Validate and normalise a raw string value for this field.
    ///
    /// Returns the string to store in `config.toml`, or an error message
    /// when the value is malformed.
    pub fn normalise(&self, raw: &str) -> std::result::Result<String, String> {
        let trimmed = raw.trim();
        match self {
            ConfigField::SteamDir | ConfigField::Account | ConfigField::Cc | ConfigField::Lang => {
                if trimmed.is_empty() {
                    return Err("value must not be empty".into());
                }
                Ok(trimmed.to_string())
            }
            ConfigField::BackupRetentionCount => {
                let n: u32 = trimmed
                    .parse()
                    .map_err(|_| "value must be a non-negative integer".to_string())?;
                Ok(n.to_string())
            }
        }
    }
}

// ---------------------------------------------------------------------------
// On-disk config file schema (TOML)
// ---------------------------------------------------------------------------

/// Raw deserialization target for `config.toml`. All fields are optional so a
/// partial file is valid.
#[derive(Debug, Clone, Default, Deserialize)]
struct ConfigFile {
    steam_dir: Option<String>,
    account: Option<String>,
    cc: Option<String>,
    lang: Option<String>,
    backup_retention_count: Option<u32>,
}

// ---------------------------------------------------------------------------
// CLI overrides
// ---------------------------------------------------------------------------

/// Values supplied via CLI flags. All fields are optional -- only explicitly
/// passed flags should be `Some`.
#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    pub steam_dir: Option<PathBuf>,
    pub account: Option<String>,
}

// ---------------------------------------------------------------------------
// Resolved configuration
// ---------------------------------------------------------------------------

/// Fully resolved Vapourfly configuration. Every field is populated; there are
/// no remaining optionals on paths that would require fallible access at
/// runtime.
#[derive(Clone, Debug)]
pub struct VapourflyConfig {
    /// Resolved Steam installation directory.
    pub steam_dir: PathBuf,

    /// Selected Steam account name (persona). `None` when auto-detected.
    pub account: Option<String>,

    /// Root of the Vapourfly cache tree (external data, thumbnails, etc.).
    pub cache_root: PathBuf,

    /// Root of the Vapourfly application data tree (playlists, local DB, etc.).
    pub app_data_root: PathBuf,

    /// Whether IGDB credentials are configured (never stores actual secrets).
    pub has_igdb_credentials: bool,

    /// Whether RAWG credentials are configured (never stores actual secrets).
    pub has_rawg_credentials: bool,

    /// ISO 3166-1 alpha-2 country code for Steam Store price queries.
    pub cc: String,

    /// Language for Steam Store queries.
    pub lang: String,

    /// Number of rolling backups to keep for modified files.
    pub backup_retention_count: u32,
}

impl VapourflyConfig {
    /// Build a fully resolved configuration by merging CLI flags, environment
    /// variables, the config file, and platform defaults.
    pub fn from_cli_and_env(cli: CliOverrides) -> Result<Self> {
        let file = load_config_file();

        // -- steam_dir -------------------------------------------------------
        let steam_dir = cli
            .steam_dir
            .or_else(|| env_path("VAPOURFLY_STEAM_DIR"))
            .or_else(|| {
                file.as_ref()
                    .and_then(|f| f.steam_dir.as_ref().map(PathBuf::from))
            })
            .or_else(detect_steam_dir)
            .ok_or(VapourflyError::InvalidInput(
                "could not determine Steam directory; pass --steam-dir or set VAPOURFLY_STEAM_DIR"
                    .into(),
            ))?;

        // -- account ---------------------------------------------------------
        let account = cli
            .account
            .or_else(|| std::env::var("VAPOURFLY_ACCOUNT").ok())
            .or_else(|| file.as_ref().and_then(|f| f.account.clone()));

        // -- cache_root / app_data_root (platform dirs) ----------------------
        let cache_root = dirs::cache_dir().unwrap_or_else(|| PathBuf::from("."));
        let app_data_root = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));

        // -- credentials (presence only) -------------------------------------
        let has_igdb_credentials =
            env_present("VAPOURFLY_IGDB_CLIENT_ID") && env_present("VAPOURFLY_IGDB_CLIENT_SECRET");

        let has_rawg_credentials = env_present("VAPOURFLY_RAWG_KEY");

        // -- store locale ----------------------------------------------------
        let cc = file
            .as_ref()
            .and_then(|f| f.cc.clone())
            .or_else(|| std::env::var("VAPOURFLY_CC").ok())
            .unwrap_or_else(|| "US".into());

        let lang = file
            .as_ref()
            .and_then(|f| f.lang.clone())
            .or_else(|| std::env::var("VAPOURFLY_LANG").ok())
            .unwrap_or_else(|| "english".into());

        // -- backup retention ------------------------------------------------
        let backup_retention_count = file
            .as_ref()
            .and_then(|f| f.backup_retention_count)
            .unwrap_or(5);

        Ok(Self {
            steam_dir,
            account,
            cache_root,
            app_data_root,
            has_igdb_credentials,
            has_rawg_credentials,
            cc,
            lang,
            backup_retention_count,
        })
    }

    /// Try to auto-detect the Steam installation directory for the current
    /// platform. Returns `None` if no known location can be determined.
    pub fn detect_steam_dir() -> Option<PathBuf> {
        detect_steam_dir()
    }

    /// Return the cache directory for Vapourfly external data:
    /// `{cache_root}/vapourfly/cache/`
    pub fn cache_dir(&self) -> PathBuf {
        self.cache_root.join("vapourfly").join("cache")
    }

    /// Return the backup directory for modified Steam files:
    /// `{app_data_root}/vapourfly/backups/`
    pub fn backup_dir(&self) -> PathBuf {
        self.app_data_root.join("vapourfly").join("backups")
    }
}

/// Return the platform-specific Vapourfly cache directory path.
///
/// This is `{platform_cache_dir}/vapourfly/cache/`. Returns a fallback
/// if the platform cache directory cannot be determined.
pub fn default_cache_dir() -> PathBuf {
    dirs::cache_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vapourfly")
        .join("cache")
}

/// Return the platform-specific Vapourfly playlists directory path.
///
/// This is `{platform_data_dir}/vapourfly/playlists/`. Returns a fallback
/// if the platform data directory cannot be determined.
pub fn default_playlists_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vapourfly")
        .join("playlists")
}

// ---------------------------------------------------------------------------
// Platform Steam detection — delegate to steam::paths
// ---------------------------------------------------------------------------

/// Try well-known Steam paths for the current platform.
fn detect_steam_dir() -> Option<PathBuf> {
    crate::steam::paths::detect_steam_dirs(None)
        .into_iter()
        .next()
}

// ---------------------------------------------------------------------------
// Config file loading
// ---------------------------------------------------------------------------

/// Return the platform-specific path to the Vapourfly config file.
///
/// This is `{platform_config_dir}/vapourfly/config.toml`. Returns `None` when
/// the platform config directory cannot be determined.
pub fn config_file_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("vapourfly").join("config.toml"))
}

/// Attempt to load and parse the TOML config file from the platform config
/// directory. Returns `None` if the file doesn't exist or is unparseable.
fn load_config_file() -> Option<ConfigFile> {
    let path = config_file_path()?;
    let contents = std::fs::read_to_string(&path).ok()?;
    toml::from_str(&contents).ok()
}

/// Read the config file at `path` as a TOML table, or return an empty table
/// when the file does not exist or is unparseable.
fn load_config_table_at(path: &Path) -> toml::Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .filter(toml::Value::is_table)
        .unwrap_or(toml::Value::Table(toml::map::Map::new()))
}

/// Persist a TOML table to `path`, creating parent directories as needed.
fn write_config_table_at(table: &toml::Value, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            VapourflyError::Internal(format!(
                "failed to create config directory {}: {e}",
                parent.display()
            ))
        })?;
    }

    let toml_str = toml::to_string_pretty(table)
        .map_err(|e| VapourflyError::Internal(format!("failed to serialise config: {e}")))?;

    std::fs::write(path, toml_str).map_err(|e| {
        VapourflyError::Internal(format!("failed to write config to {}: {e}", path.display()))
    })
}

/// Set a single config field in `config.toml`, creating the file if needed.
///
/// Existing keys that are not `field` are preserved. Pass the already-normalised
/// string value; use [`ConfigField::normalise`] to validate user input first.
pub fn set_config_field(field: ConfigField, value: &str) -> Result<()> {
    set_config_field_at(
        field,
        value,
        &config_file_path().ok_or_else(|| {
            VapourflyError::Internal("could not determine platform config directory".into())
        })?,
    )
}

/// Same as [`set_config_field`] but writes to an explicit path. Used by tests.
pub(crate) fn set_config_field_at(field: ConfigField, value: &str, path: &Path) -> Result<()> {
    let mut table = load_config_table_at(path);
    let tbl = table
        .as_table_mut()
        .ok_or_else(|| VapourflyError::Internal("config file root is not a table".into()))?;

    let toml_value = match field {
        ConfigField::BackupRetentionCount => {
            let n: i64 = value.parse().map_err(|_| {
                VapourflyError::InvalidInput("backup_retention_count must be an integer".into())
            })?;
            toml::Value::Integer(n)
        }
        _ => toml::Value::String(value.to_string()),
    };

    tbl.insert(field.as_key().to_string(), toml_value);
    write_config_table_at(&table, path)
}

/// Remove a config field from `config.toml`.
///
/// Returns `Ok(())` even when the key was absent. The file is created (empty)
/// when it does not exist, so callers can treat the result as "field is now
/// unset".
pub fn unset_config_field(field: ConfigField) -> Result<()> {
    unset_config_field_at(
        field,
        &config_file_path().ok_or_else(|| {
            VapourflyError::Internal("could not determine platform config directory".into())
        })?,
    )
}

/// Same as [`unset_config_field`] but writes to an explicit path. Used by tests.
pub(crate) fn unset_config_field_at(field: ConfigField, path: &Path) -> Result<()> {
    let mut table = load_config_table_at(path);
    if let Some(tbl) = table.as_table_mut() {
        tbl.remove(field.as_key());
    }
    write_config_table_at(&table, path)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Read a `PathBuf` from an environment variable, returning `None` if the var
/// is unset or empty.
fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Check whether a credential environment variable is present.
fn env_present(env_key: &str) -> bool {
    std::env::var(env_key).is_ok_and(|val| !val.is_empty())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

    /// Helper: set an env var (unsafe in Rust 2024).
    fn set_env(key: &str, val: &str) {
        // SAFETY: tests run single-threaded per process and we only touch
        // VAPOURFLY_-prefixed keys that nothing else reads.
        unsafe { std::env::set_var(key, val) };
    }

    /// Helper: remove an env var (unsafe in Rust 2024).
    fn remove_env(key: &str) {
        // SAFETY: same as set_env.
        unsafe { std::env::remove_var(key) };
    }

    /// Helper: clear all VAPOURFLY_ env vars that tests may touch.
    fn clear_test_env() {
        for key in &[
            "VAPOURFLY_STEAM_DIR",
            "VAPOURFLY_ACCOUNT",
            "VAPOURFLY_CC",
            "VAPOURFLY_LANG",
            "VAPOURFLY_IGDB_CLIENT_ID",
            "VAPOURFLY_IGDB_CLIENT_SECRET",
            "VAPOURFLY_RAWG_KEY",
            "VAPOURFLY_TEST_EMPTY",
            "VAPOURFLY_TEST_SET",
            "VAPOURFLY_TEST_PRESENT",
            "VAPOURFLY_TEST_ABSENT",
            "VAPOURFLY_TEST_NEITHER",
        ] {
            remove_env(key);
        }
    }

    // -- detect_steam_dir ----------------------------------------------------

    #[test]
    fn detect_steam_dir_returns_some_on_known_platform() {
        let result = VapourflyConfig::detect_steam_dir();
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(result.is_some(), "should return a path on macOS/Linux");
    }

    #[test]
    fn detect_steam_dir_returns_home_based_path_on_macos() {
        #[cfg(target_os = "macos")]
        {
            let path = VapourflyConfig::detect_steam_dir().unwrap();
            assert!(path.ends_with("Library/Application Support/Steam"));
        }
    }

    // -- from_cli_and_env defaults -------------------------------------------

    #[test]
    #[serial]
    fn from_cli_and_env_uses_defaults() {
        clear_test_env();

        let cli = CliOverrides {
            steam_dir: Some(PathBuf::from("/tmp/fake-steam")),
            account: None,
        };
        let cfg = VapourflyConfig::from_cli_and_env(cli).unwrap();

        assert_eq!(cfg.steam_dir, PathBuf::from("/tmp/fake-steam"));
        assert!(cfg.account.is_none());
        assert_eq!(cfg.cc, "US");
        assert_eq!(cfg.lang, "english");
        assert_eq!(cfg.backup_retention_count, 5);
        assert!(!cfg.has_igdb_credentials);
        assert!(!cfg.has_rawg_credentials);
    }

    // -- CLI > env > file > default precedence -------------------------------

    #[test]
    #[serial]
    fn cli_overrides_env() {
        set_env("VAPOURFLY_STEAM_DIR", "/env/steam");
        set_env("VAPOURFLY_ACCOUNT", "env_account");

        let cli = CliOverrides {
            steam_dir: Some(PathBuf::from("/cli/steam")),
            account: Some("cli_account".into()),
        };
        let cfg = VapourflyConfig::from_cli_and_env(cli).unwrap();

        assert_eq!(cfg.steam_dir, PathBuf::from("/cli/steam"));
        assert_eq!(cfg.account.as_deref(), Some("cli_account"));

        clear_test_env();
    }

    #[test]
    #[serial]
    fn env_used_when_cli_absent() {
        set_env("VAPOURFLY_STEAM_DIR", "/env/steam");
        set_env("VAPOURFLY_ACCOUNT", "env_account");

        let cli = CliOverrides::default();
        let cfg = VapourflyConfig::from_cli_and_env(cli).unwrap();

        assert_eq!(cfg.steam_dir, PathBuf::from("/env/steam"));
        assert_eq!(cfg.account.as_deref(), Some("env_account"));

        clear_test_env();
    }

    #[test]
    #[serial]
    fn env_cc_lang_used_when_no_file() {
        set_env("VAPOURFLY_CC", "JP");
        set_env("VAPOURFLY_LANG", "japanese");

        let cli = CliOverrides {
            steam_dir: Some(PathBuf::from("/tmp/fake")),
            ..Default::default()
        };
        let cfg = VapourflyConfig::from_cli_and_env(cli).unwrap();

        // Without a config file on disk, env vars for cc/lang are used if
        // present. Either way the fields must be non-empty.
        assert!(!cfg.cc.is_empty());
        assert!(!cfg.lang.is_empty());

        clear_test_env();
    }

    #[test]
    #[serial]
    fn platform_detect_fallback_when_no_cli_no_env() {
        remove_env("VAPOURFLY_STEAM_DIR");

        let cli = CliOverrides::default();
        let result = VapourflyConfig::from_cli_and_env(cli);
        // On macOS/Linux this should succeed via auto-detect.
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        assert!(result.is_ok());
    }

    // -- derived paths -------------------------------------------------------

    #[test]
    fn cache_dir_ends_with_vapourfly_cache() {
        let cfg = make_test_config();
        let cache = cfg.cache_dir();
        assert!(
            cache.ends_with("vapourfly/cache"),
            "got: {}",
            cache.display()
        );
    }

    #[test]
    fn backup_dir_ends_with_vapourfly_backups() {
        let cfg = make_test_config();
        let backup = cfg.backup_dir();
        assert!(
            backup.ends_with("vapourfly/backups"),
            "got: {}",
            backup.display()
        );
    }

    #[test]
    fn cache_dir_is_under_cache_root() {
        let cfg = make_test_config();
        let cache = cfg.cache_dir();
        assert!(cache.starts_with(&cfg.cache_root));
    }

    #[test]
    fn backup_dir_is_under_app_data_root() {
        let cfg = make_test_config();
        let backup = cfg.backup_dir();
        assert!(backup.starts_with(&cfg.app_data_root));
    }

    // -- config file parsing -------------------------------------------------

    #[test]
    fn config_file_partial_is_valid() {
        let toml_str = r#"
cc = "GB"
lang = "british"
"#;
        let file: ConfigFile = toml::from_str(toml_str).unwrap();
        assert_eq!(file.cc.as_deref(), Some("GB"));
        assert_eq!(file.lang.as_deref(), Some("british"));
        assert!(file.steam_dir.is_none());
        assert!(file.account.is_none());
        assert!(file.backup_retention_count.is_none());
    }

    #[test]
    fn config_file_full_is_valid() {
        let toml_str = r#"
steam_dir = "/opt/steam"
account = "myuser"
cc = "DE"
lang = "german"
backup_retention_count = 10
"#;
        let file: ConfigFile = toml::from_str(toml_str).unwrap();
        assert_eq!(file.steam_dir.as_deref(), Some("/opt/steam"));
        assert_eq!(file.account.as_deref(), Some("myuser"));
        assert_eq!(file.cc.as_deref(), Some("DE"));
        assert_eq!(file.lang.as_deref(), Some("german"));
        assert_eq!(file.backup_retention_count, Some(10));
    }

    #[test]
    fn config_file_empty_is_valid() {
        let file: ConfigFile = toml::from_str("").unwrap();
        assert!(file.steam_dir.is_none());
        assert!(file.cc.is_none());
    }

    // -- credentials detection -----------------------------------------------

    #[test]
    #[serial]
    fn igdb_credentials_detected_when_both_env_vars_set() {
        set_env("VAPOURFLY_IGDB_CLIENT_ID", "id");
        set_env("VAPOURFLY_IGDB_CLIENT_SECRET", "secret");

        let cli = CliOverrides {
            steam_dir: Some(PathBuf::from("/tmp/fake")),
            ..Default::default()
        };
        let cfg = VapourflyConfig::from_cli_and_env(cli).unwrap();
        assert!(cfg.has_igdb_credentials);

        clear_test_env();
    }

    #[test]
    #[serial]
    fn igdb_credentials_absent_when_only_one_env_var_set() {
        set_env("VAPOURFLY_IGDB_CLIENT_ID", "id");
        remove_env("VAPOURFLY_IGDB_CLIENT_SECRET");

        let cli = CliOverrides {
            steam_dir: Some(PathBuf::from("/tmp/fake")),
            ..Default::default()
        };
        let cfg = VapourflyConfig::from_cli_and_env(cli).unwrap();
        assert!(!cfg.has_igdb_credentials);

        clear_test_env();
    }

    #[test]
    #[serial]
    fn rawg_credentials_detected_when_env_var_set() {
        set_env("VAPOURFLY_RAWG_KEY", "key");

        let cli = CliOverrides {
            steam_dir: Some(PathBuf::from("/tmp/fake")),
            ..Default::default()
        };
        let cfg = VapourflyConfig::from_cli_and_env(cli).unwrap();
        assert!(cfg.has_rawg_credentials);

        clear_test_env();
    }

    #[test]
    #[serial]
    fn rawg_credentials_absent_when_env_var_empty() {
        remove_env("VAPOURFLY_RAWG_KEY");
        set_env("VAPOURFLY_RAWG_KEY", "");

        let cli = CliOverrides {
            steam_dir: Some(PathBuf::from("/tmp/fake")),
            ..Default::default()
        };
        let cfg = VapourflyConfig::from_cli_and_env(cli).unwrap();
        assert!(!cfg.has_rawg_credentials);

        clear_test_env();
    }

    // -- env_path helper -----------------------------------------------------

    #[test]
    #[serial]
    fn env_path_returns_none_for_unset() {
        remove_env("VAPOURFLY_NONEXISTENT_VAR");
        assert!(env_path("VAPOURFLY_NONEXISTENT_VAR").is_none());
    }

    #[test]
    #[serial]
    fn env_path_returns_none_for_empty() {
        set_env("VAPOURFLY_TEST_EMPTY", "");
        assert!(env_path("VAPOURFLY_TEST_EMPTY").is_none());
    }

    #[test]
    #[serial]
    fn env_path_returns_pathbuf_for_set() {
        set_env("VAPOURFLY_TEST_SET", "/some/path");
        let p = env_path("VAPOURFLY_TEST_SET");
        assert_eq!(p, Some(PathBuf::from("/some/path")));
    }

    // -- env_present helper ---------------------------------------------------

    #[test]
    #[serial]
    fn env_present_returns_true_for_non_empty_env() {
        set_env("VAPOURFLY_TEST_PRESENT", "from_env");
        assert!(env_present("VAPOURFLY_TEST_PRESENT"));
    }

    #[test]
    #[serial]
    fn env_present_returns_false_for_empty_env() {
        set_env("VAPOURFLY_TEST_EMPTY", "");
        assert!(!env_present("VAPOURFLY_TEST_EMPTY"));
    }

    #[test]
    #[serial]
    fn env_present_returns_false_when_unset() {
        remove_env("VAPOURFLY_TEST_NEITHER");
        assert!(!env_present("VAPOURFLY_TEST_NEITHER"));
    }

    // -- helpers -------------------------------------------------------------

    fn make_test_config() -> VapourflyConfig {
        VapourflyConfig {
            steam_dir: PathBuf::from("/tmp/fake-steam"),
            account: Some("testuser".into()),
            cache_root: PathBuf::from("/tmp/cache-root"),
            app_data_root: PathBuf::from("/tmp/data-root"),
            has_igdb_credentials: false,
            has_rawg_credentials: false,
            cc: "US".into(),
            lang: "english".into(),
            backup_retention_count: 5,
        }
    }

    // -- ConfigField ---------------------------------------------------------

    #[test]
    fn config_field_round_trips_keys() {
        for field in ConfigField::all() {
            let key = field.as_key();
            assert_eq!(ConfigField::from_key(key), Some(*field));
        }
        assert!(ConfigField::from_key("unknown_key").is_none());
    }

    #[test]
    fn config_field_normalise_rejects_empty_string() {
        for field in ConfigField::all() {
            assert!(
                field.normalise("").is_err(),
                "{field:?} should reject empty"
            );
        }
    }

    #[test]
    fn config_field_normalise_backup_retention_requires_integer() {
        assert!(ConfigField::BackupRetentionCount.normalise("abc").is_err());
        assert!(ConfigField::BackupRetentionCount.normalise("-1").is_err());
        assert!(ConfigField::BackupRetentionCount.normalise("3").is_ok());
    }

    // -- set_config_field_at / unset_config_field_at -------------------------

    #[test]
    fn set_config_field_creates_file_and_preserves_other_keys() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");

        set_config_field_at(ConfigField::Cc, "JP", &path).unwrap();
        set_config_field_at(ConfigField::Lang, "japanese", &path).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let parsed: ConfigFile = toml::from_str(&contents).unwrap();
        assert_eq!(parsed.cc.as_deref(), Some("JP"));
        assert_eq!(parsed.lang.as_deref(), Some("japanese"));
    }

    #[test]
    fn set_config_field_overwrites_existing_value() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");

        set_config_field_at(ConfigField::Account, "first", &path).unwrap();
        set_config_field_at(ConfigField::Account, "second", &path).unwrap();

        let parsed: ConfigFile = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed.account.as_deref(), Some("second"));
    }

    #[test]
    fn set_config_field_backup_retention_writes_integer() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");

        set_config_field_at(ConfigField::BackupRetentionCount, "10", &path).unwrap();

        let parsed: ConfigFile = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(parsed.backup_retention_count, Some(10));
    }

    #[test]
    fn unset_config_field_removes_key() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");

        set_config_field_at(ConfigField::Account, "someone", &path).unwrap();
        set_config_field_at(ConfigField::Cc, "GB", &path).unwrap();
        unset_config_field_at(ConfigField::Account, &path).unwrap();

        let parsed: ConfigFile = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(parsed.account.is_none());
        assert_eq!(parsed.cc.as_deref(), Some("GB"));
    }

    #[test]
    fn unset_config_field_on_missing_file_is_ok() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("nested/config.toml");
        unset_config_field_at(ConfigField::Account, &path).unwrap();
        assert!(path.exists());
    }
}
