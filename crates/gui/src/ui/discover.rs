//! Discover view: seed-driven similar-game ranking.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, prelude::*,
    px, uniform_list,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex,
    tab::{Tab, TabBar},
    tag::Tag,
    v_flex,
};

use super::dialogs::{AlertSpec, open_alert};
use crate::app::{PendingAction, View, playlist_game_count, reason_badge_label};

use crate::ui::GuiRoot;

/// Preset result counts; the selected segment writes `app.discover_count`.
const DISCOVER_COUNTS: [&str; 3] = ["10", "20", "40"];

impl GuiRoot {
    pub(crate) fn discover(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let muted = cx.theme().muted_foreground;
        v_flex()
            .id("discover")
            .size_full()
            .gap_3()
            .child(div().text_xl().font_semibold().child("Discover"))
            .child(div().text_sm().text_color(muted).child(
                "Similar picks from a game name or AppID. Writes the stable `discover` \
                         slot.",
            ))
            // Seed row: seed field + seed-from-selection.
            .child(
                h_flex().gap_2().child(self.discover_seed_field(cx)).child(
                    Button::new("disc-seed-sel")
                        .small()
                        .ghost()
                        .label("Use selected library game")
                        .tooltip("Seed from the game selected in Library")
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
                ),
            )
            // Controls row: count presets + generate/open/sync.
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(self.discover_count_tabs(cx))
                    .child(
                        Button::new("disc-go")
                            .primary()
                            .when(self.app.discover_loading, |b| {
                                b.icon(Icon::new(IconName::LoaderCircle))
                            })
                            .label("Generate")
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
                            .icon(Icon::new(IconName::ExternalLink))
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
                            .icon(Icon::new(IconName::Replace))
                            .label("Sync to collection…")
                            .disabled(self.app.discover_last_playlist.is_none() || self.app.ui_demo)
                            .on_click({
                                let entity = entity.clone();
                                move |_, window, cx| {
                                    entity.update(cx, |this, cx| {
                                        // Demo mode never writes: gate before
                                        // any dialog opens.
                                        if this.app.ui_demo {
                                            return;
                                        }
                                        let Some(pf) = this.app.discover_last_playlist.clone()
                                        else {
                                            return;
                                        };
                                        let name = pf.playlist.name.clone();
                                        open_alert(
                                            &entity,
                                            window,
                                            cx,
                                            AlertSpec {
                                                title: format!(
                                                    "Sync “{name}” to a Steam collection?"
                                                ),
                                                lines: vec![
                                                    format!(
                                                        "{} games · a backup is created first",
                                                        playlist_game_count(
                                                            &pf.playlist.content,
                                                            None
                                                        )
                                                    ),
                                                    "The dry-run diff opens next for final \
                                                     confirmation."
                                                        .into(),
                                                ],
                                                verb: "Sync".into(),
                                            },
                                            {
                                                let pf = pf.clone();
                                                move |this, cx| {
                                                    this.app.start_dry_run(
                                                        PendingAction::PlaylistSync(pf.clone()),
                                                    );
                                                    this.arm_poll(cx);
                                                    cx.notify();
                                                }
                                            },
                                        );
                                    });
                                }
                            }),
                    ),
            )
            .child(if self.app.discover_results.is_empty() {
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .text_color(muted)
                    .child(Icon::new(IconName::Inbox))
                    .child(div().text_sm().child("No picks yet"))
                    .child(
                        div()
                            .text_xs()
                            .child("Choose a seed and press Generate to rank similar games."),
                    )
                    .into_any_element()
            } else {
                let n = self.app.discover_results.len();
                uniform_list(
                    "discover.rows",
                    n,
                    cx.processor(|this, range: std::ops::Range<usize>, _, _cx| {
                        range
                            .filter_map(|ix| this.app.discover_results.get(ix).cloned())
                            .map(|pick| {
                                h_flex()
                                    .id(("discover.row", pick.app_id as usize))
                                    .h(px(44.))
                                    .gap_3()
                                    .child(
                                        div()
                                            .w(px(64.))
                                            .text_xs()
                                            .text_right()
                                            .child(format!("{:.1} pts", pick.score)),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_sm()
                                            .overflow_hidden()
                                            .whitespace_nowrap()
                                            .child(pick.name.clone()),
                                    )
                                    .child(Tag::secondary().small().child(SharedString::from(
                                        pick.reasons.first().map_or_else(
                                            || "Similar".to_string(),
                                            |r| reason_badge_label(r.code, r.description),
                                        ),
                                    )))
                            })
                            .collect()
                    }),
                )
                .flex_1()
                .into_any_element()
            })
    }

    /// Seed display field: search glyph + current seed with a clear
    /// affordance. The retained text input needs a root-provisioned
    /// `Entity<InputState>` (see WP report); until then the seed is edited
    /// via "Use selected library game" and the Library "Similar" action,
    /// exactly as before.
    fn discover_seed_field(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let seed = self.app.discover_seed.trim().to_string();
        let empty = seed.is_empty();
        let text = if empty {
            "Game name or AppID".to_string()
        } else {
            seed
        };
        h_flex()
            .w(px(280.))
            .h(px(28.))
            .px_2()
            .gap_2()
            .items_center()
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius)
            .bg(cx.theme().secondary)
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child(Icon::new(IconName::Search).small()),
            )
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .when(empty, |d| d.text_color(cx.theme().muted_foreground))
                    .child(text),
            )
            .when(!empty, |row| {
                row.child(
                    Button::new("discover.seed-clear")
                        .xsmall()
                        .ghost()
                        .label("Clear")
                        .tooltip("Clear the seed")
                        .on_click({
                            let entity = cx.entity();
                            move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.app.discover_seed = String::new();
                                    cx.notify();
                                });
                            }
                        }),
                )
            })
    }

    /// Count presets (10/20/40) as a segmented tab bar writing
    /// `app.discover_count`.
    fn discover_count_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let selected = DISCOVER_COUNTS
            .iter()
            .position(|c| c.eq_ignore_ascii_case(self.app.discover_count.trim()))
            .unwrap_or(1);
        let entity = cx.entity();
        TabBar::new("discover.count")
            .segmented()
            .small()
            .selected_index(selected)
            .on_click(move |ix, _, cx| {
                if let Some(count) = DISCOVER_COUNTS.get(*ix) {
                    let count = count.to_string();
                    entity.update(cx, |this, cx| {
                        this.app.discover_count = count;
                        cx.notify();
                    });
                }
            })
            .children(DISCOVER_COUNTS.iter().map(|c| Tab::new().label(*c)))
    }
}
