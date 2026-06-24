//! Error types for the Vapourfly core library.
//!
//! All variants carry structured context. The [`Display`] implementation
//! produces **safe** messages that redact absolute file paths (showing only
//! the file name) so that logs and CLI output don't leak filesystem layout
//! by default. Call [`VapourflyError::verbose`] when you need the full
//! detail — for example in `--verbose` / debug output.

use std::fmt;
use std::path::{Path, PathBuf};
use thiserror::Error;

// ---------------------------------------------------------------------------
// Path wrapper that only reveals the file name in Display
// ---------------------------------------------------------------------------

/// A [`PathBuf`] wrapper whose [`Display`] impl only shows the file name,
/// not the full path. Use [`SafePath::full`] to recover the original path.
#[derive(Debug, Clone)]
pub struct SafePath(PathBuf);

impl SafePath {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self(path.into())
    }

    /// Return the underlying, full path.
    pub fn full(&self) -> &Path {
        &self.0
    }

    /// Write just the file-name portion (or the whole path if there is no
    /// file-name component) to the given formatter.
    fn fmt_display(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0.file_name().and_then(|n| n.to_str()) {
            Some(name) => f.write_str(name),
            None => write!(f, "{}", self.0.display()),
        }
    }
}

impl fmt::Display for SafePath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.fmt_display(f)
    }
}

impl From<PathBuf> for SafePath {
    fn from(p: PathBuf) -> Self {
        Self(p)
    }
}

// ---------------------------------------------------------------------------
// Core error enum
// ---------------------------------------------------------------------------

#[derive(Error, Debug)]
pub enum VapourflyError {
    #[error("file not found: {path}")]
    FileNotFound { path: SafePath },

    #[error("failed to parse {format} file {path}: {reason}")]
    ParseError {
        path: SafePath,
        format: String,
        reason: String,
    },

    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),

    #[error("ambiguous account: {count} accounts found, use --account to select")]
    AmbiguousAccount { count: usize },

    #[error("unsafe write: {reason}")]
    UnsafeWrite { reason: String },

    #[error("network unavailable: {source}")]
    NetworkUnavailable {
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    #[error("credentials missing for {provider}")]
    CredentialsMissing { provider: String },

    #[error("rate limited by {provider}, retry after {retry_after_secs}s")]
    RateLimited {
        provider: String,
        retry_after_secs: u64,
    },

    #[error("using stale cache for {provider}: {reason}")]
    StaleCache { provider: String, reason: String },

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl VapourflyError {
    /// Return a verbose, human-readable string that includes full paths and
    /// all available context. Intended for `--verbose` / debug output.
    pub fn verbose(&self) -> String {
        match self {
            Self::FileNotFound { path } => {
                format!("file not found: {}", path.full().display())
            }
            Self::ParseError {
                path,
                format,
                reason,
            } => {
                format!(
                    "failed to parse {} file {}: {}",
                    format,
                    path.full().display(),
                    reason
                )
            }
            // Variants without paths use the same Display impl.
            other => format!("{other}"),
        }
    }

    /// Return a user-friendly remediation hint, if one is available.
    pub fn remediation_hint(&self) -> Option<&'static str> {
        match self {
            Self::FileNotFound { .. } => Some("check that the file exists and the path is correct"),
            Self::ParseError { .. } => Some("the file may be corrupted; try re-downloading it"),
            Self::UnsupportedFormat(_) => Some("run `vapourfly --help` to see supported formats"),
            Self::AmbiguousAccount { .. } => Some("pass --account <name> to disambiguate"),
            Self::UnsafeWrite { .. } => Some("use --force to override (data loss may occur)"),
            Self::NetworkUnavailable { .. } => Some("check your internet connection and try again"),
            Self::CredentialsMissing { .. } => {
                Some("run `vapourfly auth login` to set up credentials")
            }
            Self::RateLimited { .. } => Some("wait a moment, then try again"),
            Self::StaleCache { .. } => Some("run `vapourfly refresh` to update the cache"),
            Self::InvalidInput(_) => None,
            Self::Internal(_) => Some("this is a bug — please report it"),
        }
    }
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, VapourflyError>;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- safe-by-default display tests ---------------------------------------

    #[test]
    fn file_not_found_does_not_leak_full_path() {
        let err = VapourflyError::FileNotFound {
            path: SafePath::new("/Users/alice/.steam/config/config.vdf"),
        };
        let msg = err.to_string();
        assert!(msg.contains("config.vdf"), "should contain the file name");
        assert!(
            !msg.contains("/Users/alice"),
            "should NOT contain the full directory path"
        );
    }

    #[test]
    fn parse_error_does_not_leak_full_path() {
        let err = VapourflyError::ParseError {
            path: SafePath::new("/home/bob/.local/share/Steam/userdata/12345/localconfig.vdf"),
            format: "VDF".into(),
            reason: "unexpected token at line 42".into(),
        };
        let msg = err.to_string();
        assert!(msg.contains("localconfig.vdf"));
        assert!(!msg.contains("/home/bob"));
    }

    // -- verbose mode includes full paths ------------------------------------

    #[test]
    fn verbose_file_not_found_includes_full_path() {
        let err = VapourflyError::FileNotFound {
            path: SafePath::new("/Users/alice/.steam/config/config.vdf"),
        };
        let msg = err.verbose();
        assert!(
            msg.contains("/Users/alice/.steam/config/config.vdf"),
            "verbose message should contain the full path"
        );
    }

    #[test]
    fn verbose_parse_error_includes_full_path() {
        let err = VapourflyError::ParseError {
            path: SafePath::new("/home/bob/.local/share/Steam/userdata/12345/localconfig.vdf"),
            format: "VDF".into(),
            reason: "unexpected token".into(),
        };
        let msg = err.verbose();
        assert!(msg.contains("/home/bob/.local/share/Steam/userdata/12345/localconfig.vdf"));
    }

    // -- variants without paths are identical in both modes ------------------

    #[test]
    fn non_path_variants_match_in_display_and_verbose() {
        let variants = [
            VapourflyError::UnsupportedFormat("XML".into()),
            VapourflyError::AmbiguousAccount { count: 3 },
            VapourflyError::UnsafeWrite {
                reason: "target is a symlink".into(),
            },
            VapourflyError::CredentialsMissing {
                provider: "Steam".into(),
            },
            VapourflyError::RateLimited {
                provider: "Steam Store".into(),
                retry_after_secs: 60,
            },
            VapourflyError::StaleCache {
                provider: "Steam API".into(),
                reason: "older than 24h".into(),
            },
            VapourflyError::InvalidInput("empty username".into()),
            VapourflyError::Internal("unreachable".into()),
        ];

        for err in &variants {
            assert_eq!(
                err.to_string(),
                err.verbose(),
                "non-path variant should be identical in Display and verbose: {err:?}"
            );
        }
    }

    // -- remediation hints ---------------------------------------------------

    #[test]
    fn remediation_hints_exist_for_expected_variants() {
        let err = VapourflyError::FileNotFound {
            path: SafePath::new("x"),
        };
        assert!(err.remediation_hint().is_some());

        let err = VapourflyError::ParseError {
            path: SafePath::new("x"),
            format: "VDF".into(),
            reason: "bad".into(),
        };
        assert!(err.remediation_hint().is_some());

        let err = VapourflyError::UnsupportedFormat("XML".into());
        assert!(err.remediation_hint().is_some());

        let err = VapourflyError::AmbiguousAccount { count: 2 };
        assert!(err.remediation_hint().is_some());

        let err = VapourflyError::UnsafeWrite {
            reason: "symlink".into(),
        };
        assert!(err.remediation_hint().is_some());

        let err = VapourflyError::NetworkUnavailable {
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::ConnectionRefused,
                "connection refused",
            )),
        };
        assert!(err.remediation_hint().is_some());

        let err = VapourflyError::CredentialsMissing {
            provider: "Steam".into(),
        };
        assert!(err.remediation_hint().is_some());

        let err = VapourflyError::RateLimited {
            provider: "API".into(),
            retry_after_secs: 10,
        };
        assert!(err.remediation_hint().is_some());

        let err = VapourflyError::StaleCache {
            provider: "API".into(),
            reason: "old".into(),
        };
        assert!(err.remediation_hint().is_some());
    }

    // -- SafePath unit tests -------------------------------------------------

    #[test]
    fn safe_path_shows_only_filename() {
        let p = SafePath::new("/a/b/c/game.json");
        assert_eq!(p.to_string(), "game.json");
    }

    #[test]
    fn safe_path_with_no_parent_shows_entire_value() {
        let p = SafePath::new("game.json");
        assert_eq!(p.to_string(), "game.json");
    }

    #[test]
    fn safe_path_full_returns_original() {
        let p = SafePath::new("/a/b/c/game.json");
        assert_eq!(p.full(), Path::new("/a/b/c/game.json"));
    }
}
