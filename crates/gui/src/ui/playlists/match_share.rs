//! Playlist match report and share/export tabs.

use gpui::{ClipboardItem, Context, IntoElement, ParentElement, Styled, div, prelude::*};
use gpui_component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    tab::{Tab, TabBar},
    v_flex,
};

use crate::app::{PlaylistMatchTab, PlaylistShareTab, empty_value_label};

use crate::ui::GuiRoot;
use crate::ui::shared::empty_or;

impl GuiRoot {
    pub(crate) fn playlist_match(&self, cx: &mut Context<Self>) -> impl IntoElement {
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

    pub(crate) fn playlist_share(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
}
