//! GPUI + gpui-component presentation for [`crate::app::VapourflyApp`].

use std::path::PathBuf;
use std::time::Duration;

use gpui::{
    App, AppContext, ClipboardItem, Context, Entity, Hsla, InteractiveElement, IntoElement,
    ParentElement, SharedString, Styled, Window, div, prelude::*, px, rgb, uniform_list,
};
use gpui_component::{
    ActiveTheme, Colorize, Disableable, Sizable, StyledExt, Theme, ThemeMode as GpuiThemeMode,
    TitleBar, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    scroll::ScrollableElement,
    sidebar::{Sidebar, SidebarFooter, SidebarGroup, SidebarHeader, SidebarMenu, SidebarMenuItem},
    tab::{Tab, TabBar},
    tag::Tag,
    v_flex,
};
use vapourfly_core::dynamic::DynamicTemplate;
use vapourfly_core::models::{Game, JunkMode, PlaylistContent, PlaylistRule, ProtonTier};
use vapourfly_core::mood::EditorialMood;
use vapourfly_core::playlist;

use crate::app::{
    ARTWORK_PALETTE, GameSummary, JunkModeChoice, LibraryInsights, LibraryScope, LibrarySort,
    PendingAction, PlaylistChooser, PlaylistDetailTab, PlaylistMatchTab, PlaylistShareTab,
    QuickView, RepaintHook, VapourflyApp, View, cycle_proton_filter, empty_value_label,
    format_playtime, game_card_detail, game_primary_badge, game_shows_deck_badge,
    open_url_in_browser, playlist_avg_hltb, playlist_content_type_label, playlist_cover_app_id,
    playlist_game_count, proton_tier_label, reason_badge_label, relative_time_ago, sort_label,
    source_credential_signal, source_display_name, source_refresh_enabled, steam_capsule_uri,
};
use crate::jobs::JobWake;
use crate::theme::{self, SIDEBAR_WIDTH, ThemeMode, set_active_theme};

fn hx(c: theme::Rgb) -> Hsla {
    rgb(c.to_u32()).into()
}

fn apply_tokens(window: &mut Window, cx: &mut App, mode: ThemeMode) {
    set_active_theme(mode);
    Theme::change(
        if mode.is_dark() {
            GpuiThemeMode::Dark
        } else {
            GpuiThemeMode::Light
        },
        Some(window),
        cx,
    );
    let tokens = theme::t();
    let theme = Theme::global_mut(cx);
    theme.background = hx(tokens.canvas);
    theme.foreground = hx(tokens.text_primary);
    theme.border = hx(tokens.border);
    theme.primary = hx(tokens.accent);
    theme.primary_foreground = hx(tokens.text_inverse);
    theme.primary_hover = hx(tokens.accent).lighten(0.06);
    theme.muted = hx(tokens.surface_muted);
    theme.muted_foreground = hx(tokens.text_muted);
    theme.secondary = hx(tokens.surface_muted);
    theme.secondary_foreground = hx(tokens.text_secondary);
    theme.sidebar = hx(tokens.surface);
    theme.sidebar_foreground = hx(tokens.text_primary);
    theme.sidebar_accent = hx(tokens.accent_soft);
    theme.sidebar_accent_foreground = hx(tokens.accent_text);
    theme.sidebar_border = hx(tokens.border_soft);
    theme.danger = hx(tokens.error);
    theme.success = hx(tokens.success);
    theme.warning = hx(tokens.warning);
    theme.link = hx(tokens.accent);
    theme.ring = hx(tokens.accent);
    theme.radius = px(8.);
}

fn persist_theme(app: &VapourflyApp, mode: ThemeMode) {
    if app.ui_demo {
        return;
    }
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vapourfly");
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join("gui-theme"), mode.as_u8().to_string());
}

fn load_persisted_theme() -> ThemeMode {
    let path = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("vapourfly")
        .join("gui-theme");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| s.trim().parse::<u8>().ok())
        .map(ThemeMode::from_u8)
        .unwrap_or(ThemeMode::Light)
}

pub struct GuiRoot {
    app: VapourflyApp,
    search: Entity<InputState>,
    steam_dir_input: Entity<InputState>,
    account_input: Entity<InputState>,
    cc_input: Entity<InputState>,
    lang_input: Entity<InputState>,
    retention_input: Entity<InputState>,
    api_key_input: Entity<InputState>,
    genre_input: Entity<InputState>,
    tag_input: Entity<InputState>,
    playtime_min_input: Entity<InputState>,
    playtime_max_input: Entity<InputState>,
    hltb_min_input: Entity<InputState>,
    hltb_max_input: Entity<InputState>,
    playlist_name_input: Entity<InputState>,
    playlist_id_input: Entity<InputState>,
    playlist_desc_input: Entity<InputState>,
    playlist_csv_input: Entity<InputState>,
    playlist_edit_synced: u64,
    poll_armed: bool,
}

impl GuiRoot {
    pub fn new(
        window: &mut Window,
        cx: &mut Context<Self>,
        fixtures: Option<PathBuf>,
        ui_demo: bool,
        offline: bool,
    ) -> Self {
        let mut app = VapourflyApp::new(fixtures, ui_demo);
        app.offline_mode = offline;
        if !ui_demo {
            app.theme_mode = load_persisted_theme();
        }
        if ui_demo {
            app.populate_demo_data();
        }
        apply_tokens(window, cx, app.theme_mode);
        window.set_window_title("Vapourfly");

        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search by name or app id"));
        cx.subscribe(&search, |this, input, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                this.app.search_query = input.read(cx).value().to_string();
                this.app.library_visible_count = 48;
                cx.notify();
            }
        })
        .detach();

        let steam_dir_input = bind_input(
            window,
            cx,
            "/path/to/Steam",
            &app.steam_dir_edit,
            |app, v| {
                app.steam_dir_edit = v;
            },
        );
        let account_input = bind_input(window, cx, "account name", &app.account_edit, |app, v| {
            app.account_edit = v;
        });
        let cc_input = bind_input(window, cx, "us", &app.cc_edit, |app, v| {
            app.cc_edit = v;
        });
        let lang_input = bind_input(window, cx, "english", &app.lang_edit, |app, v| {
            app.lang_edit = v;
        });
        let retention_input = bind_input(window, cx, "5", &app.backup_retention_edit, |app, v| {
            app.backup_retention_edit = v;
        });
        let api_key_input = bind_input(
            window,
            cx,
            "paste your key (leave empty to remove)",
            &app.steam_api_key_edit,
            |app, v| {
                app.steam_api_key_edit = v;
            },
        );
        let genre_input =
            bind_filter_input(window, cx, "Any genre", &app.filter_genre, |app, v| {
                app.filter_genre = v;
            });
        let tag_input = bind_filter_input(window, cx, "Any tag", &app.filter_tag, |app, v| {
            app.filter_tag = v;
        });
        let playtime_min_input =
            bind_filter_input(window, cx, "min", &app.filter_playtime_min, |app, v| {
                app.filter_playtime_min = v;
            });
        let playtime_max_input =
            bind_filter_input(window, cx, "max", &app.filter_playtime_max, |app, v| {
                app.filter_playtime_max = v;
            });
        let hltb_min_input =
            bind_filter_input(window, cx, "min", &app.filter_hltb_min, |app, v| {
                app.filter_hltb_min = v;
            });
        let hltb_max_input =
            bind_filter_input(window, cx, "max", &app.filter_hltb_max, |app, v| {
                app.filter_hltb_max = v;
            });
        let playlist_name_input = bind_input(
            window,
            cx,
            "Playlist name",
            &app.playlist_edit_name,
            VapourflyApp::apply_playlist_name_edit,
        );
        let playlist_id_input = bind_input(
            window,
            cx,
            "playlist-id",
            &app.playlist_edit_id,
            VapourflyApp::apply_playlist_id_edit,
        );
        let playlist_desc_input = bind_input(
            window,
            cx,
            "Description",
            &app.playlist_edit_description,
            |app, v| app.playlist_edit_description = v,
        );
        let playlist_csv_input = bind_input(
            window,
            cx,
            "730, 440, 570",
            &app.playlist_edit_app_ids,
            |app, v| app.playlist_edit_app_ids = v,
        );

        let mut this = Self {
            app,
            search,
            steam_dir_input,
            account_input,
            cc_input,
            lang_input,
            retention_input,
            api_key_input,
            genre_input,
            tag_input,
            playtime_min_input,
            playtime_max_input,
            hltb_min_input,
            hltb_max_input,
            playlist_name_input,
            playlist_id_input,
            playlist_desc_input,
            playlist_csv_input,
            playlist_edit_synced: 0,
            poll_armed: false,
        };
        this.wire_repaint(cx);
        this.app.tick();
        this.arm_poll(cx);
        this
    }

    fn sync_filter_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        set_input(&self.genre_input, &self.app.filter_genre, window, cx);
        set_input(&self.tag_input, &self.app.filter_tag, window, cx);
        set_input(
            &self.playtime_min_input,
            &self.app.filter_playtime_min,
            window,
            cx,
        );
        set_input(
            &self.playtime_max_input,
            &self.app.filter_playtime_max,
            window,
            cx,
        );
        set_input(&self.hltb_min_input, &self.app.filter_hltb_min, window, cx);
        set_input(&self.hltb_max_input, &self.app.filter_hltb_max, window, cx);
    }

    fn sync_playlist_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
        set_input(
            &self.playlist_name_input,
            &self.app.playlist_edit_name,
            window,
            cx,
        );
        set_input(
            &self.playlist_id_input,
            &self.app.playlist_edit_id,
            window,
            cx,
        );
        set_input(
            &self.playlist_desc_input,
            &self.app.playlist_edit_description,
            window,
            cx,
        );
        set_input(
            &self.playlist_csv_input,
            &self.app.playlist_edit_app_ids,
            window,
            cx,
        );
    }

    fn reconcile_playlist_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.playlist_edit_synced != self.app.playlist_edit_generation {
            self.sync_playlist_inputs(window, cx);
            self.playlist_edit_synced = self.app.playlist_edit_generation;
            return;
        }
        let id_shown = self.playlist_id_input.read(cx).value().to_string();
        if self.app.playlist_id_auto && id_shown != self.app.playlist_edit_id {
            set_input(
                &self.playlist_id_input,
                &self.app.playlist_edit_id,
                window,
                cx,
            );
        }
    }

    fn wire_repaint(&mut self, cx: &mut Context<Self>) {
        let wake = JobWake::new();
        let hook_wake = wake.clone();
        self.app.repaint = RepaintHook::new(move || hook_wake.signal());
        // Worker threads cannot hold AsyncApp. JobWake hops the completion
        // signal back onto the UI task, which then ticks and notifies.
        cx.spawn(async move |this, cx| {
            loop {
                let wake = wake.clone();
                cx.background_executor()
                    .spawn(async move { wake.wait() })
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        this.app.tick();
                        cx.notify();
                        this.app.has_background_work()
                    })
                    .unwrap_or(false);
                if keep {
                    let _ = this.update(cx, |this, cx| this.arm_poll(cx));
                }
            }
        })
        .detach();
    }

    fn arm_poll(&mut self, cx: &mut Context<Self>) {
        if self.poll_armed {
            return;
        }
        self.poll_armed = true;
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(80))
                    .await;
                let keep = this
                    .update(cx, |this, cx| {
                        this.app.tick();
                        cx.notify();
                        this.app.has_background_work()
                    })
                    .unwrap_or(false);
                if !keep {
                    let _ = this.update(cx, |this, _cx| {
                        this.poll_armed = false;
                    });
                    break;
                }
            }
        })
        .detach();
    }

    fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
        self.app.current_view = view;
        if view == View::Settings && !self.app.ui_demo {
            if self.app.detected_accounts.is_empty() {
                self.app.refresh_detected_accounts();
            }
            self.app.refresh_backups();
        }
        if view == View::Playlists {
            self.app.refresh_playlist_store_ids();
            self.app.playlist_game_search = self.app.search_query.clone();
        }
        cx.notify();
    }

    fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.app.theme_mode = self.app.theme_mode.toggle();
        persist_theme(&self.app, self.app.theme_mode);
        apply_tokens(window, cx, self.app.theme_mode);
        cx.notify();
    }

    fn placeholder(&self, app_id: u32, height: f32) -> impl IntoElement {
        let (top, _) = ARTWORK_PALETTE[(app_id as usize) % ARTWORK_PALETTE.len()];
        div().w_full().h(px(height)).rounded(px(6.)).bg(hx(top))
    }

    fn shell(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_start()
                    .child(self.sidebar(cx))
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
            )
            .when(self.app.show_confirm_dialog, |this| {
                this.child(self.confirm_overlay(entity.clone(), cx))
            })
            .when(self.app.playlist_chooser != PlaylistChooser::None, |this| {
                this.child(self.chooser_overlay(entity.clone(), cx))
            })
    }

    fn top_chrome(
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

    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
            .header(SidebarHeader::new().child(div().text_sm().font_semibold().child("Library")))
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

    fn banners(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn library(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
            .size_full()
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
                    h_flex()
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

    fn library_row_owned(
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
        let last = game
            .last_played_unix
            .map(relative_time_ago)
            .unwrap_or_else(|| empty_value_label().into());
        let deck = if game_shows_deck_badge(&game) {
            "Deck"
        } else {
            empty_value_label()
        };
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
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(format!("{badge} · {play} · {deck} · {last} · {detail}")),
                    ),
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

    fn scope_tabs(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn library_filters(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn sort_cycle(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn quick_chips(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn junk_panel(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let selected = self.app.junk_selected.len();
        v_flex()
            .id("junk")
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_xl().font_semibold().child("Junk cleanup"))
                    .child(
                        Button::new("junk-back")
                            .ghost()
                            .label("Back to Library")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.show_junk_panel = false;
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(
                h_flex().gap_2().children(
                    [
                        JunkModeChoice::Default,
                        JunkModeChoice::Strict,
                        JunkModeChoice::Aggressive,
                    ]
                    .into_iter()
                    .map(|mode| {
                        let entity = entity.clone();
                        let active = self.app.junk_mode == mode;
                        Button::new(mode.label())
                            .small()
                            .when(active, |b| b.primary())
                            .label(mode.label())
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.app.junk_mode = mode;
                                    cx.notify();
                                });
                            })
                    }),
                ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("junk-preview")
                            .primary()
                            .label(if self.app.junk_preview_loading {
                                "Previewing…"
                            } else {
                                "Preview"
                            })
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.start_junk_preview();
                                        this.arm_poll(cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("junk-show-all")
                            .small()
                            .when(self.app.junk_show_all_evaluated, |b| b.primary())
                            .label("Show all evaluated")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.junk_show_all_evaluated =
                                            !this.app.junk_show_all_evaluated;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("junk-all")
                            .small()
                            .label("Select all junk")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.junk_selected = this
                                            .app
                                            .junk_results
                                            .iter()
                                            .filter(|d| d.is_junk)
                                            .map(|d| d.app_id)
                                            .collect();
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(Button::new("junk-clear").small().label("Clear").on_click({
                        let entity = entity.clone();
                        move |_, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.app.junk_selected.clear();
                                cx.notify();
                            });
                        }
                    }))
                    .child(
                        Button::new("junk-apply")
                            .small()
                            .label(format!("Apply {selected} selected"))
                            .disabled(selected == 0 || self.app.ui_demo)
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.start_dry_run(PendingAction::JunkApply);
                                        this.arm_poll(cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("junk-hide")
                            .small()
                            .label(format!("Hide {selected}"))
                            .disabled(selected == 0 || self.app.ui_demo)
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.start_dry_run(PendingAction::JunkHide);
                                        this.arm_poll(cx);
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(format!(
                        "Evaluated {} · selected {selected} · {}",
                        self.app.junk_results.len(),
                        self.app.junk_mode.label()
                    )),
            )
            .child({
                let rows: Vec<_> = self
                    .app
                    .junk_results
                    .iter()
                    .filter(|d| self.app.junk_show_all_evaluated || d.is_junk)
                    .cloned()
                    .collect();
                let n = rows.len();
                uniform_list(
                    "junk-rows",
                    n,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        let visible: Vec<_> = this
                            .app
                            .junk_results
                            .iter()
                            .filter(|d| this.app.junk_show_all_evaluated || d.is_junk)
                            .cloned()
                            .collect();
                        range
                            .filter_map(|ix| visible.get(ix).cloned())
                            .map(|d| {
                                let entity = cx.entity();
                                let id = d.app_id;
                                let checked = this.app.junk_selected.contains(&id);
                                h_flex()
                                    .id(("junk-row", id as usize))
                                    .h(px(40.))
                                    .gap_2()
                                    .child(
                                        Checkbox::new(("junk-cb", id as usize))
                                            .checked(checked)
                                            .on_click(move |_, _, cx| {
                                                entity.update(cx, |this, cx| {
                                                    if this.app.junk_selected.contains(&id) {
                                                        this.app.junk_selected.remove(&id);
                                                    } else {
                                                        this.app.junk_selected.insert(id);
                                                    }
                                                    cx.notify();
                                                });
                                            }),
                                    )
                                    .child(div().w(px(72.)).text_xs().child(id.to_string()))
                                    .child(div().flex_1().text_sm().child(d.name.clone()))
                                    .child(div().w(px(64.)).text_xs().child(if d.is_junk {
                                        "Junk"
                                    } else {
                                        "Keep"
                                    }))
                                    .child(
                                        div()
                                            .w(px(80.))
                                            .text_xs()
                                            .child(format!("{:.0}%", d.confidence * 100.0)),
                                    )
                            })
                            .collect()
                    }),
                )
                .flex_1()
            })
    }

    fn discover(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        v_flex()
            .id("discover")
            .size_full()
            .gap_3()
            .child(div().text_xl().font_semibold().child("Discover"))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child("Similar picks from a game name or AppID. Writes the stable `discover` slot."),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div().text_sm().child(format!("Seed: {}", self.app.discover_seed)),
                    )
                    .child(
                        Button::new("disc-seed-sel")
                            .small()
                            .label("Use selected library game")
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
                    )
                    .child(
                        Button::new("disc-go")
                            .primary()
                            .label(if self.app.discover_loading {
                                "Generating…"
                            } else {
                                "Generate"
                            })
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
                            .label("Sync to Steam collection")
                            .disabled(self.app.discover_last_playlist.is_none() || self.app.ui_demo)
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        if let Some(pf) = this.app.discover_last_playlist.clone() {
                                            this.app.start_dry_run(PendingAction::PlaylistSync(pf));
                                            this.arm_poll(cx);
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child({
                let n = self.app.discover_results.len();
                uniform_list("disc-rows", n, cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                    range.filter_map(|ix| this.app.discover_results.get(ix).cloned()).map(|pick| {
                        h_flex()
                            .id(("disc", pick.app_id as usize))
                            .h(px(44.))
                            .gap_3()
                            .child(div().w(px(64.)).text_xs().child(format!("{:.0}%", pick.score * 100.0)))
                            .child(div().flex_1().text_sm().child(pick.name.clone()))
                            .child(div().text_xs().text_color(cx.theme().muted_foreground).child(
                                pick.reasons.first().map_or_else(
                                    || "Similar".into(),
                                    |r| reason_badge_label(r.code, r.description),
                                ),
                            ))
                    }).collect()
                })).flex_1()
            })
    }

    fn recommend(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
                                    .child(format!(
                                        "{:.0}% · {} · {} · rating {} · {}",
                                        rec.score * 100.0,
                                        format_playtime(summary.playtime_minutes),
                                        summary
                                            .hltb_minutes
                                            .map(format_playtime)
                                            .unwrap_or_else(|| empty_value_label().into()),
                                        summary
                                            .rating_0_5
                                            .map(|r| format!("{r:.1}"))
                                            .unwrap_or_else(|| empty_value_label().into()),
                                        summary
                                            .proton_tier
                                            .map(|t| format!("{t:?}"))
                                            .unwrap_or_else(|| empty_value_label().into()),
                                    )),
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
                                            .child(format!("{:.0}%", rec.score * 100.0)),
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

    fn playlists(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        v_flex()
            .id("playlists")
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_xl().font_semibold().child("Playlists"))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(Button::new("pl-new").small().label("New").on_click({
                                let entity = entity.clone();
                                move |_, window, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.reset_playlist_editor();
                                        this.sync_playlist_inputs(window, cx);
                                        this.playlist_edit_synced =
                                            this.app.playlist_edit_generation;
                                        cx.notify();
                                    });
                                }
                            }))
                            .child(Button::new("pl-dyn").small().label("Dynamic").on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.playlist_chooser = PlaylistChooser::Dynamic;
                                        cx.notify();
                                    });
                                }
                            }))
                            .child(Button::new("pl-mood").small().label("Mood").on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.playlist_chooser = PlaylistChooser::Mood;
                                        cx.notify();
                                    });
                                }
                            }))
                            .child(
                                Button::new("pl-save")
                                    .primary()
                                    .small()
                                    .label("Save")
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                match this.app.build_playlist_from_edit_fields() {
                                                    Ok(pf) => match this.app.store_playlist(&pf) {
                                                        Ok(()) => {
                                                            this.app.success_msg = Some(format!(
                                                                "Saved {}",
                                                                pf.playlist.id
                                                            ));
                                                            this.app.refresh_playlist_store_ids();
                                                        }
                                                        Err(e) => this.app.error = Some(e),
                                                    },
                                                    Err(e) => this.app.error = Some(e),
                                                }
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(Button::new("pl-export").small().label("Export").on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("JSON", &["json"])
                                        .save_file()
                                    {
                                        entity.update(cx, |this, cx| {
                                            this.app.playlist_export_path =
                                                path.to_string_lossy().into();
                                            match this.app.export_loaded_playlist() {
                                                Ok(()) => {
                                                    this.app.success_msg =
                                                        Some("Exported playlist.".into());
                                                }
                                                Err(e) => this.app.error = Some(e),
                                            }
                                            cx.notify();
                                        });
                                    }
                                }
                            }))
                            .child(Button::new("pl-import").small().label("Import").on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("JSON", &["json"])
                                        .pick_file()
                                    {
                                        entity.update(cx, |this, cx| {
                                            this.app.playlist_import_path =
                                                path.to_string_lossy().into();
                                            match playlist::import_playlist(&path) {
                                                Ok(pf) => {
                                                    this.app.adopt_imported_playlist(
                                                        pf,
                                                        "Imported playlist.".into(),
                                                    );
                                                }
                                                Err(e) => this.app.error = Some(e.to_string()),
                                            }
                                            cx.notify();
                                        });
                                    }
                                }
                            }))
                            .child(
                                Button::new("pl-sync")
                                    .small()
                                    .label("Sync to Steam")
                                    .disabled(self.app.ui_demo)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                match this.app.build_playlist_from_edit_fields() {
                                                    Ok(pf) => {
                                                        this.app.start_dry_run(
                                                            PendingAction::PlaylistSync(pf),
                                                        );
                                                        this.arm_poll(cx);
                                                    }
                                                    Err(e) => this.app.error = Some(e),
                                                }
                                                cx.notify();
                                            });
                                        }
                                    }),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_4()
                    .child(self.playlist_rail(cx))
                    .child(self.playlist_workspace(cx)),
            )
    }

    fn playlist_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        v_flex()
            .id("pl-rail")
            .w(px(260.))
            .h_full()
            .gap_1()
            .border_r_1()
            .border_color(cx.theme().border)
            .pr_3()
            .children(self.app.playlist_rail_entries.iter().map(|(id, entry)| {
                let entity = entity.clone();
                let id = id.clone();
                let label = match entry {
                    Ok(pf) => format!(
                        "{} · {} · {}",
                        pf.playlist.name,
                        playlist_content_type_label(&pf.playlist.content),
                        playlist_game_count(
                            &pf.playlist.content,
                            self.app.playlist_match_report.as_ref()
                        )
                    ),
                    Err(e) => format!("{id} · error: {e}"),
                };
                Button::new(SharedString::from(format!("rail-{id}")))
                    .ghost()
                    .w_full()
                    .label(label)
                    .on_click(move |_, window, cx| {
                        entity.update(cx, |this, cx| {
                            if let Err(e) = this.app.load_playlist_from_store(&id) {
                                this.app.error = Some(e);
                            } else {
                                this.sync_playlist_inputs(window, cx);
                                this.playlist_edit_synced = this.app.playlist_edit_generation;
                                this.arm_poll(cx);
                            }
                            cx.notify();
                        });
                    })
            }))
    }

    fn playlist_workspace(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn playlist_hero(&self, cx: &App) -> impl IntoElement {
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

    fn playlist_games(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn playlist_rules(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn parameterized_rules(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    fn nudge_str_btn(
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

    fn playlist_match(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let tab = match self.app.playlist_match_sub_tab {
            PlaylistMatchTab::Owned => 0,
            PlaylistMatchTab::Missing => 1,
        };
        v_flex()
            .gap_2()
            .child(
                TabBar::new("match-sub")
                    .segmented()
                    .small()
                    .selected_index(tab)
                    .on_click(move |ix, _, cx| {
                        entity.update(cx, |this, cx| {
                            this.app.playlist_match_sub_tab = if *ix == 1 {
                                PlaylistMatchTab::Missing
                            } else {
                                PlaylistMatchTab::Owned
                            };
                            cx.notify();
                        });
                    })
                    .child(Tab::new().label("Owned"))
                    .child(Tab::new().label("Missing")),
            )
            .child(match &self.app.playlist_match_report {
                None => div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(if self.app.playlist_match_loading {
                        "Matching…"
                    } else {
                        "No match report yet."
                    })
                    .into_any_element(),
                Some(report) => {
                    let ids = match self.app.playlist_match_sub_tab {
                        PlaylistMatchTab::Owned => &report.owned,
                        PlaylistMatchTab::Missing => &report.missing,
                    };
                    v_flex()
                        .gap_2()
                        .child(div().text_sm().child(format!(
                            "Owned {} · missing {} · played {} · unplayed {} · hidden {} · junk {}",
                            report.owned.len(),
                            report.missing.len(),
                            report.played.len(),
                            report.unplayed.len(),
                            report.hidden.len(),
                            report.junk.len()
                        )))
                        .when_some(report.completion_price.clone(), |this, price| {
                            this.child(
                                div()
                                    .text_sm()
                                    .child(format!("Completion price: {}", price.format())),
                            )
                        })
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(if ids.is_empty() {
                                    empty_value_label().to_string()
                                } else {
                                    ids.iter()
                                        .map(ToString::to_string)
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                }),
                        )
                        .into_any_element()
                }
            })
    }

    fn playlist_share(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let tab = match self.app.playlist_share_tab {
            PlaylistShareTab::ShareCode => 0,
            PlaylistShareTab::Json => 1,
        };
        v_flex()
            .gap_2()
            .child(
                TabBar::new("share-tab")
                    .segmented()
                    .small()
                    .selected_index(tab)
                    .on_click({
                        let entity = entity.clone();
                        move |ix, _, cx| {
                            entity.update(cx, |this, cx| {
                                this.app.playlist_share_tab = if *ix == 1 {
                                    PlaylistShareTab::Json
                                } else {
                                    PlaylistShareTab::ShareCode
                                };
                                cx.notify();
                            });
                        }
                    })
                    .child(Tab::new().label("Share code"))
                    .child(Tab::new().label("JSON")),
            )
            .child(match self.app.playlist_share_tab {
                PlaylistShareTab::ShareCode => h_flex()
                    .gap_2()
                    .child(
                        Button::new("share-copy")
                            .small()
                            .label("Copy VF1")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _window, cx| {
                                    entity.update(cx, |this, cx| {
                                        match this.app.build_playlist_from_edit_fields() {
                                            Ok(pf) => {
                                                match vapourfly_core::share_code::encode_share_code(
                                                    &pf,
                                                ) {
                                                    Ok(code) => {
                                                        this.app.playlist_share_code_output =
                                                            Some(code.clone());
                                                        cx.write_to_clipboard(
                                                            ClipboardItem::new_string(code),
                                                        );
                                                        this.app.success_msg =
                                                            Some("Share code copied.".into());
                                                    }
                                                    Err(e) => this.app.error = Some(e.to_string()),
                                                }
                                            }
                                            Err(e) => this.app.error = Some(e),
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .child(
                        Button::new("share-import-toggle")
                            .small()
                            .when(self.app.playlist_show_import, |b| b.primary())
                            .label("Import VF1")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.playlist_show_import =
                                            !this.app.playlist_show_import;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .when(self.app.playlist_show_import, |this| {
                        this.child(
                            Button::new("share-paste")
                                .small()
                                .label("Paste and import")
                                .on_click({
                                    let entity = entity.clone();
                                    move |_, _, cx| {
                                        if let Some(text) =
                                            cx.read_from_clipboard().and_then(|item| item.text())
                                        {
                                            entity.update(cx, |this, cx| {
                                                this.app.playlist_share_code_input = text.clone();
                                                match vapourfly_core::share_code::decode_share_code(
                                                    &text,
                                                ) {
                                                    Ok(pf) => this.app.adopt_imported_playlist(
                                                        pf,
                                                        "Imported share code.".into(),
                                                    ),
                                                    Err(e) => this.app.error = Some(e.to_string()),
                                                }
                                                cx.notify();
                                            });
                                        }
                                    }
                                }),
                        )
                    })
                    .when(self.app.playlist_show_import, |this| {
                        this.child(div().text_xs().child(format!(
                            "Last file {}",
                            empty_or(&self.app.playlist_import_path)
                        )))
                    })
                    .when_some(self.app.playlist_share_code_output.clone(), |this, code| {
                        this.child(div().text_xs().child(code))
                    })
                    .into_any_element(),
                PlaylistShareTab::Json => v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .child(if self.app.playlist_export_path.is_empty() {
                                empty_value_label().to_string()
                            } else {
                                self.app.playlist_export_path.clone()
                            }),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                self.app
                                    .build_playlist_from_edit_fields()
                                    .ok()
                                    .and_then(|pf| serde_json::to_string_pretty(&pf).ok())
                                    .unwrap_or_else(|| empty_value_label().into()),
                            ),
                    )
                    .into_any_element(),
            })
    }

    fn collections(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        v_flex()
            .id("collections")
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_xl().font_semibold().child("Collections"))
                    .child(
                        Button::new("col-export")
                            .primary()
                            .label("Export all")
                            .on_click(move |_, _, cx| {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("JSON", &["json"])
                                    .save_file()
                                {
                                    entity.update(cx, |this, cx| {
                                        this.app.collections_export_path =
                                            path.to_string_lossy().into();
                                        match this.app.export_collections() {
                                            Ok(()) => {
                                                this.app.success_msg =
                                                    Some("Exported collections.".into());
                                            }
                                            Err(e) => this.app.error = Some(e),
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(div().id("col-grid").flex_1().overflow_y_scrollbar().child(
                div().flex().flex_row().flex_wrap().gap_3().children(
                    self.app.collections.iter().map(|c| {
                        v_flex()
                            .id(SharedString::from(c.name.clone()))
                            .w(px(220.))
                            .p_3()
                            .gap_2()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(cx.theme().radius)
                            .child(div().text_sm().font_medium().child(c.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{} games", c.app_ids.len())),
                            )
                            .child(h_flex().gap_1().children(
                                c.app_ids.iter().copied().take(4).map(|id| {
                                    let offline = self.app.demo_or_offline();
                                    let (top, _) =
                                        ARTWORK_PALETTE[(id as usize) % ARTWORK_PALETTE.len()];
                                    let uri = steam_capsule_uri(id);
                                    div()
                                        .id(("collage", id as usize))
                                        .w(px(44.))
                                        .h(px(22.))
                                        .rounded(px(3.))
                                        .bg(hx(top))
                                        .on_mouse_down(gpui::MouseButton::Left, move |_, _, _| {
                                            if !offline {
                                                crate::app::open_url_in_browser(&uri);
                                            }
                                        })
                                }),
                            ))
                            .when(c.is_hidden_collection, |this| {
                                this.child(Tag::warning().small().child("Hidden"))
                            })
                    }),
                ),
            ))
    }

    fn data_sources(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let statuses = self.app.source_statuses.clone();
        let entries: u64 = statuses.iter().map(|s| s.cache_entries as u64).sum();
        let stale: u64 = statuses.iter().map(|s| s.stale_entries as u64).sum();
        v_flex()
            .id("sources")
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_xl().font_semibold().child("Data Sources"))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("offline")
                                    .small()
                                    .when(self.app.offline_mode, |b| b.primary())
                                    .label(if self.app.offline_mode {
                                        "Offline"
                                    } else {
                                        "Online"
                                    })
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.offline_mode = !this.app.offline_mode;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("refresh-all")
                                    .primary()
                                    .label("Refresh All")
                                    .disabled(
                                        self.app.offline_mode
                                            || self.app.cache_refresh_loading
                                            || self.app.ui_demo,
                                    )
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.start_cache_refresh(None);
                                                this.arm_poll(cx);
                                                cx.notify();
                                            });
                                        }
                                    }),
                            ),
                    ),
            )
            .child(div().text_sm().child(format!(
                "Cache health · {entries} entries · {stale} stale · {} sources",
                statuses.len()
            )))
            .when_some(self.app.cache_refresh_msg.clone(), |this, msg| {
                this.child(div().text_xs().child(msg))
            })
            .children(statuses.into_iter().map(|st| {
                let entity = entity.clone();
                let id = st.name.clone();
                let enabled = source_refresh_enabled(
                    &id,
                    self.app.has_igdb,
                    self.app.has_rawg,
                    self.app.offline_mode,
                    self.app.cache_refresh_loading,
                ) && !self.app.ui_demo;
                h_flex()
                    .id(SharedString::from(id.clone()))
                    .h(px(40.))
                    .gap_3()
                    .child(
                        div()
                            .w(px(120.))
                            .font_medium()
                            .child(source_display_name(&id)),
                    )
                    .child(div().w(px(110.)).text_xs().child(
                        source_credential_signal(&id, self.app.has_igdb, self.app.has_rawg).label(),
                    ))
                    .child(
                        div()
                            .w(px(80.))
                            .text_xs()
                            .child(format!("{} entries", st.cache_entries)),
                    )
                    .child(
                        div()
                            .w(px(70.))
                            .text_xs()
                            .child(format!("{} stale", st.stale_entries)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                st.last_success
                                    .map(|t| t.to_rfc3339())
                                    .unwrap_or_else(|| "—".into()),
                            ),
                    )
                    .child(
                        Button::new(SharedString::from(format!("rf-{id}")))
                            .xsmall()
                            .label("Refresh")
                            .disabled(!enabled)
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.app.start_cache_refresh(Some(id.clone()));
                                    this.arm_poll(cx);
                                    cx.notify();
                                });
                            }),
                    )
            }))
    }

    fn settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        v_flex()
            .id("settings")
            .size_full()
            .gap_3()
            .overflow_y_scrollbar()
            .child(div().text_xl().font_semibold().child("Settings"))
            .child(section("Appearance", cx, |_cx| {
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .child(format!("Theme: {}", self.app.theme_mode.label())),
                    )
                    .child(Button::new("set-theme").small().label("Toggle").on_click({
                        let entity = entity.clone();
                        move |_, window, cx| {
                            entity.update(cx, |this, cx| this.toggle_theme(window, cx));
                        }
                    }))
                    .into_any_element()
            }))
            .child(section("Configuration", cx, |_| {
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().w(px(140.)).text_xs().child("Steam directory"))
                            .child(Input::new(&self.steam_dir_input).small().flex_1())
                            .child(
                                Button::new("pick-steam")
                                    .xsmall()
                                    .label("Browse")
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, window, cx| {
                                            if let Some(path) = rfd::FileDialog::new().pick_folder()
                                            {
                                                entity.update(cx, |this, cx| {
                                                    let value = path.to_string_lossy().into_owned();
                                                    this.app.steam_dir_edit = value.clone();
                                                    set_input(
                                                        &this.steam_dir_input,
                                                        &value,
                                                        window,
                                                        cx,
                                                    );
                                                    cx.notify();
                                                });
                                            }
                                        }
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().w(px(140.)).text_xs().child("Account override"))
                            .child(Input::new(&self.account_input).small().w(px(280.))),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().text_xs().child("Store country"))
                                    .child(Input::new(&self.cc_input).small().w(px(72.))),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().text_xs().child("Language"))
                                    .child(Input::new(&self.lang_input).small().w(px(140.))),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().text_xs().child("Backup retention"))
                                    .child(Input::new(&self.retention_input).small().w(px(64.))),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().w(px(140.)).text_xs().child("Steam Web API key"))
                            .child(Input::new(&self.api_key_input).small().w(px(300.)))
                            .child(
                                Button::new("apikey-help")
                                    .xsmall()
                                    .ghost()
                                    .label("Get a free key")
                                    .on_click(|_, _, _| {
                                        open_url_in_browser(
                                            "https://steamcommunity.com/dev/apikey",
                                        );
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("save-settings")
                                    .small()
                                    .primary()
                                    .label("Save Settings")
                                    .disabled(self.app.ui_demo)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.save_settings();
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .when_some(self.app.settings_save_msg.clone(), |this, msg| {
                                this.child(div().text_xs().child(msg))
                            }),
                    )
                    .into_any_element()
            }))
            .child(section("Detected accounts", cx, |_| {
                v_flex()
                    .gap_1()
                    .children(self.app.detected_accounts.iter().map(|a| {
                        let entity = entity.clone();
                        let id = a.steam_id64.clone();
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .child(format!("{} · {}", a.persona_name, id)),
                            )
                            .child(
                                Button::new(SharedString::from(format!("acct-{id}")))
                                    .xsmall()
                                    .label("Use")
                                    .on_click(move |_, window, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.app.account_edit = id.clone();
                                            set_input(&this.account_input, &id, window, cx);
                                            cx.notify();
                                        });
                                    }),
                            )
                    }))
                    .into_any_element()
            }))
            .child(section("Write safety", cx, |_| {
                h_flex()
                    .gap_2()
                    .child(
                        Checkbox::new("allow-steam")
                            .label("Allow writes while Steam is running")
                            .checked(self.app.allow_steam_running)
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.allow_steam_running =
                                            !this.app.allow_steam_running;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .into_any_element()
            }))
            .child(section("Setup diagnostics", cx, |_| {
                v_flex()
                    .gap_2()
                    .child(
                        Button::new("diag-run")
                            .small()
                            .label("Run setup check")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.run_setup_diagnostics();
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .when_some(self.app.setup_diagnostics.clone(), |this, text| {
                        this.child(div().text_xs().child(text))
                    })
                    .child(
                        Button::new("diag-export")
                            .small()
                            .label("Export diagnostics")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("JSON", &["json"])
                                        .save_file()
                                    {
                                        entity.update(cx, |this, cx| {
                                            this.app.diagnostics_export_path =
                                                path.to_string_lossy().into();
                                            match this.app.export_diagnostics() {
                                                Ok(()) => {
                                                    this.app.success_msg =
                                                        Some("Diagnostics exported.".into());
                                                }
                                                Err(e) => this.app.error = Some(e),
                                            }
                                            cx.notify();
                                        });
                                    }
                                }
                            }),
                    )
                    .into_any_element()
            }))
            .child(self.backups(cx))
            .child(section("About", cx, |_| {
                div()
                    .text_sm()
                    .child(format!("Vapourfly {}", env!("CARGO_PKG_VERSION")))
                    .into_any_element()
            }))
    }

    fn insights_rail(
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

    fn backups(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let backups = self.app.backups.clone();
        section("Backups", cx, move |_| {
            v_flex()
                .gap_1()
                .children(backups.iter().map(|b| {
                    let entity = entity.clone();
                    let path = b.path.clone();
                    h_flex()
                        .gap_2()
                        .child(div().flex_1().text_sm().child(b.path.display().to_string()))
                        .child(
                            Button::new(SharedString::from(path.display().to_string()))
                                .xsmall()
                                .label("Restore")
                                .disabled(self.app.ui_demo)
                                .on_click(move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.begin_backup_restore(path.clone());
                                        cx.notify();
                                    });
                                }),
                        )
                }))
                .into_any_element()
        })
    }

    fn confirm_overlay(&self, entity: Entity<Self>, cx: &App) -> impl IntoElement {
        let plan = self.app.dry_run_plan.as_ref();
        v_flex()
            .id("confirm")
            .absolute()
            .inset_0()
            .items_center()
            .justify_center()
            .bg(Hsla::from(rgb(0x000000)).opacity(0.45))
            .child(
                v_flex()
                    .w(px(520.))
                    .p_5()
                    .gap_3()
                    .rounded(cx.theme().radius_lg)
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(div().text_lg().font_semibold().child("Confirm write"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if self.app.dry_run_loading {
                                "Preparing dry-run diff…".into()
                            } else if let Some(plan) = plan {
                                format!(
                                    "Target {}\n+{} / −{} app ids",
                                    plan.target_path.display(),
                                    plan.diff.app_ids_added.len(),
                                    plan.diff.app_ids_removed.len()
                                )
                            } else {
                                "Confirm this write. A backup is created first.".into()
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("confirm-go")
                                    .primary()
                                    .label("Confirm")
                                    .disabled(self.app.ui_demo || self.app.dry_run_loading)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.execute_pending_action();
                                                this.arm_poll(cx);
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("confirm-cancel")
                                    .ghost()
                                    .label("Cancel")
                                    .on_click(move |_, _, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.app.show_confirm_dialog = false;
                                            this.app.pending_action = None;
                                            this.app.dry_run_plan = None;
                                            cx.notify();
                                        });
                                    }),
                            ),
                    ),
            )
    }

    fn chooser_overlay(&self, entity: Entity<Self>, cx: &App) -> impl IntoElement {
        v_flex()
            .id("chooser")
            .absolute()
            .inset_0()
            .items_center()
            .justify_center()
            .bg(Hsla::from(rgb(0x000000)).opacity(0.45))
            .child(
                v_flex()
                    .w(px(420.))
                    .p_5()
                    .gap_3()
                    .rounded(cx.theme().radius_lg)
                    .bg(cx.theme().background)
                    .child(div().text_lg().font_semibold().child(
                        if self.app.playlist_chooser == PlaylistChooser::Dynamic {
                            "Dynamic template"
                        } else {
                            "Editorial mood"
                        },
                    ))
                    .child(if self.app.playlist_chooser == PlaylistChooser::Dynamic {
                        v_flex()
                            .gap_2()
                            .children(
                                [DynamicTemplate::DeckSession, DynamicTemplate::FinishIt]
                                    .into_iter()
                                    .map(|tmpl| {
                                        let entity = entity.clone();
                                        let id = tmpl.id().to_string();
                                        Button::new(SharedString::from(id.clone()))
                                            .label(tmpl.label())
                                            .on_click(move |_, _, cx| {
                                                entity.update(cx, |this, cx| {
                                                    this.app.dynamic_template = id.clone();
                                                    this.app.start_dynamic_generate();
                                                    this.app.playlist_chooser =
                                                        PlaylistChooser::None;
                                                    this.arm_poll(cx);
                                                    cx.notify();
                                                });
                                            })
                                    }),
                            )
                            .into_any_element()
                    } else {
                        v_flex()
                            .gap_2()
                            .children(EditorialMood::all().iter().map(|mood| {
                                let entity = entity.clone();
                                let id = mood.id().to_string();
                                Button::new(SharedString::from(id.clone()))
                                    .label(mood.name())
                                    .on_click(move |_, _, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.app.editorial_mood = id.clone();
                                            this.app.start_mood_generate();
                                            this.app.playlist_chooser = PlaylistChooser::None;
                                            this.arm_poll(cx);
                                            cx.notify();
                                        });
                                    })
                            }))
                            .into_any_element()
                    })
                    .child(
                        Button::new("chooser-cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.app.playlist_chooser = PlaylistChooser::None;
                                    cx.notify();
                                });
                            }),
                    ),
            )
    }
}

impl Render for GuiRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.app.tick();
        self.reconcile_playlist_inputs(window, cx);
        self.shell(window, cx)
    }
}

fn bind_input(
    window: &mut Window,
    cx: &mut Context<GuiRoot>,
    placeholder: &'static str,
    initial: &str,
    write: fn(&mut VapourflyApp, String),
) -> Entity<InputState> {
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(initial.to_string())
    });
    cx.subscribe(&input, move |this, input, ev: &InputEvent, cx| {
        if matches!(ev, InputEvent::Change) {
            write(&mut this.app, input.read(cx).value().to_string());
            cx.notify();
        }
    })
    .detach();
    input
}

fn bind_filter_input(
    window: &mut Window,
    cx: &mut Context<GuiRoot>,
    placeholder: &'static str,
    initial: &str,
    write: fn(&mut VapourflyApp, String),
) -> Entity<InputState> {
    let input = cx.new(|cx| {
        InputState::new(window, cx)
            .placeholder(placeholder)
            .default_value(initial.to_string())
    });
    cx.subscribe(&input, move |this, input, ev: &InputEvent, cx| {
        if matches!(ev, InputEvent::Change) {
            write(&mut this.app, input.read(cx).value().to_string());
            this.app.library_visible_count = 48;
            cx.notify();
        }
    })
    .detach();
    input
}

fn set_input(
    input: &Entity<InputState>,
    value: &str,
    window: &mut Window,
    cx: &mut Context<GuiRoot>,
) {
    input.update(cx, |state, cx| {
        state.set_value(value.to_string(), window, cx);
    });
}

fn insight_tile(label: &str, value: String, cx: &App) -> impl IntoElement {
    h_flex()
        .justify_between()
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(SharedString::from(label.to_string())),
        )
        .child(div().text_xs().font_semibold().child(value))
}

fn section(title: &str, cx: &App, body: impl FnOnce(&App) -> gpui::AnyElement) -> impl IntoElement {
    v_flex()
        .gap_2()
        .p_3()
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius)
        .child(
            div()
                .text_sm()
                .font_semibold()
                .child(SharedString::from(title.to_string())),
        )
        .child(body(cx))
}

fn empty_or(value: &str) -> &str {
    if value.trim().is_empty() {
        empty_value_label()
    } else {
        value
    }
}

fn this_tier_label(tier: Option<ProtonTier>) -> String {
    tier.map(|t| format!("{t:?}"))
        .unwrap_or_else(|| empty_value_label().into())
}
