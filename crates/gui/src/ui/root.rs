//! GPUI entity root owning [`crate::app::VapourflyApp`] and its input bindings.

use std::path::PathBuf;
use std::time::Duration;

use gpui::{AppContext, Context, Entity, IntoElement, Window, prelude::*};
use gpui_component::input::{InputEvent, InputState};

use super::appearance::{apply_tokens, load_persisted_theme, persist_theme};
use crate::app::{RepaintHook, VapourflyApp, View};
use crate::jobs::JobWake;

pub struct GuiRoot {
    pub(crate) app: VapourflyApp,
    pub(crate) search: Entity<InputState>,
    pub(crate) steam_dir_input: Entity<InputState>,
    pub(crate) account_input: Entity<InputState>,
    pub(crate) cc_input: Entity<InputState>,
    pub(crate) lang_input: Entity<InputState>,
    pub(crate) retention_input: Entity<InputState>,
    pub(crate) api_key_input: Entity<InputState>,
    pub(crate) genre_input: Entity<InputState>,
    pub(crate) tag_input: Entity<InputState>,
    pub(crate) playtime_min_input: Entity<InputState>,
    pub(crate) playtime_max_input: Entity<InputState>,
    pub(crate) hltb_min_input: Entity<InputState>,
    pub(crate) hltb_max_input: Entity<InputState>,
    pub(crate) playlist_name_input: Entity<InputState>,
    pub(crate) playlist_id_input: Entity<InputState>,
    pub(crate) playlist_desc_input: Entity<InputState>,
    pub(crate) playlist_csv_input: Entity<InputState>,
    pub(crate) playlist_edit_synced: u64,
    pub(crate) poll_armed: bool,
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

    pub(crate) fn sync_filter_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(crate) fn sync_playlist_inputs(&self, window: &mut Window, cx: &mut Context<Self>) {
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

    pub(crate) fn reconcile_playlist_inputs(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    pub(crate) fn wire_repaint(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn arm_poll(&mut self, cx: &mut Context<Self>) {
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

    pub(crate) fn set_view(&mut self, view: View, cx: &mut Context<Self>) {
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

    pub(crate) fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.app.theme_mode = self.app.theme_mode.toggle();
        persist_theme(&self.app, self.app.theme_mode);
        apply_tokens(window, cx, self.app.theme_mode);
        cx.notify();
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

pub(crate) fn set_input(
    input: &Entity<InputState>,
    value: &str,
    window: &mut Window,
    cx: &mut Context<GuiRoot>,
) {
    input.update(cx, |state, cx| {
        state.set_value(value.to_string(), window, cx);
    });
}
