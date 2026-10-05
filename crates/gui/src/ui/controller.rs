//! Controller navigation: turns [`NavCommand`]s from the gamepad thread into
//! focus moves, activations and scrolling.
//!
//! The model follows the Steam Deck UI. The window has two zones, the
//! sidebar and the page. In the sidebar Up / Down move between
//! destinations and A or Right opens one and moves focus into the page.
//! In the page, directions first go to the focused control as arrow keys
//! (menus, selects and segmented controls handle their own), and otherwise
//! move to the previous / next tab stop. The Library grid and list have a
//! real 2-D cursor that scrolls the virtualized list to the target. B closes
//! the top overlay and otherwise returns to the sidebar.
//!
//! Synthetic key events go through `Window::dispatch_event`, so dialogs,
//! sheets and popovers keep their own focus traps and Escape handling.

use std::sync::{Arc, Mutex};

use gpui_kit::component::WindowExt;
use gpui_kit::{
    App, AppContext, Context, FocusHandle, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers,
    PlatformInput, ScrollDelta, ScrollWheelEvent, TouchPhase, WeakEntity, Window, point, px,
};

use super::GuiRoot;
use crate::app::{View, open_url_in_browser};
use crate::gamepad::{self, Direction, NavCommand};

/// Which library surface a card focus handle belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CardScope {
    Shelf,
    Grid,
    Row,
}

impl CardScope {
    pub(crate) fn from_label(scope: &str) -> Self {
        match scope {
            "shelf" => Self::Shelf,
            _ => Self::Grid,
        }
    }
}

/// Run `f` once the frame after the next one has been drawn. GPUI runs
/// next-frame callbacks *before* drawing, so one hop would still see the
/// old element tree (and its tab stops).
fn after_render(window: &Window, f: impl FnOnce(&mut Window, &mut App) + 'static) {
    window.on_next_frame(move |window, _| window.on_next_frame(f));
}

/// Press and release `key` as if typed. Returns whether a handler consumed it.
fn press_key(key: &str, window: &mut Window, cx: &mut App) -> bool {
    let Ok(keystroke) = Keystroke::parse(key) else {
        return false;
    };
    let down = window.dispatch_event(
        PlatformInput::KeyDown(KeyDownEvent {
            keystroke: keystroke.clone(),
            is_held: false,
            prefer_character_input: false,
        }),
        cx,
    );
    window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
    !down.propagate || down.default_prevented
}

fn arrow(dir: Direction) -> &'static str {
    match dir {
        Direction::Up => "up",
        Direction::Down => "down",
        Direction::Left => "left",
        Direction::Right => "right",
    }
}

fn forward(dir: Direction) -> bool {
    matches!(dir, Direction::Down | Direction::Right)
}

/// Tab / Shift-Tab through the toolkit root so focus traps still apply.
fn step_focus(forward: bool, window: &mut Window, cx: &mut App) {
    press_key(if forward { "tab" } else { "shift-tab" }, window, cx);
}

fn overlay_open(window: &mut Window, cx: &mut App) -> bool {
    window.has_active_dialog(cx) || window.has_active_sheet(cx)
}

impl GuiRoot {
    /// Start reading controllers and route their commands to this window.
    pub(super) fn wire_gamepad(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(rx) = gamepad::spawn() else {
            return;
        };
        let rx = Arc::new(Mutex::new(rx));
        let handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            loop {
                let rx = rx.clone();
                let next = cx
                    .background_executor()
                    .spawn(async move { rx.lock().ok().and_then(|rx| rx.recv().ok()) })
                    .await;
                let Some(command) = next else {
                    break;
                };
                let this = this.clone();
                if cx
                    .update_window(handle, move |_, window, cx| {
                        handle_command(this, command, window, cx);
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
    }

    pub(super) fn nav_handle(&self, view: View) -> &FocusHandle {
        let ix = View::ALL.iter().position(|v| *v == view).unwrap_or(0);
        &self.nav_focus[ix]
    }

    fn focused_nav(&self, window: &Window) -> Option<usize> {
        self.nav_focus.iter().position(|h| h.is_focused(window))
    }

    /// Focus handle of a library card, created on first use.
    pub(super) fn card_handle(&self, scope: CardScope, id: u32, cx: &mut App) -> FocusHandle {
        self.card_focus
            .borrow_mut()
            .entry((scope, id))
            .or_insert_with(|| cx.focus_handle().tab_stop(true))
            .clone()
    }

    fn focused_card(&self, window: &Window) -> Option<(CardScope, u32)> {
        self.card_focus
            .borrow()
            .iter()
            .find(|(_, h)| h.is_focused(window))
            .map(|(key, _)| *key)
    }

    /// Move focus to the first control of the current page.
    pub(super) fn enter_page(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let page = self.page_focus.clone();
        window.focus(&page, cx);
        after_render(window, move |window, cx| {
            if page.is_focused(window) {
                window.focus_next(cx);
            }
        });
        cx.notify();
    }

    fn focus_nav(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        let handle = self.nav_handle(view).clone();
        window.focus(&handle, cx);
        cx.notify();
    }

    fn open_view(&mut self, view: View, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_sheet(cx) {
            window.close_sheet(cx);
        }
        if self.app.current_view != view || self.app.show_junk_panel {
            self.set_view(view, cx);
        }
        self.enter_page(window, cx);
    }

    fn cycle_view(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let n = View::ALL.len() as isize;
        let current = View::ALL
            .iter()
            .position(|v| *v == self.app.current_view)
            .unwrap_or(0) as isize;
        let next = View::ALL[(current + step).rem_euclid(n) as usize];
        self.open_view(next, window, cx);
    }

    fn open_keyboard(&self) {
        if self.device.has_steam_keyboard() && !self.app.ui_demo {
            open_url_in_browser("steam://open/keyboard");
        }
    }

    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if window.has_active_sheet(cx) {
            window.close_sheet(cx);
        }
        if self.app.current_view != View::Library || self.app.show_junk_panel {
            self.set_view(View::Library, cx);
        }
        let search = self.search.clone();
        search.update(cx, |state, cx| state.focus(window, cx));
        // The search field only exists once the Library page has rendered.
        after_render(window, move |window, cx| {
            search.update(cx, |state, cx| state.focus(window, cx));
        });
        self.open_keyboard();
        cx.notify();
    }

    /// 2-D movement in the Library grid, shelf and list. Returns `false`
    /// when the move should fall through to ordinary tab order.
    fn library_move(
        &mut self,
        dir: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.app.current_view != View::Library || self.app.show_junk_panel {
            return false;
        }
        let Some((scope, id)) = self.focused_card(window) else {
            return false;
        };
        let games = std::rc::Rc::clone(&self.library_games);
        let len = games.len();
        let columns = self.library_signature.1.max(1);
        let shelf: Vec<u32> = self
            .library_items
            .iter()
            .find_map(|item| match item {
                super::library::LibraryItem::Shelf(ids) => Some(ids.clone()),
                _ => None,
            })
            .unwrap_or_default();
        let index_of = |id: u32| games.iter().position(|g| g.app_id == id);

        let target: Option<(CardScope, usize)> = match scope {
            CardScope::Grid => {
                let Some(ix) = index_of(id) else {
                    return false;
                };
                match dir {
                    Direction::Left if ix > 0 => Some((CardScope::Grid, ix - 1)),
                    Direction::Right if ix + 1 < len => Some((CardScope::Grid, ix + 1)),
                    Direction::Right => return true,
                    Direction::Up if ix >= columns => Some((CardScope::Grid, ix - columns)),
                    Direction::Up if !shelf.is_empty() => {
                        Some((CardScope::Shelf, (ix % columns).min(shelf.len() - 1)))
                    }
                    Direction::Down if ix + columns < len => Some((CardScope::Grid, ix + columns)),
                    // A short last row: drop to its final card.
                    Direction::Down if ix / columns < (len - 1) / columns => {
                        Some((CardScope::Grid, len - 1))
                    }
                    Direction::Down => return true,
                    _ => None,
                }
            }
            CardScope::Shelf => {
                let Some(pos) = shelf.iter().position(|s| *s == id) else {
                    return false;
                };
                match dir {
                    Direction::Left if pos > 0 => Some((CardScope::Shelf, pos - 1)),
                    Direction::Right if pos + 1 < shelf.len() => Some((CardScope::Shelf, pos + 1)),
                    Direction::Right => return true,
                    Direction::Down if len > 0 => Some((CardScope::Grid, pos.min(len - 1))),
                    _ => None,
                }
            }
            CardScope::Row => {
                let Some(ix) = index_of(id) else {
                    return false;
                };
                match dir {
                    Direction::Up if ix > 0 => Some((CardScope::Row, ix - 1)),
                    Direction::Down if ix + 1 < len => Some((CardScope::Row, ix + 1)),
                    Direction::Down => return true,
                    _ => None,
                }
            }
        };
        let Some((scope, ix)) = target else {
            return false;
        };

        let id = match scope {
            CardScope::Shelf => shelf[ix],
            CardScope::Grid | CardScope::Row => games[ix].app_id,
        };
        match scope {
            CardScope::Grid => {
                let row = ix / columns;
                if let Some(item) = self.library_items.iter().position(
                    |item| matches!(item, super::library::LibraryItem::Row(r) if *r == row),
                ) {
                    self.library_scroll.scroll_to_reveal_item(item);
                }
            }
            CardScope::Shelf => {
                if let Some(item) = self
                    .library_items
                    .iter()
                    .position(|item| matches!(item, super::library::LibraryItem::Shelf(_)))
                {
                    self.library_scroll.scroll_to_reveal_item(item);
                }
            }
            CardScope::Row => {
                self.library_rows_scroll
                    .scroll_to_item(ix, gpui_kit::ScrollStrategy::Nearest);
            }
        }
        let handle = self.card_handle(scope, id, cx);
        window.focus(&handle, cx);
        // The target row may only be laid out by the scroll above; if the
        // focus was dropped meanwhile, put it back once the row exists.
        after_render(window, move |window, cx| {
            if !handle.is_focused(window) {
                window.focus(&handle, cx);
            }
        });
        cx.notify();
        true
    }
}

/// Scroll whatever sits under the middle of the page (or the open sheet /
/// dialog) by `dy` pixels, positive down.
fn scroll_page(sidebar_width: f32, dy: f32, window: &mut Window, cx: &mut App) {
    let size = window.viewport_size();
    let (w, h): (f32, f32) = (size.width.into(), size.height.into());
    let x = if window.has_active_sheet(cx) {
        w - 260.
    } else if window.has_active_dialog(cx) {
        w / 2.
    } else {
        sidebar_width + (w - sidebar_width) / 2.
    };
    window.dispatch_event(
        PlatformInput::ScrollWheel(ScrollWheelEvent {
            position: point(px(x), px(h * 0.55)),
            // GPUI wheel deltas are positive toward the top.
            delta: ScrollDelta::Pixels(point(px(0.), px(-dy))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
        }),
        cx,
    );
}

fn handle_command(
    this: WeakEntity<GuiRoot>,
    command: NavCommand,
    window: &mut Window,
    cx: &mut App,
) {
    let Some(root) = this.upgrade() else {
        return;
    };
    match command {
        NavCommand::Connected(name) => {
            root.update(cx, |this, cx| {
                this.pad_name = Some(name);
                cx.notify();
            });
            return;
        }
        NavCommand::Disconnected => {
            root.update(cx, |this, cx| {
                this.pad_name = None;
                this.pad_active = false;
                cx.notify();
            });
            return;
        }
        _ => {}
    }
    root.update(cx, |this, cx| {
        if !this.pad_active {
            this.pad_active = true;
            cx.notify();
        }
    });

    match command {
        NavCommand::Move(dir) => move_focus(&root, dir, window, cx),
        NavCommand::Activate => {
            if window.has_focused_input(cx) {
                root.read(cx).open_keyboard();
                return;
            }
            if !overlay_open(window, cx) {
                let nav = root.read(cx).focused_nav(window);
                if let Some(ix) = nav {
                    root.update(cx, |this, cx| this.open_view(View::ALL[ix], window, cx));
                    return;
                }
                if window.focused(cx).is_none() {
                    root.update(cx, |this, cx| {
                        let view = this.app.current_view;
                        this.focus_nav(view, window, cx);
                    });
                    return;
                }
            }
            press_key("enter", window, cx);
        }
        NavCommand::Back => {
            if press_key("escape", window, cx) {
                return;
            }
            if window.has_active_sheet(cx) {
                window.close_sheet(cx);
                return;
            }
            if window.has_active_dialog(cx) {
                window.close_dialog(cx);
                return;
            }
            root.update(cx, |this, cx| {
                if this.app.show_junk_panel {
                    this.app.show_junk_panel = false;
                    this.enter_page(window, cx);
                } else if this.focused_nav(window).is_none() {
                    let view = this.app.current_view;
                    this.focus_nav(view, window, cx);
                }
            });
        }
        NavCommand::Search => {
            if window.has_active_dialog(cx) {
                return;
            }
            root.update(cx, |this, cx| this.focus_search(window, cx));
        }
        NavCommand::Keyboard => root.read(cx).open_keyboard(),
        NavCommand::PrevView | NavCommand::NextView => {
            if window.has_active_dialog(cx) {
                return;
            }
            let step = if command == NavCommand::PrevView {
                -1
            } else {
                1
            };
            root.update(cx, |this, cx| this.cycle_view(step, window, cx));
        }
        NavCommand::Page(dir) => {
            let h: f32 = window.viewport_size().height.into();
            let sidebar = root.read(cx).sidebar_width(window);
            scroll_page(sidebar, f32::from(dir) * h * 0.75, window, cx);
        }
        NavCommand::Scroll(dy) => {
            let sidebar = root.read(cx).sidebar_width(window);
            scroll_page(sidebar, dy, window, cx);
        }
        NavCommand::Menu => {
            if window.has_active_dialog(cx) {
                return;
            }
            root.update(cx, |this, cx| this.open_view(View::Settings, window, cx));
        }
        NavCommand::ToggleSidebar => root.update(cx, |this, cx| {
            this.sidebar_collapsed = !this.is_sidebar_collapsed(window);
            cx.notify();
        }),
        NavCommand::Connected(_) | NavCommand::Disconnected => {}
    }
}

fn move_focus(root: &gpui_kit::Entity<GuiRoot>, dir: Direction, window: &mut Window, cx: &mut App) {
    // Text fields keep Left / Right for the caret; Up / Down leave the field.
    if window.has_focused_input(cx) {
        match dir {
            Direction::Up | Direction::Down => step_focus(forward(dir), window, cx),
            Direction::Left | Direction::Right => {
                press_key(arrow(dir), window, cx);
            }
        }
        return;
    }

    let overlay = overlay_open(window, cx);
    if !overlay {
        let nav = root.read(cx).focused_nav(window);
        if let Some(ix) = nav {
            root.update(cx, |this, cx| match dir {
                Direction::Up if ix > 0 => this.focus_nav(View::ALL[ix - 1], window, cx),
                Direction::Down if ix + 1 < View::ALL.len() => {
                    this.focus_nav(View::ALL[ix + 1], window, cx);
                }
                Direction::Right => this.open_view(View::ALL[ix], window, cx),
                _ => {}
            });
            return;
        }
        if window.focused(cx).is_none() {
            root.update(cx, |this, cx| {
                let view = this.app.current_view;
                this.focus_nav(view, window, cx);
            });
            return;
        }
        if root.update(cx, |this, cx| this.library_move(dir, window, cx)) {
            return;
        }
    }

    if press_key(arrow(dir), window, cx) {
        return;
    }

    let page = root.read(cx).page_focus.clone();
    let before = window.focused(cx);
    let was_in_page = page.contains_focused(window, cx);
    step_focus(forward(dir), window, cx);
    if overlay || !was_in_page || page.contains_focused(window, cx) {
        return;
    }
    // Tab order ran off the page. Going back lands on the sidebar entry for
    // this page; going forward past the end stays on the last control.
    if forward(dir) {
        if let Some(before) = before {
            window.focus(&before, cx);
        }
    } else {
        root.update(cx, |this, cx| {
            let view = this.app.current_view;
            this.focus_nav(view, window, cx);
        });
    }
}
