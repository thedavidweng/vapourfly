//! Small presentational building blocks shared by every page.

use std::path::PathBuf;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Colorize, Icon, StyledExt,
    empty::{
        Empty, EmptyContent, EmptyDescription, EmptyHeader, EmptyMedia, EmptyMediaVariant,
        EmptyTitle,
    },
    h_flex,
    list::ListItem,
    tab::{Tab, TabBar},
    tag::Tag,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Div, ElementId, FocusHandle, Hsla, IntoElement, ObjectFit, ParentElement,
    SharedString, Stateful, Styled, StyledImage, div, img, linear_color_stop, linear_gradient,
    prelude::*, px,
};

use super::hx;
use crate::app::ARTWORK_PALETTE;
use crate::theme;

/// Horizontal padding of every page body.
pub(super) const PAGE_PX: f32 = 36.;

/// A clickable row on the stock `ListItem` (which owns the hover and
/// selected fills), made a tab stop whose keyboard/controller focus shows
/// the same fill as hover. SteamOS uses one highlight for both inputs.
/// `ListItem` only gets keyboard focus inside a `List`; these rows sit in
/// plain or virtualized layouts, so the tab stop is added here.
pub(super) fn focus_row(id: impl Into<ElementId>, selected: bool, cx: &App) -> ListItem {
    let highlight = cx.theme().list_hover;
    ListItem::new(id)
        .selected(selected)
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .tab_index(0)
        .focus_visible(move |s| s.bg(highlight))
}

/// [`focus_row`] with a caller-owned focus handle, so the row can be
/// focused programmatically (the controller cursor in a virtualized list).
pub(super) fn focus_row_tracked(
    id: impl Into<ElementId>,
    selected: bool,
    focus: &FocusHandle,
    cx: &App,
) -> ListItem {
    let highlight = cx.theme().list_hover;
    ListItem::new(id)
        .selected(selected)
        .rounded(cx.theme().radius)
        .cursor_pointer()
        .track_focus(focus)
        .focus_visible(move |s| s.bg(highlight))
}

/// Ring a capsule's frame on hover of its card `group` or on keyboard /
/// controller focus of the frame itself, the way SteamOS outlines a focused
/// capsule. The frame becomes the tab stop; it needs its own border width.
pub(super) fn capsule_focus(
    frame: Stateful<Div>,
    group: impl Into<SharedString>,
    cx: &App,
) -> Stateful<Div> {
    let ring = cx.theme().ring;
    frame
        .tab_index(0)
        .group_hover(group, move |s| s.border_color(ring))
        .focus_visible(move |s| s.border_color(ring))
}

/// Up to two initials for an avatar placeholder ("Demo Player" → "DP").
pub(super) fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    let picked: String = match words.as_slice() {
        [] => String::new(),
        [one] => one.chars().take(2).collect(),
        [first, .., last] => first.chars().take(1).chain(last.chars().take(1)).collect(),
    };
    picked.to_uppercase()
}

/// A Steam profile picture: the client's cached avatar when there is one,
/// else the persona's initials on Steam blue (a user glyph without a name).
pub(super) fn avatar(picture: Option<PathBuf>, persona: Option<&str>, size: f32) -> Div {
    let t = theme::t();
    let label = persona.map(initials).unwrap_or_default();
    let placeholder = move || {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(linear_gradient(
                150.,
                linear_color_stop(hx(t.accent).darken(0.05), 0.),
                linear_color_stop(hx(t.accent).darken(0.3), 1.),
            ))
            .text_size(px((size * 0.4).round()))
            .font_bold()
            .text_color(gpui_kit::white())
            .map(|this| {
                if label.is_empty() {
                    this.child(
                        Icon::new(IconName::User)
                            .size(px(size * 0.5))
                            .text_color(gpui_kit::white()),
                    )
                } else {
                    this.child(label.clone())
                }
            })
    };
    let content = match picture {
        Some(path) => img(path)
            .size_full()
            .object_fit(ObjectFit::Cover)
            .with_fallback({
                let placeholder = placeholder.clone();
                move || placeholder().into_any_element()
            })
            .into_any_element(),
        None => placeholder().into_any_element(),
    };
    div()
        .flex_none()
        .size(px(size))
        .rounded(px(theme::CORNER_SM))
        .overflow_hidden()
        .child(content)
}

/// Title block at the top of a page: a large title, one line of context,
/// and right-aligned actions.
pub(super) fn page_header(
    title: impl Into<SharedString>,
    subtitle: impl IntoElement,
    actions: impl IntoElement,
    cx: &App,
) -> impl IntoElement {
    h_flex()
        .w_full()
        .px(px(PAGE_PX))
        .pt(px(30.))
        .pb(px(20.))
        .gap_4()
        .items_end()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_1()
                .child(
                    div()
                        .text_size(px(32.))
                        .line_height(px(40.))
                        .font_bold()
                        .child(title.into()),
                )
                .child(
                    div()
                        .text_size(px(15.))
                        .text_color(cx.theme().muted_foreground)
                        .child(subtitle),
                ),
        )
        .child(h_flex().flex_none().gap_2().child(actions))
}

/// Heading for a block of content inside a page, one step below the page
/// title.
pub(super) fn section_title(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_size(px(19.))
        .line_height(px(26.))
        .font_semibold()
        .text_color(cx.theme().foreground)
        .child(text.into())
}

/// Small label above a control or a group of fields.
pub(super) fn section_label(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_size(px(13.))
        .font_medium()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

pub(super) fn caption(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_size(px(13.))
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// Bordered surface used for grouped content.
pub(super) fn panel(cx: &App) -> Div {
    v_flex()
        .bg(cx.theme().group_box)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius_lg)
}

pub(super) fn stat_tile(
    label: impl Into<SharedString>,
    value: impl Into<SharedString>,
    tone: Option<Hsla>,
    cx: &App,
) -> impl IntoElement {
    v_flex()
        .flex_1()
        .min_w(px(110.))
        .gap_0p5()
        .px_4()
        .py_3()
        .bg(cx.theme().group_box)
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius_lg)
        .child(caption(label, cx))
        .child(
            div()
                .text_size(px(24.))
                .font_semibold()
                .text_color(tone.unwrap_or(cx.theme().foreground))
                .child(value.into()),
        )
}

/// A coloured dot followed by a short label.
pub(super) fn status(label: impl Into<SharedString>, tone: Hsla, cx: &App) -> impl IntoElement {
    h_flex()
        .gap_1p5()
        .text_size(px(13.))
        .text_color(cx.theme().muted_foreground)
        .child(div().flex_none().size(px(6.)).rounded_full().bg(tone))
        .child(label.into())
}

/// Translucent pill drawn over cover art.
pub(super) fn art_pill(label: impl Into<SharedString>) -> impl IntoElement {
    div()
        .px_1p5()
        .py_0p5()
        .rounded(px(theme::CORNER_SM))
        .bg(gpui_kit::black().opacity(0.62))
        .text_color(gpui_kit::white())
        .text_size(px(12.))
        .font_medium()
        .child(label.into())
}

/// Cover art that fills its parent. Falls back to a generated duotone cover
/// carrying the game's name when Steam artwork is not available.
pub(super) fn cover(
    art: Option<PathBuf>,
    app_id: u32,
    name: &str,
    title_size: Option<f32>,
) -> AnyElement {
    let fallback_name = name.to_string();
    let fallback = move || generated_cover(app_id, &fallback_name, title_size).into_any_element();
    match art {
        Some(path) => img(path)
            .size_full()
            .object_fit(ObjectFit::Cover)
            .with_fallback(fallback.clone())
            .with_loading(fallback)
            .into_any_element(),
        None => fallback(),
    }
}

fn generated_cover(app_id: u32, name: &str, title_size: Option<f32>) -> impl IntoElement {
    let (light, deep) = ARTWORK_PALETTE[(app_id as usize) % ARTWORK_PALETTE.len()];
    let light = hx(light);
    let deep = hx(deep);
    div()
        .size_full()
        .relative()
        .overflow_hidden()
        .bg(linear_gradient(
            160.,
            linear_color_stop(light, 0.),
            linear_color_stop(deep, 1.),
        ))
        .child(
            div()
                .absolute()
                .top(px(-40.))
                .right(px(-30.))
                .size(px(150.))
                .rounded_full()
                .bg(light.lighten(0.2).opacity(0.22)),
        )
        .child(
            div()
                .absolute()
                .bottom(px(-60.))
                .left(px(-20.))
                .size(px(140.))
                .rounded_full()
                .bg(deep.darken(0.2).opacity(0.35)),
        )
        .when_some(title_size, |this, size| {
            this.child(
                div()
                    .absolute()
                    .left(px(12.))
                    .right(px(12.))
                    .bottom(px(10.))
                    .text_size(px(size))
                    .line_height(px(size * 1.15))
                    .font_semibold()
                    .text_color(gpui_kit::white().opacity(0.94))
                    .child(name.to_string()),
            )
        })
}

/// Portrait (2:3) capsule that fills its parent. The generated fallback
/// is laid out like key art: a duotone field with the title at the foot.
pub(super) fn poster(art: Option<PathBuf>, app_id: u32, name: &str, radius: f32) -> AnyElement {
    let fallback_name = name.to_string();
    let fallback = move || generated_poster(app_id, &fallback_name, radius).into_any_element();
    match art {
        Some(path) => img(path)
            .size_full()
            .rounded(px(radius))
            .object_fit(ObjectFit::Cover)
            .with_fallback(fallback.clone())
            .with_loading(fallback)
            .into_any_element(),
        None => fallback(),
    }
}

// Decorative shapes stay clear of the corners: children are clipped to the
// rectangle, not to the rounded outline.
fn generated_poster(app_id: u32, name: &str, radius: f32) -> impl IntoElement {
    let (light, deep) = ARTWORK_PALETTE[(app_id as usize) % ARTWORK_PALETTE.len()];
    let light = hx(light);
    let deep = hx(deep);
    div()
        .size_full()
        .relative()
        .overflow_hidden()
        .rounded(px(radius))
        .bg(linear_gradient(
            165.,
            linear_color_stop(light, 0.),
            linear_color_stop(deep, 0.9),
        ))
        .child(
            div()
                .absolute()
                .top(px(24.))
                .right(px(-96.))
                .size(px(220.))
                .rounded_full()
                .bg(light.lighten(0.25).opacity(0.25)),
        )
        .child(
            div()
                .absolute()
                .top(px(90.))
                .left(px(-60.))
                .size(px(160.))
                .rounded_full()
                .bg(deep.darken(0.3).opacity(0.3)),
        )
        .child(
            div()
                .absolute()
                .inset_0()
                .rounded(px(radius))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(gpui_kit::black().opacity(0.), 0.5),
                    linear_color_stop(gpui_kit::black().opacity(0.55), 1.),
                )),
        )
        .child(
            div()
                .absolute()
                .left(px(14.))
                .right(px(14.))
                .bottom(px(16.))
                .text_size(px(21.))
                .line_height(px(24.))
                .font_bold()
                .text_color(gpui_kit::white())
                .child(name.to_string()),
        )
}

/// Wide generated backdrop used behind a hero when no artwork exists.
pub(super) fn backdrop(app_id: u32) -> impl IntoElement {
    let (light, deep) = ARTWORK_PALETTE[(app_id as usize) % ARTWORK_PALETTE.len()];
    let light = hx(light);
    let deep = hx(deep);
    div()
        .size_full()
        .relative()
        .overflow_hidden()
        .bg(linear_gradient(
            120.,
            linear_color_stop(deep, 0.),
            linear_color_stop(light, 1.),
        ))
        .child(
            div()
                .absolute()
                .top(px(-220.))
                .right(px(-80.))
                .size(px(640.))
                .rounded_full()
                .bg(light.lighten(0.2).opacity(0.3)),
        )
        .child(
            div()
                .absolute()
                .bottom(px(-260.))
                .right(px(360.))
                .size(px(520.))
                .rounded_full()
                .bg(deep.darken(0.2).opacity(0.4)),
        )
}

pub(super) fn empty_state(
    icon: IconName,
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    action: Option<AnyElement>,
) -> impl IntoElement {
    let empty = Empty::new().header(
        EmptyHeader::new()
            .media(
                EmptyMedia::new()
                    .with_variant(EmptyMediaVariant::Icon)
                    .child(Icon::new(icon)),
            )
            .title(EmptyTitle::new().child(title.into()))
            .description(EmptyDescription::new().child(description.into())),
    );
    match action {
        Some(action) => empty.content(EmptyContent::new().child(action)),
        None => empty,
    }
}

/// Icon sized for inline use beside text.
pub(super) fn inline_icon(icon: IconName, color: Hsla) -> Icon {
    Icon::new(icon).text_color(color)
}

/// Segmented control for picking one of a few options. `selected` is `None`
/// when no option applies yet.
pub(super) fn segmented(
    id: &'static str,
    tabs: impl IntoIterator<Item = Tab>,
    selected: Option<usize>,
) -> TabBar {
    let bar = TabBar::new(id).segmented().children(tabs);
    match selected {
        Some(ix) => bar.selected_index(ix),
        None => bar,
    }
}

/// Semantic colour of a [`tone_tag`].
#[derive(Clone, Copy)]
pub(super) enum Tone {
    Accent,
    Success,
    Warning,
    Danger,
}

/// Small tag on a soft tint of its tone, readable in both themes.
pub(super) fn tone_tag(tone: Tone, label: impl Into<SharedString>) -> Tag {
    let t = theme::t();
    let (bg, fg) = match tone {
        Tone::Accent => (t.accent_soft, t.accent_text),
        Tone::Success => (t.success_soft, t.success),
        Tone::Warning => (t.warning_soft, t.warning),
        Tone::Danger => (t.error_soft, t.error),
    };
    Tag::custom(hx(bg), hx(fg), hx(bg)).child(label.into())
}

#[cfg(test)]
mod tests {
    use super::initials;

    #[test]
    fn initials_take_first_and_last_words() {
        assert_eq!(initials("Demo Player"), "DP");
        assert_eq!(initials("ada lovelace byron"), "AB");
        assert_eq!(initials("MarcusPierce"), "MA");
        assert_eq!(initials("  "), "");
    }
}
