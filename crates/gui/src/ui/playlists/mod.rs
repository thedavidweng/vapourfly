//! Playlists master view and stored-playlist rail.

mod match_share;
mod rules;
mod workspace;

use std::path::PathBuf;

use gpui::{
    Action, Context, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, Window,
    div, prelude::FluentBuilder, px,
};
use gpui_component::{
    ActiveTheme, Disableable, IconName, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{ContextMenuExt, PopupMenu},
    v_flex,
};
use vapourfly_core::playlist;

use crate::app::{
    PendingAction, PlaylistChooser, playlist_content_type_label, playlist_game_count,
};

use super::dialogs::{AlertSpec, open_alert};

use crate::ui::GuiRoot;

/// Rail row command: load this stored playlist into the editor.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = vapourfly, no_json)]
pub(crate) struct RailOpen(pub SharedString);

/// Rail row command: sync this stored playlist to Steam (confirmed via the
/// alert, then the central dry-run dialog).
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = vapourfly, no_json)]
pub(crate) struct RailSync(pub SharedString);

/// Rail row command: export this stored playlist to a file chosen in the
/// native save dialog.
#[derive(Clone, PartialEq, Debug, Action)]
#[action(namespace = vapourfly, no_json)]
pub(crate) struct RailExport(pub SharedString);

impl GuiRoot {
    pub(crate) fn playlists(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
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
                            .child(
                                Button::new("pl-new")
                                    .small()
                                    .icon(IconName::Plus)
                                    .label("New")
                                    .tooltip("New playlist")
                                    .on_click({
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
                                    }),
                            )
                            .child(
                                Button::new("pl-dyn")
                                    .small()
                                    .icon(IconName::Bot)
                                    .label("Dynamic…")
                                    .tooltip("Generate from dynamic template")
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.playlist_chooser =
                                                    PlaylistChooser::Dynamic;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("pl-mood")
                                    .small()
                                    .icon(IconName::Palette)
                                    .label("Mood…")
                                    .tooltip("Generate from editorial mood")
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.playlist_chooser = PlaylistChooser::Mood;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("pl-save")
                                    .primary()
                                    .small()
                                    .icon(IconName::CircleCheck)
                                    .label("Save")
                                    .tooltip("Save playlist")
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.save_playlist_from_editor(cx);
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("pl-export")
                                    .small()
                                    .icon(IconName::ExternalLink)
                                    .label("Export")
                                    .tooltip("Export to file…")
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            if let Some(path) = rfd::FileDialog::new()
                                                .add_filter("JSON", &["json"])
                                                .save_file()
                                            {
                                                entity.update(cx, |this, cx| {
                                                    this.export_loaded_playlist_to(path, cx);
                                                });
                                            }
                                        }
                                    }),
                            )
                            .child(
                                Button::new("pl-import")
                                    .small()
                                    .icon(IconName::File)
                                    .label("Import")
                                    .tooltip("Import from file…")
                                    .on_click({
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
                                                        Err(e) => {
                                                            this.app.error = Some(e.to_string());
                                                        }
                                                    }
                                                    cx.notify();
                                                });
                                            }
                                        }
                                    }),
                            )
                            .child(
                                Button::new("pl-sync")
                                    .small()
                                    .icon(IconName::Replace)
                                    .label("Sync to Steam")
                                    .tooltip("Sync to Steam")
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

    pub(crate) fn playlist_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let demo = self.app.ui_demo;
        let selected_id = self.app.playlist_load_selected.clone();
        let list_active = cx.theme().list_active;
        let accent_fg = cx.theme().accent_foreground;
        v_flex()
            .id("pl-rail")
            .w(px(260.))
            .h_full()
            .gap_1()
            .border_r_1()
            .border_color(cx.theme().border)
            .pr_3()
            // Rail commands arrive from each row's context menu; handlers sit
            // on this ancestor so the dispatched actions bubble here.
            .on_action(cx.listener(|this, action: &RailOpen, window, cx| {
                this.open_stored_playlist(&action.0, window, cx);
            }))
            .on_action(cx.listener(|this, action: &RailSync, window, cx| {
                this.sync_stored_playlist(&action.0, window, cx);
            }))
            .on_action(cx.listener(|this, action: &RailExport, _window, cx| {
                this.export_stored_playlist(&action.0, cx);
            }))
            .children(self.app.playlist_rail_entries.iter().enumerate().map(
                |(ix, (key, entry))| {
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
                        Err(e) => format!("{key} · error: {e}"),
                    };
                    let active = *key == selected_id;
                    let key = key.clone();
                    let menu_key = key.clone();
                    let entity = entity.clone();
                    Button::new(("playlist.rail-item", ix))
                        .ghost()
                        .w_full()
                        .label(label)
                        .when(active, |b| b.bg(list_active).text_color(accent_fg))
                        .on_click(move |_, window, cx| {
                            entity.update(cx, |this, cx| {
                                this.open_stored_playlist(&key, window, cx);
                            });
                        })
                        .context_menu(move |menu, _, _| {
                            rail_menu(menu, menu_key.clone(), menu_key.clone(), demo)
                        })
                },
            ))
    }
}

/// Build the per-row rail context menu: Open / Sync… / Export… dispatch the
/// row-scoped actions handled on the rail container.
fn rail_menu(menu: PopupMenu, key: String, menu_key: String, demo: bool) -> PopupMenu {
    menu.menu("Open", Box::new(RailOpen(key.clone().into())))
        .separator()
        .menu_with_disabled("Sync…", Box::new(RailSync(menu_key.into())), demo)
        .menu("Export…", Box::new(RailExport(key.into())))
}

impl GuiRoot {
    /// Commit the playlist editor into the store (Save button and the
    /// cmd-s action share this path).
    pub(crate) fn save_playlist_from_editor(&mut self, cx: &mut Context<Self>) {
        match self.app.build_playlist_from_edit_fields() {
            Ok(pf) => match self.app.store_playlist(&pf) {
                Ok(()) => {
                    self.app.success_msg = Some(format!("Saved {}", pf.playlist.id));
                    self.app.refresh_playlist_store_ids();
                }
                Err(e) => self.app.error = Some(e),
            },
            Err(e) => self.app.error = Some(e),
        }
        cx.notify();
    }
}

impl GuiRoot {
    /// Load a stored playlist into the editor. Shared by the rail button and
    /// the rail context-menu Open item.
    fn open_stored_playlist(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = self.app.load_playlist_from_store(id) {
            self.app.error = Some(e);
        } else {
            self.sync_playlist_inputs(window, cx);
            self.playlist_edit_synced = self.app.playlist_edit_generation;
            self.arm_poll(cx);
        }
        cx.notify();
    }

    /// Sync a stored playlist from the rail. Demo mode never writes;
    /// otherwise confirm the target playlist, then hand off to the central
    /// dry-run dialog, which performs the final confirmation.
    fn sync_stored_playlist(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.app.ui_demo {
            return;
        }
        let Some(pf) = self
            .app
            .playlist_rail_entries
            .iter()
            .find_map(|(entry_id, entry)| {
                if entry_id == id {
                    entry.as_ref().ok().cloned()
                } else {
                    None
                }
            })
        else {
            return;
        };
        let games = playlist_game_count(&pf.playlist.content, None);
        let lines = vec![
            format!("{} · {} games", pf.playlist.name, games),
            "A dry-run diff opens before anything is written.".to_string(),
        ];
        let entity = cx.entity();
        open_alert(
            &entity,
            window,
            cx,
            AlertSpec {
                title: format!("Sync “{}”?", pf.playlist.name),
                lines,
                verb: "Sync".into(),
            },
            move |this, cx| {
                // The alert callback is an `Fn` (the dialog rebuilds every
                // frame), so clone instead of consuming.
                this.app
                    .start_dry_run(PendingAction::PlaylistSync(pf.clone()));
                this.arm_poll(cx);
                cx.notify();
            },
        );
    }

    /// Export a stored playlist from the rail: load it, then reuse the
    /// toolbar's native save-dialog flow.
    fn export_stored_playlist(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Err(e) = self.app.load_playlist_from_store(id) {
            self.app.error = Some(e);
            cx.notify();
            return;
        }
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .save_file()
        {
            self.export_loaded_playlist_to(path, cx);
        }
        cx.notify();
    }

    /// Apply the chosen path and write the loaded playlist. Shared by the
    /// toolbar Export button and the rail Export… item.
    fn export_loaded_playlist_to(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.app.playlist_export_path = path.to_string_lossy().into();
        match self.app.export_loaded_playlist() {
            Ok(()) => {
                self.app.success_msg = Some("Exported playlist.".into());
            }
            Err(e) => self.app.error = Some(e),
        }
        cx.notify();
    }
}
