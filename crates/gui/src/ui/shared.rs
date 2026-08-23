//! Presentation helpers shared across views.

use gpui::{App, Hsla, IntoElement, ParentElement, SharedString, Styled, div, px, rgb};
use gpui_component::{ActiveTheme, StyledExt, h_flex, v_flex};
use vapourfly_core::models::ProtonTier;

use crate::app::{ARTWORK_PALETTE, empty_value_label};
use crate::theme::{self};

pub(crate) fn hx(c: theme::Rgb) -> Hsla {
    rgb(c.to_u32()).into()
}

use crate::ui::GuiRoot;

impl GuiRoot {
    pub(crate) fn placeholder(&self, app_id: u32, height: f32) -> impl IntoElement {
        let (top, _) = ARTWORK_PALETTE[(app_id as usize) % ARTWORK_PALETTE.len()];
        div().w_full().h(px(height)).rounded(px(6.)).bg(hx(top))
    }
}

pub(crate) fn insight_tile(label: &str, value: String, cx: &App) -> impl IntoElement {
    h_flex()
        .justify_between()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(SharedString::from(label.to_string())),
        )
        .child(div().text_xs().font_semibold().child(value))
}

pub(crate) fn section(
    title: &str,
    cx: &App,
    body: impl FnOnce(&App) -> gpui::AnyElement,
) -> impl IntoElement {
    v_flex()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .child(
            div()
                .text_sm()
                .font_semibold()
                .child(SharedString::from(title.to_string())),
        )
        .child(body(cx))
}

pub(crate) fn empty_or(value: &str) -> &str {
    if value.trim().is_empty() {
        empty_value_label()
    } else {
        value
    }
}

pub(crate) fn this_tier_label(tier: Option<ProtonTier>) -> String {
    tier.map(|t| format!("{t:?}"))
        .unwrap_or_else(|| empty_value_label().into())
}
