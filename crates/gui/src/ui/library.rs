//! Library view: filters, virtualized game rows, insights rail.

use gpui::{
    ClipboardItem, Context, Entity, Hsla, InteractiveElement, IntoElement, ParentElement, Styled,
    div, prelude::*, px, uniform_list,
};
use gpui_component::{
    ActiveTheme, Sizable, StyledExt, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    tab::{Tab, TabBar},
    v_flex,
};
use vapourfly_core::models::Game;

use super::shared::{hx, insight_tile};
use crate::app::{
    ARTWORK_PALETTE, LibraryInsights, LibraryScope, LibrarySort, QuickView, View,
    cycle_proton_filter, format_playtime, game_card_detail, game_primary_badge,
    game_shows_deck_badge, proton_tier_label, relative_time_ago, sort_label,
};

use crate::ui::GuiRoot;

impl GuiRoot {
    pub(crate) fn library(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let ready = self.app.library_ready();
        let games = if ready {
            self.app.filtered_games()
        } else {
            Vec::new()
        };
        let shown: Vec<Game> = games
            .iter()
            .take(self.app.library_visible_count)
            .cloned()
            .collect();
        let total = games.len();
        let all = self
            .app
            .scan_result
            .as_ref()
            .map(|s| s.games.as_slice())
            .unwrap_or(&[]);
        let installed = all.iter().filter(|g| g.installed).count();
        let playtime: u32 = all.iter().map(|g| g.playtime_minutes.unwrap_or(0)).sum();

        v_flex()
            .id("library")
            .flex_1()
            .min_h_0()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        v_flex()
                            .child(div().text_xl().font_semibold().child("Library"))
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(if self.app.loading {
                                        "Scanning…".into()
                                    } else if !ready {
                                        "Preparing library…".into()
                                    } else {
                                        format!(
                                            "{total} shown · {installed} installed · {}",
                                            format_playtime(playtime)
                                        )
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Button::new("refresh").small().label("Refresh").on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.start_scan();
                                        this.arm_poll(cx);
                                        cx.notify();
                                    });
                                }
                            }))
                            .child(Button::new("junk-open").small().label("Junk…").on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.show_junk_panel = true;
                                        cx.notify();
                                    });
                                }
                            })),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(Input::new(&self.search).cleanable(true).small().w(px(280.)))
                    .child(self.scope_tabs(cx)),
            )
            .child(self.library_filters(cx))
            .child(self.quick_chips(cx))
            .child(if ready {
                let insights = self.app.library_insights(&games);
                let rows = shown.len().min(self.app.library_visible_count);
                let entity = entity.clone();
                let list = uniform_list(
                    "library-rows",
                    rows,
                    cx.processor(move |this, range: std::ops::Range<usize>, _, cx| {
                        let games = this.app.filtered_games();
                        let selected = this.app.library_selected_app_id;
                        let border = cx.theme().border.opacity(0.5);
                        let muted = cx.theme().muted_foreground;
                        let stripe = cx.theme().secondary;
                        range
                            .filter_map(|ix| games.get(ix).cloned())
                            .map(|game| {
                                Self::library_row_owned(
                                    entity.clone(),
                                    game,
                                    selected,
                                    border,
                                    muted,
                                    stripe,
                                )
                            })
                            .collect()
                    }),
                )
                .flex_1()
                .min_w_0();
                let rail = self.insights_rail(&insights, cx);
                if self.app.rails_below {
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .gap_3()
                        .child(list)
                        .child(rail)
                        .into_any_element()
                } else {
                    // `h_flex()` centers on the cross axis (`h_flex()` is
                    // `flex().flex_row().items_center()`), which collapses the
                    // uniform_list to zero height: a virtual list measures its
                    // content height as 0, and centered children keep their
                    // content size instead of stretching to the row height.
                    // Use a plain flex row (default cross-axis stretch) so the
                    // virtual list fills the available height.
                    div()
                        .flex()
                        .flex_row()
                        .flex_1()
                        .min_h_0()
                        .gap_4()
                        .child(list)
                        .child(rail)
                        .into_any_element()
                }
            } else {
                div()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child("Preparing hydrated library snapshot…"),
                    )
                    .into_any_element()
            })
            .when(ready && total > self.app.library_visible_count, |this| {
                this.child(
                    Button::new("load-more")
                        .label("Load more")
                        .on_click(move |_, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.app.library_visible_count =
                                    this.app.library_visible_count.saturating_add(48);
                                cx.notify();
                            });
                        }),
                )
            })
    }

    pub(crate) fn library_row_owned(
        entity: Entity<Self>,
        game: Game,
        selected_id: Option<u32>,
        border: Hsla,
        muted: Hsla,
        stripe: Hsla,
    ) -> impl IntoElement {
        let id = game.app_id;
        let name = game.name.clone();
        let selected = selected_id == Some(id);
        let (badge, _, _) = game_primary_badge(&game);
        let detail = game_card_detail(&game);
        let play = format_playtime(game.playtime_minutes.unwrap_or(0));
        let last = game.last_played_unix.map(relative_time_ago);
        let deck = if game_shows_deck_badge(&game) {
            Some("Deck")
        } else {
            None
        };
        // Join only the segments that have a value; raw `None` placeholders in
        // the middle of a dot-separated line read like debug output.
        let mut meta: Vec<String> = vec![badge.into(), play];
        if let Some(deck) = deck {
            meta.push(deck.into());
        }
        if let Some(last) = &last {
            meta.push(last.clone());
        }
        if !detail.is_empty() {
            meta.push(detail);
        }
        let (top, _) = ARTWORK_PALETTE[(id as usize) % ARTWORK_PALETTE.len()];
        h_flex()
            .id(("lib-row", id as usize))
            .h(px(56.))
            .px_2()
            .gap_3()
            .w_full()
            .border_b_1()
            .border_color(border)
            .when(selected, |this| this.bg(stripe))
            .on_mouse_down(gpui::MouseButton::Left, {
                let entity = entity.clone();
                move |_, _, cx| {
                    entity.update(cx, |this, cx| {
                        this.app.library_selected_app_id = Some(id);
                        cx.notify();
                    });
                }
            })
            .child(div().w(px(72.)).h(px(40.)).rounded(px(6.)).bg(hx(top)))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_sm()
                            .font_medium()
                            .overflow_hidden()
                            .whitespace_nowrap()
                            .child(name),
                    )
                    .child(div().text_xs().text_color(muted).child(meta.join(" · "))),
            )
            .child(
                Button::new(("disc", id as usize))
                    .xsmall()
                    .ghost()
                    .label("Similar")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.app.discover_seed = id.to_string();
                                this.app.current_view = View::Discover;
                                this.app.start_discover_generate();
                                this.arm_poll(cx);
                                cx.notify();
                            });
                        }
                    }),
            )
            .child(
                Button::new(("copy", id as usize))
                    .xsmall()
                    .ghost()
                    .label("Copy ID")
                    .on_click(move |_, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(id.to_string()));
                        window.push_notification(format!("Copied {id}"), cx);
                    }),
            )
            .child(
                Button::new(("store", id as usize))
                    .xsmall()
                    .ghost()
                    .label("Store")
                    .on_click(move |_, _, _| {
                        crate::app::open_url_in_browser(&format!(
                            "https://store.steampowered.com/app/{id}"
                        ));
                    }),
            )
    }

    pub(crate) fn scope_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = LibraryScope::all()
            .iter()
            .position(|s| *s == self.app.library_scope)
            .unwrap_or(0);
        let entity = cx.entity();
        TabBar::new("scope")
            .segmented()
            .small()
            .selected_index(selected)
            .on_click(move |ix, _, cx| {
                if let Some(scope) = LibraryScope::all().get(*ix).copied() {
                    entity.update(cx, |this, cx| {
                        this.app.apply_library_scope(scope);
                        match scope {
                            LibraryScope::All => {
                                this.app.filter_installed_only = false;
                                this.app.filter_unplayed_only = false;
                                this.app.filter_not_hidden = false;
                            }
                            LibraryScope::Installed => {
                                this.app.filter_installed_only = true;
                                this.app.filter_unplayed_only = false;
                            }
                            LibraryScope::Unplayed => {
                                this.app.filter_unplayed_only = true;
                                this.app.filter_installed_only = false;
                            }
                            LibraryScope::Hidden => {
                                this.app.filter_installed_only = false;
                                this.app.filter_unplayed_only = false;
                            }
                        }
                        cx.notify();
                    });
                }
            })
            .children(
                LibraryScope::all()
                    .iter()
                    .map(|s| Tab::new().label(s.label())),
            )
    }

    pub(crate) fn library_filters(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(self.sort_cycle(cx))
                    .child(
                        Button::new("sort-dir")
                            .small()
                            .when(self.app.library_sort_desc, |b| b.primary())
                            .label(if self.app.library_sort_desc {
                                "Descending"
                            } else {
                                "Ascending"
                            })
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.library_sort_desc = !this.app.library_sort_desc;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("deck-filter")
                            .small()
                            .when(self.app.filter_deck_compatible, |b| b.primary())
                            .label("Steam Deck")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.filter_deck_compatible =
                                            !this.app.filter_deck_compatible;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("ctrl-filter")
                            .small()
                            .when(self.app.filter_controller_full, |b| b.primary())
                            .label("Full controller")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.filter_controller_full =
                                            !this.app.filter_controller_full;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("junk-ex")
                            .small()
                            .when(self.app.filter_not_junk, |b| b.primary())
                            .label("Hide junk")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.filter_not_junk = !this.app.filter_not_junk;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("hidden-ex")
                            .small()
                            .when(self.app.filter_not_hidden, |b| b.primary())
                            .label("Exclude hidden")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.filter_not_hidden = !this.app.filter_not_hidden;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("proton-filter")
                            .small()
                            .when(self.app.filter_proton_tier.is_some(), |b| b.primary())
                            .label(format!(
                                "Proton {}",
                                self.app
                                    .filter_proton_tier
                                    .map(proton_tier_label)
                                    .unwrap_or("Any")
                            ))
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.filter_proton_tier =
                                            cycle_proton_filter(this.app.filter_proton_tier);
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Genre"),
                    )
                    .child(
                        Input::new(&self.genre_input)
                            .cleanable(true)
                            .small()
                            .w(px(140.)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Tag"),
                    )
                    .child(
                        Input::new(&self.tag_input)
                            .cleanable(true)
                            .small()
                            .w(px(140.)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("Playtime"),
                    )
                    .child(Input::new(&self.playtime_min_input).small().w(px(64.)))
                    .child(div().text_xs().child("–"))
                    .child(Input::new(&self.playtime_max_input).small().w(px(64.)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("HLTB"),
                    )
                    .child(Input::new(&self.hltb_min_input).small().w(px(64.)))
                    .child(div().text_xs().child("–"))
                    .child(Input::new(&self.hltb_max_input).small().w(px(64.))),
            )
    }

    pub(crate) fn sort_cycle(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        Button::new("sort")
            .small()
            .label(format!(
                "Sort: {}{}",
                sort_label(self.app.library_sort_by),
                if self.app.library_sort_desc {
                    " ↓"
                } else {
                    " ↑"
                }
            ))
            .on_click(move |_, _, cx| {
                entity.update(cx, |this, cx| {
                    this.app.library_sort_by = match this.app.library_sort_by {
                        LibrarySort::InstalledThenPlaytime => LibrarySort::Name,
                        LibrarySort::Name => LibrarySort::Playtime,
                        LibrarySort::Playtime => LibrarySort::Hltb,
                        LibrarySort::Hltb => LibrarySort::Rating,
                        LibrarySort::Rating => LibrarySort::AppId,
                        LibrarySort::AppId => LibrarySort::InstalledThenPlaytime,
                    };
                    cx.notify();
                });
            })
    }

    pub(crate) fn quick_chips(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let current = self.app.library_quick_view;
        h_flex().gap_1().children(
            [
                QuickView::All,
                QuickView::Cozy,
                QuickView::StoryRich,
                QuickView::GreatOnDeck,
                QuickView::ShortSessions,
            ]
            .into_iter()
            .map(move |qv| {
                let entity = entity.clone();
                Button::new(qv.label())
                    .small()
                    .when(current == qv, |b| b.primary())
                    .label(qv.label())
                    .on_click(move |_, window, cx| {
                        entity.update(cx, |this, cx| {
                            this.app.apply_quick_view(qv);
                            this.sync_filter_inputs(window, cx);
                            cx.notify();
                        });
                    })
            }),
        )
    }

    pub(crate) fn insights_rail(
        &self,
        insights: &LibraryInsights,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let entity = cx.entity();
        let frac = if insights.matching == 0 {
            0.0
        } else {
            insights.backlog as f32 / insights.matching as f32
        };
        let bar_w = (160.0 * frac).clamp(0.0, 160.0);
        v_flex()
            .id("insights")
            .w(px(220.))
            .min_w(px(220.))
            .gap_2()
            .p_3()
            .rounded(cx.theme().radius_lg)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().secondary)
            .child(div().text_sm().font_semibold().child("Library insights"))
            .child(insight_tile("Total games", insights.total.to_string(), cx))
            .child(insight_tile(
                "Installed",
                insights.installed.to_string(),
                cx,
            ))
            .child(insight_tile(
                "Estimated playtime",
                format_playtime(insights.playtime),
                cx,
            ))
            .child(insight_tile("Junk excluded", insights.junk.to_string(), cx))
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(div().text_xs().child("Backlog"))
                            .child(
                                div()
                                    .text_xs()
                                    .font_semibold()
                                    .child(insights.backlog.to_string()),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .h(px(6.))
                                    .w(px(160.))
                                    .rounded(px(3.))
                                    .bg(cx.theme().muted)
                                    .child(
                                        div()
                                            .h_full()
                                            .w(px(bar_w))
                                            .rounded(px(3.))
                                            .bg(cx.theme().primary),
                                    ),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{:.0}%", frac * 100.0)),
                            ),
                    ),
            )
            .child(div().text_xs().font_semibold().child("Recent activity"))
            .child(if insights.recent.is_empty() {
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("No recent activity")
                    .into_any_element()
            } else {
                v_flex()
                    .gap_1()
                    .children(insights.recent.iter().map(|(id, name, unix, play)| {
                        h_flex()
                            .gap_2()
                            .child(div().w(px(32.)).h(px(32.)).rounded(px(4.)).bg(hx(
                                ARTWORK_PALETTE[(*id as usize) % ARTWORK_PALETTE.len()].0,
                            )))
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_xs()
                                            .font_medium()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .child(name.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(format!(
                                                "{} · {}",
                                                relative_time_ago(*unix),
                                                format_playtime(*play)
                                            )),
                                    ),
                            )
                    }))
                    .child(
                        Button::new("view-history")
                            .xsmall()
                            .ghost()
                            .label("View full history")
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.app.library_sort_by = LibrarySort::InstalledThenPlaytime;
                                    this.app.library_sort_desc = false;
                                    cx.notify();
                                });
                            }),
                    )
                    .into_any_element()
            })
            .child(insight_tile("Hidden", insights.hidden.to_string(), cx))
            .when(insights.avg_hltb_minutes > 0, |this| {
                this.child(insight_tile(
                    "Avg HLTB",
                    format!("{}h", insights.avg_hltb_minutes / 60),
                    cx,
                ))
            })
    }
}
