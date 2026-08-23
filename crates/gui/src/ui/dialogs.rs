//! Dialog plumbing on top of `WindowExt::open_dialog`.
//!
//! Two rules keep this layer safe:
//!
//! 1. **Never open from inside `render`.** `open_dialog` updates the `Root`
//!    entity, which is the ancestor invoking our render — nested updates
//!    panic. Every open here goes through `window.defer`.
//! 2. **The demo-mode write gate lives here.** `VapourflyApp::
//!    execute_pending_action` has no `ui_demo` guard by design; every
//!    confirming button is `disabled(ui_demo || loading)` *and* re-checks
//!    the flag before committing.
//!
//! Dialog builders and footers run every frame (`Fn` closures), so they may
//! only capture owned clones and read state through the entity handle.

use gpui::{
    App, Context, Entity, IntoElement, ParentElement, SharedString, Styled, WeakEntity, Window,
    div, px,
};
use gpui_component::{
    ActiveTheme, Disableable, Sizable, StyledExt, WindowExt, button::Button,
    button::ButtonVariants, v_flex,
};
use vapourfly_core::dynamic::DynamicTemplate;
use vapourfly_core::mood::EditorialMood;

use crate::app::{PendingAction, PlaylistChooser, playlist_game_count};

use super::root::GuiRoot;

/// Tracks which requested dialogs have actually been opened so a render
/// pass can request one exactly once.
#[derive(Default)]
pub(crate) struct DialogPump {
    pub(crate) confirm_open: bool,
    pub(crate) chooser_open: bool,
}

/// Copy describing a pending write, derived from app state each frame.
struct WriteConfirmCopy {
    title: String,
    verb: &'static str,
}

fn write_confirm_copy(action: &PendingAction) -> WriteConfirmCopy {
    match action {
        PendingAction::JunkApply => WriteConfirmCopy {
            title: "Apply junk candidates?".into(),
            verb: "Apply",
        },
        PendingAction::JunkHide => WriteConfirmCopy {
            title: "Hide the selected games?".into(),
            verb: "Hide",
        },
        PendingAction::RecommendCollection => WriteConfirmCopy {
            title: "Save recommendations as a Steam collection?".into(),
            verb: "Save",
        },
        PendingAction::PlaylistSync(pf) => WriteConfirmCopy {
            title: format!("Sync playlist “{}”?", pf.playlist.id),
            verb: "Sync",
        },
        PendingAction::BackupRestore(_) => WriteConfirmCopy {
            title: "Restore backup?".into(),
            verb: "Restore",
        },
    }
}

impl GuiRoot {
    /// Called once per render: opens any requested dialog exactly once.
    pub(crate) fn pump_dialogs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.app.show_confirm_dialog && !self.dialog_pump.confirm_open {
            self.dialog_pump.confirm_open = true;
            let weak = cx.entity().downgrade();
            window.defer(cx, move |window, cx| open_write_confirm(weak, window, cx));
        }
        if self.app.playlist_chooser != PlaylistChooser::None && !self.dialog_pump.chooser_open {
            self.dialog_pump.chooser_open = true;
            let weak = cx.entity().downgrade();
            window.defer(cx, move |window, cx| {
                open_playlist_chooser(weak, window, cx);
            });
        }
    }

    pub(crate) fn clear_confirm(&mut self, cx: &mut Context<Self>) {
        self.app.show_confirm_dialog = false;
        self.app.pending_action = None;
        self.app.dry_run_plan = None;
        self.app.dry_run_loading = false;
        self.dialog_pump.confirm_open = false;
        cx.notify();
    }

    pub(crate) fn clear_chooser(&mut self, cx: &mut Context<Self>) {
        self.app.playlist_chooser = PlaylistChooser::None;
        self.dialog_pump.chooser_open = false;
        cx.notify();
    }
}

/// Open the dry-run write confirmation for the current pending action.
fn open_write_confirm(root: WeakEntity<GuiRoot>, window: &mut Window, cx: &mut App) {
    let Some(root) = root.upgrade() else {
        return;
    };
    let weak = root.downgrade();
    let weak_for_close = weak.clone();
    window.open_dialog(cx, move |dialog, _, cx| {
        // Rebuilt every frame so the diff appears as soon as the dry run
        // finishes without reopening the dialog.
        let (copy, lines, loading, demo) = {
            let this = root.read(cx);
            let copy = this
                .app
                .pending_action
                .as_ref()
                .map(write_confirm_copy)
                .unwrap_or(WriteConfirmCopy {
                    title: "Confirm write?".into(),
                    verb: "Confirm",
                });
            let mut lines: Vec<String> = Vec::new();
            if this.app.dry_run_loading {
                lines.push("Preparing dry-run diff…".into());
            } else if let Some(plan) = this.app.dry_run_plan.as_ref() {
                lines.push(plan.target_path.display().to_string());
                lines.push(format!(
                    "+{} / −{} app ids",
                    plan.diff.app_ids_added.len(),
                    plan.diff.app_ids_removed.len()
                ));
            }
            if let Some(PendingAction::PlaylistSync(pf)) = this.app.pending_action.as_ref() {
                lines.push(format!(
                    "{} · {} games · a backup is created first",
                    pf.playlist.name,
                    playlist_game_count(&pf.playlist.content, None)
                ));
            } else {
                lines.push("A backup is created first.".into());
            }
            (copy, lines, this.app.dry_run_loading, this.app.ui_demo)
        };

        dialog
            .title(div().font_semibold().child(copy.title))
            .w(px(520.))
            .keyboard(true)
            .overlay_closable(true)
            .child(v_flex().gap_1().children(lines.into_iter().map(|line| {
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(line)
            })))
            .footer({
                let weak = weak.clone();
                let root = root.clone();
                move |_ok, _cancel, _window, _cx| {
                    vec![
                        Button::new("dialog-cancel")
                            .ghost()
                            .small()
                            .label("Cancel")
                            .on_click({
                                let weak = weak.clone();
                                move |_, window, cx| {
                                    window.close_dialog(cx);
                                    if let Some(this) = weak.upgrade() {
                                        this.update(cx, |this, cx| this.clear_confirm(cx));
                                    }
                                }
                            })
                            .into_any_element(),
                        Button::new("dialog-ok")
                            .primary()
                            .small()
                            .label(copy.verb)
                            // Demo mode never writes: gate the button AND the
                            // handler below (execute_pending_action is ungated).
                            .disabled(demo || loading)
                            .on_click({
                                let root = root.clone();
                                move |_, window, cx| {
                                    let demo = root.read(cx).app.ui_demo;
                                    if demo {
                                        return;
                                    }
                                    window.close_dialog(cx);
                                    root.update(cx, |this, cx| {
                                        this.app.execute_pending_action();
                                        this.app.show_confirm_dialog = false;
                                        this.app.pending_action = None;
                                        this.app.dry_run_plan = None;
                                        this.dialog_pump.confirm_open = false;
                                        this.arm_poll(cx);
                                        cx.notify();
                                    });
                                }
                            })
                            .into_any_element(),
                    ]
                }
            })
            .on_close({
                let weak_for_close = weak_for_close.clone();
                move |_, _, cx| {
                    if let Some(this) = weak_for_close.upgrade() {
                        this.update(cx, |this, cx| this.clear_confirm(cx));
                    }
                }
            })
    });
}

/// Open the Dynamic-template / Editorial-mood chooser as a dialog.
fn open_playlist_chooser(root: WeakEntity<GuiRoot>, window: &mut Window, cx: &mut App) {
    let Some(root) = root.upgrade() else {
        return;
    };
    let weak = root.downgrade();
    let weak_for_close = weak.clone();
    let (title, options): (String, Vec<(String, String)>) = {
        let this = root.read(cx);
        match this.app.playlist_chooser {
            PlaylistChooser::Dynamic => (
                "Dynamic template".into(),
                [DynamicTemplate::DeckSession, DynamicTemplate::FinishIt]
                    .into_iter()
                    .map(|t| (t.id().to_string(), t.label().to_string()))
                    .collect(),
            ),
            PlaylistChooser::Mood => (
                "Editorial mood".into(),
                EditorialMood::all()
                    .iter()
                    .map(|m| (m.id().to_string(), m.name().to_string()))
                    .collect(),
            ),
            PlaylistChooser::None => ("Choose".into(), Vec::new()),
        }
    };

    window.open_dialog(cx, move |dialog, _, _| {
        dialog
            .title(div().font_semibold().child(title.clone()))
            .w(px(420.))
            .keyboard(true)
            .overlay_closable(true)
            .child(
                v_flex()
                    .gap_1()
                    .children(
                        options
                            .clone()
                            .into_iter()
                            .enumerate()
                            .map(|(ix, (id, label))| {
                                let weak = weak.clone();
                                Button::new(("chooser-option", ix))
                                    .ghost()
                                    .small()
                                    .label(label)
                                    .on_click(move |_, window, cx| {
                                        window.close_dialog(cx);
                                        if let Some(this) = weak.upgrade() {
                                            this.update(cx, |this, cx| {
                                                match this.app.playlist_chooser {
                                                    PlaylistChooser::Dynamic => {
                                                        this.app.dynamic_template = id.clone();
                                                        this.app.start_dynamic_generate();
                                                    }
                                                    PlaylistChooser::Mood => {
                                                        this.app.editorial_mood = id.clone();
                                                        this.app.start_mood_generate();
                                                    }
                                                    PlaylistChooser::None => {}
                                                }
                                                this.app.playlist_chooser = PlaylistChooser::None;
                                                this.dialog_pump.chooser_open = false;
                                                this.arm_poll(cx);
                                                cx.notify();
                                            });
                                        }
                                    })
                            }),
                    ),
            )
            .on_close({
                let weak_for_close = weak_for_close.clone();
                move |_, _, cx| {
                    if let Some(this) = weak_for_close.upgrade() {
                        this.update(cx, |this, cx| this.clear_chooser(cx));
                    }
                }
            })
    });
}

/// Callback run when an alert dialog's verb button is pressed.
type AlertCallback = std::rc::Rc<dyn Fn(&mut GuiRoot, &mut Context<GuiRoot>)>;

/// A one-off confirmation for non-dry-run flows (backup restore, sync…).
pub(crate) struct AlertSpec {
    pub title: String,
    pub lines: Vec<String>,
    /// Verb on the confirming button, e.g. "Restore".
    pub verb: String,
}

/// Open an alert-style confirm dialog. `on_ok` runs only when the user
/// confirms; Escape and the overlay dismiss without side effects.
pub(crate) fn open_alert(
    entity: &Entity<GuiRoot>,
    window: &mut Window,
    cx: &mut Context<GuiRoot>,
    spec: AlertSpec,
    on_ok: impl Fn(&mut GuiRoot, &mut Context<GuiRoot>) + 'static,
) {
    let weak = entity.downgrade();
    let on_ok: AlertCallback = std::rc::Rc::new(on_ok);
    window.defer(cx, move |window, cx| {
        // Normalize to cheaply-cloneable types up front: the dialog builder
        // below is an `Fn` re-invoked every frame and may only clone.
        let AlertSpec { title, lines, verb } = spec;
        let title: SharedString = title.into();
        let verb: SharedString = verb.into();
        let body_lines: Vec<SharedString> = lines.into_iter().map(Into::into).collect();
        let on_ok = std::rc::Rc::clone(&on_ok);
        window.open_dialog(cx, move |dialog, _, cx| {
            let weak_for_ok = weak.clone();
            let on_ok = std::rc::Rc::clone(&on_ok);
            dialog
                .title(div().font_semibold().child(title.clone()))
                .w(px(480.))
                .keyboard(true)
                .overlay_closable(true)
                .child(v_flex().gap_1().children(body_lines.iter().map(|line| {
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(line.clone())
                })))
                .footer({
                    let verb = verb.clone();
                    move |_ok, _cancel, _window, _cx| {
                        vec![
                            Button::new("alert-cancel")
                                .ghost()
                                .small()
                                .label("Cancel")
                                .on_click(|_, window, cx| {
                                    window.close_dialog(cx);
                                })
                                .into_any_element(),
                            Button::new("alert-ok")
                                .primary()
                                .small()
                                .label(verb.clone())
                                .on_click({
                                    let weak = weak_for_ok.clone();
                                    let on_ok = std::rc::Rc::clone(&on_ok);
                                    move |_, window, cx| {
                                        window.close_dialog(cx);
                                        if let Some(this) = weak.upgrade() {
                                            this.update(cx, |this, cx| on_ok(this, cx));
                                        }
                                    }
                                })
                                .into_any_element(),
                        ]
                    }
                })
        });
    });
}
