//! Light + dark design tokens (ADR-0006).
//!
//! Product screens ship a warm light canvas with orchid accents. The earlier
//! desktop shell used a cool dark surface hierarchy with a violet brand
//! accent. Both palettes are kept and switched at runtime so the app can
//! match the reference light UI without abandoning the dark design system.
//!
//! Theme preference persists across launches via eframe application storage
//! (not domain config) — see [`ThemeMode::as_u8`] / [`ThemeMode::from_u8`].

use std::sync::atomic::{AtomicU8, Ordering};

use eframe::egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Light,
    Dark,
}

impl ThemeMode {
    pub fn toggle(self) -> Self {
        match self {
            Self::Light => Self::Dark,
            Self::Dark => Self::Light,
        }
    }

    pub fn is_dark(self) -> bool {
        matches!(self, Self::Dark)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
        }
    }

    /// Serialize for eframe storage persistence.
    pub fn as_u8(self) -> u8 {
        match self {
            Self::Light => 0,
            Self::Dark => 1,
        }
    }

    /// Deserialize from eframe storage. Unknown values default to Light.
    pub fn from_u8(v: u8) -> Self {
        if v == 1 { Self::Dark } else { Self::Light }
    }
}

#[derive(Clone, Copy)]
pub struct Tokens {
    pub canvas: Color32,
    pub surface: Color32,
    pub surface_raised: Color32,
    pub surface_muted: Color32,
    pub surface_sunken: Color32,
    pub border: Color32,
    pub border_soft: Color32,
    pub text_primary: Color32,
    pub text_secondary: Color32,
    pub text_muted: Color32,
    pub text_inverse: Color32,
    pub accent: Color32,
    pub accent_soft: Color32,
    pub accent_text: Color32,
    pub success: Color32,
    pub success_soft: Color32,
    pub error: Color32,
    pub error_soft: Color32,
    pub warning: Color32,
    pub warning_soft: Color32,
}

impl Tokens {
    pub const LIGHT: Self = Self {
        canvas: Color32::from_rgb(250, 250, 251),
        surface: Color32::from_rgb(255, 255, 255),
        surface_raised: Color32::from_rgb(255, 255, 255),
        surface_muted: Color32::from_rgb(247, 247, 249),
        surface_sunken: Color32::from_rgb(243, 243, 246),
        border: Color32::from_rgb(224, 223, 228),
        border_soft: Color32::from_rgb(235, 234, 238),
        text_primary: Color32::from_rgb(24, 22, 29),
        text_secondary: Color32::from_rgb(92, 88, 101),
        text_muted: Color32::from_rgb(137, 132, 145),
        text_inverse: Color32::from_rgb(255, 255, 255),
        accent: Color32::from_rgb(139, 48, 153),
        accent_soft: Color32::from_rgb(248, 237, 250),
        accent_text: Color32::from_rgb(122, 36, 137),
        success: Color32::from_rgb(46, 147, 75),
        success_soft: Color32::from_rgb(234, 247, 236),
        error: Color32::from_rgb(210, 70, 76),
        error_soft: Color32::from_rgb(253, 238, 239),
        warning: Color32::from_rgb(185, 112, 20),
        warning_soft: Color32::from_rgb(255, 246, 229),
    };

    // Restored from the dark design-system shell (ADR-0006).
    pub const DARK: Self = Self {
        canvas: Color32::from_rgb(14, 16, 22),
        surface: Color32::from_rgb(18, 20, 26),
        surface_raised: Color32::from_rgb(26, 29, 36),
        surface_muted: Color32::from_rgb(34, 38, 48),
        surface_sunken: Color32::from_rgb(14, 16, 22),
        border: Color32::from_rgb(52, 58, 70),
        border_soft: Color32::from_rgb(42, 46, 56),
        text_primary: Color32::from_rgb(236, 238, 244),
        text_secondary: Color32::from_rgb(158, 166, 180),
        text_muted: Color32::from_rgb(110, 118, 132),
        text_inverse: Color32::from_rgb(18, 20, 26),
        accent: Color32::from_rgb(156, 110, 220),
        accent_soft: Color32::from_rgb(48, 36, 72),
        accent_text: Color32::from_rgb(196, 168, 255),
        success: Color32::from_rgb(72, 180, 120),
        success_soft: Color32::from_rgb(28, 52, 40),
        error: Color32::from_rgb(220, 90, 90),
        error_soft: Color32::from_rgb(56, 28, 28),
        warning: Color32::from_rgb(220, 170, 70),
        warning_soft: Color32::from_rgb(56, 44, 24),
    };
}

// Tag tints: reference cards use per-tag pastel tints, not a single accent hue.

/// (background, foreground) pairs: pink, blue, green, orange, violet, teal.
const TAG_TINTS_LIGHT: [(Color32, Color32); 6] = [
    (
        Color32::from_rgb(252, 233, 242),
        Color32::from_rgb(173, 35, 106),
    ),
    (
        Color32::from_rgb(230, 240, 253),
        Color32::from_rgb(31, 90, 173),
    ),
    (
        Color32::from_rgb(230, 246, 234),
        Color32::from_rgb(23, 128, 63),
    ),
    (
        Color32::from_rgb(255, 242, 227),
        Color32::from_rgb(178, 94, 13),
    ),
    (
        Color32::from_rgb(241, 233, 252),
        Color32::from_rgb(112, 51, 182),
    ),
    (
        Color32::from_rgb(226, 246, 245),
        Color32::from_rgb(13, 131, 120),
    ),
];

const TAG_TINTS_DARK: [(Color32, Color32); 6] = [
    (
        Color32::from_rgb(58, 32, 46),
        Color32::from_rgb(240, 150, 195),
    ),
    (
        Color32::from_rgb(30, 42, 62),
        Color32::from_rgb(140, 180, 240),
    ),
    (
        Color32::from_rgb(30, 50, 38),
        Color32::from_rgb(130, 210, 155),
    ),
    (
        Color32::from_rgb(58, 44, 26),
        Color32::from_rgb(235, 180, 110),
    ),
    (
        Color32::from_rgb(44, 34, 62),
        Color32::from_rgb(185, 155, 240),
    ),
    (
        Color32::from_rgb(26, 50, 48),
        Color32::from_rgb(120, 215, 205),
    ),
];

/// Deterministic (background, foreground) tint for a tag/genre name.
/// Same name → same hue, across views and sessions.
pub fn tag_tint(name: &str) -> (Color32, Color32) {
    let tints = if ThemeMode::from_u8(ACTIVE_THEME.load(Ordering::Relaxed)).is_dark() {
        &TAG_TINTS_DARK
    } else {
        &TAG_TINTS_LIGHT
    };
    let mut hash: u32 = 5381;
    for b in name.bytes() {
        hash = hash.wrapping_mul(33) ^ u32::from(b.to_ascii_lowercase());
    }
    tints[(hash as usize) % tints.len()]
}

/// Fixed tint by palette index (pink, blue, green, orange, violet, teal) —
/// for editorial chips whose colors are part of the design, not hashed.
pub fn tint(index: usize) -> (Color32, Color32) {
    let tints = if ThemeMode::from_u8(ACTIVE_THEME.load(Ordering::Relaxed)).is_dark() {
        &TAG_TINTS_DARK
    } else {
        &TAG_TINTS_LIGHT
    };
    tints[index % tints.len()]
}

/// Active theme for free functions that paint chrome/cards outside App methods.
static ACTIVE_THEME: AtomicU8 = AtomicU8::new(0);

pub fn set_active_theme(mode: ThemeMode) {
    ACTIVE_THEME.store(mode.as_u8(), Ordering::Relaxed);
}

#[inline]
pub fn t() -> Tokens {
    match ThemeMode::from_u8(ACTIVE_THEME.load(Ordering::Relaxed)) {
        ThemeMode::Dark => Tokens::DARK,
        ThemeMode::Light => Tokens::LIGHT,
    }
}

// Type scale: a slightly larger display step preserves the generous hierarchy
// in the reference screens while regular controls remain compact.

pub const TS_XS: f32 = 11.0;
pub const TS_SM: f32 = 12.0;
pub const TS_BODY: f32 = 13.5;
pub const TS_MD: f32 = 15.0;
pub const TS_LG: f32 = 18.0;
pub const TS_XL: f32 = 23.0;
pub const TS_2XL: f32 = 27.0;

// Spacing scale on a 4px grid.
pub const SP_1: f32 = 4.0;
pub const SP_2: f32 = 8.0;
pub const SP_3: f32 = 12.0;
pub const SP_4: f32 = 16.0;
pub const SP_6: f32 = 24.0;

pub const TOPBAR_HEIGHT: f32 = 58.0;
pub const SIDEBAR_WIDTH: f32 = 132.0;
/// Compact (icon-only) sidebar width, used at 1024–1179px window width.
pub const SIDEBAR_WIDTH_COMPACT: f32 = 76.0;

/// Below this width the sidebar shrinks to icon-only (76px).
pub const BP_COMPACT_SIDEBAR: f32 = 1180.0;
/// Below this width the central panel padding shrinks from 24px to 16px and
/// insight rails move below the main content.
pub const BP_COMPACT_PADDING: f32 = 1280.0;
/// At or above this width the full two-column layout is used.
pub const BP_DESKTOP: f32 = 1280.0;

/// True when the sidebar should be compact (icon-only, 76px).
/// Active at 1024–1179px.
pub fn is_compact_sidebar(width: f32) -> bool {
    (1024.0..BP_COMPACT_SIDEBAR).contains(&width)
}

/// True when the central panel should use compact (16px) padding.
/// Active below 1280px.
pub fn is_compact_padding(width: f32) -> bool {
    width < BP_COMPACT_PADDING
}

/// True when insight rails should move below the main content (single-column).
/// Active at 1024–1279px.
pub fn rails_below(width: f32) -> bool {
    (1024.0..BP_DESKTOP).contains(&width)
}
pub const CORNER_SM: f32 = 6.0;
pub const CORNER_MD: f32 = 10.0;
pub const CORNER_LG: f32 = 14.0;
pub const CORNER_PILL: f32 = 20.0;

pub const RECOMMEND_CARD_IMG_W: f32 = 220.0;
pub const RECOMMEND_CARD_IMG_H: f32 = 124.0;

pub const POSTER_H: f32 = 142.0;
/// Minimum library card width; the grid stretches cards to fill the row.
pub const GAME_CARD_W: f32 = 206.0;

/// Height of card artwork rendered at `width`, following Steam's header
/// capsule aspect (460×215) so the CDN image fills its box exactly.
pub fn card_art_height(width: f32) -> f32 {
    (width * 215.0 / 460.0).round()
}

/// Full library-card height for a given card width: aspect-correct artwork
/// plus the fixed text stack (title + chips + meta + actions + spacing) —
/// no dead space below the action row.
pub fn game_card_height(card_w: f32) -> f32 {
    card_art_height(card_w - 12.0) + 126.0
}

/// Card width that fills `main_width` with `columns` cards and SP_3 gaps.
///
/// Keeps one extra gap of slack so the wrapped row never overflows by a
/// fraction of a pixel (which would wrap the last card onto its own row).
pub fn library_card_width(main_width: f32, columns: usize) -> f32 {
    let columns = columns.max(1) as f32;
    ((main_width - columns * SP_3) / columns).floor()
}
pub const LIBRARY_RAIL_WIDTH: f32 = 214.0;

pub fn library_main_width(available: f32, rail_below: bool) -> f32 {
    if rail_below {
        available
    } else {
        (available - LIBRARY_RAIL_WIDTH - SP_4).max(GAME_CARD_W)
    }
}

/// Number of game-card columns that fit in the main library width.
pub fn library_grid_columns(main_width: f32) -> usize {
    ((main_width + SP_3) / (GAME_CARD_W + SP_3)).floor() as usize
}

/// Cast f32 spacing to i8 for egui::Margin.
pub const fn m(v: f32) -> i8 {
    v as i8
}

/// Apply the active theme's style and visuals to an egui context.
///
/// Called once at startup (from `eframe::run_native`'s creation callback) and
/// again whenever the user toggles the theme.
pub fn configure_ui(ctx: &egui::Context, mode: ThemeMode) {
    set_active_theme(mode);
    let p = t();
    let egui_theme = if mode.is_dark() {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };

    let mut style = (*ctx.style_of(egui_theme)).clone();
    style.spacing.item_spacing = egui::vec2(SP_2, SP_2);
    style.spacing.button_padding = egui::vec2(SP_3, 6.0);
    style.spacing.window_margin = egui::Margin::same(m(SP_4));
    style.spacing.indent = SP_3;
    style.interaction.selectable_labels = false;

    let mut visuals = if mode.is_dark() {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    visuals.panel_fill = p.canvas;
    visuals.window_fill = p.surface;
    visuals.faint_bg_color = p.surface_muted;
    visuals.extreme_bg_color = p.surface_sunken;
    visuals.hyperlink_color = p.accent;
    visuals.selection.bg_fill = p.accent_soft;
    visuals.selection.stroke.color = p.accent_text;
    visuals.widgets.noninteractive.bg_fill = p.canvas;
    visuals.widgets.noninteractive.weak_bg_fill = p.canvas;
    visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.noninteractive.fg_stroke.color = p.text_secondary;
    visuals.widgets.inactive.bg_fill = p.surface;
    visuals.widgets.inactive.weak_bg_fill = p.surface;
    visuals.widgets.inactive.fg_stroke.color = p.text_secondary;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, p.border);
    visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.hovered.bg_fill = p.accent_soft;
    visuals.widgets.hovered.weak_bg_fill = p.accent_soft;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, p.accent);
    visuals.widgets.hovered.fg_stroke.color = p.text_primary;
    visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.active.bg_fill = p.accent;
    visuals.widgets.active.weak_bg_fill = p.accent;
    visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, p.accent);
    visuals.widgets.active.fg_stroke.color = p.text_inverse;
    visuals.widgets.active.corner_radius = egui::CornerRadius::same(6);
    visuals.widgets.open.bg_fill = p.surface;
    visuals.widgets.open.weak_bg_fill = p.surface;
    visuals.widgets.open.bg_stroke = egui::Stroke::new(1.0, p.accent);
    visuals.widgets.open.fg_stroke.color = p.text_primary;
    visuals.widgets.open.corner_radius = egui::CornerRadius::same(6);
    visuals.override_text_color = Some(p.text_primary);
    visuals.dark_mode = mode.is_dark();
    style.visuals = visuals;

    ctx.set_style_of(egui_theme, style);
    ctx.set_theme(egui_theme);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_sidebar_breakpoint() {
        // Below 1024: not compact (mobile/narrow — handled differently).
        assert!(!is_compact_sidebar(800.0));
        assert!(!is_compact_sidebar(1023.0));
        // 1024–1179: compact (icon-only sidebar).
        assert!(is_compact_sidebar(1024.0));
        assert!(is_compact_sidebar(1100.0));
        assert!(is_compact_sidebar(1179.0));
        // 1180+: full sidebar.
        assert!(!is_compact_sidebar(1180.0));
        assert!(!is_compact_sidebar(1920.0));
    }

    #[test]
    fn compact_padding_breakpoint() {
        // Below 1280: compact padding (16px).
        assert!(is_compact_padding(800.0));
        assert!(is_compact_padding(1024.0));
        assert!(is_compact_padding(1279.0));
        // 1280+: full padding (24px).
        assert!(!is_compact_padding(1280.0));
        assert!(!is_compact_padding(1920.0));
    }

    #[test]
    fn rails_below_breakpoint() {
        // Below 1024: not rails-below (too narrow for two-column at all).
        assert!(!rails_below(800.0));
        assert!(!rails_below(1023.0));
        // 1024–1279: rails move below main.
        assert!(rails_below(1024.0));
        assert!(rails_below(1200.0));
        assert!(rails_below(1279.0));
        // 1280+: two-column side-by-side.
        assert!(!rails_below(1280.0));
        assert!(!rails_below(1920.0));
    }

    #[test]
    fn library_columns_match_reference_widths() {
        // 1280px window: 132px sidebar + 48px central margins = 1100px.
        let at_1280 = library_main_width(1100.0, false);
        assert_eq!(library_grid_columns(at_1280), 4);

        // 1440px window: 132px sidebar + 48px central margins = 1260px.
        let at_1440 = library_main_width(1260.0, false);
        assert_eq!(library_grid_columns(at_1440), 4);

        // When the rail stacks below, the grid receives the complete width.
        assert_eq!(library_main_width(820.0, true), 820.0);
    }
}
