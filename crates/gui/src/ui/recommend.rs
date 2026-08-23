//! Session-planner recommendation view.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, prelude::*,
    px, uniform_list,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex,
    switch::Switch,
    tag::Tag,
    v_flex,
};
use vapourfly_core::models::JunkMode;

use super::dialogs::{AlertSpec, open_alert};
use super::motion::{DUR_BASE, rise_in};
use crate::app::{
    GameSummary, PendingAction, format_playtime, proton_tier_label, reason_badge_label,
};

use crate::ui::GuiRoot;

impl GuiRoot {
    pub(crate) fn recommend(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let muted = cx.theme().muted_foreground;
        v_flex()
            .id("recommend")
            .size_full()
            .gap_3()
            .child(div().text_xl().font_semibold().child("Recommendations"))
            // Session planner row; control ids are unchanged.
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .child(format!("Minutes {}", self.app.recommend_minutes)),
                    )
                    .child(
                        Button::new("rec-m-minus")
                            .xsmall()
                            .icon(Icon::new(IconName::Minus))
                            .label("−30")
                            .tooltip("Decrease session length")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        let n = this
                                            .app
                                            .recommend_minutes
                                            .parse::<u32>()
                                            .unwrap_or(120);
                                        this.app.recommend_minutes =
                                            n.saturating_sub(30).max(15).to_string();
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("rec-m-plus")
                            .xsmall()
                            .icon(Icon::new(IconName::Plus))
                            .label("+30")
                            .tooltip("Increase session length")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        let n = this
                                            .app
                                            .recommend_minutes
                                            .parse::<u32>()
                                            .unwrap_or(120);
                                        this.app.recommend_minutes = (n + 30).to_string();
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        div()
                            .text_sm()
                            .child(format!("Count {}", self.app.recommend_count)),
                    )
                    .child(
                        Switch::new("recommend.deck-switch")
                            .checked(self.app.recommend_deck)
                            .label("Great on Deck only")
                            .on_click({
                                let entity = entity.clone();
                                move |checked: &bool, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.recommend_deck = *checked;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Switch::new("recommend.installed-switch")
                            .checked(self.app.recommend_installed_only)
                            .label("Installed only")
                            .on_click({
                                let entity = entity.clone();
                                move |checked: &bool, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.recommend_installed_only = *checked;
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            // Action row.
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("rec-go")
                            .primary()
                            .when(self.app.recommend_loading, |b| {
                                b.icon(Icon::new(IconName::LoaderCircle))
                            })
                            .label(if self.app.recommend_loading {
                                "Scoring…"
                            } else {
                                "Generate"
                            })
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.start_recommend_preview();
                                        this.arm_poll(cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("rec-save")
                            .small()
                            .label("Save as Steam collection…")
                            .disabled(self.app.recommend_results.is_empty() || self.app.ui_demo)
                            .on_click({
                                let entity = entity.clone();
                                move |_, window, cx| {
                                    entity.update(cx, |this, cx| {
                                        // Demo mode never writes: gate before
                                        // any dialog opens.
                                        if this.app.ui_demo {
                                            return;
                                        }
                                        let n = this.app.recommend_results.len();
                                        open_alert(
                                            &entity,
                                            window,
                                            cx,
                                            AlertSpec {
                                                title:
                                                    "Save recommendations as a Steam collection?"
                                                        .into(),
                                                lines: vec![
                                                    format!("{n} games from the current preview"),
                                                    "The dry-run diff opens next for final \
                                                     confirmation."
                                                        .into(),
                                                ],
                                                verb: "Save".into(),
                                            },
                                            move |this, cx| {
                                                this.app.start_dry_run(
                                                    PendingAction::RecommendCollection,
                                                );
                                                this.arm_poll(cx);
                                                cx.notify();
                                            },
                                        );
                                    });
                                }
                            }),
                    ),
            )
            .child(if self.app.recommend_results.is_empty() {
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .text_color(muted)
                    .child(Icon::new(IconName::Inbox))
                    .child(div().text_sm().child("No recommendations yet"))
                    .child(
                        div()
                            .text_xs()
                            .child("Set a session length and press Generate to plan one."),
                    )
                    .into_any_element()
            } else {
                let top: Vec<_> = self.app.recommend_results.iter().take(3).cloned().collect();
                let top_row =
                    h_flex()
                        .gap_3()
                        .children(top.into_iter().enumerate().map(|(i, rec)| {
                            let summary = self
                                .app
                                .prepared_games(JunkMode::Default)
                                .as_ref()
                                .and_then(|games| games.iter().find(|g| g.app_id == rec.app_id))
                                .map(GameSummary::from)
                                .unwrap_or_default();
                            // Composite score is a weight sum, not a percentage;
                            // show it as points on the card header, right-aligned
                            // next to the rank. Metadata segments join only when
                            // present so no raw `None` placeholders appear.
                            let mut meta: Vec<String> =
                                vec![format_playtime(summary.playtime_minutes)];
                            if let Some(hltb) = summary.hltb_minutes {
                                meta.push(format_playtime(hltb));
                            }
                            if let Some(rating) = summary.rating_0_5 {
                                meta.push(format!("{rating:.1}/5"));
                            }
                            if let Some(tier) = summary.proton_tier {
                                meta.push(proton_tier_label(tier).to_string());
                            }
                            v_flex()
                                .id(("recommend.top", rec.app_id as usize))
                                .w(px(220.))
                                .p_2()
                                .gap_1()
                                .border_1()
                                .border_color(cx.theme().border)
                                .rounded(cx.theme().radius)
                                .child(self.placeholder(rec.app_id, 124.))
                                .child(
                                    h_flex()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_xs()
                                                .text_color(muted)
                                                .child(format!("#{}", i + 1)),
                                        )
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_semibold()
                                                .child(format!("{:.1} pts", rec.score)),
                                        ),
                                )
                                .child(div().text_sm().font_medium().child(rec.name.clone()))
                                .child(div().text_xs().text_color(muted).child(meta.join(" · ")))
                        }));
                let n = self.app.recommend_results.len().saturating_sub(3);
                let list = uniform_list(
                    "recommend.rows",
                    n,
                    cx.processor(|this, range: std::ops::Range<usize>, _, _cx| {
                        range
                            .filter_map(|ix| this.app.recommend_results.get(ix + 3).cloned())
                            .map(|rec| {
                                h_flex()
                                    .id(("recommend.row", rec.app_id as usize))
                                    .h(px(36.))
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(px(56.))
                                            .text_xs()
                                            .text_right()
                                            .child(format!("{:.1} pts", rec.score)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_sm()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .child(rec.name.clone()),
                                    )
                                    // "Why this pick?" pill: the dominant
                                    // reason for the pick, when present.
                                    .when_some(rec.reasons.first(), |row, r| {
                                        row.child(Tag::secondary().small().child(
                                            SharedString::from(reason_badge_label(
                                                &r.code,
                                                &r.description,
                                            )),
                                        ))
                                    })
                            })
                            .collect()
                    }),
                )
                .flex_1();
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    // One material-arriving settle for the whole top-3 row;
                    // never per-row inside long lists.
                    .child(rise_in(top_row, "rec-top-rise", DUR_BASE, cx))
                    .child(list)
                    .into_any_element()
            })
    }
}
