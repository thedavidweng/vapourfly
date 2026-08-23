//! Playlist detail workspace: hero and game membership tab.

use gpui::{App, Context, IntoElement, ParentElement, Styled, div, prelude::*, px};
use gpui_component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    tab::{Tab, TabBar},
    v_flex,
};
use vapourfly_core::models::{JunkMode, PlaylistContent};

use crate::app::{
    ARTWORK_PALETTE, PlaylistDetailTab, empty_value_label, format_playtime, playlist_avg_hltb,
    playlist_content_type_label, playlist_cover_app_id, playlist_game_count,
};

use crate::ui::GuiRoot;
use crate::ui::root::set_input;
use crate::ui::shared::hx;

impl GuiRoot {
    pub(crate) fn playlist_workspace(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let tab = match self.app.playlist_detail_tab {
            PlaylistDetailTab::Games => 0,
            PlaylistDetailTab::Rules => 1,
            PlaylistDetailTab::Match => 2,
        };
        v_flex()
            .flex_1()
            .min_w_0()
            .gap_3()
            .child(self.playlist_hero(cx))
            .child(
                TabBar::new("pl-tabs")
                    .underline()
                    .selected_index(tab)
                    .on_click({
                        let entity = entity.clone();
                        move |ix, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.app.playlist_detail_tab = match ix {
                                    1 => PlaylistDetailTab::Rules,
                                    2 => PlaylistDetailTab::Match,
                                    _ => PlaylistDetailTab::Games,
                                };
                                if this.app.playlist_detail_tab == PlaylistDetailTab::Match {
                                    if let Ok(pf) = this.app.build_playlist_from_edit_fields() {
                                        this.app.match_playlist_against_library_background(&pf);
                                        this.arm_poll(cx);
                                    }
                                }
                                cx.notify();
                            });
                        }
                    })
                    .child(Tab::new().label("Games"))
                    .child(Tab::new().label("Rules"))
                    .child(Tab::new().label("Match")),
            )
            .child(match self.app.playlist_detail_tab {
                PlaylistDetailTab::Games => self.playlist_games(cx).into_any_element(),
                PlaylistDetailTab::Rules => self.playlist_rules(cx).into_any_element(),
                PlaylistDetailTab::Match => self.playlist_match(cx).into_any_element(),
            })
            .child(self.playlist_share(cx))
    }

    pub(crate) fn playlist_hero(&self, cx: &App) -> impl IntoElement {
        let content = if self.app.playlist_edit_rules.trim().is_empty() {
            PlaylistContent::Manual {
                app_ids: self
                    .app
                    .playlist_edit_app_ids
                    .split(',')
                    .filter_map(|s| s.trim().parse().ok())
                    .collect(),
            }
        } else {
            PlaylistContent::Rules {
                rules: self.app.parse_current_rules().unwrap_or_default(),
            }
        };
        let cover = playlist_cover_app_id(&content);
        let (top, _) = ARTWORK_PALETTE[(cover as usize) % ARTWORK_PALETTE.len()];
        let games = self.app.prepared_games(JunkMode::Default);
        let avg = games
            .as_ref()
            .and_then(|g| playlist_avg_hltb(&content, self.app.playlist_match_report.as_ref(), g));
        h_flex()
            .gap_3()
            .child(div().w(px(88.)).h(px(48.)).rounded(px(6.)).bg(hx(top)))
            .child(
                v_flex()
                    .gap_1()
                    .flex_1()
                    .min_w_0()
                    .child(Input::new(&self.playlist_name_input).small().w_full())
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Input::new(&self.playlist_id_input).small().w(px(180.)))
                            .child(Input::new(&self.playlist_desc_input).small().flex_1()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!(
                                "{} · {} games · avg HLTB {}",
                                playlist_content_type_label(&content),
                                playlist_game_count(
                                    &content,
                                    self.app.playlist_match_report.as_ref()
                                ),
                                avg.map(format_playtime)
                                    .unwrap_or_else(|| empty_value_label().into()),
                            )),
                    ),
            )
    }

    pub(crate) fn playlist_games(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let ids: Vec<u32> = self
            .app
            .playlist_edit_app_ids
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_xs().text_color(cx.theme().muted_foreground).child(
                        "Add/remove from the prepared library, or type comma-separated AppIDs.",
                    ))
                    .child(
                        Button::new("pl-adv-csv")
                            .xsmall()
                            .when(self.app.playlist_show_advanced_csv, |b| b.primary())
                            .label("CSV editor")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.playlist_show_advanced_csv =
                                            !this.app.playlist_show_advanced_csv;
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(Input::new(&self.playlist_csv_input).small().w_full())
            .child(
                h_flex().gap_2().children(
                    self.app
                        .prepared_games(JunkMode::Default)
                        .into_iter()
                        .flat_map(|games| games.iter().cloned().collect::<Vec<_>>())
                        .filter(|g| {
                            self.app.playlist_game_search.is_empty()
                                || g.name
                                    .to_lowercase()
                                    .contains(&self.app.playlist_game_search.to_lowercase())
                                || g.app_id.to_string().contains(&self.app.playlist_game_search)
                        })
                        .take(12)
                        .map(|g| {
                            let entity = entity.clone();
                            let id = g.app_id;
                            let on = ids.contains(&id);
                            Button::new(("addg", id as usize))
                                .small()
                                .when(on, |b| b.primary())
                                .label(g.name)
                                .on_click(move |_, window, cx| {
                                    entity.update(cx, |this, cx| {
                                        let mut set: Vec<u32> = this
                                            .app
                                            .playlist_edit_app_ids
                                            .split(',')
                                            .filter_map(|s| s.trim().parse().ok())
                                            .collect();
                                        if let Some(pos) = set.iter().position(|x| *x == id) {
                                            set.remove(pos);
                                        } else {
                                            set.push(id);
                                        }
                                        this.app.playlist_edit_app_ids = set
                                            .iter()
                                            .map(ToString::to_string)
                                            .collect::<Vec<_>>()
                                            .join(",");
                                        set_input(
                                            &this.playlist_csv_input,
                                            &this.app.playlist_edit_app_ids,
                                            window,
                                            cx,
                                        );
                                        cx.notify();
                                    });
                                })
                        }),
                ),
            )
    }
}
