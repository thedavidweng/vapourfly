//! Session-planner recommendation view.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Styled, div, prelude::*, px,
    uniform_list,
};
use gpui_component::{
    ActiveTheme, Disableable, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use vapourfly_core::models::JunkMode;

use crate::app::{GameSummary, PendingAction, format_playtime, proton_tier_label};

use crate::ui::GuiRoot;

impl GuiRoot {
    pub(crate) fn recommend(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        v_flex()
            .id("recommend")
            .size_full()
            .gap_3()
            .child(div().text_xl().font_semibold().child("Recommendations"))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .child(format!("Minutes {}", self.app.recommend_minutes)),
                    )
                    .child(Button::new("rec-m-minus").xsmall().label("−30").on_click({
                        let entity = entity.clone();
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| {
                                let n = this.app.recommend_minutes.parse::<u32>().unwrap_or(120);
                                this.app.recommend_minutes =
                                    n.saturating_sub(30).max(15).to_string();
                                cx.notify();
                            });
                        }
                    }))
                    .child(Button::new("rec-m-plus").xsmall().label("+30").on_click({
                        let entity = entity.clone();
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| {
                                let n = this.app.recommend_minutes.parse::<u32>().unwrap_or(120);
                                this.app.recommend_minutes = (n + 30).to_string();
                                cx.notify();
                            });
                        }
                    }))
                    .child(
                        div()
                            .text_sm()
                            .child(format!("Count {}", self.app.recommend_count)),
                    )
                    .child(
                        Button::new("rec-deck")
                            .small()
                            .when(self.app.recommend_deck, |b| b.primary())
                            .label("Deck")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.recommend_deck = !this.app.recommend_deck;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("rec-inst")
                            .small()
                            .when(self.app.recommend_installed_only, |b| b.primary())
                            .label("Installed only")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.recommend_installed_only =
                                            !this.app.recommend_installed_only;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("rec-go")
                            .primary()
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
                            .label("Save as Steam collection")
                            .disabled(self.app.recommend_results.is_empty() || self.app.ui_demo)
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.start_dry_run(PendingAction::RecommendCollection);
                                        this.arm_poll(cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child({
                let top: Vec<_> = self.app.recommend_results.iter().take(3).cloned().collect();
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
                        // show it as points. Metadata segments join only when
                        // present so no raw `None` placeholders appear.
                        let mut meta: Vec<String> = vec![
                            format!("{:.1} pts", rec.score),
                            format_playtime(summary.playtime_minutes),
                        ];
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
                            .id(("rec-top", rec.app_id as usize))
                            .w(px(220.))
                            .p_2()
                            .gap_1()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(cx.theme().radius)
                            .child(self.placeholder(rec.app_id, 124.))
                            .child(div().text_xs().child(format!("#{}", i + 1)))
                            .child(div().text_sm().font_medium().child(rec.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(meta.join(" · ")),
                            )
                    }))
            })
            .child({
                let n = self.app.recommend_results.len().saturating_sub(3);
                uniform_list(
                    "rec-rest",
                    n,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        range
                            .filter_map(|ix| this.app.recommend_results.get(ix + 3).cloned())
                            .map(|rec| {
                                h_flex()
                                    .id(("rec-row", rec.app_id as usize))
                                    .h(px(36.))
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(px(48.))
                                            .text_xs()
                                            .child(format!("{:.1}", rec.score)),
                                    )
                                    .child(div().flex_1().text_sm().child(rec.name.clone()))
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(
                                                rec.reasons
                                                    .first()
                                                    .map_or("", |r| r.description.as_str())
                                                    .to_string(),
                                            ),
                                    )
                            })
                            .collect()
                    }),
                )
                .flex_1()
            })
    }
}
