//! Playlist rule editor.
//!
//! Every adder lives behind the single "Add rule…" dropdown; the controls
//! below it only stage values (genre/tag presets, proton tier, numeric
//! nudges) that the menu entries then turn into [`PlaylistRule`]s.

use gpui::{
    Action, Context, Entity, InteractiveElement as _, IntoElement, ParentElement, SharedString,
    Styled, div, prelude::*,
};
use gpui_component::{
    ActiveTheme, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    v_flex,
};
use vapourfly_core::models::{PlaylistRule, ProtonTier};

use crate::app::{VapourflyApp, empty_value_label};

use crate::ui::GuiRoot;
use crate::ui::shared::{empty_or, this_tier_label};

/// Menu command: append a playlist rule of the given kind.
///
/// Dispatched by the "Add rule…" dropdown items and handled on the rules
/// container; the payload slug is matched in
/// [`GuiRoot::append_staged_rule`].
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = vapourfly, no_json)]
pub(crate) struct AddRuleKind(pub &'static str);

impl GuiRoot {
    pub(crate) fn playlist_rules(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let rules = self.app.parse_current_rules().unwrap_or_default();
        v_flex()
            .gap_2()
            .on_action(cx.listener(|this, action: &AddRuleKind, _, cx| {
                this.append_staged_rule(action.0, cx);
            }))
            .child(self.add_rule_menu_button(entity.clone()))
            .children(rules.iter().enumerate().map(|(i, rule)| {
                let entity = entity.clone();
                h_flex()
                    .gap_2()
                    .child(div().text_sm().child(crate::app::rule_label(rule)))
                    .child(
                        Button::new(("pl.rule-remove", i))
                            .xsmall()
                            .ghost()
                            .icon(IconName::Delete)
                            .tooltip("Remove rule")
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
            .child(self.advanced_json_disclosure(entity.clone()))
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

    /// The consolidated "Add rule…" entry point for every rule kind.
    fn add_rule_menu_button(&self, entity: Entity<Self>) -> impl IntoElement {
        Button::new("pl.add-rule")
            .small()
            .icon(IconName::Plus)
            .label("Add rule")
            .tooltip("Add rule")
            .dropdown_menu(move |menu, _, cx| build_add_rule_menu(menu, &entity.read(cx).app))
    }

    /// Applies an "Add rule…" menu choice. Each arm carries the exact body
    /// of the inline add button it replaces.
    fn append_staged_rule(&mut self, kind: &str, cx: &mut Context<Self>) {
        let rule: Option<PlaylistRule> = match kind {
            "installed" => Some(PlaylistRule::Installed),
            "not-hidden" => Some(PlaylistRule::NotHidden),
            "not-junk" => Some(PlaylistRule::NotJunk),
            "controller" => Some(PlaylistRule::ControllerSupportFull),
            "genre" => {
                let g = self.app.playlist_rule_genre.clone();
                (!g.is_empty()).then_some(PlaylistRule::HasGenre { genre: g })
            }
            "tag" => {
                let t = self.app.playlist_rule_tag.clone();
                (!t.is_empty()).then_some(PlaylistRule::HasTag { tag: t })
            }
            "hltb" => self
                .app
                .playlist_rule_hltb_max
                .parse::<u32>()
                .ok()
                .map(|minutes| PlaylistRule::HltbMaxMinutes { minutes }),
            "proton" => self
                .app
                .playlist_rule_proton_tier
                .map(|tier| PlaylistRule::ProtonAtLeast { tier }),
            "playtime" => {
                let min = self
                    .app
                    .playlist_rule_playtime_min
                    .parse::<u32>()
                    .unwrap_or(0);
                let max = self
                    .app
                    .playlist_rule_playtime_max
                    .parse::<u32>()
                    .unwrap_or(0);
                if min <= max {
                    Some(PlaylistRule::PlaytimeBetween { min, max })
                } else {
                    self.app.error = Some("Playtime min must be ≤ max.".into());
                    None
                }
            }
            "rating" => self
                .app
                .playlist_rule_rating_min
                .parse::<f32>()
                .ok()
                .filter(|r| (0.0..=5.0).contains(r))
                .map(|rating_0_5| PlaylistRule::RatingAtLeast { rating_0_5 }),
            _ => None,
        };
        if let Some(rule) = rule
            && let Err(e) = self.app.append_rule_to_json(rule)
        {
            self.app.error = Some(e);
        }
        cx.notify();
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
                                Button::new(SharedString::from(format!("pl.genre-preset-{g}")))
                                    .xsmall()
                                    .when(self.app.playlist_rule_genre == g, |b| b.primary())
                                    .label(g)
                                    .on_click(move |_, _, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.app.playlist_rule_genre = g.into();
                                            cx.notify();
                                        });
                                    })
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
                        Button::new(SharedString::from(format!("pl.tag-preset-{t}")))
                            .xsmall()
                            .when(self.app.playlist_rule_tag == t, |b| b.primary())
                            .label(t)
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.app.playlist_rule_tag = t.into();
                                    cx.notify();
                                });
                            })
                    })),
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
                    .child(
                        h_flex()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded_sm()
                            .overflow_hidden()
                            .child(Self::nudge_str_btn(
                                entity.clone(),
                                "pl.hltb-dec",
                                "−15",
                                "Decrease",
                                |app| {
                                    let n = app.playlist_rule_hltb_max.parse::<u32>().unwrap_or(60);
                                    app.playlist_rule_hltb_max =
                                        n.saturating_sub(15).max(15).to_string();
                                },
                            ))
                            .child(Self::nudge_str_btn(
                                entity.clone(),
                                "pl.hltb-inc",
                                "+15",
                                "Increase",
                                |app| {
                                    let n = app.playlist_rule_hltb_max.parse::<u32>().unwrap_or(60);
                                    app.playlist_rule_hltb_max = (n + 15).to_string();
                                },
                            )),
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
                            Button::new(SharedString::from(format!("pl.proton-preset-{tier:?}")))
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
                    .child(
                        h_flex()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded_sm()
                            .overflow_hidden()
                            .child(Self::nudge_str_btn(
                                entity.clone(),
                                "pl.playtime-min-inc",
                                "min+10",
                                "Increase minimum",
                                |app| {
                                    let n =
                                        app.playlist_rule_playtime_min.parse::<u32>().unwrap_or(0);
                                    app.playlist_rule_playtime_min = (n + 10).to_string();
                                },
                            ))
                            .child(Self::nudge_str_btn(
                                entity.clone(),
                                "pl.playtime-max-inc",
                                "max+30",
                                "Increase maximum",
                                |app| {
                                    let n =
                                        app.playlist_rule_playtime_max.parse::<u32>().unwrap_or(60);
                                    app.playlist_rule_playtime_max = (n + 30).to_string();
                                },
                            )),
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
                        "pl.rating-inc",
                        "+0.5",
                        "Increase",
                        |app| {
                            let n = app.playlist_rule_rating_min.parse::<f32>().unwrap_or(0.0);
                            app.playlist_rule_rating_min = ((n + 0.5).clamp(0.0, 5.0)).to_string();
                        },
                    )),
            )
    }

    /// Disclosure toggle for the raw rules JSON preview.
    fn advanced_json_disclosure(&self, entity: Entity<Self>) -> impl IntoElement {
        let open = self.app.playlist_show_advanced_json;
        Button::new("pl.adv-json")
            .small()
            .ghost()
            .icon(if open {
                IconName::ChevronUp
            } else {
                IconName::ChevronDown
            })
            .label("Advanced JSON")
            .tooltip(if open {
                "Hide advanced JSON"
            } else {
                "Show advanced JSON"
            })
            .on_click(move |_, _, cx| {
                entity.update(cx, |this, cx| {
                    this.app.playlist_show_advanced_json = !this.app.playlist_show_advanced_json;
                    cx.notify();
                });
            })
    }

    pub(crate) fn nudge_str_btn(
        entity: Entity<Self>,
        id: &'static str,
        label: &'static str,
        tooltip: &'static str,
        f: impl Fn(&mut VapourflyApp) + 'static,
    ) -> impl IntoElement {
        Button::new(id)
            .xsmall()
            .ghost()
            .label(label)
            .tooltip(tooltip)
            .on_click(move |_, _, cx| {
                entity.update(cx, |this, cx| {
                    f(&mut this.app);
                    cx.notify();
                });
            })
    }
}

/// Builds the grouped "Add rule…" menu entries.
///
/// Quick rules append directly; parameterized entries consume the staged
/// values edited alongside and are disabled while their value is unset.
/// Disabled states mirror each arm's guard in
/// [`GuiRoot::append_staged_rule`], which stays the single source of truth.
fn build_add_rule_menu(menu: PopupMenu, app: &VapourflyApp) -> PopupMenu {
    let genre = app.playlist_rule_genre.as_str();
    let tag = app.playlist_rule_tag.as_str();
    let hltb = app.playlist_rule_hltb_max.parse::<u32>().ok();
    let tier = app.playlist_rule_proton_tier;
    let rating = app
        .playlist_rule_rating_min
        .parse::<f32>()
        .ok()
        .filter(|r| (0.0..=5.0).contains(r));
    menu.item(PopupMenuItem::label("Quick rules"))
        .menu("Installed", Box::new(AddRuleKind("installed")))
        .menu("Not hidden", Box::new(AddRuleKind("not-hidden")))
        .menu("Not junk", Box::new(AddRuleKind("not-junk")))
        .menu("Full controller", Box::new(AddRuleKind("controller")))
        .separator()
        .item(PopupMenuItem::label("From staged values"))
        .menu_with_disabled(
            if genre.is_empty() {
                "Add genre".to_string()
            } else {
                format!("Add genre '{genre}'")
            },
            Box::new(AddRuleKind("genre")),
            genre.is_empty(),
        )
        .menu_with_disabled(
            if tag.is_empty() {
                "Add tag".to_string()
            } else {
                format!("Add tag '{tag}'")
            },
            Box::new(AddRuleKind("tag")),
            tag.is_empty(),
        )
        .menu_with_disabled(
            match hltb {
                Some(m) => format!("Add HLTB max {m}m"),
                None => "Add HLTB max".to_string(),
            },
            Box::new(AddRuleKind("hltb")),
            hltb.is_none(),
        )
        .menu_with_disabled(
            format!("Add Proton ≥ {}", this_tier_label(tier)),
            Box::new(AddRuleKind("proton")),
            tier.is_none(),
        )
        .menu(
            format!(
                "Add playtime {}–{}",
                empty_or(&app.playlist_rule_playtime_min),
                empty_or(&app.playlist_rule_playtime_max)
            ),
            Box::new(AddRuleKind("playtime")),
        )
        .menu_with_disabled(
            match rating {
                Some(r) => format!("Add rating ≥ {r}"),
                None => "Add rating".to_string(),
            },
            Box::new(AddRuleKind("rating")),
            rating.is_none(),
        )
}
