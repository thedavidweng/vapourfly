//! Legacy hand-drawn overlays; slated for removal once dialogs land.

use gpui::{
    App, Entity, Hsla, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div,
    px, rgb,
};
use gpui_component::{
    ActiveTheme, Disableable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use vapourfly_core::dynamic::DynamicTemplate;
use vapourfly_core::mood::EditorialMood;

use crate::app::PlaylistChooser;

use crate::ui::GuiRoot;

impl GuiRoot {
    pub(crate) fn confirm_overlay(&self, entity: Entity<Self>, cx: &App) -> impl IntoElement {
        let plan = self.app.dry_run_plan.as_ref();
        v_flex()
            .id("confirm")
            .absolute()
            .inset_0()
            .items_center()
            .justify_center()
            .bg(Hsla::from(rgb(0x000000)).opacity(0.45))
            .child(
                v_flex()
                    .w(px(520.))
                    .p_5()
                    .gap_3()
                    .rounded(cx.theme().radius_lg)
                    .bg(cx.theme().background)
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(div().text_lg().font_semibold().child("Confirm write"))
                    .child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().muted_foreground)
                            .child(if self.app.dry_run_loading {
                                "Preparing dry-run diff…".into()
                            } else if let Some(plan) = plan {
                                format!(
                                    "Target {}\n+{} / −{} app ids",
                                    plan.target_path.display(),
                                    plan.diff.app_ids_added.len(),
                                    plan.diff.app_ids_removed.len()
                                )
                            } else {
                                "Confirm this write. A backup is created first.".into()
                            }),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("confirm-go")
                                    .primary()
                                    .label("Confirm")
                                    .disabled(self.app.ui_demo || self.app.dry_run_loading)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, _, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.execute_pending_action();
                                                this.arm_poll(cx);
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .child(
                                Button::new("confirm-cancel")
                                    .ghost()
                                    .label("Cancel")
                                    .on_click(move |_, _, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.app.show_confirm_dialog = false;
                                            this.app.pending_action = None;
                                            this.app.dry_run_plan = None;
                                            cx.notify();
                                        });
                                    }),
                            ),
                    ),
            )
    }

    pub(crate) fn chooser_overlay(&self, entity: Entity<Self>, cx: &App) -> impl IntoElement {
        v_flex()
            .id("chooser")
            .absolute()
            .inset_0()
            .items_center()
            .justify_center()
            .bg(Hsla::from(rgb(0x000000)).opacity(0.45))
            .child(
                v_flex()
                    .w(px(420.))
                    .p_5()
                    .gap_3()
                    .rounded(cx.theme().radius_lg)
                    .bg(cx.theme().background)
                    .child(div().text_lg().font_semibold().child(
                        if self.app.playlist_chooser == PlaylistChooser::Dynamic {
                            "Dynamic template"
                        } else {
                            "Editorial mood"
                        },
                    ))
                    .child(if self.app.playlist_chooser == PlaylistChooser::Dynamic {
                        v_flex()
                            .gap_2()
                            .children(
                                [DynamicTemplate::DeckSession, DynamicTemplate::FinishIt]
                                    .into_iter()
                                    .map(|tmpl| {
                                        let entity = entity.clone();
                                        let id = tmpl.id().to_string();
                                        Button::new(SharedString::from(id.clone()))
                                            .label(tmpl.label())
                                            .on_click(move |_, _, cx| {
                                                entity.update(cx, |this, cx| {
                                                    this.app.dynamic_template = id.clone();
                                                    this.app.start_dynamic_generate();
                                                    this.app.playlist_chooser =
                                                        PlaylistChooser::None;
                                                    this.arm_poll(cx);
                                                    cx.notify();
                                                });
                                            })
                                    }),
                            )
                            .into_any_element()
                    } else {
                        v_flex()
                            .gap_2()
                            .children(EditorialMood::all().iter().map(|mood| {
                                let entity = entity.clone();
                                let id = mood.id().to_string();
                                Button::new(SharedString::from(id.clone()))
                                    .label(mood.name())
                                    .on_click(move |_, _, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.app.editorial_mood = id.clone();
                                            this.app.start_mood_generate();
                                            this.app.playlist_chooser = PlaylistChooser::None;
                                            this.arm_poll(cx);
                                            cx.notify();
                                        });
                                    })
                            }))
                            .into_any_element()
                    })
                    .child(
                        Button::new("chooser-cancel")
                            .ghost()
                            .label("Cancel")
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.app.playlist_chooser = PlaylistChooser::None;
                                    cx.notify();
                                });
                            }),
                    ),
            )
    }
}
