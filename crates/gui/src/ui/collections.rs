//! Steam collections grid.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, prelude::*,
    px,
};
use gpui_component::{
    ActiveTheme, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex,
    scroll::ScrollableElement,
    tag::Tag,
    v_flex,
};

use super::shared::hx;
use crate::app::{ARTWORK_PALETTE, steam_capsule_uri};

use crate::ui::GuiRoot;

impl GuiRoot {
    pub(crate) fn collections(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        v_flex()
            .id("collections")
            .size_full()
            .gap_3()
            .child(
                h_flex()
                    .justify_between()
                    .child(div().text_xl().font_semibold().child("Collections"))
                    .child(
                        Button::new("col-export")
                            .primary()
                            .label("Export all")
                            .on_click(move |_, _, cx| {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("JSON", &["json"])
                                    .save_file()
                                {
                                    entity.update(cx, |this, cx| {
                                        this.app.collections_export_path =
                                            path.to_string_lossy().into();
                                        match this.app.export_collections() {
                                            Ok(()) => {
                                                this.app.success_msg =
                                                    Some("Exported collections.".into());
                                            }
                                            Err(e) => this.app.error = Some(e),
                                        }
                                        cx.notify();
                                    });
                                }
                            }),
                    ),
            )
            .child(div().id("col-grid").flex_1().overflow_y_scrollbar().child(
                div().flex().flex_row().flex_wrap().gap_3().children(
                    self.app.collections.iter().map(|c| {
                        v_flex()
                            .id(SharedString::from(c.name.clone()))
                            .w(px(220.))
                            .p_3()
                            .gap_2()
                            .border_1()
                            .border_color(cx.theme().border)
                            .rounded(cx.theme().radius)
                            .child(div().text_sm().font_medium().child(c.name.clone()))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(format!("{} games", c.app_ids.len())),
                            )
                            .child(h_flex().gap_1().children(
                                c.app_ids.iter().copied().take(4).map(|id| {
                                    let offline = self.app.demo_or_offline();
                                    let (top, _) =
                                        ARTWORK_PALETTE[(id as usize) % ARTWORK_PALETTE.len()];
                                    let uri = steam_capsule_uri(id);
                                    div()
                                        .id(("collage", id as usize))
                                        .w(px(44.))
                                        .h(px(22.))
                                        .rounded(px(3.))
                                        .bg(hx(top))
                                        .on_mouse_down(gpui::MouseButton::Left, move |_, _, _| {
                                            if !offline {
                                                crate::app::open_url_in_browser(&uri);
                                            }
                                        })
                                }),
                            ))
                            .when(c.is_hidden_collection, |this| {
                                this.child(Tag::warning().small().child("Hidden"))
                            })
                    }),
                ),
            ))
    }
}
