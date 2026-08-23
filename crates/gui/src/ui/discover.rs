//! Discover view.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Styled, div, px, uniform_list,
};
use gpui_component::{
    ActiveTheme, Disableable, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};

use crate::app::{PendingAction, View, reason_badge_label};

use crate::ui::GuiRoot;

impl GuiRoot {
    pub(crate) fn discover(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        v_flex()
            .id("discover")
            .size_full()
            .gap_3()
            .child(div().text_xl().font_semibold().child("Discover"))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Similar picks from a game name or AppID. Writes the stable `discover` slot."),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div().text_sm().child(format!("Seed: {}", self.app.discover_seed)),
                    )
                    .child(
                        Button::new("disc-seed-sel")
                            .small()
                            .label("Use selected library game")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        if let Some(id) = this.app.library_selected_app_id {
                                            this.app.discover_seed = id.to_string();
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("disc-go")
                            .primary()
                            .label(if self.app.discover_loading {
                                "Generating…"
                            } else {
                                "Generate"
                            })
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.start_discover_generate();
                                        this.arm_poll(cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("disc-open")
                            .small()
                            .label("Open in Playlists")
                            .disabled(self.app.discover_last_playlist.is_none())
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        if let Some(pf) = this.app.discover_last_playlist.clone() {
                                            this.app.adopt_playlist_for_edit(&pf);
                                            this.app.current_view = View::Playlists;
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("disc-sync")
                            .small()
                            .label("Sync to Steam collection")
                            .disabled(self.app.discover_last_playlist.is_none() || self.app.ui_demo)
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        if let Some(pf) = this.app.discover_last_playlist.clone() {
                                            this.app.start_dry_run(PendingAction::PlaylistSync(pf));
                                            this.arm_poll(cx);
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child({
                let n = self.app.discover_results.len();
                uniform_list("disc-rows", n, cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range.filter_map(|ix| this.app.discover_results.get(ix).cloned()).map(|pick| {
                        h_flex()
                            .id(("disc", pick.app_id as usize))
                            .h(px(44.))
                            .gap_3()
                            .child(
                                div().w(px(64.)).text_xs().child(format!("{:.1} pts", pick.score)),
                            )
                            .child(div().flex_1().text_sm().child(pick.name.clone()))
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(
                                pick.reasons.first().map_or_else(
                                    || "Similar".into(),
                                    |r| reason_badge_label(r.code, r.description),
                                ),
                            ))
                    }).collect()
                })).flex_1()
            })
    }
}
