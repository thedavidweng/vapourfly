//! Data-source status and cache health.

use gpui::{
    App, Context, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div,
    prelude::*, px,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex,
    switch::Switch,
    tag::Tag,
    v_flex,
};

use crate::app::{
    CredentialSignal, source_credential_signal, source_display_name, source_refresh_enabled,
};

use crate::ui::GuiRoot;

/// One "label … value" cache-health metric; the fixed width keeps the
/// numbers right-aligned across the summary line.
fn cache_metric(label: &str, value: String, warning: bool, cx: &App) -> impl IntoElement {
    h_flex()
        .w(px(104.))
        .justify_between()
        .child(
            div()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child(SharedString::from(label.to_string())),
        )
        .child(
            div()
                .text_sm()
                .font_medium()
                .map(|this| {
                    if warning {
                        this.text_color(cx.theme().warning)
                    } else {
                        this
                    }
                })
                .child(value),
        )
}

impl GuiRoot {
    pub(crate) fn data_sources(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let statuses = self.app.source_statuses.clone();
        let entries: u64 = statuses.iter().map(|s| s.cache_entries as u64).sum();
        let stale: u64 = statuses.iter().map(|s| s.stale_entries as u64).sum();
        v_flex()
            .id("sources")
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_xl().font_semibold().child("Data Sources"))
                    .child(
                        h_flex()
                            .items_center()
                            .gap_4()
                            .child(
                                Switch::new("sources.offline-switch")
                                    .checked(self.app.offline_mode)
                                    .label("Offline mode")
                                    .on_click({
                                        let entity = entity.clone();
                                        move |checked: &bool, _window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.offline_mode = *checked;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("sources.refresh-all")
                                    .outline()
                                    .icon(Icon::new(IconName::Redo2))
                                    .label("Refresh All")
                                    .tooltip("Refresh all sources")
                                    .loading(self.app.cache_refresh_loading)
                                    .disabled(
                                        self.app.offline_mode
                                            || self.app.cache_refresh_loading
                                            || self.app.ui_demo,
                                    )
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.start_cache_refresh(None);
                                                this.arm_poll(cx);
                                                cx.notify();
                                            });
                                        }
                                    }),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_4()
                    .child(div().text_sm().font_semibold().child("Cache health"))
                    .child(cache_metric("entries", entries.to_string(), false, cx))
                    .child(cache_metric("stale", stale.to_string(), stale > 0, cx))
                    .child(cache_metric("sources", statuses.len().to_string(), false, cx)),
            )
            .when(self.app.offline_mode || entries == 0, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .text_color(cx.theme().muted_foreground)
                        .child(Icon::new(IconName::Inbox))
                        .child(div().text_sm().child(if self.app.offline_mode {
                            "Offline mode is on — stats come from the local disk cache; network refreshes are paused."
                        } else {
                            "No cached source data yet — scan your library, then Refresh all to populate caches."
                        })),
                )
            })
            .when_some(self.app.cache_refresh_msg.clone(), |this, msg| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(msg),
                )
            })
            .children(statuses.into_iter().enumerate().map(|(ix, st)| {
                let entity = entity.clone();
                let id = st.name.clone();
                let enabled = source_refresh_enabled(
                    &id,
                    self.app.has_igdb,
                    self.app.has_rawg,
                    self.app.offline_mode,
                    self.app.cache_refresh_loading,
                ) && !self.app.ui_demo;
                let signal =
                    source_credential_signal(&id, self.app.has_igdb, self.app.has_rawg);
                let label = source_display_name(&id);
                h_flex()
                    .id(("sources.row", ix))
                    .h(px(40.))
                    .gap_3()
                    .child(
                        div()
                            .w(px(120.))
                            .font_medium()
                            .child(SharedString::from(label)),
                    )
                    .child(
                        div().w(px(110.)).child(
                            match signal {
                                CredentialSignal::Missing => Tag::warning(),
                                CredentialSignal::Optional => Tag::info(),
                                CredentialSignal::Configured
                                | CredentialSignal::NotRequired => Tag::secondary(),
                            }
                            .small()
                            .child(signal.label()),
                        ),
                    )
                    .child(
                        div()
                            .w(px(80.))
                            .child(Tag::secondary().small().child(format!(
                                "{} entries",
                                st.cache_entries
                            ))),
                    )
                    .child(
                        div().w(px(70.)).child(
                            if st.stale_entries > 0 {
                                Tag::warning().small()
                            } else {
                                Tag::secondary().small()
                            }
                            .child(format!("{} stale", st.stale_entries)),
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(
                                st.last_success
                                    .map(|t| t.to_rfc3339())
                                    .unwrap_or_else(|| "—".into()),
                            ),
                    )
                    .child(
                        Button::new(("sources.refresh".into(), id.clone()))
                            .xsmall()
                            .ghost()
                            .icon(Icon::new(IconName::Redo2))
                            .tooltip(format!("Refresh {label}"))
                            .loading(self.app.cache_refresh_loading)
                            .disabled(!enabled)
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.app.start_cache_refresh(Some(id.clone()));
                                    this.arm_poll(cx);
                                    cx.notify();
                                });
                            }),
                    )
            }))
    }
}
