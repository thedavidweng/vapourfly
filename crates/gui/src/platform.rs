//! Where the GUI is running: plain desktop, SteamOS desktop mode, or the
//! Steam Deck / SteamOS Game Mode session (gamescope).
//!
//! Detection reads only environment variables and `/etc/os-release`, so it
//! is cheap and side-effect free. Steam exports `SteamDeck=1` on Deck
//! hardware, `SteamOS=1` on SteamOS, and `SteamGamepadUI=1` for anything it
//! launches from the gamepad UI; gamescope exports `GAMESCOPE_WAYLAND_DISPLAY`.

/// The session the GUI runs in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceProfile {
    /// Running on Steam Deck hardware.
    pub steam_deck: bool,
    /// Running on SteamOS (Deck or another SteamOS device).
    pub steamos: bool,
    /// Running inside the Game Mode / Big Picture session: fullscreen,
    /// controller-first, Steam always running.
    pub game_mode: bool,
}

impl DeviceProfile {
    /// Detect from the real process environment.
    pub fn detect() -> Self {
        let os_release = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        Self::from_env(|k| std::env::var(k).ok(), &os_release)
    }

    /// Detect from an environment lookup and `/etc/os-release` contents.
    pub fn from_env(env: impl Fn(&str) -> Option<String>, os_release: &str) -> Self {
        let flag = |k: &str| env(k).is_some_and(|v| v.trim() == "1");
        let steamos_release = os_release.lines().any(|line| {
            let line = line.trim();
            line == "ID=steamos" || line == "ID=\"steamos\""
        });
        let steam_deck = flag("SteamDeck");
        let steamos = steam_deck || flag("SteamOS") || steamos_release;
        let game_mode = flag("SteamGamepadUI")
            || env("GAMESCOPE_WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
        Self {
            steam_deck,
            steamos,
            game_mode,
        }
    }

    /// The Deck preset used by `--deck`: a 1280×800 Game Mode session.
    pub fn simulated_deck() -> Self {
        Self {
            steam_deck: true,
            steamos: true,
            game_mode: true,
        }
    }

    /// Steam's on-screen keyboard can be summoned with `steam://open/keyboard`.
    pub fn has_steam_keyboard(self) -> bool {
        self.steamos || self.game_mode
    }

    /// Short label for Settings and diagnostics.
    pub fn label(self) -> &'static str {
        match (self.steam_deck, self.steamos, self.game_mode) {
            (true, _, true) => "Steam Deck · Game Mode",
            (true, _, false) => "Steam Deck · Desktop Mode",
            (false, true, true) => "SteamOS · Game Mode",
            (false, true, false) => "SteamOS · Desktop Mode",
            (false, false, true) => "Steam Big Picture",
            (false, false, false) => "Desktop",
        }
    }
}

/// Native Steam Deck panel size in logical pixels.
pub const DECK_SCREEN: (f32, f32) = (1280., 800.);

const PREFERRED_WINDOW: (f32, f32) = (1440., 920.);
const MIN_WINDOW: (f32, f32) = (1024., 700.);

/// Opening window size and minimum size for a display of `display` logical
/// pixels. Both shrink to fit small or highly scaled screens (a Deck in
/// Desktop Mode at 125% is 1024×640) so the window never opens larger than
/// the screen or refuses to fit it.
pub fn window_geometry(display: Option<(f32, f32)>, simulate_deck: bool) -> WindowGeometry {
    if simulate_deck {
        return WindowGeometry {
            size: DECK_SCREEN,
            min: (DECK_SCREEN.0.min(MIN_WINDOW.0), 640.),
        };
    }
    let Some((dw, dh)) = display.filter(|(w, h)| *w > 0. && *h > 0.) else {
        return WindowGeometry {
            size: PREFERRED_WINDOW,
            min: MIN_WINDOW,
        };
    };
    // Leave room for panels and docks on desktop sessions.
    let size = (
        PREFERRED_WINDOW.0.min((dw * 0.94).floor()),
        PREFERRED_WINDOW.1.min((dh * 0.9).floor()),
    );
    WindowGeometry {
        size,
        min: (MIN_WINDOW.0.min(size.0), MIN_WINDOW.1.min(size.1)),
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowGeometry {
    pub size: (f32, f32),
    pub min: (f32, f32),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| {
            pairs
                .iter()
                .find(|(key, _)| *key == k)
                .map(|(_, v)| (*v).to_string())
        }
    }

    #[test]
    fn plain_desktop_detects_nothing() {
        let p = DeviceProfile::from_env(env(&[]), "ID=ubuntu\n");
        assert_eq!(p, DeviceProfile::default());
        assert_eq!(p.label(), "Desktop");
        assert!(!p.has_steam_keyboard());
    }

    #[test]
    fn deck_game_mode_from_steam_env() {
        let p = DeviceProfile::from_env(
            env(&[
                ("SteamDeck", "1"),
                ("GAMESCOPE_WAYLAND_DISPLAY", "gamescope-0"),
            ]),
            "",
        );
        assert!(p.steam_deck && p.steamos && p.game_mode);
        assert_eq!(p.label(), "Steam Deck · Game Mode");
    }

    #[test]
    fn steamos_desktop_from_os_release() {
        let p = DeviceProfile::from_env(env(&[]), "NAME=\"SteamOS\"\nID=steamos\n");
        assert!(p.steamos && !p.steam_deck && !p.game_mode);
        assert!(p.has_steam_keyboard());
        let quoted = DeviceProfile::from_env(env(&[]), "ID=\"steamos\"");
        assert!(quoted.steamos);
    }

    #[test]
    fn window_fits_small_and_scaled_screens() {
        let big = window_geometry(Some((2560., 1440.)), false);
        assert_eq!(big.size, (1440., 920.));
        assert_eq!(big.min, (1024., 700.));

        let deck = window_geometry(Some(DECK_SCREEN), false);
        assert!(deck.size.0 <= 1280. && deck.size.1 <= 800.);
        assert!(deck.min.0 <= deck.size.0 && deck.min.1 <= deck.size.1);

        let scaled = window_geometry(Some((1024., 640.)), false);
        assert!(scaled.size.1 <= 640. && scaled.min.1 <= scaled.size.1);

        assert_eq!(window_geometry(None, false).size, (1440., 920.));
        assert_eq!(window_geometry(None, true).size, DECK_SCREEN);
    }

    #[test]
    fn flags_must_be_one() {
        let p = DeviceProfile::from_env(
            env(&[("SteamDeck", "0"), ("SteamGamepadUI", "0")]),
            "ID_LIKE=steamos",
        );
        assert_eq!(p, DeviceProfile::default());
        let big_picture = DeviceProfile::from_env(env(&[("SteamGamepadUI", "1")]), "");
        assert!(big_picture.game_mode && !big_picture.steamos);
    }
}
