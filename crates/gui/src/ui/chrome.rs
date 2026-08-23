//! Application shell: window chrome, sidebar navigation, status banners.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div, prelude::*, px,
};
use gpui_component::{
    ActiveTheme, Sizable, StyledExt, TitleBar,
    button::{Button, ButtonVariants},
    h_flex,
    sidebar::{Sidebar, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem},
    v_flex,
};

use super::shared::hx;
use crate::app::{PlaylistChooser, View, format_playtime};
use crate::theme::{self, SIDEBAR_WIDTH};

use crate::ui::GuiRoot;

impl GuiRoot {
    pub(crate) fn shell(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let width: f32 = window.viewport_size().width.into();
        self.app.rails_below = theme::rails_below(width);
        let tokens = theme::t();
        let view = self.app.current_view;
        let games_n = self.app.scan_result.as_ref().map_or(0, |s| s.games.len());
        let playtime = self.app.scan_result.as_ref().map_or(0, |s| {
            s.games
                .iter()
                .map(|g| g.playtime_minutes.unwrap_or(0))
                .sum::<u32>()
        });
        let entity = cx.entity();

        v_flex()
            .size_full()
            .bg(hx(tokens.canvas))
            .text_color(hx(tokens.text_primary))
            .child(self.top_chrome(window, cx, games_n, playtime))
            .child(
                h_flex().flex_1().min_h_0().child(self.sidebar(cx)).child(
                    v_flex()
                        .id("main")
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .px_6()
                        .py_4()
                        .gap_3()
                        .child(self.banners(cx))
                        .child(match view {
                            View::Library if self.app.show_junk_panel => {
                                self.junk_panel(cx).into_any_element()
                            }
                            View::Library => self.library(cx).into_any_element(),
                            View::Discover => self.discover(cx).into_any_element(),
                            View::Recommendations => self.recommend(cx).into_any_element(),
                            View::Playlists => self.playlists(cx).into_any_element(),
                            View::Collections => self.collections(cx).into_any_element(),
                            View::DataSources => self.data_sources(cx).into_any_element(),
                            View::Settings => self.settings(cx).into_any_element(),
                        }),
                ),
            )
            .when(self.app.show_confirm_dialog, |this| {
                this.child(self.confirm_overlay(entity.clone(), cx))
            })
            .when(self.app.playlist_chooser != PlaylistChooser::None, |this| {
                this.child(self.chooser_overlay(entity.clone(), cx))
            })
    }

    pub(crate) fn top_chrome(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
        games_n: usize,
        playtime: u32,
    ) -> impl IntoElement {
        let entity = cx.entity();
        let theme_label = if self.app.theme_mode.is_dark() {
            "Light"
        } else {
            "Dark"
        };
        TitleBar::new().child(
            h_flex()
                .w_full()
                .px_3()
                .gap_3()
                .child(div().text_sm().font_semibold().child("Vapourfly"))
                .child(div().text_color(cx.theme().muted_foreground).child("›"))
                .child(
                    div()
                        .text_sm()
                        .font_medium()
                        .child(self.app.current_view.label()),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!(
                            "{} games · {} · {}",
                            games_n,
                            format_playtime(playtime),
                            if self.app.offline_mode {
                                "Offline"
                            } else {
                                "Online"
                            }
                        )),
                )
                .child(
                    Button::new("theme-toggle")
                        .ghost()
                        .small()
                        .label(theme_label)
                        .on_click(move |_, window, cx| {
                            entity.update(cx, |this, cx| this.toggle_theme(window, cx));
                        }),
                ),
        )
    }

    pub(crate) fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let current = self.app.current_view;
        let item = |dest: View| {
            let entity = entity.clone();
            SidebarMenuItem::new(dest.label())
                .active(current == dest)
                .on_click(move |_, _, cx| {
                    entity.update(cx, |this, cx| this.set_view(dest, cx));
                })
        };
        let browse: Vec<View> = {
            let mut out = vec![View::Discover];
            out.extend(
                View::ALL
                    .iter()
                    .copied()
                    .filter(|v| !matches!(v, View::Discover | View::DataSources | View::Settings)),
            );
            out
        };
        let maintain: Vec<View> = View::ALL
            .iter()
            .copied()
            .filter(|v| matches!(v, View::DataSources | View::Settings))
            .collect();
        Sidebar::left()
            .collapsible(false)
            .w(px(SIDEBAR_WIDTH))
            // Brand slot; the active page is already shown in the title bar
            // breadcrumb, and the version lives in the sidebar footer.
            .header(SidebarHeader::new().child(div().text_sm().font_semibold().child("Vapourfly")))
            .footer(
                SidebarFooter::new().child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                ),
            )
            .child(
                SidebarGroup::new("Browse")
                    .child(SidebarMenu::new().children(browse.into_iter().map(item))),
            )
            .child(
                SidebarGroup::new("Maintain")
                    .child(SidebarMenu::new().children(maintain.into_iter().map(item))),
            )
    }

    pub(crate) fn banners(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        v_flex()
            .gap_2()
            .when_some(self.app.error.clone(), |this, err| {
                let entity = entity.clone();
                this.child(
                    h_flex()
                        .gap_2()
                        .p_2()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().danger.opacity(0.12))
                        .child(div().flex_1().text_sm().child(format!("Error: {err}")))
                        .child(
                            Button::new("dismiss-err")
                                .ghost()
                                .small()
                                .label("Dismiss")
                                .on_click(move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.error = None;
                                        cx.notify();
                                    });
                                }),
                        ),
                )
            })
            .when_some(self.app.success_msg.clone(), |this, msg| {
                let entity = entity.clone();
                this.child(
                    h_flex()
                        .gap_2()
                        .p_2()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().success.opacity(0.12))
                        .child(div().flex_1().text_sm().child(msg))
                        .child(
                            Button::new("dismiss-ok")
                                .ghost()
                                .small()
                                .label("Dismiss")
                                .on_click(move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.success_msg = None;
                                        cx.notify();
                                    });
                                }),
                        ),
                )
            })
    }
}
