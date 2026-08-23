//! Data-source status and cache health.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, prelude::*,
    px,
};
use gpui_component::{
    ActiveTheme, Disableable, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};

use crate::app::{source_credential_signal, source_display_name, source_refresh_enabled};

use crate::ui::GuiRoot;

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
                            .gap_2()
                            .child(
                                Button::new("offline")
                                    .small()
                                    .when(self.app.offline_mode, |b| b.primary())
                                    .label(if self.app.offline_mode {
                                        "Offline"
                                    } else {
                                        "Online"
                                    })
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.offline_mode = !this.app.offline_mode;
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("refresh-all")
                                    .primary()
                                    .label("Refresh All")
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
            .child(div().text_sm().child(format!(
                "Cache health · {entries} entries · {stale} stale · {} sources",
                statuses.len()
            )))
            .when_some(self.app.cache_refresh_msg.clone(), |this, msg| {
                this.child(div().text_xs().child(msg))
            })
            .children(statuses.into_iter().map(|st| {
                let entity = entity.clone();
                let id = st.name.clone();
                let enabled = source_refresh_enabled(
                    &id,
                    self.app.has_igdb,
                    self.app.has_rawg,
                    self.app.offline_mode,
                    self.app.cache_refresh_loading,
                ) && !self.app.ui_demo;
                h_flex()
                    .id(SharedString::from(id.clone()))
                    .h(px(40.))
                    .gap_3()
                    .child(
                        div()
                            .w(px(120.))
                            .font_medium()
                            .child(source_display_name(&id)),
                    )
                    .child(div().w(px(110.)).text_xs().child(
                        source_credential_signal(&id, self.app.has_igdb, self.app.has_rawg).label(),
                    ))
                    .child(
                        div()
                            .w(px(80.))
                            .text_xs()
                            .child(format!("{} entries", st.cache_entries)),
                    )
                    .child(
                        div()
                            .w(px(70.))
                            .text_xs()
                            .child(format!("{} stale", st.stale_entries)),
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
                        Button::new(SharedString::from(format!("rf-{id}")))
                            .xsmall()
                            .label("Refresh")
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
