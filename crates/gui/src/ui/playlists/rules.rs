//! Playlist rule editor.

use gpui::{Context, Entity, IntoElement, ParentElement, SharedString, Styled, div, prelude::*};
use gpui_component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use vapourfly_core::models::{PlaylistRule, ProtonTier};

use crate::app::{VapourflyApp, empty_value_label};

use crate::ui::GuiRoot;
use crate::ui::shared::{empty_or, this_tier_label};

impl GuiRoot {
    pub(crate) fn playlist_rules(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let rules = self.app.parse_current_rules().unwrap_or_default();
        v_flex()
            .gap_2()
            .child(
                h_flex().gap_1().children(
                    [
                        ("Installed", PlaylistRule::Installed),
                        ("Not hidden", PlaylistRule::NotHidden),
                        ("Not junk", PlaylistRule::NotJunk),
                        ("Full controller", PlaylistRule::ControllerSupportFull),
                    ]
                    .into_iter()
                    .map(|(label, rule)| {
                        let entity = entity.clone();
                        Button::new(label)
                            .xsmall()
                            .label(label)
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    if let Err(e) = this.app.append_rule_to_json(rule.clone()) {
                                        this.app.error = Some(e);
                                    }
                                    cx.notify();
                                });
                            })
                    }),
                ),
            )
            .children(rules.iter().enumerate().map(|(i, rule)| {
                let entity = entity.clone();
                h_flex()
                    .gap_2()
                    .child(div().text_sm().child(crate::app::rule_label(rule)))
                    .child(
                        Button::new(("rm-rule", i))
                            .xsmall()
                            .ghost()
                            .label("Remove")
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    if let Ok(mut rs) = this.app.parse_current_rules() {
                                        if i < rs.len() {
                                            rs.remove(i);
                                            this.app.playlist_edit_rules =
                                                serde_json::to_string_pretty(&rs)
                                                    .unwrap_or_default();
                                        }
                                    }
                                    cx.notify();
                                });
                            }),
                    )
            }))
            .child(self.parameterized_rules(cx))
            .child(
                Button::new("pl-adv-json")
                    .small()
                    .when(self.app.playlist_show_advanced_json, |b| b.primary())
                    .label("Advanced JSON")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.app.playlist_show_advanced_json =
                                    !this.app.playlist_show_advanced_json;
                                cx.notify();
                            });
                        }
                    }),
            )
            .when(self.app.playlist_show_advanced_json, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(if self.app.playlist_edit_rules.trim().is_empty() {
                            empty_value_label().to_string()
                        } else {
                            self.app.playlist_edit_rules.clone()
                        }),
                )
            })
    }

    pub(crate) fn parameterized_rules(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_1()
                    .child(div().text_xs().child(format!(
                        "Genre {}",
                        if self.app.playlist_rule_genre.is_empty() {
                            empty_value_label()
                        } else {
                            self.app.playlist_rule_genre.as_str()
                        }
                    )))
                    .children(
                        ["Cozy", "Story Rich", "Action", "Shooter"]
                            .into_iter()
                            .map(|g| {
                                let entity = entity.clone();
                                Button::new(SharedString::from(format!("genre-{g}")))
                                    .xsmall()
                                    .label(g)
                                    .on_click(move |_, _, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.app.playlist_rule_genre = g.into();
                                            cx.notify();
                                        });
                                    })
                            }),
                    )
                    .child(
                        Button::new("add-genre")
                            .xsmall()
                            .primary()
                            .label("Add genre")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        let g = this.app.playlist_rule_genre.clone();
                                        if !g.is_empty() {
                                            let _ = this.app.append_rule_to_json(
                                                PlaylistRule::HasGenre { genre: g },
                                            );
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(div().text_xs().child(format!(
                        "Tag {}",
                        if self.app.playlist_rule_tag.is_empty() {
                            empty_value_label()
                        } else {
                            self.app.playlist_rule_tag.as_str()
                        }
                    )))
                    .children(["cozy", "multiplayer", "story"].into_iter().map(|t| {
                        let entity = entity.clone();
                        Button::new(SharedString::from(format!("tag-{t}")))
                            .xsmall()
                            .label(t)
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.app.playlist_rule_tag = t.into();
                                    cx.notify();
                                });
                            })
                    }))
                    .child(
                        Button::new("add-tag")
                            .xsmall()
                            .primary()
                            .label("Add tag")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        let t = this.app.playlist_rule_tag.clone();
                                        if !t.is_empty() {
                                            let _ = this.app.append_rule_to_json(
                                                PlaylistRule::HasTag { tag: t },
                                            );
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(div().text_xs().child(format!(
                        "HLTB max {}m",
                        if self.app.playlist_rule_hltb_max.is_empty() {
                            empty_value_label().to_string()
                        } else {
                            self.app.playlist_rule_hltb_max.clone()
                        }
                    )))
                    .child(Self::nudge_str_btn(
                        entity.clone(),
                        "hltb-minus",
                        "−15",
                        |app| {
                            let n = app.playlist_rule_hltb_max.parse::<u32>().unwrap_or(60);
                            app.playlist_rule_hltb_max = n.saturating_sub(15).max(15).to_string();
                        },
                    ))
                    .child(Self::nudge_str_btn(
                        entity.clone(),
                        "hltb-plus",
                        "+15",
                        |app| {
                            let n = app.playlist_rule_hltb_max.parse::<u32>().unwrap_or(60);
                            app.playlist_rule_hltb_max = (n + 15).to_string();
                        },
                    ))
                    .child(
                        Button::new("add-hltb")
                            .xsmall()
                            .primary()
                            .label("Add HLTB")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        if let Ok(m) =
                                            this.app.playlist_rule_hltb_max.parse::<u32>()
                                        {
                                            let _ = this.app.append_rule_to_json(
                                                PlaylistRule::HltbMaxMinutes { minutes: m },
                                            );
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(div().text_xs().child(format!(
                        "Proton {}",
                        this_tier_label(self.app.playlist_rule_proton_tier)
                    )))
                    .children(
                        [
                            ProtonTier::Bronze,
                            ProtonTier::Silver,
                            ProtonTier::Gold,
                            ProtonTier::Platinum,
                            ProtonTier::Native,
                        ]
                        .into_iter()
                        .map(|tier| {
                            let entity = entity.clone();
                            Button::new(SharedString::from(format!("pt-{tier:?}")))
                                .xsmall()
                                .when(self.app.playlist_rule_proton_tier == Some(tier), |b| {
                                    b.primary()
                                })
                                .label(format!("{tier:?}"))
                                .on_click(move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.playlist_rule_proton_tier = Some(tier);
                                        cx.notify();
                                    });
                                })
                        }),
                    )
                    .child(
                        Button::new("add-proton")
                            .xsmall()
                            .primary()
                            .label("Add Proton")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        if let Some(tier) = this.app.playlist_rule_proton_tier {
                                            let _ = this.app.append_rule_to_json(
                                                PlaylistRule::ProtonAtLeast { tier },
                                            );
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(div().text_xs().child(format!(
                        "Playtime {}–{}",
                        empty_or(&self.app.playlist_rule_playtime_min),
                        empty_or(&self.app.playlist_rule_playtime_max)
                    )))
                    .child(Self::nudge_str_btn(
                        entity.clone(),
                        "ptmin+",
                        "min+10",
                        |app| {
                            let n = app.playlist_rule_playtime_min.parse::<u32>().unwrap_or(0);
                            app.playlist_rule_playtime_min = (n + 10).to_string();
                        },
                    ))
                    .child(Self::nudge_str_btn(
                        entity.clone(),
                        "ptmax+",
                        "max+30",
                        |app| {
                            let n = app.playlist_rule_playtime_max.parse::<u32>().unwrap_or(60);
                            app.playlist_rule_playtime_max = (n + 30).to_string();
                        },
                    ))
                    .child(
                        Button::new("add-playtime")
                            .xsmall()
                            .primary()
                            .label("Add playtime")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        let min = this
                                            .app
                                            .playlist_rule_playtime_min
                                            .parse::<u32>()
                                            .unwrap_or(0);
                                        let max = this
                                            .app
                                            .playlist_rule_playtime_max
                                            .parse::<u32>()
                                            .unwrap_or(0);
                                        if min <= max {
                                            let _ = this.app.append_rule_to_json(
                                                PlaylistRule::PlaytimeBetween { min, max },
                                            );
                                        } else {
                                            this.app.error =
                                                Some("Playtime min must be ≤ max.".into());
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(div().text_xs().child(format!(
                        "Rating ≥ {}",
                        empty_or(&self.app.playlist_rule_rating_min)
                    )))
                    .child(Self::nudge_str_btn(
                        entity.clone(),
                        "rate+",
                        "+0.5",
                        |app| {
                            let n = app.playlist_rule_rating_min.parse::<f32>().unwrap_or(0.0);
                            app.playlist_rule_rating_min = ((n + 0.5).clamp(0.0, 5.0)).to_string();
                        },
                    ))
                    .child(
                        Button::new("add-rating")
                            .xsmall()
                            .primary()
                            .label("Add rating")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        if let Ok(r) =
                                            this.app.playlist_rule_rating_min.parse::<f32>()
                                        {
                                            if (0.0..=5.0).contains(&r) {
                                                let _ = this.app.append_rule_to_json(
                                                    PlaylistRule::RatingAtLeast { rating_0_5: r },
                                                );
                                            }
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
    }

    pub(crate) fn nudge_str_btn(
        entity: Entity<Self>,
        id: &'static str,
        label: &'static str,
        f: impl Fn(&mut VapourflyApp) + 'static,
    ) -> impl IntoElement {
        Button::new(id)
            .xsmall()
            .label(label)
            .on_click(move |_, _, cx| {
                entity.update(cx, |this, cx| {
                    f(&mut this.app);
                    cx.notify();
                });
            })
    }
}
