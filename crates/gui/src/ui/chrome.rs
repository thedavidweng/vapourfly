//! Application shell: window chrome, collapsible sidebar navigation, status
//! banners.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement, Styled, Window, div,
    prelude::*, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, Sizable, StyledExt, TitleBar,
    breadcrumb::{Breadcrumb, BreadcrumbItem},
    button::{Button, ButtonVariants},
    h_flex,
    sidebar::{Sidebar, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem},
    v_flex,
};

use super::motion::{DUR_BASE, collapse_width, rise_in};
use crate::app::{View, format_playtime};
use crate::theme::{self, SIDEBAR_WIDTH};

use crate::ui::GuiRoot;

/// gpui-component's [`Sidebar`] paints its own fixed collapsed width — a
/// private 48px constant applied after any externally refined style — so that
/// value is mirrored here as the animation endpoint. Collapsing therefore
/// snaps to the component's width, while expanding eases out from it.
const SIDEBAR_COLLAPSED_PX: f32 = 48.0;

impl GuiRoot {
    pub(crate) fn shell(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let width: f32 = window.viewport_size().width.into();
        self.app.rails_below = theme::rails_below(width);
        let collapsed = self
            .sidebar_collapsed
            .unwrap_or_else(|| theme::is_compact_sidebar(width));
        let view = self.app.current_view;
        let games_n = self.app.scan_result.as_ref().map_or(0, |s| s.games.len());
        let playtime = self.app.scan_result.as_ref().map_or(0, |s| {
            s.games
                .iter()
                .map(|g| g.playtime_minutes.unwrap_or(0))
                .sum::<u32>()
        });

        let shell = v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.top_chrome(window, cx, games_n, playtime))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(self.sidebar(collapsed, cx))
                    .child(
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
            );
        super::actions::apply_root_handlers(shell, cx)
    }

    pub(crate) fn top_chrome(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
        games_n: usize,
        playtime: u32,
    ) -> impl IntoElement {
        let entity = cx.entity();
        // Dark mode shows Sun (offering Light); light mode shows Moon.
        let theme_icon = if self.app.theme_mode.is_dark() {
            IconName::Sun
        } else {
            IconName::Moon
        };
        let connectivity = if self.app.offline_mode {
            "Offline"
        } else {
            "Online"
        };
        TitleBar::new().child(
            h_flex()
                .w_full()
                .px_3()
                .gap_3()
                .child(
                    Button::new("sidebar-toggle")
                        .ghost()
                        .small()
                        .icon(Icon::new(IconName::PanelLeft))
                        .tooltip("Toggle sidebar")
                        .on_click({
                            let entity = entity.clone();
                            move |_, window, cx| {
                                let width: f32 = window.viewport_size().width.into();
                                entity.update(cx, |this, cx| {
                                    let effective = this
                                        .sidebar_collapsed
                                        .unwrap_or(crate::theme::is_compact_sidebar(width));
                                    this.sidebar_collapsed = Some(!effective);
                                    cx.notify();
                                });
                            }
                        }),
                )
                .child(
                    div().id("breadcrumb").child(
                        Breadcrumb::new()
                            .child(BreadcrumbItem::new("Vapourfly").font_semibold())
                            .child(
                                BreadcrumbItem::new(self.app.current_view.label()).font_medium(),
                            ),
                    ),
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
                            connectivity,
                        )),
                )
                .child(
                    Button::new("theme-toggle")
                        .ghost()
                        .small()
                        .icon(Icon::new(theme_icon))
                        .tooltip("Toggle theme")
                        .on_click(move |_, window, cx| {
                            entity.update(cx, |this, cx| this.toggle_theme(window, cx));
                        }),
                ),
        )
    }

    pub(crate) fn sidebar(&self, collapsed: bool, cx: &mut Context<Self>) -> AnyElement {
        let entity = cx.entity();
        let current = self.app.current_view;
        let item = |dest: View| {
            let entity = entity.clone();
            SidebarMenuItem::new(dest.label())
                .icon(Icon::new(match dest {
                    View::Discover => IconName::Globe,
                    View::Library => IconName::LayoutDashboard,
                    View::Recommendations => IconName::Star,
                    View::Playlists => IconName::GalleryVerticalEnd,
                    View::Collections => IconName::FolderOpen,
                    View::DataSources => IconName::FolderClosed,
                    View::Settings => IconName::Settings2,
                }))
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

        let mut sidebar = Sidebar::left()
            .collapsible(true)
            .collapsed(collapsed)
            .w(px(SIDEBAR_WIDTH))
            .child(
                SidebarGroup::new("Browse")
                    .child(SidebarMenu::new().children(browse.into_iter().map(item))),
            )
            .child(
                SidebarGroup::new("Maintain")
                    .child(SidebarMenu::new().children(maintain.into_iter().map(item))),
            );
        if !collapsed {
            // Brand slot; the active page is already shown in the title bar
            // breadcrumb, and the version lives in the sidebar footer. Both
            // drop out entirely while the rail is collapsed.
            sidebar = sidebar
                .header(
                    SidebarHeader::new().child(div().text_sm().font_semibold().child("Vapourfly")),
                )
                .footer(
                    SidebarFooter::new().child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(format!("v{}", env!("CARGO_PKG_VERSION"))),
                    ),
                );
        }
        collapse_width(
            sidebar,
            "sidebar-width",
            collapsed,
            SIDEBAR_WIDTH,
            SIDEBAR_COLLAPSED_PX,
            cx,
        )
    }

    pub(crate) fn banners(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let container = v_flex()
            .gap_2()
            .when_some(self.app.error.clone(), |this, err| {
                let entity = entity.clone();
                this.child(
                    h_flex()
                        .gap_2()
                        .p_2()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().danger.opacity(0.12))
                        .child(
                            Icon::new(IconName::TriangleAlert)
                                .size_4()
                                .text_color(cx.theme().danger),
                        )
                        .child(div().flex_1().text_sm().child(format!("Error: {err}")))
                        .child(
                            Button::new("banner-error-dismiss")
                                .ghost()
                                .small()
                                .icon(Icon::new(IconName::Close))
                                .tooltip("Dismiss")
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
                        .child(
                            Icon::new(IconName::CircleCheck)
                                .size_4()
                                .text_color(cx.theme().success),
                        )
                        .child(div().flex_1().text_sm().child(msg))
                        .child(
                            Button::new("banner-success-dismiss")
                                .ghost()
                                .small()
                                .icon(Icon::new(IconName::Close))
                                .tooltip("Dismiss")
                                .on_click(move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.success_msg = None;
                                        cx.notify();
                                    });
                                }),
                        ),
                )
            });
        rise_in(container, "banners-rise", DUR_BASE, cx)
    }
}
