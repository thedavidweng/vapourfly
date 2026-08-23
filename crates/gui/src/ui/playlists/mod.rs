//! Playlists master view and stored-playlist rail.

mod match_share;
mod rules;
mod workspace;

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, px,
};
use gpui_component::{
    ActiveTheme, Disableable, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use vapourfly_core::playlist;

use crate::app::{
    PendingAction, PlaylistChooser, playlist_content_type_label, playlist_game_count,
};

use crate::ui::GuiRoot;

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

    pub(crate) fn playlist_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
}
