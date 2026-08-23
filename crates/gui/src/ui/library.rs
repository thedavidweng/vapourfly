//! Library view: filters, virtualized game rows, insights rail.

use gpui::{
    Action, App, ClipboardItem, Context, ElementId, Entity, Hsla, InteractiveElement, IntoElement,
    ParentElement, Styled, Window, div, prelude::*, px, uniform_list,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, Selectable, Sizable, StyledExt, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    menu::{ContextMenuExt, DropdownMenu},
    notification::Notification,
    select::Select,
    tab::{Tab, TabBar},
    v_flex,
};
use vapourfly_core::models::Game;

use super::{
    actions::{CopyAppId, OpenStorePage, SimilarGames},
    shared::{hx, insight_tile},
};
use crate::app::{
    ARTWORK_PALETTE, LibraryInsights, LibraryScope, LibrarySort, QuickView, View, format_playtime,
    game_card_detail, game_primary_badge, game_shows_deck_badge, open_url_in_browser,
    relative_time_ago,
};

use crate::ui::GuiRoot;

/// Toolbar command: toggle the Steam Deck compatibility filter.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = vapourfly, no_json)]
pub(crate) struct ToggleDeckFilter;

/// Toolbar command: toggle the full-controller-support filter.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = vapourfly, no_json)]
pub(crate) struct ToggleControllerFilter;

/// Toolbar command: toggle junk exclusion.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = vapourfly, no_json)]
pub(crate) struct ToggleHideJunkFilter;

/// Toolbar command: toggle hidden-game exclusion.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = vapourfly, no_json)]
pub(crate) struct ToggleExcludeHiddenFilter;

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
        let scanning = self.app.loading;

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
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .when(scanning || !ready, |this| {
                                        this.child(
                                            Icon::new(IconName::LoaderCircle)
                                                .small()
                                                .text_color(cx.theme().muted_foreground),
                                        )
                                    })
                                    .child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().muted_foreground)
                                            .child(if scanning {
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
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            // While scanning the label is replaced by a
                            // spinner (`loading`), which reads differently
                            // from a disabled control: work is happening.
                            .child(
                                Button::new("library.refresh")
                                    .small()
                                    .label("Refresh")
                                    .loading_icon(Icon::new(IconName::LoaderCircle))
                                    .loading(scanning)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.start_scan();
                                                this.arm_poll(cx);
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("library.junk")
                                    .small()
                                    .label("Junk…")
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.show_junk_panel = true;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap_3()
                    .child(
                        Input::new(&self.search)
                            .prefix(Icon::new(IconName::Search).small())
                            .cleanable(true)
                            .small()
                            .w(px(280.)),
                    )
                    .child(self.scope_tabs(cx)),
            )
            .child(self.library_filters(cx))
            .child(self.quick_chips(cx))
            .child(if !ready {
                div()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                Icon::new(IconName::LoaderCircle)
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Preparing hydrated library snapshot…"),
                            ),
                    )
                    .into_any_element()
            } else if total == 0 {
                div()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .child(
                        v_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                Icon::new(IconName::Inbox).text_color(cx.theme().muted_foreground),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("No games match your filters."),
                            ),
                    )
                    .into_any_element()
            } else {
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
            })
            .when(ready && total > self.app.library_visible_count, |this| {
                // A stale snapshot re-prepare (or a scan) keeps the button in
                // its loading state instead of looking dead.
                let busy = self.app.loading || self.app.prepare_job_id.is_some();
                this.child(
                    h_flex().justify_center().child(
                        Button::new("library.load-more")
                            .label(if busy { "Loading…" } else { "Load more" })
                            .loading_icon(Icon::new(IconName::LoaderCircle))
                            .loading(busy)
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.library_visible_count =
                                            this.app.library_visible_count.saturating_add(48);
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
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
            .id(("library.row", id))
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
            // The buttons only dispatch; the behavior lives once in the
            // row's `on_action` handlers below, so click and context-menu
            // paths can never drift apart.
            .child(
                Button::new((ElementId::from(("library.row-action", id)), "similar"))
                    .xsmall()
                    .ghost()
                    .icon(Icon::new(IconName::Replace))
                    .tooltip("Similar games")
                    .on_click({
                        let entity = entity.clone();
                        move |_, _, cx| seed_discover_from(&entity, id, cx)
                    }),
            )
            .child(
                Button::new((ElementId::from(("library.row-action", id)), "copy"))
                    .xsmall()
                    .ghost()
                    .icon(Icon::new(IconName::Copy))
                    .tooltip("Copy App ID")
                    .on_click(move |_, window, cx| copy_app_id(id, window, cx)),
            )
            .child(
                Button::new((ElementId::from(("library.row-action", id)), "store"))
                    .xsmall()
                    .ghost()
                    .icon(Icon::new(IconName::ExternalLink))
                    .tooltip("Open store page")
                    .on_click(move |_, _, _| open_store_page(id)),
            )
            .on_action({
                let entity = entity.clone();
                move |_: &SimilarGames, _, cx| seed_discover_from(&entity, id, cx)
            })
            .on_action(move |a: &CopyAppId, window, cx| copy_app_id(a.0, window, cx))
            .on_action(move |a: &OpenStorePage, _, _| open_store_page(a.0))
            .context_menu(move |menu, _, _| {
                menu.menu("Similar games", Box::new(SimilarGames(id)))
                    .menu("Copy App ID", Box::new(CopyAppId(id)))
                    .separator()
                    .menu("Open store page", Box::new(OpenStorePage(id)))
            })
    }

    pub(crate) fn scope_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = LibraryScope::all()
            .iter()
            .position(|s| *s == self.app.library_scope)
            .unwrap_or(0);
        let entity = cx.entity();
        TabBar::new("library.scope")
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
        let deck = self.app.filter_deck_compatible;
        let controller = self.app.filter_controller_full;
        let hide_junk = self.app.filter_not_junk;
        let exclude_hidden = self.app.filter_not_hidden;
        v_flex()
            .id("library.filters")
            .gap_2()
            // The collapsed "More filters" menu dispatches these actions;
            // handling them here (an ancestor of the trigger) keeps every
            // toggle's body in one place.
            .on_action(cx.listener(|this, _: &ToggleDeckFilter, _, cx| {
                this.app.filter_deck_compatible = !this.app.filter_deck_compatible;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleControllerFilter, _, cx| {
                this.app.filter_controller_full = !this.app.filter_controller_full;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleHideJunkFilter, _, cx| {
                this.app.filter_not_junk = !this.app.filter_not_junk;
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ToggleExcludeHiddenFilter, _, cx| {
                this.app.filter_not_hidden = !this.app.filter_not_hidden;
                cx.notify();
            }))
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        // Select's own element id is derived from its state
                        // entity, so the domain id lives on the wrapper.
                        div()
                            .id("library.sort-select")
                            .child(Select::new(&self.library_sort_select).small().w(px(200.))),
                    )
                    .child(
                        Button::new("library.sort-direction")
                            .small()
                            .ghost()
                            .icon(Icon::new(if self.app.library_sort_desc {
                                IconName::SortDescending
                            } else {
                                IconName::SortAscending
                            }))
                            .tooltip("Toggle sort direction")
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
                        div()
                            .id("library.tier-select")
                            .child(Select::new(&self.library_tier_select).small().w(px(150.))),
                    )
                    .child(
                        Button::new("library.more-filters")
                            .small()
                            .ghost()
                            .icon(Icon::new(IconName::EllipsisVertical))
                            .tooltip("More filters")
                            .dropdown_menu(move |menu, _, _| {
                                menu.menu_with_check("Steam Deck", deck, Box::new(ToggleDeckFilter))
                                    .menu_with_check(
                                        "Full controller",
                                        controller,
                                        Box::new(ToggleControllerFilter),
                                    )
                                    .menu_with_check(
                                        "Hide junk",
                                        hide_junk,
                                        Box::new(ToggleHideJunkFilter),
                                    )
                                    .menu_with_check(
                                        "Exclude hidden",
                                        exclude_hidden,
                                        Box::new(ToggleExcludeHiddenFilter),
                                    )
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

    pub(crate) fn quick_chips(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let current = self.app.library_quick_view;
        h_flex().id("library.quick-chips").gap_1().children(
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
                let active = current == qv;
                Button::new((ElementId::from("library.quick-chip"), qv.label()))
                    .small()
                    .ghost()
                    .selected(active)
                    .label(qv.label())
                    // Editorial chips carry their fixed palette tint as
                    // the selected fill; `All` stays on the theme accent.
                    .when(active, |b| match quick_chip_tint(qv) {
                        Some((bg, fg)) => b.bg(hx(bg)).text_color(hx(fg)),
                        None => b,
                    })
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
            .id("library.insights")
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
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Icon::new(IconName::Inbox)
                            .small()
                            .text_color(cx.theme().muted_foreground),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child("No recent activity"),
                    )
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
                        Button::new("library.view-history")
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

/// Editorial palette entry for a quick-view chip; `All` is neutral.
fn quick_chip_tint(qv: QuickView) -> Option<(crate::theme::Rgb, crate::theme::Rgb)> {
    match qv {
        QuickView::All => None,
        QuickView::Cozy => Some(crate::theme::tint(2)),
        QuickView::StoryRich => Some(crate::theme::tint(4)),
        QuickView::GreatOnDeck => Some(crate::theme::tint(3)),
        QuickView::ShortSessions => Some(crate::theme::tint(1)),
    }
}

/// Shared body for the row copy affordance: clipboard write plus toast.
fn copy_app_id(app_id: u32, window: &mut Window, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(app_id.to_string()));
    window.push_notification(Notification::success(format!("Copied {app_id}")), cx);
}

/// Shared body for the row store affordance.
fn open_store_page(app_id: u32) {
    open_url_in_browser(&format!("https://store.steampowered.com/app/{app_id}"));
}

/// Shared body for the row discover affordance: seed Discover and switch views.
fn seed_discover_from(entity: &Entity<GuiRoot>, app_id: u32, cx: &mut App) {
    entity.update(cx, |this, cx| {
        this.app.discover_seed = app_id.to_string();
        this.app.current_view = View::Discover;
        this.app.start_discover_generate();
        this.arm_poll(cx);
        cx.notify();
    });
}
