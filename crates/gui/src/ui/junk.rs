//! Junk triage panel.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Styled, div, prelude::*, rems,
    uniform_list,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    tab::{Tab, TabBar},
    tag::Tag,
    v_flex,
};

use crate::app::{JunkModeChoice, PendingAction};

use crate::ui::GuiRoot;

/// Mode choices in display order for the segmented mode switcher.
const JUNK_MODES: [JunkModeChoice; 3] = [
    JunkModeChoice::Default,
    JunkModeChoice::Strict,
    JunkModeChoice::Aggressive,
];

impl GuiRoot {
    pub(crate) fn junk_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let selected = self.app.junk_selected.len();
        let preview_loading = self.app.junk_preview_loading;
        v_flex()
            .id("junk")
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_xl().font_semibold().child("Junk cleanup"))
                    .child(
                        Button::new("junk.back")
                            .ghost()
                            .label("Back to Library")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.show_junk_panel = false;
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child({
                let active_ix = JUNK_MODES
                    .iter()
                    .position(|mode| *mode == self.app.junk_mode)
                    .unwrap_or(0);
                TabBar::new("junk.mode")
                    .segmented()
                    .small()
                    .selected_index(active_ix)
                    .on_click({
                        let entity = entity.clone();
                        move |ix, _, cx| {
                            if let Some(mode) = JUNK_MODES.get(*ix).copied() {
                                entity.update(cx, |this, cx| {
                                    this.app.junk_mode = mode;
                                    cx.notify();
                                });
                            }
                        }
                    })
                    .children(JUNK_MODES.iter().map(|mode| Tab::new().label(mode.label())))
            })
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("junk.preview")
                            .outline()
                            .icon(if preview_loading {
                                IconName::LoaderCircle
                            } else {
                                IconName::Eye
                            })
                            .label("Preview")
                            .disabled(preview_loading)
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.start_junk_preview();
                                        this.arm_poll(cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("junk.show-all")
                            .small()
                            .when(self.app.junk_show_all_evaluated, |b| b.primary())
                            .label("Show all evaluated")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.junk_show_all_evaluated =
                                            !this.app.junk_show_all_evaluated;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("junk.select-all")
                            .small()
                            .ghost()
                            .label("Select all junk")
                            .tooltip("Select all candidates")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.junk_selected = this
                                            .app
                                            .junk_results
                                            .iter()
                                            .filter(|d| d.is_junk)
                                            .map(|d| d.app_id)
                                            .collect();
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("junk.select-clear")
                            .small()
                            .ghost()
                            .label("Clear")
                            .tooltip("Clear selection")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.junk_selected.clear();
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("junk.apply")
                            .small()
                            .label("Apply to collection…")
                            .disabled(selected == 0 || self.app.ui_demo)
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.start_dry_run(PendingAction::JunkApply);
                                        this.arm_poll(cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("junk.hide")
                            .small()
                            .outline()
                            .label("Hide…")
                            .disabled(selected == 0 || self.app.ui_demo)
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.start_dry_run(PendingAction::JunkHide);
                                        this.arm_poll(cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(div().child(format!("Detection mode: {}", self.app.junk_mode.label())))
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                div().child(format!("Evaluated {}", self.app.junk_results.len())),
                            )
                            .child(div().child(format!("Selected {selected}"))),
                    ),
            )
            .child({
                let rows: Vec<_> = self
                    .app
                    .junk_results
                    .iter()
                    .filter(|d| self.app.junk_show_all_evaluated || d.is_junk)
                    .cloned()
                    .collect();
                let n = rows.len();
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .when(n == 0, |this| {
                        this.child(
                            v_flex()
                                .flex_1()
                                .items_center()
                                .justify_center()
                                .gap_2()
                                .text_color(cx.theme().muted_foreground)
                                .child(Icon::new(IconName::Inbox).size_6())
                                .child(
                                    div()
                                        .text_sm()
                                        .child("No candidates match the current mode"),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .child("Try another detection mode or run Preview."),
                                ),
                        )
                    })
                    .when(n > 0, |this| {
                        this.child({
                            div().flex().flex_row().flex_1().min_h_0().child(
                                uniform_list(
                                    "junk-rows",
                                    n,
                                    cx.processor(
                                        |this, range: std::ops::Range<usize>, _window, cx| {
                                            let visible: Vec<_> = this
                                                .app
                                                .junk_results
                                                .iter()
                                                .filter(|d| {
                                                    this.app.junk_show_all_evaluated || d.is_junk
                                                })
                                                .cloned()
                                                .collect();
                                            range
                                                .filter_map(|ix| visible.get(ix).cloned())
                                                .map(|d| {
                                                    let entity = cx.entity();
                                                    let id = d.app_id;
                                                    let checked =
                                                        this.app.junk_selected.contains(&id);
                                                    h_flex()
                                                        .id(("junk-row", id as usize))
                                                        .h_10()
                                                        .items_center()
                                                        .gap_2()
                                                        .child(
                                                            Checkbox::new(("junk-cb", id as usize))
                                                                .checked(checked)
                                                                .on_click(move |_, _, cx| {
                                                                    entity.update(
                                                                        cx,
                                                                        |this, cx| {
                                                                            if this
                                                                                .app
                                                                                .junk_selected
                                                                                .contains(&id)
                                                                            {
                                                                                this.app
                                                                                    .junk_selected
                                                                                    .remove(&id);
                                                                            } else {
                                                                                this.app
                                                                                    .junk_selected
                                                                                    .insert(id);
                                                                            }
                                                                            cx.notify();
                                                                        },
                                                                    );
                                                                }),
                                                        )
                                                        .child(
                                                            div()
                                                                .w(rems(4.5))
                                                                .text_xs()
                                                                .child(id.to_string()),
                                                        )
                                                        .child(
                                                            div()
                                                                .flex_1()
                                                                .text_sm()
                                                                .child(d.name.clone()),
                                                        )
                                                        .child(div().w_16().child(if d.is_junk {
                                                            Tag::warning().small().child("Junk")
                                                        } else {
                                                            Tag::secondary().small().child("Keep")
                                                        }))
                                                        .child(div().w_20().child(
                                                            Tag::secondary().small().child(
                                                                format!(
                                                                    "{:.0}%",
                                                                    d.confidence * 100.0
                                                                ),
                                                            ),
                                                        ))
                                                })
                                                .collect()
                                        },
                                    ),
                                )
                                .flex_1(),
                            )
                        })
                    })
            })
    }
}
