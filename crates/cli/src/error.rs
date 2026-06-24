//! CLI error presentation.
//!
//! Catches [`VapourflyError`] variants and turns them into user-friendly
//! anyhow messages with optional remediation hints. The [`Verbosity`] wrapper
//! controls whether full paths are shown (verbose mode) or redacted (default).

use anyhow::Result;
use std::process::ExitCode;
use vapourfly_core::error::VapourflyError;

/// Controls the verbosity of error output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub enum Verbosity {
    /// Redact absolute paths; show remediation hints.
    Normal,
    /// Show full paths and all context.
    Verbose,
}

/// Format a [`VapourflyError`] into an anyhow error with user-friendly text.
#[allow(dead_code)]
pub fn format_error(err: VapourflyError, verbosity: Verbosity) -> anyhow::Error {
    let message = match verbosity {
        Verbosity::Normal => err.to_string(),
        Verbosity::Verbose => err.verbose(),
    };

    match err.remediation_hint() {
        Some(hint) => anyhow::anyhow!("{message}\n  hint: {hint}"),
        None => anyhow::anyhow!(message),
    }
}

/// Convenience: map an `Option<T>` into a [`VapourflyError::FileNotFound`]
/// when the value is `None`.
#[allow(dead_code)]
pub fn require_file<T>(value: Option<T>, path: std::path::PathBuf) -> Result<T> {
    value.ok_or_else(|| {
        format_error(
            VapourflyError::FileNotFound {
                path: vapourfly_core::SafePath::new(path),
            },
            Verbosity::Normal,
        )
    })
}

/// Print a top-level error to stderr and return a suitable exit code.
#[allow(dead_code)]
pub fn exit_with_error(err: anyhow::Error, verbosity: Verbosity) -> ExitCode {
    eprintln!("error: {err}");

    if verbosity == Verbosity::Verbose {
        for cause in err.chain().skip(1) {
            eprintln!("  Caused by: {cause}");
        }
    }

    ExitCode::FAILURE
}

#[cfg(test)]
mod tests {
    use super::*;
    use vapourfly_core::SafePath;

    #[test]
    fn normal_mode_does_not_leak_paths() {
        let err = VapourflyError::FileNotFound {
            path: SafePath::new("/Users/david/.steam/config.vdf"),
        };
        let formatted = format_error(err, Verbosity::Normal).to_string();
        assert!(!formatted.contains("/Users/david"));
        assert!(formatted.contains("config.vdf"));
    }

    #[test]
    fn verbose_mode_includes_full_paths() {
        let err = VapourflyError::FileNotFound {
            path: SafePath::new("/Users/david/.steam/config.vdf"),
        };
        let formatted = format_error(err, Verbosity::Verbose).to_string();
        assert!(formatted.contains("/Users/david/.steam/config.vdf"));
    }

    #[test]
    fn remediation_hint_is_attached_as_context() {
        let err = VapourflyError::CredentialsMissing {
            provider: "Steam".into(),
        };
        let formatted = format_error(err, Verbosity::Normal).to_string();
        assert!(
            formatted.contains("vapourfly auth login"),
            "expected remediation hint in error message, got: {formatted}"
        );
    }

    #[test]
    fn rate_limited_message_includes_retry_hint() {
        let err = VapourflyError::RateLimited {
            provider: "Steam Store".into(),
            retry_after_secs: 30,
        };
        let formatted = format_error(err, Verbosity::Normal).to_string();
        assert!(formatted.contains("retry after 30s"));
    }

    #[test]
    fn require_file_returns_error_on_none() {
        let result = require_file::<()>(None, std::path::PathBuf::from("/tmp/missing.vdf"));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("missing.vdf"));
        assert!(!msg.contains("/tmp"));
    }

    #[test]
    fn require_file_returns_value_on_some() {
        let result = require_file(Some(42), std::path::PathBuf::from("/tmp/x"));
        assert_eq!(result.unwrap(), 42);
    }
}
