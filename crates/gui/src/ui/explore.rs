//! Discover (seeded similarity) and Recommendations (time-boxed picks).

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Disableable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex,
    scroll::ScrollableElement,
    select::Select,
    switch::Switch,
    tab::Tab,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, InteractiveElement, IntoElement, ParentElement, SharedString, Styled,
    Window, div, prelude::*, px,
};
use vapourfly_core::models::JunkMode;

use super::widgets::{self, PAGE_PX};
use super::{GuiRoot, hx};
use crate::app::{
    GameSummary, PendingAction, View, format_playtime, proton_tier_label, reason_badge_label,
};
use crate::theme;

const PICK_MIN_W: f32 = 210.;
const PICK_GAP: f32 = 20.;

const TIME_OPTIONS: [(&str, u32); 5] = [
    ("30m", 30),
    ("1h", 60),
    ("2h", 120),
    ("3h", 180),
    ("4h+", 240),
];
const COUNT_OPTIONS: [u32; 3] = [5, 10, 20];
const DISCOVER_COUNTS: [u32; 3] = [10, 20, 40];

/// Ranker scores are additive weights, so show them relative to the best
/// result in the same set.
fn relative(score: f32, best: f32) -> f32 {
    if best > 0. { score / best } else { 0. }
}

fn match_label(score: f32) -> String {
    format!("{:.0}% match", (score * 100.).clamp(0., 100.))
}

impl GuiRoot {
    fn explore_width(&self, window: &Window) -> f32 {
        let width: f32 = window.viewport_size().width.into();
        width - self.sidebar_width(window) - 10. - PAGE_PX * 2.
    }

    /// Keep the seed picker showing whatever game `discover_seed` points at,
    /// including seeds set from a Library card's "Find similar".
    pub(super) fn reconcile_seed_selection(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Ok(id) = self.app.discover_seed.trim().parse::<u32>() else {
            return;
        };
        let Some((name, _)) = self.discover_seed_options.iter().find(|(_, i)| *i == id) else {
            return;
        };
        let name = name.clone();
        let current = self.discover_seed_select.read(cx).selected_value().cloned();
        if current.as_ref() != Some(&name) {
            self.discover_seed_select.update(cx, |state, cx| {
                state.set_selected_value(&name, window, cx);
            });
        }
    }

    fn pick_card(
        &self,
        app_id: u32,
        name: &str,
        score: f32,
        reason: Option<String>,
        card_w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let art_h = theme::card_art_height(card_w);
        let owned = self.find_game(app_id).is_some();
        let group = SharedString::from(format!("pick-{app_id}"));
        let frame = div()
            .id(("pick-art", app_id as usize))
            .relative()
            .w_full()
            .h(px(art_h))
            .rounded(px(theme::CORNER_LG))
            .overflow_hidden()
            .border_2()
            .border_color(cx.theme().border.opacity(0.6));
        let frame = if owned {
            widgets::capsule_focus(frame, group.clone(), cx).on_click(cx.listener(
                move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.open_game_sheet(app_id, window, cx);
                },
            ))
        } else {
            frame
        };
        v_flex()
            .id(("pick", app_id as usize))
            .group(group)
            .w(px(card_w))
            .gap_2p5()
            .when(owned, |this| {
                this.cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_game_sheet(app_id, window, cx);
                    }))
            })
            .child(
                frame
                    .child(widgets::cover(
                        self.artwork.header(app_id),
                        app_id,
                        name,
                        Some(15.),
                    ))
                    .child(
                        div()
                            .absolute()
                            .top(px(8.))
                            .left(px(8.))
                            .child(widgets::art_pill(match_label(score))),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(15.))
                            .font_medium()
                            .truncate()
                            .child(name.to_string()),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(reason.unwrap_or_else(|| "Similar feel".into())),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn discover(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let has_playlist = self.app.discover_last_playlist.is_some();
        let seed_id = self.app.discover_seed.trim().parse::<u32>().ok();
        let seed_game = seed_id.and_then(|id| self.find_game(id));
        let width = self.explore_width(window);
        let columns = (((width + PICK_GAP) / (PICK_MIN_W + PICK_GAP)).floor() as usize).max(1);
        let card_w = ((width - PICK_GAP * (columns as f32 - 1.)) / columns as f32).floor();

        let actions = h_flex()
            .gap_2()
            .child(
                Button::new("disc-open")
                    .outline()
                    .icon(IconName::ListMusic)
                    .label("Open as playlist")
                    .disabled(!has_playlist)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(pf) = this.app.discover_last_playlist.clone() {
                            this.app.adopt_playlist_for_edit(&pf);
                            this.set_view(View::Playlists, cx);
                        }
                    })),
            )
            .child(
                Button::new("disc-sync")
                    .outline()
                    .icon(IconName::Upload)
                    .label("Sync to Steam")
                    .disabled(!has_playlist || self.app.ui_demo)
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(pf) = this.app.discover_last_playlist.clone() {
                            this.app.start_dry_run(PendingAction::PlaylistSync(pf));
                            this.arm_poll(cx);
                        }
                        cx.notify();
                    })),
            );

        let seed_panel = widgets::panel(cx).mx(px(PAGE_PX)).p_4().child(
            h_flex()
                .gap_4()
                .child(
                    div()
                        .flex_none()
                        .w(px(160.))
                        .h(px(75.))
                        .rounded(cx.theme().radius)
                        .overflow_hidden()
                        .bg(cx.theme().muted)
                        .when_some(seed_game.as_ref(), |this, g| {
                            this.child(widgets::cover(
                                self.artwork.header(g.app_id),
                                g.app_id,
                                &g.name,
                                None,
                            ))
                        }),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_1p5()
                        .child(widgets::section_label("Start from a game you love", cx))
                        .child(
                            Select::new(&self.discover_seed_select)
                                .placeholder("Choose a game from your library…")
                                .search_placeholder("Search games")
                                .cleanable(true)
                                .w(px(380.)),
                        ),
                )
                .child(
                    v_flex()
                        .gap_1p5()
                        .child(widgets::section_label("Picks", cx))
                        .child(
                            widgets::segmented(
                                "disc-count",
                                DISCOVER_COUNTS
                                    .iter()
                                    .map(|n| Tab::new().label(n.to_string())),
                                DISCOVER_COUNTS
                                    .iter()
                                    .position(|n| n.to_string() == self.app.discover_count.trim()),
                            )
                            .on_click(cx.listener(
                                |this, ix: &usize, _, cx| {
                                    if let Some(n) = DISCOVER_COUNTS.get(*ix) {
                                        this.app.discover_count = n.to_string();
                                        cx.notify();
                                    }
                                },
                            )),
                        ),
                )
                .child(
                    Button::new("disc-go")
                        .primary()
                        .icon(IconName::Sparkles)
                        .label(if seed_id.is_some() {
                            "Find matches"
                        } else {
                            "Match my taste"
                        })
                        .loading(self.app.discover_loading)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.app.start_discover_generate();
                            this.arm_poll(cx);
                            cx.notify();
                        })),
                ),
        );

        let results: Vec<_> = self.app.discover_results.clone();
        let best = results.iter().map(|p| p.score).fold(0., f32::max);
        let body = if results.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(widgets::empty_state(
                    IconName::Compass,
                    "Pick a seed to begin",
                    "Vapourfly compares genres, tags and play style to surface the closest matches you already own.",
                    None,
                ))
                .into_any_element()
        } else {
            v_flex()
                .id("disc-results")
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(
                    v_flex()
                        .px(px(PAGE_PX))
                        .pt_6()
                        .pb_8()
                        .gap_4()
                        .child(widgets::section_label(
                            format!(
                                "{} matches{}",
                                results.len(),
                                seed_game
                                    .as_ref()
                                    .map(|g| format!(" for {}", g.name))
                                    .unwrap_or_default()
                            ),
                            cx,
                        ))
                        .child(
                            div()
                                .flex()
                                .flex_wrap()
                                .gap_x(px(PICK_GAP))
                                .gap_y(px(26.))
                                .children(results.iter().map(|pick| {
                                    let reason = pick
                                        .reasons
                                        .first()
                                        .map(|r| reason_badge_label(r.code, r.description));
                                    self.pick_card(
                                        pick.app_id,
                                        &pick.name,
                                        relative(pick.score, best),
                                        reason,
                                        card_w,
                                        cx,
                                    )
                                })),
                        ),
                )
                .into_any_element()
        };

        v_flex()
            .id("discover")
            .size_full()
            .child(widgets::page_header(
                "Discover",
                "Find the games in your library that feel most like one you love.",
                actions,
                cx,
            ))
            .child(seed_panel)
            .child(body)
    }

    pub(super) fn recommend(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let minutes = self.app.recommend_minutes.parse::<u32>().unwrap_or(120);
        let count = self.app.recommend_count.parse::<u32>().unwrap_or(10);
        let t = theme::t();

        let actions = h_flex()
            .gap_2()
            .child(
                Button::new("rec-save")
                    .outline()
                    .icon(IconName::FolderPlus)
                    .label("Save as collection")
                    .disabled(self.app.recommend_results.is_empty() || self.app.ui_demo)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.app.start_dry_run(PendingAction::RecommendCollection);
                        this.arm_poll(cx);
                        cx.notify();
                    })),
            )
            .child(
                Button::new("rec-go")
                    .primary()
                    .icon(IconName::Sparkles)
                    .label("Generate")
                    .loading(self.app.recommend_loading)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.app.start_recommend_preview();
                        this.arm_poll(cx);
                        cx.notify();
                    })),
            );

        let field = |label: &'static str, control: AnyElement, cx: &App| {
            v_flex()
                .gap_1p5()
                .child(widgets::caption(label, cx))
                .child(control)
        };

        let controls = widgets::panel(cx).mx(px(PAGE_PX)).px_4().py_3().child(
            h_flex()
                .gap_8()
                .items_end()
                .child(field(
                    "Time you have",
                    widgets::segmented(
                        "rec-time",
                        TIME_OPTIONS
                            .iter()
                            .map(|(label, _)| Tab::new().label(*label)),
                        TIME_OPTIONS.iter().position(|(_, v)| *v == minutes),
                    )
                    .on_click(cx.listener(|this, ix: &usize, _, cx| {
                        if let Some((_, value)) = TIME_OPTIONS.get(*ix) {
                            this.app.recommend_minutes = value.to_string();
                            cx.notify();
                        }
                    }))
                    .into_any_element(),
                    cx,
                ))
                .child(field(
                    "Picks",
                    widgets::segmented(
                        "rec-count",
                        COUNT_OPTIONS
                            .iter()
                            .map(|v| Tab::new().label(v.to_string())),
                        COUNT_OPTIONS.iter().position(|v| *v == count),
                    )
                    .on_click(cx.listener(|this, ix: &usize, _, cx| {
                        if let Some(value) = COUNT_OPTIONS.get(*ix) {
                            this.app.recommend_count = value.to_string();
                            cx.notify();
                        }
                    }))
                    .into_any_element(),
                    cx,
                ))
                .child(div().flex_1())
                .child(
                    h_flex()
                        .gap_5()
                        .pb_1()
                        .child(
                            Switch::new("rec-deck")
                                .checked(self.app.recommend_deck)
                                .label("Steam Deck")
                                .on_click(cx.listener(|this, v: &bool, _, cx| {
                                    this.app.recommend_deck = *v;
                                    cx.notify();
                                })),
                        )
                        .child(
                            Switch::new("rec-inst")
                                .checked(self.app.recommend_installed_only)
                                .label("Installed only")
                                .on_click(cx.listener(|this, v: &bool, _, cx| {
                                    this.app.recommend_installed_only = *v;
                                    cx.notify();
                                })),
                        ),
                ),
        );

        let prepared = self.app.prepared_games(JunkMode::Default);
        let summary = |id: u32| {
            prepared
                .as_ref()
                .and_then(|games| games.iter().find(|g| g.app_id == id))
                .map(GameSummary::from)
                .unwrap_or_default()
        };
        let results = self.app.recommend_results.clone();
        let best = results.iter().map(|r| r.score).fold(0., f32::max);

        let body = if results.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(widgets::empty_state(
                    IconName::Sparkles,
                    "What should you play tonight?",
                    "Choose how much time you have, then generate a ranked shortlist from your backlog.",
                    None,
                ))
                .into_any_element()
        } else {
            let featured = results.iter().take(3).enumerate().map(|(rank, rec)| {
                let s = summary(rec.app_id);
                let reason = rec
                    .reasons
                    .first()
                    .map(|r| r.description.clone())
                    .unwrap_or_default();
                let id = rec.app_id;
                let group = SharedString::from(format!("rec-top-{id}"));
                let frame = div()
                    .id(("rec-top-art", id as usize))
                    .relative()
                    .w_full()
                    .h(px(200.))
                    .rounded(cx.theme().radius_lg)
                    .overflow_hidden()
                    .border_2()
                    .border_color(cx.theme().border.opacity(0.6));
                v_flex()
                    .id(("rec-top", id as usize))
                    .group(group.clone())
                    .flex_1()
                    .min_w_0()
                    .gap_3()
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_game_sheet(id, window, cx);
                    }))
                    .child(
                        widgets::capsule_focus(frame, group, cx)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.open_game_sheet(id, window, cx);
                            }))
                            .child(widgets::cover(
                                self.artwork.header(id),
                                id,
                                &rec.name,
                                Some(18.),
                            ))
                            .child(
                                h_flex()
                                    .absolute()
                                    .top(px(10.))
                                    .left(px(10.))
                                    .gap_1()
                                    .child(widgets::art_pill(format!("#{}", rank + 1)))
                                    .child(widgets::art_pill(match_label(relative(
                                        rec.score, best,
                                    )))),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_base()
                                    .font_semibold()
                                    .truncate()
                                    .child(rec.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .text_color(cx.theme().muted_foreground)
                                    .line_clamp(2)
                                    .child(reason),
                            )
                            .child(
                                h_flex()
                                    .pt_1()
                                    .gap_3()
                                    .text_size(px(13.))
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!(
                                        "{} to beat",
                                        s.hltb_minutes.map_or_else(|| "—".into(), format_playtime)
                                    ))
                                    .child("·")
                                    .child(match s.playtime_minutes {
                                        0 => "Unplayed".to_string(),
                                        m => format!("{} played", format_playtime(m)),
                                    })
                                    .when_some(s.rating_0_5, |this, r| {
                                        this.child("·").child(format!("{r:.1}★"))
                                    }),
                            ),
                    )
            });

            let rest = results.iter().enumerate().skip(3).map(|(ix, rec)| {
                let s = summary(rec.app_id);
                let id = rec.app_id;
                let reason = rec
                    .reasons
                    .first()
                    .map(|r| r.description.clone())
                    .unwrap_or_default();
                let content = h_flex()
                    .w_full()
                    .gap_4()
                    .child(
                        div()
                            .w(px(24.))
                            .text_size(px(15.))
                            .font_medium()
                            .text_color(cx.theme().muted_foreground)
                            .child((ix + 1).to_string()),
                    )
                    .child(
                        div()
                            .flex_none()
                            .w(px(120.))
                            .h(px(56.))
                            .rounded(px(theme::CORNER_SM))
                            .overflow_hidden()
                            .child(widgets::cover(self.artwork.header(id), id, &rec.name, None)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_medium()
                                    .truncate()
                                    .child(rec.name.clone()),
                            )
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(reason),
                            ),
                    )
                    .child(
                        div()
                            .w(px(90.))
                            .text_size(px(15.))
                            .text_color(cx.theme().muted_foreground)
                            .child(s.proton_tier.map_or("—", proton_tier_label)),
                    )
                    .child(
                        div()
                            .w(px(100.))
                            .text_size(px(15.))
                            .text_color(cx.theme().muted_foreground)
                            .child(s.hltb_minutes.map_or_else(
                                || "—".into(),
                                |m| format!("{} to beat", format_playtime(m)),
                            )),
                    )
                    .child(
                        div()
                            .w(px(90.))
                            .text_size(px(15.))
                            .font_medium()
                            .text_color(hx(t.accent_text))
                            .child(match_label(relative(rec.score, best))),
                    );
                widgets::focus_row(("rec-row", id as usize), false, cx)
                    .h(px(76.))
                    .px_2()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_game_sheet(id, window, cx);
                    }))
                    .child(content)
            });

            v_flex()
                .id("rec-results")
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(
                    v_flex()
                        .px(px(PAGE_PX))
                        .pt_6()
                        .pb_8()
                        .gap_6()
                        .child(widgets::section_title("Top picks", cx))
                        .child(h_flex().items_start().gap_5().children(featured))
                        .when(results.len() > 3, |this| {
                            this.child(
                                v_flex()
                                    .gap_2()
                                    .child(widgets::section_title("Also worth a look", cx))
                                    .child(v_flex().children(rest)),
                            )
                        }),
                )
                .into_any_element()
        };

        v_flex()
            .id("recommend")
            .size_full()
            .child(widgets::page_header(
                "Recommendations",
                "A ranked shortlist from your backlog, sized to the time you have.",
                actions,
                cx,
            ))
            .child(controls)
            .child(body)
    }
}
