//! Purposeful-minimum motion: short ease-out fades for overlays and panels.
//!
//! Budget (ADR-0006 amendment): every animation is opacity-first, ≤250ms,
//! ease-out. There is no OS reduce-motion query in gpui 0.2.2, so the
//! preference is persisted next to the theme file and exposed as a
//! Settings → Appearance switch. When reduce-motion is on every helper
//! returns its element unchanged.

use std::{path::PathBuf, time::Duration};

use gpui::{
    Animation, AnimationExt, AnyElement, App, ElementId, Global, IntoElement, SharedString, Styled,
    px,
};
use gpui_component::animation::cubic_bezier;

/// Fast feedback (chips, small fades).
pub const DUR_FAST: Duration = Duration::from_millis(150);
/// Default surface transition (dialogs, banners).
pub const DUR_BASE: Duration = Duration::from_millis(200);
/// Larger surfaces (sheets, wide panels).
pub const DUR_SLOW: Duration = Duration::from_millis(250);

/// Global reduce-motion preference; see [`reduce_motion`].
#[derive(Default)]
pub struct ReduceMotion(pub(crate) bool);

impl Global for ReduceMotion {}

/// True when animations should be suppressed.
pub fn reduce_motion(cx: &App) -> bool {
    cx.try_global::<ReduceMotion>().is_some_and(|g| g.0)
}

/// Load the persisted preference into the global. Called once at startup.
pub fn init(cx: &mut App) {
    let on = std::fs::read_to_string(preference_path())
        .ok()
        .is_some_and(|s| s.trim() == "1");
    cx.set_global(ReduceMotion(on));
}

/// Update the preference and persist it (callers gate demo mode).
pub fn set_reduce_motion(on: bool, cx: &mut App) {
    let _ = std::fs::create_dir_all(
        preference_path()
            .parent()
            .map(PathBuf::from)
            .unwrap_or_default(),
    );
    let _ = std::fs::write(preference_path(), u8::from(on).to_string());
    cx.set_global(ReduceMotion(on));
}

fn preference_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vapourfly")
        .join("gui-reduce-motion")
}

/// Apple-style deceleration curve used across all Vapourfly motion.
pub fn ease_out() -> impl Fn(f32) -> f32 {
    cubic_bezier(0.16, 1.0, 0.3, 1.0)
}

/// Fade an element in (`opacity 0→1`). Returns the element unchanged when
/// reduce-motion is active.
pub fn fade_in<E: IntoElement + Styled + 'static>(
    el: E,
    id: impl Into<ElementId>,
    dur: Duration,
    cx: &App,
) -> AnyElement {
    if reduce_motion(cx) {
        return el.into_any_element();
    }
    el.with_animation(
        id,
        Animation::new(dur).with_easing(ease_out()),
        |el, delta| el.opacity(delta),
    )
    .into_any_element()
}

/// Fade in with a subtle upward settle (`6px → 0`). For dialog content.
pub fn rise_in<E: IntoElement + Styled + 'static>(
    el: E,
    id: impl Into<ElementId>,
    dur: Duration,
    cx: &App,
) -> AnyElement {
    if reduce_motion(cx) {
        return el.into_any_element();
    }
    el.with_animation(
        id,
        Animation::new(dur).with_easing(ease_out()),
        |el, delta| {
            let remain = 1.0 - delta;
            el.opacity(delta).mt(px(-6.0 * remain))
        },
    )
    .into_any_element()
}

/// Animate a container's width between two states (sidebar collapse).
///
/// The animation id includes the target state so each toggle remounts the
/// animation and eases from the opposite endpoint instead of snapping.
pub fn collapse_width<E: IntoElement + Styled + 'static>(
    el: E,
    base_id: &'static str,
    collapsed: bool,
    full_px: f32,
    compact_px: f32,
    cx: &App,
) -> AnyElement {
    if reduce_motion(cx) {
        return el.into_any_element();
    }
    let id: String = if collapsed {
        format!("{base_id}-in")
    } else {
        format!("{base_id}-out")
    };
    let (from, to) = if collapsed {
        (full_px, compact_px)
    } else {
        (compact_px, full_px)
    };
    el.with_animation(
        SharedString::from(id),
        Animation::new(DUR_BASE).with_easing(ease_out()),
        move |el, delta| el.w(px(from + (to - from) * delta)),
    )
    .into_any_element()
}
