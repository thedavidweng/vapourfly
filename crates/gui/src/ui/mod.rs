//! GPUI + gpui-component presentation for [`crate::app::VapourflyApp`].
//!
//! The shell paints the sidebar and title bar on the `surface` token and
//! sets the active page flush beside it on the lighter `canvas` panel, as
//! SteamOS does. Each destination
//! lives in its own module; shared building blocks are in [`widgets`].

mod controller;
mod explore;
mod library;
mod manage;
mod playlists;
mod widgets;

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Colorize, Disableable, Sizable, StyledExt, Theme, ThemeMode as GpuiThemeMode,
    TitleBar, WindowExt,
    button::{Button, ButtonVariants},
    dialog::DialogFooter,
    h_flex,
    input::{InputEvent, InputState},
    notification::Notification,
    select::{SearchableVec, SelectEvent, SelectState},
    spinner::Spinner,
    v_flex,
};
use gpui_kit::{
    App, AppContext, Context, Entity, FocusHandle, Hsla, IntoElement, ListAlignment, ListState,
    ParentElement, SharedString, Styled, UniformListScrollHandle, Window, div, linear_color_stop,
    linear_gradient, prelude::*, px, rgb,
};

use crate::app::{PendingAction, RepaintHook, VapourflyApp, View};
use crate::artwork::ArtworkStore;
use crate::jobs::JobWake;
use crate::platform::DeviceProfile;
use crate::theme::{self, SIDEBAR_WIDTH, SIDEBAR_WIDTH_COMPACT, ThemeMode, set_active_theme};
use controller::CardScope;
use vapourfly_core::models::Game;

pub(crate) fn hx(c: theme::Rgb) -> Hsla {
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
    let t = theme::t();
    let dark = mode.is_dark();
    Theme::update(cx, |theme| {
        // The root sets rem from font_size, so this scales every rem-based
        // spacing and control height along with the text.
        theme.font_size = px(16.);
        theme.radius = px(theme::CORNER_MD);
        theme.radius_lg = px(theme::CORNER_LG);
        theme.shadow = true;

        let accent = hx(t.accent);
        theme.background = hx(t.canvas);
        theme.foreground = hx(t.text_primary);
        theme.border = hx(t.border);
        theme.input = hx(t.border);
        // The gamepad UI marks focus in white; blue stays the selection colour.
        theme.ring = if dark { gpui_kit::white() } else { accent };
        theme.selection = accent.opacity(0.28);
        theme.caret = accent;
        theme.link = hx(t.accent_text);
        theme.link_hover = hx(t.accent_text);
        theme.link_active = hx(t.accent_text);

        theme.primary = accent;
        // Steam's primary DialogButton brightens toward #47bfff on hover.
        let primary_hover = if dark {
            rgb(0x47bfff).into()
        } else {
            accent.darken(0.06)
        };
        theme.primary_hover = primary_hover;
        theme.primary_active = accent.darken(0.06);
        theme.primary_foreground = hx(t.text_inverse);
        theme.button_primary = accent;
        theme.button_primary_hover = primary_hover;
        theme.button_primary_active = accent.darken(0.06);
        theme.button_primary_foreground = hx(t.text_inverse);

        theme.secondary = hx(t.surface_muted);
        theme.secondary_hover = hx(t.control_hover);
        theme.secondary_active = hx(t.control_active);
        theme.secondary_foreground = hx(t.text_primary);
        theme.button = hx(t.surface_muted);
        theme.button_hover = hx(t.control_hover);
        theme.button_active = hx(t.control_active);
        theme.button_foreground = hx(t.text_primary);
        theme.button_secondary = hx(t.surface_muted);
        theme.button_secondary_hover = hx(t.control_hover);
        theme.button_secondary_active = hx(t.control_active);
        theme.button_secondary_foreground = hx(t.text_primary);

        theme.muted = hx(t.surface_muted);
        theme.muted_foreground = hx(t.text_muted);
        theme.accent = hx(t.surface_muted);
        theme.accent_foreground = hx(t.text_primary);
        theme.popover = hx(t.surface_raised);
        theme.popover_foreground = hx(t.text_primary);
        theme.overlay = Hsla::black().opacity(if dark { 0.6 } else { 0.32 });

        theme.sidebar = hx(t.surface);
        theme.sidebar_foreground = hx(t.text_secondary);
        theme.sidebar_accent = hx(t.surface_muted);
        theme.sidebar_accent_foreground = hx(t.text_primary);
        theme.sidebar_border = hx(t.surface);
        theme.sidebar_primary = accent;
        theme.sidebar_primary_foreground = hx(t.text_inverse);
        theme.title_bar = hx(t.surface);
        theme.title_bar_border = hx(t.surface);

        theme.colors.list = hx(t.canvas);
        theme.list_even = hx(t.canvas);
        theme.list_head = hx(t.canvas);
        theme.list_hover = hx(t.surface_muted);
        theme.list_active = hx(t.accent_soft);
        theme.list_active_border = accent;
        theme.table = hx(t.canvas);
        theme.table_even = hx(t.canvas);
        theme.table_head = hx(t.canvas);
        theme.table_head_foreground = hx(t.text_muted);
        theme.table_hover = hx(t.surface_muted);
        theme.table_active = hx(t.accent_soft);
        theme.table_active_border = accent;
        theme.table_row_border = hx(t.border_soft);

        theme.tab_bar = hx(t.canvas);
        theme.tab_bar_segmented = hx(t.surface_muted);
        theme.tab = hx(t.canvas).opacity(0.);
        theme.tab_active = if dark {
            hx(t.surface_muted).lighten(0.08)
        } else {
            hx(t.surface_raised)
        };
        theme.tab_active_foreground = hx(t.text_primary);
        theme.tab_foreground = hx(t.text_secondary);

        theme.group_box = hx(t.surface_raised);
        theme.group_box_foreground = hx(t.text_primary);
        theme.description_list_label = hx(t.surface_sunken);
        theme.description_list_label_foreground = hx(t.text_muted);
        theme.skeleton = hx(t.surface_muted);
        theme.progress_bar = accent;
        theme.slider_bar = accent;
        theme.slider_thumb = hx(t.text_inverse);
        theme.switch = hx(t.border);
        theme.switch_thumb = gpui_kit::white();
        theme.scrollbar_thumb = hx(t.text_muted).opacity(0.35);
        theme.scrollbar_thumb_hover = hx(t.text_muted).opacity(0.55);

        theme.danger = hx(t.error);
        theme.danger_hover = hx(t.error).lighten(0.04);
        theme.danger_active = hx(t.error).darken(0.04);
        theme.danger_foreground = hx(t.text_inverse);
        // Destructive buttons fill with Steam's #de3618 rather than the
        // softer red used for error text.
        let danger_fill: Hsla = if dark {
            rgb(0xde3618).into()
        } else {
            hx(t.error)
        };
        theme.button_danger = danger_fill;
        theme.button_danger_hover = danger_fill.lighten(0.06);
        theme.button_danger_active = danger_fill.darken(0.06);
        theme.button_danger_foreground = hx(t.text_inverse);
        theme.success = hx(t.success);
        theme.warning = hx(t.warning);
    });
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
        .map_or(ThemeMode::Dark, ThemeMode::from_u8)
}

/// Grid or row presentation for the Library page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum LibraryLayout {
    #[default]
    Grid,
    List,
}

type SeedSelect = SelectState<SearchableVec<SharedString>>;

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
    playlist_search_input: Entity<InputState>,
    rule_genre_input: Entity<InputState>,
    rule_tag_input: Entity<InputState>,
    rule_hltb_input: Entity<InputState>,
    rule_playtime_min_input: Entity<InputState>,
    rule_playtime_max_input: Entity<InputState>,
    rule_rating_input: Entity<InputState>,
    discover_seed_select: Entity<SeedSelect>,
    /// (display name, app id) behind each entry of `discover_seed_select`.
    discover_seed_options: Vec<(SharedString, u32)>,
    playlist_edit_synced: u64,
    poll_armed: bool,
    artwork: ArtworkStore,
    library_layout: LibraryLayout,
    /// Scroll state and rows of the Library page (hero, shelf, grid rows).
    library_scroll: ListState,
    library_items: Vec<library::LibraryItem>,
    /// Games behind `library_items`, filtered once per frame.
    library_games: Rc<Vec<Game>>,
    library_signature: (usize, usize, Option<u32>, u32),
    playlist_share_tab_open: bool,
    sidebar_collapsed: bool,
    confirm_open: bool,
    last_notice: Option<(String, Instant)>,
    /// Desktop, SteamOS desktop, or Deck / Game Mode.
    device: DeviceProfile,
    /// Name of the connected controller, if any.
    pad_name: Option<String>,
    /// The controller was the most recent input; drives the button hints.
    pad_active: bool,
    /// Sidebar entries in [`View::ALL`] order.
    nav_focus: Vec<FocusHandle>,
    /// The page container; focusing it then `focus_next` enters the page.
    page_focus: FocusHandle,
    /// Library cards and rows, keyed so the controller cursor can find and
    /// focus cards in rows the virtualized list has not rendered yet.
    card_focus: RefCell<HashMap<(CardScope, u32), FocusHandle>>,
    library_rows_scroll: UniformListScrollHandle,
}

/// Command-line and environment choices the window opens with.
#[derive(Clone, Debug, Default)]
pub struct LaunchOptions {
    pub fixtures: Option<PathBuf>,
    pub ui_demo: bool,
    pub offline: bool,
    pub theme: Option<ThemeMode>,
    pub start_view: Option<String>,
    pub device: DeviceProfile,
}

impl GuiRoot {
    pub fn new(window: &mut Window, cx: &mut Context<Self>, options: LaunchOptions) -> Self {
        let LaunchOptions {
            fixtures,
            ui_demo,
            offline,
            theme: theme_override,
            start_view,
            device,
        } = options;
        let mut app = VapourflyApp::new(fixtures, ui_demo);
        app.offline_mode = offline;
        app.theme_mode = theme_override.unwrap_or_else(|| {
            if ui_demo {
                ThemeMode::Dark
            } else {
                load_persisted_theme()
            }
        });
        if ui_demo {
            app.populate_demo_data();
        } else {
            app.load_accounts_quietly();
        }
        apply_tokens(window, cx, app.theme_mode);
        window.set_window_title("Vapourfly");

        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search your library"));
        cx.subscribe(&search, |this, input, ev: &InputEvent, cx| {
            if matches!(ev, InputEvent::Change) {
                this.app.search_query = input.read(cx).value().to_string();
                this.app.library_visible_count = 48;
                cx.notify();
            }
        })
        .detach();

        // Empty fields fall through to env vars, detection or defaults, so
        // the placeholders show the value that applies when left empty.
        let effective = app.config.clone();
        let steam_dir_hint = effective.as_ref().map_or_else(
            || "/path/to/Steam".to_string(),
            |c| c.steam_dir.to_string_lossy().into_owned(),
        );
        let cc_hint = effective.as_ref().map_or("US".into(), |c| c.cc.clone());
        let lang_hint = effective
            .as_ref()
            .map_or("english".into(), |c| c.lang.clone());
        let retention_hint = effective
            .as_ref()
            .map_or("5".into(), |c| c.backup_retention_count.to_string());
        let api_key_hint = if app.steam_api_key_edit.is_empty()
            && effective
                .as_ref()
                .is_some_and(|c| c.steam_api_key.is_some())
        {
            "Using VAPOURFLY_STEAM_API_KEY"
        } else {
            "Paste your key, or leave empty to remove it"
        };
        let steam_dir_input =
            bind_input(window, cx, steam_dir_hint, &app.steam_dir_edit, |app, v| {
                app.steam_dir_edit = v;
            });
        let account_input = bind_input(
            window,
            cx,
            "Detected automatically",
            &app.account_edit,
            |app, v| {
                app.account_edit = v;
            },
        );
        let cc_input = bind_input(window, cx, cc_hint, &app.cc_edit, |app, v| {
            app.cc_edit = v;
        });
        let lang_input = bind_input(window, cx, lang_hint, &app.lang_edit, |app, v| {
            app.lang_edit = v;
        });
        let retention_input = bind_input(
            window,
            cx,
            retention_hint,
            &app.backup_retention_edit,
            |app, v| {
                app.backup_retention_edit = v;
            },
        );
        let api_key_input = bind_input(
            window,
            cx,
            api_key_hint,
            &app.steam_api_key_edit,
            |app, v| {
                app.steam_api_key_edit = v;
            },
        );
        api_key_input.update(cx, |state, cx| state.set_masked(true, window, cx));
        let genre_input =
            bind_filter_input(window, cx, "Any genre", &app.filter_genre, |app, v| {
                app.filter_genre = v;
            });
        let tag_input = bind_filter_input(window, cx, "Any tag", &app.filter_tag, |app, v| {
            app.filter_tag = v;
        });
        let playtime_min_input =
            bind_filter_input(window, cx, "Min", &app.filter_playtime_min, |app, v| {
                app.filter_playtime_min = v;
            });
        let playtime_max_input =
            bind_filter_input(window, cx, "Max", &app.filter_playtime_max, |app, v| {
                app.filter_playtime_max = v;
            });
        let hltb_min_input =
            bind_filter_input(window, cx, "Min", &app.filter_hltb_min, |app, v| {
                app.filter_hltb_min = v;
            });
        let hltb_max_input =
            bind_filter_input(window, cx, "Max", &app.filter_hltb_max, |app, v| {
                app.filter_hltb_max = v;
            });
        let playlist_name_input = bind_input(
            window,
            cx,
            "Untitled playlist",
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
            "Add a description",
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
        let playlist_search_input = bind_input(
            window,
            cx,
            "Search games to add",
            &app.playlist_game_search,
            |app, v| app.playlist_game_search = v,
        );
        let rule_genre_input = bind_input(
            window,
            cx,
            "e.g. Cozy",
            &app.playlist_rule_genre,
            |app, v| app.playlist_rule_genre = v,
        );
        let rule_tag_input = bind_input(
            window,
            cx,
            "e.g. multiplayer",
            &app.playlist_rule_tag,
            |app, v| app.playlist_rule_tag = v,
        );
        let rule_hltb_input = bind_input(
            window,
            cx,
            "Minutes",
            &app.playlist_rule_hltb_max,
            |app, v| app.playlist_rule_hltb_max = v,
        );
        let rule_playtime_min_input = bind_input(
            window,
            cx,
            "Min",
            &app.playlist_rule_playtime_min,
            |app, v| app.playlist_rule_playtime_min = v,
        );
        let rule_playtime_max_input = bind_input(
            window,
            cx,
            "Max",
            &app.playlist_rule_playtime_max,
            |app, v| app.playlist_rule_playtime_max = v,
        );
        let rule_rating_input = bind_input(
            window,
            cx,
            "0 – 5",
            &app.playlist_rule_rating_min,
            |app, v| app.playlist_rule_rating_min = v,
        );

        let discover_seed_select = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<SharedString>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        cx.subscribe(
            &discover_seed_select,
            |this, _, ev: &SelectEvent<SearchableVec<SharedString>>, cx| {
                let SelectEvent::Confirm(value) = ev;
                match value {
                    Some(value) => {
                        if let Some((_, id)) =
                            this.discover_seed_options.iter().find(|(n, _)| n == value)
                        {
                            this.app.discover_seed = id.to_string();
                        }
                    }
                    None => this.app.discover_seed.clear(),
                }
                cx.notify();
            },
        )
        .detach();

        let wake = JobWake::new();
        let art_wake = wake.clone();
        let artwork = ArtworkStore::new(app.cache_dir.clone(), Arc::new(move || art_wake.signal()));

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
            playlist_search_input,
            rule_genre_input,
            rule_tag_input,
            rule_hltb_input,
            rule_playtime_min_input,
            rule_playtime_max_input,
            rule_rating_input,
            discover_seed_select,
            discover_seed_options: Vec::new(),
            playlist_edit_synced: 0,
            poll_armed: false,
            artwork,
            library_layout: LibraryLayout::Grid,
            library_scroll: ListState::new(0, ListAlignment::Top, px(800.)),
            library_items: Vec::new(),
            library_games: Rc::new(Vec::new()),
            library_signature: (0, 0, None, 0),
            playlist_share_tab_open: false,
            sidebar_collapsed: false,
            confirm_open: false,
            last_notice: None,
            device,
            pad_name: None,
            pad_active: false,
            nav_focus: View::ALL
                .iter()
                .map(|_| cx.focus_handle().tab_stop(true))
                .collect(),
            page_focus: cx.focus_handle(),
            card_focus: RefCell::new(HashMap::new()),
            library_rows_scroll: UniformListScrollHandle::new(),
        };
        this.wire_repaint(wake, cx);
        this.wire_gamepad(window, cx);
        if let Some(view) = start_view.as_deref().and_then(View::from_slug) {
            this.set_view(view, cx);
        }
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

    /// Keep the Discover seed picker in step with the scanned library.
    fn reconcile_seed_options(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(games) = self.app.scan_result.as_ref().map(|s| &s.games) else {
            return;
        };
        if games.len() == self.discover_seed_options.len() {
            return;
        }
        let mut options: Vec<(SharedString, u32)> = games
            .iter()
            .map(|g| (SharedString::from(g.name.clone()), g.app_id))
            .collect();
        options.sort_by_key(|o| o.0.to_lowercase());
        let names: Vec<SharedString> = options.iter().map(|(n, _)| n.clone()).collect();
        self.discover_seed_options = options;
        self.discover_seed_select.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(names), window, cx);
        });
    }

    fn wire_repaint(&mut self, wake: JobWake, cx: &mut Context<Self>) {
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
        self.app.show_junk_panel = false;
        if view == View::Settings && !self.app.ui_demo {
            if self.app.detected_accounts.is_empty() {
                self.app.refresh_detected_accounts();
            }
            self.app.refresh_backups();
        }
        if view == View::Playlists {
            self.app.refresh_playlist_store_ids();
        }
        cx.notify();
    }

    fn toggle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.app.theme_mode = self.app.theme_mode.toggle();
        persist_theme(&self.app, self.app.theme_mode);
        apply_tokens(window, cx, self.app.theme_mode);
        cx.notify();
    }

    /// Show `error` / `success_msg` as toasts. The same text is not repeated
    /// within a few seconds so a job that re-reports its state stays quiet.
    fn flush_notices(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let notices = [
            self.app.error.take().map(|m| (m, true)),
            self.app.success_msg.take().map(|m| (m, false)),
        ];
        for (message, is_error) in notices.into_iter().flatten() {
            let repeat = self.last_notice.as_ref().is_some_and(|(last, at)| {
                *last == message && at.elapsed() < Duration::from_secs(4)
            });
            if repeat {
                continue;
            }
            self.last_notice = Some((message.clone(), Instant::now()));
            let note = if is_error {
                Notification::error(message)
            } else {
                Notification::success(message)
            };
            window.push_notification(note, cx);
        }
    }

    /// Mirror the app's confirmation gate into a modal dialog.
    fn sync_confirm_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let wanted = self.app.show_confirm_dialog;
        if wanted && !self.confirm_open {
            self.confirm_open = true;
            let entity = cx.entity();
            window.open_dialog(cx, move |dialog, _, cx| {
                confirm_dialog(dialog, entity.clone(), cx)
            });
        } else if !wanted && self.confirm_open {
            self.confirm_open = false;
            if window.has_active_dialog(cx) {
                window.close_dialog(cx);
            }
        }
    }

    fn sidebar_width(&self, window: &Window) -> f32 {
        if self.is_sidebar_collapsed(window) {
            SIDEBAR_WIDTH_COMPACT
        } else {
            SIDEBAR_WIDTH
        }
    }

    fn is_sidebar_collapsed(&self, window: &Window) -> bool {
        let width: f32 = window.viewport_size().width.into();
        self.sidebar_collapsed || theme::is_compact_sidebar(width)
    }

    fn shell(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let width: f32 = window.viewport_size().width.into();
        self.app.rails_below = theme::rails_below(width);
        let view = self.app.current_view;
        let page = match view {
            View::Library if self.app.show_junk_panel => {
                self.junk_panel(window, cx).into_any_element()
            }
            View::Library => self.library(window, cx).into_any_element(),
            View::Discover => self.discover(window, cx).into_any_element(),
            View::Recommendations => self.recommend(cx).into_any_element(),
            View::Playlists => self.playlists(cx).into_any_element(),
            View::Collections => self.collections(window, cx).into_any_element(),
            View::DataSources => self.data_sources(cx).into_any_element(),
            View::Settings => self.settings(cx).into_any_element(),
        };

        // Dark pages pick up a faint Steam-blue glow at the top edge.
        let canvas = cx.theme().background;
        let page_bg = if self.app.theme_mode.is_dark() {
            linear_gradient(
                180.,
                linear_color_stop(canvas.blend(hx(theme::t().accent_soft).opacity(0.6)), 0.),
                linear_color_stop(canvas, 0.32),
            )
        } else {
            canvas.into()
        };
        let hints = self.pad_name.is_some() && self.pad_active && window.last_input_was_keyboard();
        v_flex()
            .size_full()
            .bg(cx.theme().sidebar)
            .text_color(cx.theme().foreground)
            .child(self.title_bar(window, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(self.sidebar(window, cx))
                    .child(
                        v_flex()
                            .id("page")
                            .track_focus(&self.page_focus)
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .overflow_hidden()
                            .bg(page_bg)
                            .child(page),
                    ),
            )
            .when(hints, |this| this.child(self.controller_hints(cx)))
    }

    /// Steam-style footer naming what each controller button does. Shown
    /// while the controller is the active input; any mouse use hides it.
    fn controller_hints(&self, cx: &App) -> impl IntoElement {
        let glyph = |label: &'static str| {
            div()
                .flex_none()
                .min_w(px(24.))
                .h(px(24.))
                .px(px(if label.len() > 1 { 6. } else { 0. }))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(cx.theme().foreground.opacity(0.9))
                .text_color(cx.theme().sidebar)
                .text_size(px(12.))
                .font_bold()
                .child(label)
        };
        let hint = |glyphs: &[&'static str], text: &'static str| {
            h_flex()
                .gap_1p5()
                .children(glyphs.iter().map(|g| glyph(g)))
                .child(
                    div()
                        .text_size(px(14.))
                        .text_color(cx.theme().foreground)
                        .child(text),
                )
        };
        h_flex()
            .id("controller-hints")
            .flex_none()
            .w_full()
            .h(px(44.))
            .px(px(22.))
            .gap(px(22.))
            .justify_end()
            .border_t_1()
            .border_color(cx.theme().border.opacity(0.6))
            .child(hint(&["LB", "RB"], "Switch page"))
            .child(hint(&["R"], "Scroll"))
            .child(hint(&["Y"], "Search"))
            .when(self.device.has_steam_keyboard(), |this| {
                this.child(hint(&["X"], "Keyboard"))
            })
            .child(hint(&["B"], "Back"))
            .child(hint(&["A"], "Select"))
    }

    fn title_bar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.is_sidebar_collapsed(window);
        let (status, busy, tone) = self.sync_status();
        let dark = self.app.theme_mode.is_dark();
        TitleBar::new().h(px(52.)).child(
            h_flex()
                .w_full()
                .pr_3()
                .gap_2()
                .child(
                    Button::new("toggle-sidebar")
                        .ghost()
                        .icon(if collapsed {
                            IconName::PanelLeftOpen
                        } else {
                            IconName::PanelLeftClose
                        })
                        .tooltip(if collapsed {
                            "Show sidebar"
                        } else {
                            "Hide sidebar"
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.sidebar_collapsed = !this.is_sidebar_collapsed(window);
                            cx.notify();
                        })),
                )
                .child(div().flex_1())
                .child(
                    h_flex()
                        .gap_2()
                        .px_2p5()
                        .h(px(30.))
                        .rounded_full()
                        .border_1()
                        .border_color(cx.theme().border)
                        .text_size(px(13.))
                        .text_color(cx.theme().muted_foreground)
                        .child(if busy {
                            Spinner::new().small().into_any_element()
                        } else {
                            div()
                                .size(px(6.))
                                .rounded_full()
                                .bg(tone)
                                .into_any_element()
                        })
                        .child(status),
                )
                .child(
                    Button::new("theme-toggle")
                        .ghost()
                        .icon(if dark { IconName::Sun } else { IconName::Moon })
                        .tooltip(if dark {
                            "Switch to light"
                        } else {
                            "Switch to dark"
                        })
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.toggle_theme(window, cx);
                        })),
                ),
        )
    }

    fn sync_status(&self) -> (SharedString, bool, Hsla) {
        let t = theme::t();
        if self.app.loading {
            return ("Scanning library…".into(), true, hx(t.accent));
        }
        if self.app.write_loading {
            return ("Writing to Steam…".into(), true, hx(t.accent));
        }
        if self.app.cache_refresh_loading {
            return ("Refreshing metadata…".into(), true, hx(t.accent));
        }
        if self.app.ui_demo {
            return ("Demo library".into(), false, hx(t.warning));
        }
        if self.app.offline_mode {
            return ("Offline".into(), false, hx(t.text_muted));
        }
        if self.app.scan_result.is_some() {
            ("Up to date".into(), false, hx(t.success))
        } else {
            ("Not scanned".into(), false, hx(t.text_muted))
        }
    }

    fn sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let collapsed = self.is_sidebar_collapsed(window);
        let current = self.app.current_view;
        let games = self.app.scan_result.as_ref().map_or(0, |s| s.games.len());
        let playlists = self.app.playlist_store_ids.len();
        let collections = self.app.collections.len();
        let t = theme::t();
        let dark = self.app.theme_mode.is_dark();
        let muted = cx.theme().muted_foreground;
        // SteamOS marks the current destination with a full-width band and
        // a white label rather than a rounded pill.
        let active_bg = if dark {
            gpui_kit::white().opacity(0.08)
        } else {
            gpui_kit::black().opacity(0.06)
        };
        // Hover and keyboard/controller focus share one band, as in SteamOS,
        // and it is brighter than the current-page band so the two never
        // read as the same state.
        let hover_bg = hx(t.control_hover);
        let item = |dest: View, icon: IconName, count: Option<usize>| {
            let active = current == dest;
            h_flex()
                .id(SharedString::from(format!("nav-{}", dest.label())))
                .track_focus(self.nav_handle(dest))
                .h(px(48.))
                .px(px(if collapsed { 0. } else { 22. }))
                .when(collapsed, |this| this.justify_center())
                .gap(px(14.))
                .cursor_pointer()
                .text_size(px(16.))
                .font_medium()
                .when(active, |this| {
                    this.bg(active_bg).text_color(cx.theme().foreground)
                })
                .when(!active, |this| {
                    this.text_color(hx(t.text_secondary))
                        .hover(|s| s.bg(hover_bg).text_color(cx.theme().foreground))
                })
                .focus_visible(|s| s.bg(hover_bg).text_color(cx.theme().foreground))
                .on_click(cx.listener(move |this, _, _, cx| this.set_view(dest, cx)))
                .child(
                    gpui_kit::component::Icon::new(icon)
                        .size(px(20.))
                        .text_color(if active { cx.theme().foreground } else { muted }),
                )
                .when(!collapsed, |this| {
                    this.child(div().flex_1().truncate().child(dest.label()))
                        .when_some(count.filter(|n| *n > 0), |this, n| {
                            this.child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(muted)
                                    .child(n.to_string()),
                            )
                        })
                })
        };
        let divider = || {
            div()
                .my(px(8.))
                .mx(px(if collapsed { 14. } else { 22. }))
                .h(px(1.))
                .bg(cx.theme().border.opacity(0.7))
        };

        v_flex()
            .id("nav")
            .flex_none()
            .h_full()
            .w(px(self.sidebar_width(window)))
            .pb_4()
            .child(profile(collapsed, &self.app, cx))
            .child(divider())
            .child(item(View::Library, IconName::LibraryBig, Some(games)))
            .child(item(View::Discover, IconName::Compass, None))
            .child(item(View::Recommendations, IconName::Sparkles, None))
            .child(divider())
            .child(item(View::Playlists, IconName::ListMusic, Some(playlists)))
            .child(item(View::Collections, IconName::Layers, Some(collections)))
            .child(divider())
            .child(item(View::DataSources, IconName::Database, None))
            .child(item(View::Settings, IconName::Settings, None))
            .child(div().flex_1())
            .when(!collapsed, |this| {
                this.child(
                    div()
                        .px(px(22.))
                        .text_size(px(13.))
                        .text_color(muted)
                        .child(format!("Vapourfly {}", env!("CARGO_PKG_VERSION"))),
                )
            })
    }
}

/// The signed-in Steam user: avatar, persona name and login name, the way
/// SteamOS heads its side menu.
fn profile(collapsed: bool, app: &VapourflyApp, cx: &App) -> impl IntoElement {
    let account = app.active_account();
    let avatar = widgets::avatar(
        account.and_then(|a| app.avatar_path(a)),
        account.map(|a| a.persona_name.as_str()),
        if collapsed { 40. } else { 48. },
    );
    let (name, detail) = match account {
        Some(a) => (a.persona_name.clone(), a.account_name.clone()),
        None => ("Steam".to_string(), "No account detected".to_string()),
    };
    h_flex()
        .h(px(84.))
        .gap(px(14.))
        .px(px(if collapsed { 0. } else { 22. }))
        .when(collapsed, |this| this.justify_center())
        .child(avatar)
        .when(!collapsed, |this| {
            this.child(
                v_flex()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .font_bold()
                            .text_color(cx.theme().foreground)
                            .truncate()
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(detail),
                    ),
            )
        })
}

fn confirm_dialog(
    dialog: gpui_kit::component::dialog::Dialog,
    entity: Entity<GuiRoot>,
    cx: &App,
) -> gpui_kit::component::dialog::Dialog {
    let this = entity.read(cx);
    let app = &this.app;
    let t = theme::t();
    let restore = matches!(app.pending_action, Some(PendingAction::BackupRestore(_)));
    let title = match &app.pending_action {
        Some(PendingAction::JunkApply) => "Add games to the Junk collection?",
        Some(PendingAction::JunkHide) => "Hide these games in Steam?",
        Some(PendingAction::RecommendCollection) => "Save picks as a Steam collection?",
        Some(PendingAction::PlaylistSync(_)) => "Sync playlist to Steam?",
        Some(PendingAction::BackupRestore(_)) => "Restore this backup?",
        None => "Confirm write",
    };

    let body = if app.dry_run_loading {
        h_flex()
            .gap_2()
            .text_size(px(15.))
            .text_color(cx.theme().muted_foreground)
            .child(Spinner::new())
            .child("Preparing a preview of the change…")
            .into_any_element()
    } else if let Some(plan) = app.dry_run_plan.as_ref() {
        let added = plan.diff.app_ids_added.len();
        let removed = plan.diff.app_ids_removed.len();
        v_flex()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .child(widgets::stat_tile(
                        "Added",
                        format!("+{added}"),
                        Some(hx(t.success)),
                        cx,
                    ))
                    .child(widgets::stat_tile(
                        "Removed",
                        format!("−{removed}"),
                        Some(hx(t.error)),
                        cx,
                    )),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(widgets::caption("Target file", cx))
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_color(cx.theme().muted_foreground)
                            .child(plan.target_path.display().to_string()),
                    ),
            )
            .into_any_element()
    } else if restore {
        div()
            .text_size(px(15.))
            .text_color(cx.theme().muted_foreground)
            .child("Steam's collections file will be replaced with this backup. The current file is backed up first.")
            .into_any_element()
    } else {
        div()
            .text_size(px(15.))
            .text_color(cx.theme().muted_foreground)
            .child("Nothing to preview yet.")
            .into_any_element()
    };

    let confirm_disabled = app.ui_demo || app.dry_run_loading;
    let cancel_entity = entity.clone();
    let close_entity = entity.clone();
    dialog
        .title(title)
        .w(px(480.))
        .on_close(move |_, _, cx| {
            close_entity.update(cx, |this, cx| {
                this.confirm_open = false;
                this.app.show_confirm_dialog = false;
                this.app.pending_action = None;
                this.app.dry_run_plan = None;
                cx.notify();
            });
        })
        .child(
            v_flex().gap_4().child(body).child(
                h_flex()
                    .gap_2()
                    .p_2p5()
                    .rounded(cx.theme().radius)
                    .bg(cx.theme().muted)
                    .text_size(px(13.))
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        gpui_kit::component::Icon::new(IconName::ShieldCheck)
                            .text_color(hx(t.success)),
                    )
                    .child("Vapourfly backs up Steam's collections file before every write."),
            )
            // Steam never exits in Game Mode, so the running-Steam guard
            // would refuse the write with no obvious way forward.
            .when(
                this.device.game_mode && !app.allow_steam_running && !app.ui_demo,
                |this| {
                    this.child(
                        h_flex()
                            .gap_2()
                            .p_2p5()
                            .rounded(cx.theme().radius)
                            .bg(hx(t.warning_soft))
                            .text_size(px(13.))
                            .text_color(hx(t.warning))
                            .child(gpui_kit::component::Icon::new(IconName::TriangleAlert))
                            .child(
                                "Steam is always running in Game Mode, so this write will be refused. \
                                 Switch to Desktop Mode and exit Steam first.",
                            ),
                    )
                },
            ),
        )
        .footer(
            DialogFooter::new()
                .child(
                    Button::new("confirm-cancel")
                        .ghost()
                        .label("Cancel")
                        .on_click(move |_, window, cx| {
                            cancel_entity.update(cx, |this, cx| {
                                this.confirm_open = false;
                                this.app.show_confirm_dialog = false;
                                this.app.pending_action = None;
                                this.app.dry_run_plan = None;
                                cx.notify();
                            });
                            window.close_dialog(cx);
                        }),
                )
                .child(
                    Button::new("confirm-go")
                        .primary()
                        .label(if restore { "Restore" } else { "Write to Steam" })
                        .disabled(confirm_disabled)
                        .on_click(move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.confirm_open = false;
                                this.app.execute_pending_action();
                                this.arm_poll(cx);
                                cx.notify();
                            });
                            window.close_dialog(cx);
                        }),
                ),
        )
}

impl Render for GuiRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.app.tick();
        self.artwork.configure(
            self.app
                .scan_result
                .as_ref()
                .map(|s| PathBuf::from(&s.steam_dir))
                .filter(|p| !p.as_os_str().is_empty()),
            !self.app.demo_or_offline(),
        );
        self.reconcile_playlist_inputs(window, cx);
        self.reconcile_seed_options(window, cx);
        self.reconcile_seed_selection(window, cx);
        self.flush_notices(window, cx);
        self.sync_confirm_dialog(window, cx);
        self.shell(window, cx)
    }
}

fn bind_input(
    window: &mut Window,
    cx: &mut Context<GuiRoot>,
    placeholder: impl Into<SharedString>,
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
