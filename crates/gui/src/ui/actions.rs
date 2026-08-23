//! Commands modeled once as GPUI actions.
//!
//! The native macOS menu bar (`cx.set_menus`), the key bindings
//! (`bind_keys`), and any in-window control dispatch the same action, so a
//! command behaves identically no matter where it is invoked from.
//! Handlers live on the app-root element via [`apply_root_handlers`] —
//! actions bubble from the focused surface up to that container.

use gpui::{
    App, Context, Div, Focusable as _, InteractiveElement as _, KeyBinding, Menu, MenuItem, actions,
};

use super::root::GuiRoot;
use crate::app::View;

actions!(
    vapourfly,
    [
        NavigateDiscover,
        NavigateLibrary,
        NavigateRecommendations,
        NavigatePlaylists,
        NavigateCollections,
        NavigateDataSources,
        NavigateSettings,
        Refresh,
        ToggleTheme,
        ToggleSidebar,
        FocusSearch,
        SavePlaylist,
        OpenJunk,
        Quit,
    ]
);

/// Bind the default shortcuts. Global context: Vapourfly commands stay
/// live regardless of which surface holds focus.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-1", NavigateDiscover, None),
        KeyBinding::new("cmd-2", NavigateLibrary, None),
        KeyBinding::new("cmd-3", NavigateRecommendations, None),
        KeyBinding::new("cmd-4", NavigatePlaylists, None),
        KeyBinding::new("cmd-5", NavigateCollections, None),
        KeyBinding::new("cmd-6", NavigateDataSources, None),
        KeyBinding::new("cmd-7", NavigateSettings, None),
        KeyBinding::new("cmd-r", Refresh, None),
        KeyBinding::new("cmd-shift-t", ToggleTheme, None),
        KeyBinding::new("cmd-b", ToggleSidebar, None),
        KeyBinding::new("cmd-f", FocusSearch, None),
        KeyBinding::new("cmd-s", SavePlaylist, None),
        KeyBinding::new("cmd-j", OpenJunk, None),
        KeyBinding::new("cmd-q", Quit, None),
    ]);
}

/// Native menu bar (rendered as NSMenu on macOS).
pub fn menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "Vapourfly".into(),
            items: vec![
                MenuItem::action("About Vapourfly", NavigateSettings),
                MenuItem::separator(),
                MenuItem::action("Quit Vapourfly", Quit),
            ],
        },
        Menu {
            name: "View".into(),
            items: vec![
                MenuItem::action("Discover", NavigateDiscover),
                MenuItem::action("Library", NavigateLibrary),
                MenuItem::action("Recommendations", NavigateRecommendations),
                MenuItem::action("Playlists", NavigatePlaylists),
                MenuItem::action("Collections", NavigateCollections),
                MenuItem::action("Data Sources", NavigateDataSources),
                MenuItem::action("Settings", NavigateSettings),
                MenuItem::separator(),
                MenuItem::action("Toggle Sidebar", ToggleSidebar),
                MenuItem::action("Toggle Theme", ToggleTheme),
            ],
        },
        Menu {
            name: "Library".into(),
            items: vec![
                MenuItem::action("Refresh Library", Refresh),
                MenuItem::action("Search", FocusSearch),
                MenuItem::action("Open Junk…", OpenJunk),
            ],
        },
    ]
}

/// Element-level action handlers for the app root container.
///
/// Rendered every frame on the outermost `Div`, so actions dispatched from
/// the menu bar or keyboard bubble from the focused surface up to this
/// container and resolve here.
pub(crate) fn apply_root_handlers(el: Div, cx: &mut Context<GuiRoot>) -> Div {
    el.on_action(cx.listener(|this, _: &NavigateDiscover, _, cx| {
        this.set_view(View::Discover, cx);
    }))
    .on_action(cx.listener(|this, _: &NavigateLibrary, _, cx| {
        this.set_view(View::Library, cx);
    }))
    .on_action(cx.listener(|this, _: &NavigateRecommendations, _, cx| {
        this.set_view(View::Recommendations, cx);
    }))
    .on_action(cx.listener(|this, _: &NavigatePlaylists, _, cx| {
        this.set_view(View::Playlists, cx);
    }))
    .on_action(cx.listener(|this, _: &NavigateCollections, _, cx| {
        this.set_view(View::Collections, cx);
    }))
    .on_action(cx.listener(|this, _: &NavigateDataSources, _, cx| {
        this.set_view(View::DataSources, cx);
    }))
    .on_action(cx.listener(|this, _: &NavigateSettings, _, cx| {
        this.set_view(View::Settings, cx);
    }))
    .on_action(cx.listener(|this, _: &Refresh, _, cx| {
        this.app.start_scan();
        this.arm_poll(cx);
        cx.notify();
    }))
    .on_action(cx.listener(|this, _: &ToggleTheme, window, cx| {
        this.toggle_theme(window, cx);
    }))
    .on_action(cx.listener(|this, _: &ToggleSidebar, window, _cx| {
        let width: f32 = window.viewport_size().width.into();
        let effective = this
            .sidebar_collapsed
            .unwrap_or(crate::theme::is_compact_sidebar(width));
        this.sidebar_collapsed = Some(!effective);
    }))
    .on_action(cx.listener(|this, _: &FocusSearch, window, cx| {
        this.search.read(cx).focus_handle(cx).focus(window);
    }))
    .on_action(cx.listener(|this, _: &SavePlaylist, _, cx| {
        this.save_playlist_from_editor(cx);
    }))
    .on_action(cx.listener(|this, _: &OpenJunk, _, cx| {
        this.app.current_view = View::Library;
        this.app.show_junk_panel = true;
        cx.notify();
    }))
    .on_action(cx.listener(|_: &mut GuiRoot, _: &Quit, _, cx| {
        cx.quit();
    }))
}
