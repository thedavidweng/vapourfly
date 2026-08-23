//! Junk triage panel.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Styled, div, prelude::*, px,
    uniform_list,
};
use gpui_component::{
    ActiveTheme, Disableable, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex, v_flex,
};

use crate::app::{JunkModeChoice, PendingAction};

use crate::ui::GuiRoot;

impl GuiRoot {
    pub(crate) fn junk_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let selected = self.app.junk_selected.len();
        v_flex()
            .id("junk")
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_xl().font_semibold().child("Junk cleanup"))
                    .child(
                        Button::new("junk-back")
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
            .child(
                h_flex().gap_2().children(
                    [
                        JunkModeChoice::Default,
                        JunkModeChoice::Strict,
                        JunkModeChoice::Aggressive,
                    ]
                    .into_iter()
                    .map(|mode| {
                        let entity = entity.clone();
                        let active = self.app.junk_mode == mode;
                        Button::new(mode.label())
                            .small()
                            .when(active, |b| b.primary())
                            .label(mode.label())
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.app.junk_mode = mode;
                                    cx.notify();
                                });
                            })
                    }),
                ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("junk-preview")
                            .primary()
                            .label(if self.app.junk_preview_loading {
                                "Previewing…"
                            } else {
                                "Preview"
                            })
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
                        Button::new("junk-show-all")
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
                        Button::new("junk-all")
                            .small()
                            .label("Select all junk")
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
                    .child(Button::new("junk-clear").small().label("Clear").on_click({
                        let entity = entity.clone();
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.app.junk_selected.clear();
                                cx.notify();
                            });
                        }
                    }))
                    .child(
                        Button::new("junk-apply")
                            .small()
                            .label(format!("Apply {selected} selected"))
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
                        Button::new("junk-hide")
                            .small()
                            .label(format!("Hide {selected}"))
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
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "Evaluated {} · selected {selected} · {}",
                        self.app.junk_results.len(),
                        self.app.junk_mode.label()
                    )),
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
                uniform_list(
                    "junk-rows",
                    n,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        let visible: Vec<_> = this
                            .app
                            .junk_results
                            .iter()
                            .filter(|d| this.app.junk_show_all_evaluated || d.is_junk)
                            .cloned()
                            .collect();
                        range
                            .filter_map(|ix| visible.get(ix).cloned())
                            .map(|d| {
                                let entity = cx.entity();
                                let id = d.app_id;
                                let checked = this.app.junk_selected.contains(&id);
                                h_flex()
                                    .id(("junk-row", id as usize))
                                    .h(px(40.))
                                    .gap_2()
                                    .child(
                                        Checkbox::new(("junk-cb", id as usize))
                                            .checked(checked)
                                            .on_click(move |_, _, cx| {
                                                entity.update(cx, |this, cx| {
                                                    if this.app.junk_selected.contains(&id) {
                                                        this.app.junk_selected.remove(&id);
                                                    } else {
                                                        this.app.junk_selected.insert(id);
                                                    }
                                                    cx.notify();
                                                });
                                            }),
                                    )
                                    .child(div().w(px(72.)).text_xs().child(id.to_string()))
                                    .child(div().flex_1().text_sm().child(d.name.clone()))
                                    .child(div().w(px(64.)).text_xs().child(if d.is_junk {
                                        "Junk"
                                    } else {
                                        "Keep"
                                    }))
                                    .child(
                                        div()
                                            .w(px(80.))
                                            .text_xs()
                                            .child(format!("{:.0}%", d.confidence * 100.0)),
                                    )
                            })
                            .collect()
                    }),
                )
                .flex_1()
            })
    }
}
