//! Output formatting helpers for the CLI.
//!
//! Thin presentation wrappers over [`vapourfly_core::display`] so domain
//! wording stays shared with the GUI.

use vapourfly_core::display;
use vapourfly_core::models::{JunkMode, JunkSignal};

/// Mask a Steam ID, showing only the last 4 characters.
pub fn mask_id(id: &str) -> String {
    display::mask_id(id)
}

/// Truncate a string to `max_len` display characters, adding an ellipsis if truncated.
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{truncated}\u{2026}")
    }
}

/// Format a [`JunkSignal`] into a human-readable reason string.
pub fn format_junk_signal(signal: &JunkSignal) -> String {
    display::format_junk_signal(signal)
}

/// Format a [`JunkMode`] into a display string.
pub fn format_junk_mode(mode: &JunkMode) -> &'static str {
    display::format_junk_mode(mode)
}
