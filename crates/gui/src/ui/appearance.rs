//! Theme token bridge between the ADR-0006 palettes and gpui-component.

use std::path::PathBuf;

use gpui::{App, Hsla, Window, px, rgb};
use gpui_component::{Colorize, Theme, ThemeMode as GpuiThemeMode};

use super::shared::hx;
use crate::app::VapourflyApp;
use crate::theme::{self, ThemeMode, set_active_theme};

pub(crate) fn apply_tokens(window: &mut Window, cx: &mut App, mode: ThemeMode) {
    set_active_theme(mode);
    Theme::change(
        if mode.is_dark() {
            GpuiThemeMode::Dark
        } else {
            GpuiThemeMode::Light
        },
        Some(window),
        cx,
    );
    let tokens = theme::t();
    // Hover/active derivations deepen on the warm light canvas and lift on
    // the cool dark one, matching how each palette encodes emphasis.
    let shift = |c: theme::Rgb, amount: f32| {
        let base = hx(c);
        if mode.is_dark() {
            base.lighten(amount)
        } else {
            base.darken(amount)
        }
    };
    let theme = Theme::global_mut(cx);

    // Canvas and chrome.
    theme.background = hx(tokens.canvas);
    theme.foreground = hx(tokens.text_primary);
    theme.border = hx(tokens.border);
    theme.title_bar = hx(tokens.surface);
    theme.title_bar_border = hx(tokens.border_soft);
    theme.window_border = hx(tokens.border);
    theme.overlay = Hsla::from(rgb(0x000000)).opacity(0.45);

    // Primary (brand accent) family.
    theme.primary = hx(tokens.accent);
    theme.primary_foreground = hx(tokens.text_inverse);
    theme.primary_hover = shift(tokens.accent, 0.08);
    theme.primary_active = shift(tokens.accent, 0.16);

    // Secondary / muted surfaces.
    theme.secondary = hx(tokens.surface_muted);
    theme.secondary_foreground = hx(tokens.text_secondary);
    theme.secondary_hover = shift(tokens.surface_muted, 0.04);
    theme.secondary_active = shift(tokens.surface_muted, 0.08);
    theme.muted = hx(tokens.surface_muted);
    theme.muted_foreground = hx(tokens.text_muted);

    // Accent (soft selection surfaces).
    theme.accent = hx(tokens.accent_soft);
    theme.accent_foreground = hx(tokens.accent_text);

    // Status families.
    theme.danger = hx(tokens.error);
    theme.danger_foreground = hx(tokens.text_inverse);
    theme.danger_hover = shift(tokens.error, 0.08);
    theme.danger_active = shift(tokens.error, 0.16);
    theme.success = hx(tokens.success);
    theme.success_foreground = hx(tokens.text_inverse);
    theme.success_hover = shift(tokens.success, 0.08);
    theme.success_active = shift(tokens.success, 0.16);
    theme.warning = hx(tokens.warning);
    theme.warning_foreground = hx(tokens.text_inverse);
    theme.warning_hover = shift(tokens.warning, 0.08);
    theme.warning_active = shift(tokens.warning, 0.16);
    theme.info = hx(tokens.accent);
    theme.info_foreground = hx(tokens.text_inverse);
    theme.info_hover = shift(tokens.accent, 0.08);
    theme.info_active = shift(tokens.accent, 0.16);

    // Inputs and focus.
    theme.input = hx(tokens.surface_sunken);
    theme.caret = hx(tokens.text_primary);
    theme.ring = hx(tokens.accent);
    theme.selection = hx(tokens.accent_soft);

    // Links.
    theme.link = hx(tokens.accent);
    theme.link_hover = hx(tokens.accent_text);
    theme.link_active = hx(tokens.accent_text);

    // Lists (library rows, menus).
    theme.list = hx(tokens.surface);
    theme.list_hover = hx(tokens.surface_muted);
    theme.list_active = hx(tokens.accent_soft);
    theme.list_active_border = hx(tokens.accent);
    theme.list_even = hx(tokens.canvas);
    theme.list_head = hx(tokens.surface_muted);

    // Tables mirror the list family plus header text and row separators.
    theme.table = hx(tokens.surface);
    theme.table_hover = hx(tokens.surface_muted);
    theme.table_active = hx(tokens.accent_soft);
    theme.table_active_border = hx(tokens.accent);
    theme.table_even = hx(tokens.canvas);
    theme.table_head = hx(tokens.surface_muted);
    theme.table_head_foreground = hx(tokens.text_secondary);
    theme.table_row_border = hx(tokens.border_soft);

    // Tabs.
    theme.tab = hx(tokens.surface_muted);
    theme.tab_foreground = hx(tokens.text_secondary);
    theme.tab_active = hx(tokens.surface_raised);
    theme.tab_active_foreground = hx(tokens.text_primary);
    theme.tab_bar = hx(tokens.canvas);
    theme.tab_bar_segmented = hx(tokens.surface_sunken);

    // Sidebar.
    theme.sidebar = hx(tokens.surface);
    theme.sidebar_foreground = hx(tokens.text_primary);
    theme.sidebar_accent = hx(tokens.accent_soft);
    theme.sidebar_accent_foreground = hx(tokens.accent_text);
    theme.sidebar_border = hx(tokens.border_soft);
    theme.sidebar_primary = hx(tokens.accent);
    theme.sidebar_primary_foreground = hx(tokens.text_inverse);

    // Popovers and cards.
    theme.popover = hx(tokens.surface_raised);
    theme.popover_foreground = hx(tokens.text_primary);
    theme.group_box = hx(tokens.surface);
    theme.group_box_foreground = hx(tokens.text_primary);
    theme.accordion = hx(tokens.surface);
    theme.accordion_hover = hx(tokens.surface_muted);

    // Scrollbars, skeletons, misc component skins.
    theme.scrollbar = hx(tokens.surface_muted);
    theme.scrollbar_thumb = hx(tokens.border);
    theme.scrollbar_thumb_hover = hx(tokens.text_muted);
    theme.skeleton = hx(tokens.surface_muted);
    theme.progress_bar = hx(tokens.accent);
    theme.switch = hx(tokens.surface_sunken);
    theme.switch_thumb = hx(tokens.surface_raised);
    theme.slider_bar = hx(tokens.surface_muted);
    theme.slider_thumb = hx(tokens.accent);
    theme.drag_border = hx(tokens.accent);
    theme.drop_target = hx(tokens.accent_soft);
    theme.tiles = hx(tokens.canvas);

    // Charts reuse the editorial tag-tint hues so data viz shares the
    // card language; bull/bear follow the status greens/reds.
    theme.chart_1 = hx(theme::tint(0).1);
    theme.chart_2 = hx(theme::tint(1).1);
    theme.chart_3 = hx(theme::tint(2).1);
    theme.chart_4 = hx(theme::tint(3).1);
    theme.chart_5 = hx(theme::tint(4).1);
    theme.bullish = hx(tokens.success);
    theme.bearish = hx(tokens.error);

    theme.radius = px(8.);
}

pub(crate) fn persist_theme(app: &VapourflyApp, mode: ThemeMode) {
    if app.ui_demo {
        return;
    }
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vapourfly");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("gui-theme"), mode.as_u8().to_string());
}

pub(crate) fn load_persisted_theme() -> ThemeMode {
    let path = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vapourfly")
        .join("gui-theme");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .map(ThemeMode::from_u8)
        .unwrap_or(ThemeMode::Light)
}
