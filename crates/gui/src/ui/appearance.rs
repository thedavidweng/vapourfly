//! Theme token bridge between the ADR-0006 palettes and gpui-component.

use std::path::PathBuf;

use gpui::{App, Window, px};
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
    let theme = Theme::global_mut(cx);
    theme.background = hx(tokens.canvas);
    theme.foreground = hx(tokens.text_primary);
    theme.border = hx(tokens.border);
    theme.primary = hx(tokens.accent);
    theme.primary_foreground = hx(tokens.text_inverse);
    theme.primary_hover = hx(tokens.accent).lighten(0.06);
    theme.muted = hx(tokens.surface_muted);
    theme.muted_foreground = hx(tokens.text_muted);
    theme.secondary = hx(tokens.surface_muted);
    theme.secondary_foreground = hx(tokens.text_secondary);
    theme.sidebar = hx(tokens.surface);
    theme.sidebar_foreground = hx(tokens.text_primary);
    theme.sidebar_accent = hx(tokens.accent_soft);
    theme.sidebar_accent_foreground = hx(tokens.accent_text);
    theme.sidebar_border = hx(tokens.border_soft);
    theme.danger = hx(tokens.error);
    theme.success = hx(tokens.success);
    theme.warning = hx(tokens.warning);
    theme.link = hx(tokens.accent);
    theme.ring = hx(tokens.accent);
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
