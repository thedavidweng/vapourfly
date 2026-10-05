//! Library: toolbar, filters, the cover grid / row list, the game detail
//! sheet, and the Junk cleanup workflow that branches off it.

use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Colorize, Disableable, Selectable, Sizable, StyledExt, WindowExt,
    button::{Button, ButtonCustomVariant, ButtonVariants},
    checkbox::Checkbox,
    description_list::DescriptionList,
    h_flex,
    input::Input,
    menu::{ContextMenuExt, DropdownMenu, PopupMenu, PopupMenuItem},
    popover::Popover,
    progress::Progress,
    skeleton::Skeleton,
    switch::Switch,
    tab::Tab,
    tag::Tag,
    v_flex,
};
use gpui_kit::{
    Anchor, AnyElement, App, ClipboardItem, Context, Entity, Hsla, InteractiveElement, IntoElement,
    ObjectFit, ParentElement, SharedString, Styled, StyledImage, Window, div, img,
    linear_color_stop, linear_gradient, list, prelude::*, px, rgb, uniform_list,
};
use vapourfly_core::models::{ControllerSupport, Game, JunkMode, ProtonTier};

use super::controller::CardScope;
use super::widgets::{self, PAGE_PX};
use super::{GuiRoot, LibraryLayout, hx};
use crate::app::{
    JunkModeChoice, LibraryScope, LibrarySort, PendingAction, QuickView, View, format_playtime,
    game_shows_deck_badge, open_url_in_browser, proton_tier_label, relative_time_ago, sort_label,
};
use crate::theme;

const GRID_GAP_X: f32 = 20.;
const GRID_GAP_Y: f32 = 30.;
const CARD_MIN_W: f32 = 168.;
/// Title and meta lines under a portrait capsule.
const CARD_TEXT_H: f32 = 46.;
const HERO_H: f32 = 400.;
/// Shortest hero, used on 800px-tall screens such as the Steam Deck.
const HERO_MIN_H: f32 = 300.;

/// Hero height for a window `viewport_h` tall: full size on desktop, cut
/// down on short screens so the Recently played shelf starts above the fold.
fn hero_height(viewport_h: f32) -> f32 {
    (viewport_h * 0.4).round().clamp(HERO_MIN_H, HERO_H)
}
const LIST_ROW_H: f32 = 72.;

const SORTS: [LibrarySort; 6] = [
    LibrarySort::InstalledThenPlaytime,
    LibrarySort::Name,
    LibrarySort::Playtime,
    LibrarySort::Hltb,
    LibrarySort::Rating,
    LibrarySort::AppId,
];

const PRESETS: [QuickView; 4] = [
    QuickView::Cozy,
    QuickView::StoryRich,
    QuickView::GreatOnDeck,
    QuickView::ShortSessions,
];

/// Grid geometry for a given content width.
#[derive(Clone, Copy)]
struct GridMetrics {
    columns: usize,
    card_w: f32,
    art_h: f32,
    row_h: f32,
    hero_h: f32,
}

impl GridMetrics {
    fn for_width(width: f32) -> Self {
        let columns = (((width + GRID_GAP_X) / (CARD_MIN_W + GRID_GAP_X)).floor() as usize).max(1);
        let card_w = ((width - GRID_GAP_X * (columns as f32 - 1.)) / columns as f32).floor();
        // Steam portrait capsules are 600×900, drawn inside a 2px focus ring.
        let art_h = ((card_w - 8.) * 1.5).round() + 8.;
        Self {
            columns,
            card_w,
            art_h,
            row_h: art_h + 12. + CARD_TEXT_H + GRID_GAP_Y,
            hero_h: HERO_H,
        }
    }
}

fn card_status(game: &Game) -> (&'static str, gpui_kit::Hsla) {
    let t = theme::t();
    if game.is_junk {
        ("Junk", hx(t.error))
    } else if game.is_hidden {
        ("Hidden", hx(t.text_muted))
    } else if game.installed {
        ("Installed", hx(t.success))
    } else {
        ("Not installed", hx(t.text_muted).opacity(0.6))
    }
}

fn played_label(game: &Game) -> String {
    match game.playtime_minutes.unwrap_or(0) {
        0 => "Unplayed".into(),
        m => format!("{} played", format_playtime(m)),
    }
}

fn hltb_minutes(game: &Game) -> Option<u32> {
    game.hltb
        .as_ref()
        .and_then(|h| h.main_story_seconds)
        .map(|s| s / 60)
}

fn genres(game: &Game) -> Vec<String> {
    let from_store = game
        .steam_store
        .as_ref()
        .map(|s| s.genres.clone())
        .unwrap_or_default();
    let mut out = if from_store.is_empty() {
        game.rawg
            .as_ref()
            .map(|r| r.genres.clone())
            .filter(|g| !g.is_empty())
            .or_else(|| game.igdb.as_ref().map(|i| i.genres.clone()))
            .unwrap_or_default()
    } else {
        from_store
    };
    out.truncate(4);
    out
}

/// One entry in the scrolling Library grid view.
#[derive(Clone, Debug)]
pub(super) enum LibraryItem {
    Hero(u32),
    Shelf(Vec<u32>),
    AllHeader(usize),
    Row(usize),
}

fn shelf_heading(title: &'static str, detail: Option<String>, cx: &App) -> impl IntoElement {
    h_flex()
        .px(px(PAGE_PX))
        .gap_3()
        .items_baseline()
        .child(
            div()
                .text_size(px(21.))
                .font_semibold()
                .text_color(cx.theme().foreground)
                .child(title),
        )
        .when_some(detail, |this, detail| {
            this.child(
                div()
                    .text_size(px(14.))
                    .text_color(cx.theme().muted_foreground)
                    .child(detail),
            )
        })
}

/// Height of the hero / detail call-to-action buttons. The stock Large
/// button is 32px; Steam's game-page actions are a controller-sized 48px.
const HERO_BUTTON_H: f32 = 48.;

/// Steam's green call to action, kept identical in both themes. A stock
/// `Button` (so focus ring, Enter/Space and tab order come from the toolkit)
/// painted with the client's GreenPlay gradient, rgb(138,195,41) to
/// rgb(74,122,22); the variant supplies the hover and pressed fills.
fn play_button(id: &'static str, installed: bool, cx: &App) -> Button {
    let light: Hsla = rgb(0x8ac329).into();
    let deep: Hsla = rgb(0x4a7a16).into();
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(deep)
                .foreground(gpui_kit::white())
                .hover(light)
                .active(deep.darken(0.05))
                .shadow(true),
        )
        .icon(if installed {
            IconName::Play
        } else {
            IconName::Download
        })
        .label(if installed { "Play" } else { "Install" })
        .h(px(HERO_BUTTON_H))
        .px(px(30.))
        .text_size(px(17.))
        .font_semibold()
        .bg(linear_gradient(
            90.,
            linear_color_stop(light, 0.),
            linear_color_stop(deep, 0.6),
        ))
}

/// Translucent stock `Button` for use on top of hero artwork in either theme.
fn overlay_button(id: &'static str, icon: IconName, label: &'static str, cx: &App) -> Button {
    Button::new(id)
        .custom(
            ButtonCustomVariant::new(cx)
                .color(gpui_kit::white().opacity(0.14))
                .foreground(gpui_kit::white())
                .hover(gpui_kit::white().opacity(0.26))
                .active(gpui_kit::white().opacity(0.1)),
        )
        .icon(icon)
        .label(label)
        .h(px(HERO_BUTTON_H))
        .px(px(22.))
        .text_size(px(16.))
        .font_medium()
}

/// Hand the game to Steam: run it when installed, otherwise open the
/// install dialog. The demo library has no Steam to hand off to.
pub(super) fn launch_game(
    id: u32,
    name: &str,
    installed: bool,
    demo: bool,
    window: &mut Window,
    cx: &mut App,
) {
    if demo {
        window.push_notification(format!("Demo library: Steam would start {name}"), cx);
        return;
    }
    let verb = if installed { "rungameid" } else { "install" };
    open_url_in_browser(&format!("steam://{verb}/{id}"));
}

/// Context-menu actions shared by grid cards, rows and the detail sheet.
fn game_menu(menu: PopupMenu, entity: Entity<GuiRoot>, id: u32) -> PopupMenu {
    let details = entity.clone();
    let similar = entity;
    menu.item(
        PopupMenuItem::new("View details")
            .icon(IconName::PanelRight)
            .on_click(move |_, window, cx| {
                details.update(cx, |this, cx| this.open_game_sheet(id, window, cx));
            }),
    )
    .item(
        PopupMenuItem::new("Find similar games")
            .icon(IconName::Sparkles)
            .on_click(move |_, _, cx| {
                similar.update(cx, |this, cx| this.find_similar(id, cx));
            }),
    )
    .separator()
    .item(
        PopupMenuItem::new("Copy App ID")
            .icon(IconName::Copy)
            .on_click(move |_, window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(id.to_string()));
                window.push_notification(format!("Copied App ID {id}"), cx);
            }),
    )
    .item(
        PopupMenuItem::new("Open store page")
            .icon(IconName::ExternalLink)
            .on_click(move |_, _, _| {
                open_url_in_browser(&format!("https://store.steampowered.com/app/{id}"));
            }),
    )
}

impl GuiRoot {
    fn content_width(&self, window: &Window) -> f32 {
        let width: f32 = window.viewport_size().width.into();
        // Page inset (8) + panel border (2) + page padding on both sides.
        width - self.sidebar_width(window) - 10. - PAGE_PX * 2.
    }

    pub(super) fn find_game(&self, id: u32) -> Option<Game> {
        self.app
            .prepared_games(JunkMode::Default)
            .and_then(|games| games.iter().find(|g| g.app_id == id).cloned())
            .or_else(|| {
                self.app
                    .scan_result
                    .as_ref()
                    .and_then(|s| s.games.iter().find(|g| g.app_id == id).cloned())
            })
    }

    pub(super) fn find_similar(&mut self, id: u32, cx: &mut Context<Self>) {
        self.app.discover_seed = id.to_string();
        self.set_view(View::Discover, cx);
        self.app.start_discover_generate();
        self.arm_poll(cx);
        cx.notify();
    }

    pub(super) fn open_game_sheet(&mut self, id: u32, window: &mut Window, cx: &mut Context<Self>) {
        self.app.library_selected_app_id = Some(id);
        cx.notify();
        let entity = cx.entity();
        window.open_sheet(cx, move |sheet, _, cx| {
            let this = entity.read(cx);
            let Some(game) = this.find_game(id) else {
                return sheet.title("Game not found");
            };
            let body = this.game_detail(&game, entity.clone(), cx);
            sheet
                .title(SharedString::from(game.name.clone()))
                .size(px(520.))
                .child(body)
        });
    }

    fn game_detail(&self, game: &Game, entity: Entity<Self>, cx: &App) -> impl IntoElement {
        let id = game.app_id;
        let t = theme::t();
        let (status, tone) = card_status(game);
        let last_played = game
            .last_played_unix
            .map_or_else(|| "Never".to_string(), relative_time_ago);
        let controller = game
            .pcgw
            .as_ref()
            .map_or("Unknown", |p| match p.controller_support {
                ControllerSupport::Full => "Full support",
                ControllerSupport::Partial => "Partial support",
                _ => "None",
            });
        let rating = game
            .rawg
            .as_ref()
            .and_then(|r| r.rating_0_5)
            .or_else(|| {
                game.igdb
                    .as_ref()
                    .and_then(|i| i.rating_0_100)
                    .map(|r| r / 20.)
            })
            .map_or_else(|| "—".to_string(), |r| format!("{r:.1} / 5"));
        let description = game
            .steam_store
            .as_ref()
            .and_then(|s| s.short_description.clone());
        let developer = game
            .steam_store
            .as_ref()
            .and_then(|s| s.developers.first().cloned());

        let similar = entity.clone();
        let demo = self.app.ui_demo;
        let installed = game.installed;
        let name = game.name.clone();
        v_flex()
            .gap_5()
            .pb_6()
            .child(
                div()
                    .w_full()
                    .h(px(220.))
                    .rounded(cx.theme().radius_lg)
                    .overflow_hidden()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(widgets::cover(
                        self.artwork.hero(id).or_else(|| self.artwork.header(id)),
                        id,
                        &game.name,
                        Some(26.),
                    )),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_3()
                            .child(widgets::status(status, tone, cx))
                            .when_some(developer, |this, dev| {
                                this.child(widgets::caption(dev, cx))
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .flex_wrap()
                            .children(genres(game).into_iter().map(|g| Tag::secondary().child(g)))
                            .when(game_shows_deck_badge(game), |this| {
                                this.child(widgets::tone_tag(widgets::Tone::Accent, "Steam Deck"))
                            }),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(play_button("detail-play", installed, cx).on_click(
                        move |_, window, cx| {
                            launch_game(id, &name, installed, demo, window, cx);
                        },
                    ))
                    .child(
                        Button::new("detail-similar")
                            .outline()
                            .icon(IconName::Sparkles)
                            .label("Find similar")
                            .on_click(move |_, window, cx| {
                                window.close_sheet(cx);
                                similar.update(cx, |this, cx| this.find_similar(id, cx));
                            }),
                    )
                    .child(
                        Button::new("detail-store")
                            .outline()
                            .icon(IconName::ExternalLink)
                            .label("Store page")
                            .on_click(move |_, _, _| {
                                open_url_in_browser(&format!(
                                    "https://store.steampowered.com/app/{id}"
                                ));
                            }),
                    )
                    .child(
                        Button::new("detail-copy")
                            .ghost()
                            .icon(IconName::Copy)
                            .tooltip("Copy App ID")
                            .on_click(move |_, window, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(id.to_string()));
                                window.push_notification(format!("Copied App ID {id}"), cx);
                            }),
                    ),
            )
            .when_some(description, |this, text| {
                this.child(
                    div()
                        .text_size(px(15.))
                        .line_height(px(21.))
                        .text_color(cx.theme().muted_foreground)
                        .child(text),
                )
            })
            .child(
                DescriptionList::vertical()
                    .columns(2)
                    .bordered(false)
                    .item(
                        "Playtime",
                        format_playtime(game.playtime_minutes.unwrap_or(0)),
                        1,
                    )
                    .item(
                        "Last two weeks",
                        format_playtime(game.playtime_2wks_minutes.unwrap_or(0)),
                        1,
                    )
                    .item("Last played", last_played, 1)
                    .item(
                        "Time to beat",
                        hltb_minutes(game).map_or_else(|| "—".to_string(), format_playtime),
                        1,
                    )
                    .item(
                        "ProtonDB",
                        game.protondb
                            .as_ref()
                            .map_or("—", |p| proton_tier_label(p.tier)),
                        1,
                    )
                    .item("Controller", controller, 1)
                    .item("Rating", rating, 1)
                    .item("App ID", id.to_string(), 1)
                    .item(
                        "Steam collections",
                        if game.steam_collections.is_empty() {
                            "None".to_string()
                        } else {
                            game.steam_collections.join(", ")
                        },
                        2,
                    ),
            )
            .child(
                h_flex()
                    .gap_1p5()
                    .text_size(px(13.))
                    .text_color(hx(t.text_muted))
                    .child(widgets::inline_icon(IconName::Info, hx(t.text_muted)))
                    .child("Metadata comes from your local cache. Refresh it in Data Sources."),
            )
    }

    fn active_filter_labels(&self) -> Vec<(&'static str, String)> {
        let app = &self.app;
        let mut out = Vec::new();
        if app.library_quick_view != QuickView::All {
            out.push(("preset", app.library_quick_view.label().to_string()));
        }
        if app.filter_deck_compatible {
            out.push(("deck", "Steam Deck ready".into()));
        }
        if app.filter_controller_full {
            out.push(("controller", "Full controller".into()));
        }
        if let Some(tier) = app.filter_proton_tier {
            out.push(("proton", format!("Proton {}+", proton_tier_label(tier))));
        }
        if app.filter_not_junk {
            out.push(("junk", "Hide junk".into()));
        }
        if app.filter_not_hidden {
            out.push(("hidden", "Exclude hidden".into()));
        }
        if !app.filter_genre.trim().is_empty() {
            out.push(("genre", format!("Genre: {}", app.filter_genre.trim())));
        }
        if !app.filter_tag.trim().is_empty() {
            out.push(("tag", format!("Tag: {}", app.filter_tag.trim())));
        }
        if !app.filter_playtime_min.trim().is_empty() || !app.filter_playtime_max.trim().is_empty()
        {
            out.push(("playtime", "Playtime range".into()));
        }
        if !app.filter_hltb_min.trim().is_empty() || !app.filter_hltb_max.trim().is_empty() {
            out.push(("hltb", "Length range".into()));
        }
        out
    }

    fn clear_filter(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let app = &mut self.app;
        match key {
            "preset" => {
                app.apply_quick_view(QuickView::All);
                self.reapply_scope();
            }
            "deck" => app.filter_deck_compatible = false,
            "controller" => app.filter_controller_full = false,
            "proton" => app.filter_proton_tier = None,
            "junk" => app.filter_not_junk = false,
            "hidden" => app.filter_not_hidden = false,
            "genre" => app.filter_genre.clear(),
            "tag" => app.filter_tag.clear(),
            "playtime" => {
                app.filter_playtime_min.clear();
                app.filter_playtime_max.clear();
            }
            "hltb" => {
                app.filter_hltb_min.clear();
                app.filter_hltb_max.clear();
            }
            _ => {}
        }
        self.sync_filter_inputs(window, cx);
        cx.notify();
    }

    fn reset_filters(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.app.apply_quick_view(QuickView::All);
        self.app.filter_not_junk = false;
        self.app.filter_not_hidden = false;
        self.reapply_scope();
        self.sync_filter_inputs(window, cx);
        cx.notify();
    }

    fn set_scope(&mut self, scope: LibraryScope) {
        self.app.apply_library_scope(scope);
        self.reapply_scope();
    }

    fn reapply_scope(&mut self) {
        let app = &mut self.app;
        match app.library_scope {
            LibraryScope::All => {
                app.filter_installed_only = false;
                app.filter_unplayed_only = false;
            }
            LibraryScope::Installed => {
                app.filter_installed_only = true;
                app.filter_unplayed_only = false;
            }
            LibraryScope::Unplayed => {
                app.filter_unplayed_only = true;
                app.filter_installed_only = false;
            }
            LibraryScope::Hidden => {
                app.filter_installed_only = false;
                app.filter_unplayed_only = false;
            }
        }
    }

    pub(super) fn library(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let ready = self.app.library_ready();
        let games = if ready {
            self.app.filtered_games()
        } else {
            Vec::new()
        };
        let all = self
            .app
            .scan_result
            .as_ref()
            .map(|s| s.games.as_slice())
            .unwrap_or(&[]);
        let installed = all.iter().filter(|g| g.installed).count();
        let playtime: u32 = all.iter().map(|g| g.playtime_minutes.unwrap_or(0)).sum();
        let subtitle = if self.app.loading && all.is_empty() {
            "Scanning your Steam library…".to_string()
        } else if all.is_empty() {
            "Connect your Steam library to get started.".to_string()
        } else {
            format!(
                "{} games · {installed} installed · {} played",
                all.len(),
                format_playtime(playtime)
            )
        };

        let actions = h_flex()
            .gap_2()
            .child(
                Button::new("junk-open")
                    .outline()
                    .icon(IconName::WandSparkles)
                    .label("Clean up")
                    .tooltip("Find and tuck away junk entries")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.app.show_junk_panel = true;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("refresh")
                    .outline()
                    .icon(IconName::RefreshCw)
                    .loading(self.app.loading)
                    .tooltip("Rescan Steam library")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.app.start_scan();
                        this.arm_poll(cx);
                        cx.notify();
                    })),
            );

        let width = self.content_width(window);
        let mut m = GridMetrics::for_width(width);
        m.hero_h = hero_height(window.viewport_size().height.into());
        let body = if !ready {
            self.library_skeleton(m, cx).into_any_element()
        } else if games.is_empty() {
            let has_filters =
                !self.active_filter_labels().is_empty() || !self.app.search_query.is_empty();
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(widgets::empty_state(
                    IconName::SearchX,
                    if has_filters {
                        "No games match"
                    } else {
                        "Your library is empty"
                    },
                    if has_filters {
                        "Try a different search or clear some filters."
                    } else {
                        "Point Vapourfly at your Steam folder in Settings, then rescan."
                    },
                    has_filters.then(|| {
                        Button::new("empty-reset")
                            .outline()
                            .label("Clear filters")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.reset_filters(window, cx);
                                this.search.update(cx, |s, cx| s.set_value("", window, cx));
                            }))
                            .into_any_element()
                    }),
                ))
                .into_any_element()
        } else {
            match self.library_layout {
                LibraryLayout::Grid => self.library_feed(games, m, cx).into_any_element(),
                LibraryLayout::List => {
                    self.library_games = Rc::new(games);
                    self.library_list(cx).into_any_element()
                }
            }
        };

        v_flex()
            .id("library")
            .size_full()
            .child(widgets::page_header("Library", subtitle, actions, cx))
            .child(self.library_toolbar(cx))
            .child(self.active_filter_chips(cx))
            .child(body)
    }

    /// True when the page shows the whole library unfiltered, which is when
    /// the "continue playing" hero and the recent shelf make sense.
    fn library_is_home(&self) -> bool {
        self.app.library_scope == LibraryScope::All
            && self.app.search_query.trim().is_empty()
            && self.active_filter_labels().is_empty()
    }

    fn library_toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let scope = self.app.library_scope;
        let layout = self.library_layout;
        let filter_count = self.active_filter_labels().len();
        let sort = self.app.library_sort_by;
        let desc = self.app.library_sort_desc;
        let entity = cx.entity();

        h_flex()
            .px(px(PAGE_PX))
            .pb_5()
            .gap_2()
            .child(
                Input::new(&self.search)
                    .prefix(widgets::inline_icon(
                        IconName::Search,
                        cx.theme().muted_foreground,
                    ))
                    .cleanable(true)
                    .w(px(260.)),
            )
            .child(
                widgets::segmented(
                    "scope",
                    LibraryScope::all()
                        .iter()
                        .map(|s| Tab::new().label(s.label())),
                    LibraryScope::all().iter().position(|s| *s == scope),
                )
                .on_click(cx.listener(|this, ix: &usize, _, cx| {
                    if let Some(scope) = LibraryScope::all().get(*ix).copied() {
                        this.set_scope(scope);
                        cx.notify();
                    }
                })),
            )
            .child(div().flex_1())
            .child(self.filters_popover(filter_count, entity.clone()))
            .child(
                Button::new("sort")
                    .outline()
                    .icon(IconName::ArrowUpDown)
                    .label(sort_label(sort))
                    .dropdown_caret(true)
                    .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                        let mut menu = menu.label("Sort by");
                        for option in SORTS {
                            let entity = entity.clone();
                            menu = menu.item(
                                PopupMenuItem::new(sort_label(option))
                                    .checked(option == sort)
                                    .on_click(move |_, _, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.app.library_sort_by = option;
                                            cx.notify();
                                        });
                                    }),
                            );
                        }
                        menu = menu.separator().label("Order");
                        for (label, value) in [("Ascending", false), ("Descending", true)] {
                            let entity = entity.clone();
                            menu = menu.item(
                                PopupMenuItem::new(label).checked(desc == value).on_click(
                                    move |_, _, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.app.library_sort_desc = value;
                                            cx.notify();
                                        });
                                    },
                                ),
                            );
                        }
                        menu
                    }),
            )
            .child(
                widgets::segmented(
                    "layout",
                    [
                        Tab::new().icon(IconName::LayoutGrid).aria_label("Grid"),
                        Tab::new().icon(IconName::List).aria_label("List"),
                    ],
                    Some(usize::from(layout == LibraryLayout::List)),
                )
                .on_click(cx.listener(|this, ix: &usize, _, cx| {
                    this.library_layout = if *ix == 1 {
                        LibraryLayout::List
                    } else {
                        LibraryLayout::Grid
                    };
                    cx.notify();
                })),
            )
    }

    fn filters_popover(&self, count: usize, entity: Entity<Self>) -> impl IntoElement {
        let inputs = [
            self.genre_input.clone(),
            self.tag_input.clone(),
            self.playtime_min_input.clone(),
            self.playtime_max_input.clone(),
            self.hltb_min_input.clone(),
            self.hltb_max_input.clone(),
        ];
        Popover::new("filters")
            .anchor(Anchor::TopRight)
            .trigger(
                Button::new("filters-btn")
                    .outline()
                    .icon(IconName::SlidersHorizontal)
                    .label(if count > 0 {
                        format!("Filters · {count}")
                    } else {
                        "Filters".to_string()
                    }),
            )
            .content(move |_, _, cx| {
                let app = &entity.read(cx).app;
                let preset = app.library_quick_view;
                let deck = app.filter_deck_compatible;
                let controller = app.filter_controller_full;
                let not_junk = app.filter_not_junk;
                let not_hidden = app.filter_not_hidden;
                let proton = app.filter_proton_tier;
                let [genre, tag, pt_min, pt_max, hltb_min, hltb_max] = inputs.clone();
                let tiers = [
                    None,
                    Some(ProtonTier::Bronze),
                    Some(ProtonTier::Silver),
                    Some(ProtonTier::Gold),
                    Some(ProtonTier::Platinum),
                ];

                let toggle = |id: &'static str,
                              label: &'static str,
                              checked: bool,
                              write: fn(&mut GuiRoot, bool)| {
                    let entity = entity.clone();
                    Switch::new(id).checked(checked).label(label).on_click(
                        move |value: &bool, _, cx| {
                            let value = *value;
                            entity.update(cx, |this, cx| {
                                write(this, value);
                                cx.notify();
                            });
                        },
                    )
                };

                v_flex()
                    .w(px(340.))
                    .gap_4()
                    .p_1()
                    .child(
                        v_flex()
                            .gap_2()
                            .child(widgets::section_label("Presets", cx))
                            .child(h_flex().gap_1p5().flex_wrap().children(
                                PRESETS.into_iter().map(|qv| {
                                    let entity = entity.clone();
                                    Button::new(SharedString::from(format!(
                                        "preset-{}",
                                        qv.label()
                                    )))
                                    .small()
                                    .outline()
                                    .label(qv.label())
                                    .selected(preset == qv)
                                    .on_click(
                                        move |_, window, cx| {
                                            entity.update(cx, |this, cx| {
                                                let next = if this.app.library_quick_view == qv {
                                                    QuickView::All
                                                } else {
                                                    qv
                                                };
                                                this.app.apply_quick_view(next);
                                                this.reapply_scope();
                                                this.sync_filter_inputs(window, cx);
                                                cx.notify();
                                            });
                                        },
                                    )
                                }),
                            )),
                    )
                    .child(
                        v_flex()
                            .gap_2p5()
                            .child(widgets::section_label("Compatibility", cx))
                            .child(toggle("f-deck", "Steam Deck ready", deck, |this, v| {
                                this.app.filter_deck_compatible = v;
                            }))
                            .child(toggle(
                                "f-controller",
                                "Full controller support",
                                controller,
                                |this, v| this.app.filter_controller_full = v,
                            ))
                            .child(
                                v_flex()
                                    .gap_1p5()
                                    .child(widgets::caption("Minimum ProtonDB tier", cx))
                                    .child(
                                        widgets::segmented(
                                            "f-proton",
                                            tiers.iter().map(|tier| {
                                                Tab::new()
                                                    .label(tier.map_or("Any", proton_tier_label))
                                            }),
                                            tiers.iter().position(|tier| *tier == proton),
                                        )
                                        .small()
                                        .on_click({
                                            let entity = entity.clone();
                                            move |ix: &usize, _, cx| {
                                                let tier = tiers.get(*ix).copied().flatten();
                                                entity.update(cx, |this, cx| {
                                                    this.app.filter_proton_tier = tier;
                                                    cx.notify();
                                                });
                                            }
                                        }),
                                    ),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_2p5()
                            .child(widgets::section_label("Library", cx))
                            .child(toggle("f-junk", "Hide junk", not_junk, |this, v| {
                                this.app.filter_not_junk = v;
                            }))
                            .child(toggle(
                                "f-hidden",
                                "Exclude hidden games",
                                not_hidden,
                                |this, v| {
                                    this.app.filter_not_hidden = v;
                                },
                            )),
                    )
                    .child(
                        v_flex()
                            .gap_2()
                            .child(widgets::section_label("Metadata", cx))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(Input::new(&genre).cleanable(true).flex_1())
                                    .child(Input::new(&tag).cleanable(true).flex_1()),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(px(110.))
                                            .child(widgets::caption("Playtime (min)", cx)),
                                    )
                                    .child(Input::new(&pt_min).flex_1())
                                    .child(Input::new(&pt_max).flex_1()),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(
                                        div()
                                            .w(px(110.))
                                            .child(widgets::caption("Length (min)", cx)),
                                    )
                                    .child(Input::new(&hltb_min).flex_1())
                                    .child(Input::new(&hltb_max).flex_1()),
                            ),
                    )
                    .child(h_flex().justify_end().child(
                        Button::new("f-reset").ghost().label("Reset all").on_click({
                            let entity = entity.clone();
                            move |_, window, cx| {
                                entity.update(cx, |this, cx| this.reset_filters(window, cx));
                            }
                        }),
                    ))
            })
    }

    fn active_filter_chips(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let labels = self.active_filter_labels();
        h_flex()
            .px(px(PAGE_PX))
            .gap_1p5()
            .flex_wrap()
            .when(!labels.is_empty(), |this| this.pb_3())
            .children(labels.into_iter().map(|(key, label)| {
                Button::new(SharedString::from(format!("chip-{key}")))
                    .small()
                    .secondary()
                    .label(label)
                    .icon(IconName::X)
                    .tooltip("Remove filter")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.clear_filter(key, window, cx);
                    }))
            }))
    }

    fn library_skeleton(&self, m: GridMetrics, cx: &App) -> impl IntoElement {
        div()
            .px(px(PAGE_PX))
            .flex()
            .flex_wrap()
            .gap_x(px(GRID_GAP_X))
            .gap_y(px(GRID_GAP_Y))
            .children((0..m.columns * 2).map(|_| {
                v_flex()
                    .w(px(m.card_w))
                    .gap_2()
                    .child(
                        Skeleton::new()
                            .w_full()
                            .h(px(m.art_h))
                            .rounded(cx.theme().radius_lg),
                    )
                    .child(Skeleton::new().w(px(m.card_w * 0.7)).h(px(16.)))
                    .child(Skeleton::new().w(px(m.card_w * 0.4)).h(px(13.)))
            }))
    }

    /// The scrolling grid view: on the unfiltered library a hero for the
    /// most recent game and a shelf of other recent games sit above the
    /// full portrait grid.
    fn library_feed(
        &mut self,
        games: Vec<Game>,
        m: GridMetrics,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut items = Vec::new();
        let mut hero = None;
        if self.library_is_home() {
            let mut recent: Vec<&Game> = games
                .iter()
                .filter(|g| {
                    g.last_played_unix.unwrap_or(0) > 0 || g.playtime_2wks_minutes.unwrap_or(0) > 0
                })
                .collect();
            recent.sort_by_key(|g| {
                std::cmp::Reverse((
                    g.last_played_unix.unwrap_or(0),
                    g.playtime_2wks_minutes.unwrap_or(0),
                    g.playtime_minutes.unwrap_or(0),
                ))
            });
            if let Some(first) = recent.first() {
                hero = Some(first.app_id);
                items.push(LibraryItem::Hero(first.app_id));
            }
            let shelf: Vec<u32> = recent
                .iter()
                .skip(1)
                .take(m.columns)
                .map(|g| g.app_id)
                .collect();
            if !shelf.is_empty() {
                items.push(LibraryItem::Shelf(shelf));
            }
            items.push(LibraryItem::AllHeader(games.len()));
        }
        let rows = games.len().div_ceil(m.columns);
        items.extend((0..rows).map(LibraryItem::Row));

        let signature = (items.len(), m.columns, hero, m.hero_h.to_bits());
        if signature != self.library_signature {
            self.library_signature = signature;
            self.library_scroll.reset(items.len());
        }
        self.library_items = items;
        self.library_games = Rc::new(games);

        list(
            self.library_scroll.clone(),
            cx.processor(move |this, ix: usize, _, cx| this.library_item(ix, m, cx)),
        )
        .flex_1()
        .min_h_0()
    }

    fn library_item(&self, ix: usize, m: GridMetrics, cx: &mut Context<Self>) -> AnyElement {
        let Some(item) = self.library_items.get(ix) else {
            return div().into_any_element();
        };
        let games = Rc::clone(&self.library_games);
        match item {
            LibraryItem::Hero(id) => match games.iter().find(|g| g.app_id == *id) {
                Some(game) => self.library_hero(game, m.hero_h, cx),
                None => div().into_any_element(),
            },
            LibraryItem::Shelf(ids) => v_flex()
                .w_full()
                .pt(px(36.))
                .pb(px(8.))
                .gap_4()
                .child(shelf_heading("Recently played", None, cx))
                .child(
                    h_flex()
                        .px(px(PAGE_PX))
                        .gap(px(GRID_GAP_X))
                        .items_start()
                        .children(
                            ids.iter()
                                .filter_map(|id| games.iter().find(|g| g.app_id == *id))
                                .map(|g| self.game_card(g, m, "shelf", cx)),
                        ),
                )
                .into_any_element(),
            LibraryItem::AllHeader(count) => div()
                .w_full()
                .pt(px(28.))
                .pb_4()
                .child(shelf_heading(
                    "All games",
                    Some(format!(
                        "{count} · {}",
                        sort_label(self.app.library_sort_by)
                    )),
                    cx,
                ))
                .into_any_element(),
            LibraryItem::Row(row) => h_flex()
                .id(("grid-row", *row))
                .w_full()
                .h(px(m.row_h))
                .px(px(PAGE_PX))
                .gap(px(GRID_GAP_X))
                .items_start()
                .children(
                    games
                        .iter()
                        .skip(row * m.columns)
                        .take(m.columns)
                        .map(|g| self.game_card(g, m, "grid", cx)),
                )
                .into_any_element(),
        }
    }

    fn library_hero(&self, game: &Game, hero_h: f32, cx: &mut Context<Self>) -> AnyElement {
        let id = game.app_id;
        let name = game.name.clone();
        let installed = game.installed;
        let demo = self.app.ui_demo;
        let backdrop = match self.artwork.hero(id).or_else(|| self.artwork.header(id)) {
            Some(path) => {
                let fallback = move || widgets::backdrop(id).into_any_element();
                img(path)
                    .size_full()
                    .object_fit(ObjectFit::Cover)
                    .with_fallback(fallback)
                    .with_loading(fallback)
                    .into_any_element()
            }
            None => widgets::backdrop(id).into_any_element(),
        };
        let title = match self.artwork.logo(id) {
            Some(path) => img(path)
                .h(px(112.))
                .max_w(px(440.))
                .object_fit(ObjectFit::Contain)
                .into_any_element(),
            None => div()
                .text_size(px(46.))
                .line_height(px(52.))
                .font_bold()
                .text_color(gpui_kit::white())
                .child(name.clone())
                .into_any_element(),
        };
        let mut meta = vec![game.last_played_unix.map_or_else(
            || "Played recently".into(),
            |t| format!("Last played {}", relative_time_ago(t)),
        )];
        if let Some(m) = game.playtime_minutes.filter(|m| *m > 0) {
            meta.push(format!("{} played", format_playtime(m)));
        }
        if let Some(m) = hltb_minutes(game) {
            meta.push(format!("{} to beat", format_playtime(m)));
        }

        let dark = self.app.theme_mode.is_dark();
        let foot = if dark {
            cx.theme().background
        } else {
            gpui_kit::black()
        };
        div()
            .id("library-hero")
            .relative()
            .w_full()
            .h(px(hero_h))
            .overflow_hidden()
            .bg(gpui_kit::black())
            .child(backdrop)
            .child(div().absolute().inset_0().bg(linear_gradient(
                90.,
                linear_color_stop(gpui_kit::black().opacity(0.85), 0.),
                linear_color_stop(gpui_kit::black().opacity(0.), 0.72),
            )))
            // Dark fades into the page; light keeps a dark foot so the white
            // hero text stays readable.
            .child(div().absolute().inset_0().bg(linear_gradient(
                180.,
                linear_color_stop(foot.opacity(0.), 0.45),
                linear_color_stop(foot.opacity(if dark { 1. } else { 0.75 }), 1.),
            )))
            .child(
                v_flex()
                    .absolute()
                    .left(px(PAGE_PX + 8.))
                    .bottom(px(44.))
                    .max_w(px(640.))
                    .gap_4()
                    .child(
                        div()
                            .text_size(px(13.))
                            .font_semibold()
                            .text_color(gpui_kit::white().opacity(0.72))
                            .child("CONTINUE PLAYING"),
                    )
                    .child(title)
                    .child(
                        div()
                            .text_size(px(16.))
                            .text_color(gpui_kit::white().opacity(0.85))
                            .child(meta.join("  ·  ")),
                    )
                    .child(
                        h_flex()
                            .pt_2()
                            .gap_3()
                            .child(play_button("hero-play", installed, cx).on_click(
                                move |_, window, cx| {
                                    launch_game(id, &name, installed, demo, window, cx);
                                },
                            ))
                            .child(
                                overlay_button("hero-details", IconName::Info, "Details", cx)
                                    .on_click(cx.listener(move |this, _, window, cx| {
                                        this.open_game_sheet(id, window, cx);
                                    })),
                            )
                            .child(
                                overlay_button(
                                    "hero-similar",
                                    IconName::Sparkles,
                                    "More like this",
                                    cx,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.find_similar(id, cx);
                                    },
                                )),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn game_card(
        &self,
        game: &Game,
        m: GridMetrics,
        scope: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = game.app_id;
        let selected = self.app.library_selected_app_id == Some(id);
        let (status, tone) = card_status(game);
        let focus = if self.app.theme_mode.is_dark() {
            gpui_kit::white()
        } else {
            cx.theme().primary
        };
        let border = if selected {
            cx.theme().primary
        } else {
            gpui_kit::transparent_black()
        };
        let entity = cx.entity();
        let group = SharedString::from(format!("{scope}-card-{id}"));
        let focus_handle = self.card_handle(CardScope::from_label(scope), id, cx);

        v_flex()
            .id(SharedString::from(format!("{scope}-{id}")))
            .group(group.clone())
            .w(px(m.card_w))
            .gap_3()
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_game_sheet(id, window, cx);
            }))
            .child(
                // The capsule frame, not the whole card, is the tab stop so
                // the focus ring lands where hover draws it. It handles its
                // own click and stops propagation so the card's click (for
                // the title area) does not fire a second time.
                div()
                    .id(SharedString::from(format!("{scope}-art-{id}")))
                    .relative()
                    .w_full()
                    .h(px(m.art_h))
                    .rounded(px(theme::CORNER_LG))
                    .p(px(2.))
                    .border_2()
                    .border_color(border)
                    .track_focus(&focus_handle)
                    .when(!selected, |this| {
                        this.group_hover(group.clone(), move |s| s.border_color(focus))
                    })
                    .focus_visible(move |s| s.border_color(focus))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.open_game_sheet(id, window, cx);
                    }))
                    .child(
                        div()
                            .relative()
                            .size_full()
                            .rounded(px(theme::CORNER_SM))
                            .overflow_hidden()
                            .shadow_md()
                            .child(widgets::poster(
                                self.artwork.portrait(id),
                                id,
                                &game.name,
                                theme::CORNER_SM,
                            ))
                            .when(game_shows_deck_badge(game), |this| {
                                this.child(
                                    div()
                                        .absolute()
                                        .top(px(8.))
                                        .left(px(8.))
                                        .child(widgets::art_pill("Deck")),
                                )
                            }),
                    ),
            )
            .child(
                v_flex()
                    .px_1()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(15.))
                            .line_height(px(20.))
                            .font_medium()
                            .truncate()
                            .child(game.name.clone()),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(widgets::status(status, tone, cx))
                            .child(
                                div()
                                    .text_size(px(13.))
                                    .text_color(cx.theme().muted_foreground)
                                    .truncate()
                                    .child(played_label(game)),
                            ),
                    ),
            )
            .context_menu(move |menu, _, _| game_menu(menu, entity.clone(), id))
            .into_any_element()
    }

    fn library_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let total = self.library_games.len();
        let head = |label: &'static str, w: Option<f32>| {
            let cell = div()
                .text_size(px(13.))
                .font_medium()
                .text_color(cx.theme().muted_foreground)
                .child(label);
            match w {
                Some(w) => cell.w(px(w)),
                None => cell.flex_1(),
            }
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                h_flex()
                    .mx(px(PAGE_PX))
                    .px_2()
                    .pb_2()
                    .gap_4()
                    .border_b_1()
                    .border_color(cx.theme().border)
                    .child(div().w(px(120.)))
                    .child(head("Game", None))
                    .child(head("Status", Some(120.)))
                    .child(head("Played", Some(96.)))
                    .child(head("Last played", Some(110.)))
                    .child(head("Time to beat", Some(100.))),
            )
            .child(
                uniform_list(
                    "library-rows",
                    total,
                    cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                        let games = Rc::clone(&this.library_games);
                        range
                            .filter_map(|ix| games.get(ix))
                            .map(|g| this.library_row(g, cx))
                            .collect()
                    }),
                )
                .track_scroll(&self.library_rows_scroll)
                .flex_1()
                .min_h_0(),
            )
    }

    fn library_row(&self, game: &Game, cx: &mut Context<Self>) -> AnyElement {
        let id = game.app_id;
        let selected = self.app.library_selected_app_id == Some(id);
        let (status, tone) = card_status(game);
        let entity = cx.entity();
        let muted = cx.theme().muted_foreground;
        let cell = |text: String, w: f32| {
            div()
                .w(px(w))
                .text_size(px(15.))
                .text_color(muted)
                .truncate()
                .child(text)
        };
        let content = h_flex()
            .w_full()
            .gap_4()
            .child(
                div()
                    .flex_none()
                    .w(px(120.))
                    .h(px(56.))
                    .rounded(px(theme::CORNER_SM))
                    .overflow_hidden()
                    .child(widgets::cover(
                        self.artwork.header(id),
                        id,
                        &game.name,
                        None,
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(15.))
                    .font_medium()
                    .truncate()
                    .child(game.name.clone()),
            )
            .child(div().w(px(120.)).child(widgets::status(status, tone, cx)))
            .child(cell(
                match game.playtime_minutes.unwrap_or(0) {
                    0 => "—".into(),
                    m => format_playtime(m),
                },
                96.,
            ))
            .child(cell(
                game.last_played_unix
                    .map_or_else(|| "Never".into(), relative_time_ago),
                110.,
            ))
            .child(cell(
                hltb_minutes(game).map_or_else(|| "—".into(), format_playtime),
                100.,
            ));
        let focus_handle = self.card_handle(CardScope::Row, id, cx);
        let row = widgets::focus_row_tracked(("row", id as usize), selected, &focus_handle, cx)
            .w_full()
            .h(px(LIST_ROW_H))
            .px_2()
            .on_click(cx.listener(move |this, _, window, cx| {
                this.open_game_sheet(id, window, cx);
            }))
            .child(content)
            .context_menu(move |menu, _, _| game_menu(menu, entity.clone(), id));
        div().w_full().px(px(PAGE_PX)).child(row).into_any_element()
    }

    pub(super) fn junk_panel(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = theme::t();
        let selected = self.app.junk_selected.len();
        let evaluated = self.app.junk_results.len();
        let flagged = self.app.junk_results.iter().filter(|d| d.is_junk).count();
        let mode = self.app.junk_mode;
        let modes = [
            JunkModeChoice::Default,
            JunkModeChoice::Strict,
            JunkModeChoice::Aggressive,
        ];
        let writes_blocked = selected == 0 || self.app.ui_demo;
        let visible = self
            .app
            .junk_results
            .iter()
            .filter(|d| self.app.junk_show_all_evaluated || d.is_junk)
            .count();

        let back = Button::new("junk-back")
            .ghost()
            .icon(IconName::ArrowLeft)
            .label("Library")
            .on_click(cx.listener(|this, _, _, cx| {
                this.app.show_junk_panel = false;
                cx.notify();
            }));

        let list = if evaluated == 0 {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(widgets::empty_state(
                    IconName::WandSparkles,
                    "No preview yet",
                    "Run a preview to see which entries look like demos, tools or leftovers.",
                    Some(
                        Button::new("junk-empty-run")
                            .primary()
                            .label("Run preview")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.app.start_junk_preview();
                                this.arm_poll(cx);
                                cx.notify();
                            }))
                            .into_any_element(),
                    ),
                ))
                .into_any_element()
        } else {
            v_flex()
                .flex_1()
                .min_h_0()
                .child(
                    h_flex()
                        .mx(px(PAGE_PX))
                        .px_2()
                        .pb_2()
                        .gap_4()
                        .border_b_1()
                        .border_color(cx.theme().border)
                        .text_size(px(13.))
                        .font_medium()
                        .text_color(cx.theme().muted_foreground)
                        .child(div().w(px(20.)))
                        .child(div().w(px(86.)))
                        .child(div().flex_1().child("Game"))
                        .child(div().w(px(90.)).child("Verdict"))
                        .child(div().w(px(180.)).child("Confidence")),
                )
                .child(
                    uniform_list(
                        "junk-rows",
                        visible,
                        cx.processor(|this, range: std::ops::Range<usize>, _, cx| {
                            let rows: Vec<_> = this
                                .app
                                .junk_results
                                .iter()
                                .filter(|d| this.app.junk_show_all_evaluated || d.is_junk)
                                .cloned()
                                .collect();
                            range
                                .filter_map(|ix| rows.get(ix).cloned())
                                .map(|d| {
                                    let id = d.app_id;
                                    let checked = this.app.junk_selected.contains(&id);
                                    let toggle = cx.listener(move |this, _: &bool, _, cx| {
                                        if !this.app.junk_selected.remove(&id) {
                                            this.app.junk_selected.insert(id);
                                        }
                                        cx.notify();
                                    });
                                    let row = h_flex()
                                        .id(("junk-row", id as usize))
                                        .w_full()
                                        .h(px(64.))
                                        .px_2()
                                        .gap_4()
                                        .rounded(cx.theme().radius)
                                        .hover(|s| s.bg(cx.theme().list_hover))
                                        .child(
                                            div().w(px(20.)).child(
                                                Checkbox::new(("junk-cb", id as usize))
                                                    .checked(checked)
                                                    .on_click(toggle),
                                            ),
                                        )
                                        .child(
                                            div()
                                                .w(px(86.))
                                                .h(px(40.))
                                                .rounded(px(theme::CORNER_SM))
                                                .overflow_hidden()
                                                .child(widgets::cover(
                                                    this.artwork.header(id),
                                                    id,
                                                    &d.name,
                                                    None,
                                                )),
                                        )
                                        .child(
                                            div()
                                                .flex_1()
                                                .min_w_0()
                                                .text_size(px(15.))
                                                .truncate()
                                                .child(d.name.clone()),
                                        )
                                        .child(div().w(px(90.)).flex().child(if d.is_junk {
                                            widgets::tone_tag(widgets::Tone::Danger, "Junk")
                                        } else {
                                            Tag::secondary().child("Keep")
                                        }))
                                        .child(
                                            h_flex()
                                                .w(px(180.))
                                                .gap_3()
                                                .child(
                                                    div().flex_1().child(
                                                        Progress::new(("junk-conf", id as usize))
                                                            .value(d.confidence * 100.),
                                                    ),
                                                )
                                                .child(
                                                    div()
                                                        .w(px(36.))
                                                        .text_size(px(13.))
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(format!(
                                                            "{:.0}%",
                                                            d.confidence * 100.
                                                        )),
                                                ),
                                        );
                                    div().w_full().px(px(PAGE_PX)).child(row)
                                })
                                .collect()
                        }),
                    )
                    .flex_1()
                    .min_h_0(),
                )
                .into_any_element()
        };

        v_flex()
            .id("junk")
            .size_full()
            .child(div().px(px(PAGE_PX - 8.)).pt_4().child(back))
            .child(widgets::page_header(
                "Junk cleanup",
                "Find demos, tools and abandoned leftovers, then tuck them out of sight.",
                h_flex()
                    .gap_2()
                    .child(
                        widgets::segmented(
                            "junk-mode",
                            modes.iter().map(|m| Tab::new().label(m.label())),
                            modes.iter().position(|m| *m == mode),
                        )
                        .on_click(cx.listener(
                            move |this, ix: &usize, _, cx| {
                                if let Some(m) = modes.get(*ix) {
                                    this.app.junk_mode = *m;
                                    cx.notify();
                                }
                            },
                        )),
                    )
                    .child(
                        Button::new("junk-preview")
                            .primary()
                            .icon(IconName::Play)
                            .label("Run preview")
                            .loading(self.app.junk_preview_loading)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.app.start_junk_preview();
                                this.arm_poll(cx);
                                cx.notify();
                            })),
                    ),
                cx,
            ))
            .when(evaluated > 0, |this| {
                this.child(
                    h_flex()
                        .px(px(PAGE_PX))
                        .pb_4()
                        .gap_3()
                        .child(widgets::stat_tile(
                            "Evaluated",
                            evaluated.to_string(),
                            None,
                            cx,
                        ))
                        .child(widgets::stat_tile(
                            "Flagged as junk",
                            flagged.to_string(),
                            Some(hx(t.error)),
                            cx,
                        ))
                        .child(widgets::stat_tile(
                            "Selected",
                            selected.to_string(),
                            Some(hx(t.accent_text)),
                            cx,
                        )),
                )
                .child(
                    h_flex().px(px(PAGE_PX)).pb_3().child(
                        Switch::new("junk-show-all")
                            .checked(self.app.junk_show_all_evaluated)
                            .label("Show games marked keep")
                            .on_click(cx.listener(|this, v: &bool, _, cx| {
                                this.app.junk_show_all_evaluated = *v;
                                cx.notify();
                            })),
                    ),
                )
            })
            .child(list)
            .when(evaluated > 0, |this| {
                this.child(
                    h_flex()
                        .px(px(PAGE_PX))
                        .py_3()
                        .gap_2()
                        .border_t_1()
                        .border_color(cx.theme().border)
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_medium()
                                .child(format!("{selected} selected")),
                        )
                        .child(
                            Button::new("junk-all")
                                .ghost()
                                .label("Select all flagged")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.app.junk_selected = this
                                        .app
                                        .junk_results
                                        .iter()
                                        .filter(|d| d.is_junk)
                                        .map(|d| d.app_id)
                                        .collect();
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("junk-clear")
                                .ghost()
                                .label("Clear")
                                .disabled(selected == 0)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.app.junk_selected.clear();
                                    cx.notify();
                                })),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("junk-apply")
                                .outline()
                                .icon(IconName::FolderInput)
                                .label("Add to Junk collection")
                                .disabled(writes_blocked)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.app.start_dry_run(PendingAction::JunkApply);
                                    this.arm_poll(cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("junk-hide")
                                .danger()
                                .icon(IconName::EyeOff)
                                .label("Hide in Steam")
                                .disabled(writes_blocked)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.app.start_dry_run(PendingAction::JunkHide);
                                    this.arm_poll(cx);
                                    cx.notify();
                                })),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{HERO_H, HERO_MIN_H, hero_height};

    #[test]
    fn hero_shrinks_on_short_screens() {
        assert_eq!(hero_height(1080.), HERO_H);
        assert_eq!(hero_height(800.), 320.);
        assert_eq!(hero_height(600.), HERO_MIN_H);
    }
}
