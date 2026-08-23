//! Playlist match report and share/export tabs.

use gpui::{ClipboardItem, Context, IntoElement, ParentElement, Styled, div, prelude::*};
use gpui_component::{
    ActiveTheme, IconName, Sizable, StyledExt, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    notification::Notification,
    tab::{Tab, TabBar},
    tag::Tag,
    v_flex,
};
use vapourfly_core::models::{PlaylistContent, PlaylistFile};

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
                TabBar::new("pl.match-tab")
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
                        .child(h_flex().flex_wrap().gap_2().children([
                            report_pill("Owned", report.owned.len(), Tag::secondary()),
                            report_pill("Missing", report.missing.len(), Tag::warning()),
                            report_pill("Played", report.played.len(), Tag::success()),
                            report_pill("Unplayed", report.unplayed.len(), Tag::info()),
                            report_pill("Hidden", report.hidden.len(), Tag::warning()),
                            report_pill("Junk", report.junk.len(), Tag::danger()),
                        ]))
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
                TabBar::new("pl.share-tab")
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
                    .flex_wrap()
                    .child(
                        Button::new("pl.share-copy")
                            .small()
                            .icon(IconName::Copy)
                            .label("Copy VF1")
                            .tooltip("Copy share code to clipboard")
                            .on_click({
                                let entity = entity.clone();
                                move |_, window, cx| {
                                    let mut copied_summary: Option<String> = None;
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
                                                        copied_summary = Some(vf1_summary(&pf));
                                                    }
                                                    Err(e) => this.app.error = Some(e.to_string()),
                                                }
                                            }
                                            Err(e) => this.app.error = Some(e),
                                        }
                                        cx.notify();
                                    });
                                    if let Some(summary) = copied_summary {
                                        window.push_notification(
                                            Notification::success(format!(
                                                "Share code copied · {summary}"
                                            )),
                                            cx,
                                        );
                                    }
                                }
                            }),
                    )
                    .child(
                        Button::new("pl.share-import-toggle")
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
                            Button::new("pl.share-paste")
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

/// Renders one match-report summary pill with its count trailing on the
/// right.
fn report_pill(label: &'static str, count: usize, pill: Tag) -> impl IntoElement {
    pill.small().child(
        h_flex()
            .items_center()
            .gap_1()
            .child(div().child(label))
            .child(div().font_medium().child(count.to_string())),
    )
}

/// One-line human summary of what a VF1 share code encodes.
fn vf1_summary(pf: &PlaylistFile) -> String {
    let scope = match &pf.playlist.content {
        PlaylistContent::Manual { app_ids } => format!("{} games", app_ids.len()),
        PlaylistContent::Rules { rules } => format!("{} rules", rules.len()),
    };
    format!("'{}' · {scope}", pf.playlist.name)
}
