//! Shared human-readable formatting for domain values.
//!
//! Presentation helpers used by CLI and GUI so wording stays consistent.

use crate::models::{JunkMode, JunkSignal};

/// Mask a Steam ID, showing only the last 4 characters.
pub fn mask_id(id: &str) -> String {
    if id.len() <= 4 {
        "***".to_string()
    } else {
        format!("***{}", &id[id.len() - 4..])
    }
}

/// Format a [`JunkSignal`] into a human-readable reason string.
pub fn format_junk_signal(signal: &JunkSignal) -> String {
    match signal {
        JunkSignal::LowPlaytime { minutes } => format!("low playtime ({minutes}m)"),
        JunkSignal::ShortCompletion { seconds, .. } => {
            let h = *seconds as f32 / 3600.0;
            format!("short story ({h:.1}h)")
        }
        JunkSignal::LowRating { rating_0_5, .. } => {
            format!("low rating ({rating_0_5:.1})")
        }
    }
}

/// Format a [`JunkMode`] into a display string.
pub fn format_junk_mode(mode: &JunkMode) -> &'static str {
    match mode {
        JunkMode::Default => "default",
        JunkMode::Strict => "strict",
        JunkMode::Aggressive => "aggressive",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{HltbSource, JunkSignal, RatingSource};

    #[test]
    fn mask_id_keeps_last_four() {
        assert_eq!(mask_id("76561198000000000"), "***0000");
        assert_eq!(mask_id("ab"), "***");
    }

    #[test]
    fn junk_signal_formatting() {
        let s = format_junk_signal(&JunkSignal::LowPlaytime { minutes: 10 });
        assert!(s.contains("10"));
        let s = format_junk_signal(&JunkSignal::ShortCompletion {
            seconds: 3600,
            source: HltbSource::HltbScrape,
        });
        assert!(s.contains("1.0"));
        let s = format_junk_signal(&JunkSignal::LowRating {
            rating_0_5: 1.5,
            source: RatingSource::Rawg,
        });
        assert!(s.contains("1.5"));
    }
}
