//! Settings view and backup management.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, SharedString, Styled, div, prelude::*,
    px,
};
use gpui_component::{
    ActiveTheme, Disableable, Icon, IconName, Sizable, StyledExt, WindowExt,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    input::Input,
    notification::Notification,
    scroll::ScrollableElement,
    switch::Switch,
    v_flex,
};

use super::dialogs::{AlertSpec, open_alert};
use super::shared::section;
use crate::app::open_url_in_browser;

use crate::ui::GuiRoot;
use crate::ui::root::set_input;

impl GuiRoot {
    pub(crate) fn settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        v_flex()
            .id("settings")
            .size_full()
            .gap_3()
            .overflow_y_scrollbar()
            .child(div().text_xl().font_semibold().child("Settings"))
            // Theme mode is owned by the title bar toggle + the ToggleTheme
            // action; this section only carries the reduce-motion preference.
            .child(section("Appearance", cx, |cx| {
                Switch::new("settings.reduce-motion")
                    .checked(super::motion::reduce_motion(cx))
                    .label("Reduce motion")
                    .disabled(self.app.ui_demo)
                    .on_click({
                        let entity = entity.clone();
                        move |checked: &bool, _window, cx| {
                            entity.update(cx, |this, cx| {
                                if this.app.ui_demo {
                                    return;
                                }
                                super::motion::set_reduce_motion(*checked, cx);
                                cx.notify();
                            });
                        }
                    })
                    .into_any_element()
            }))
            .child(section("Configuration", cx, |_| {
                v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().w(px(140.)).text_xs().child("Steam directory"))
                            .child(Input::new(&self.steam_dir_input).small().flex_1())
                            .child(
                                Button::new("pick-steam")
                                    .xsmall()
                                    .icon(Icon::new(IconName::FolderClosed))
                                    .tooltip("Browse for Steam directory")
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, window, cx| {
                                            if let Some(path) = rfd::FileDialog::new().pick_folder()
                                            {
                                                entity.update(cx, |this, cx| {
                                                    let value = path.to_string_lossy().into_owned();
                                                    this.app.steam_dir_edit = value.clone();
                                                    set_input(
                                                        &this.steam_dir_input,
                                                        &value,
                                                        window,
                                                        cx,
                                                    );
                                                    cx.notify();
                                                });
                                            }
                                        }
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().w(px(140.)).text_xs().child("Account override"))
                            .child(Input::new(&self.account_input).small().w(px(280.))),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().text_xs().child("Store country"))
                                    .child(Input::new(&self.cc_input).small().w(px(72.))),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().text_xs().child("Language"))
                                    .child(Input::new(&self.lang_input).small().w(px(140.))),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .child(div().text_xs().child("Backup retention"))
                                    .child(Input::new(&self.retention_input).small().w(px(64.))),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(div().w(px(140.)).text_xs().child("Steam Web API key"))
                            .child(Input::new(&self.api_key_input).small().w(px(300.)))
                            .child(
                                Button::new("settings.api-key-help")
                                    .xsmall()
                                    .ghost()
                                    .icon(Icon::new(IconName::ExternalLink))
                                    .tooltip("Open Steam Web API key page")
                                    .on_click(|_, _, _| {
                                        open_url_in_browser(
                                            "https://steamcommunity.com/dev/apikey",
                                        );
                                    }),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("save-settings")
                                    .small()
                                    .primary()
                                    .label("Save settings")
                                    .disabled(self.app.ui_demo)
                                    .on_click({
                                        let entity = entity.clone();
                                        move |_, window, cx| {
                                            entity.update(cx, |this, cx| {
                                                this.app.save_settings();
                                                // Success surfaces as a toast;
                                                // failures keep the inline text.
                                                let saved = this
                                                    .app
                                                    .settings_save_msg
                                                    .as_deref()
                                                    .is_some_and(|m| m.starts_with("Saved"));
                                                if saved {
                                                    window.push_notification(
                                                        Notification::success("Settings saved"),
                                                        cx,
                                                    );
                                                    this.app.settings_save_msg = None;
                                                }
                                                cx.notify();
                                            });
                                        }
                                    }),
                            )
                            .when_some(self.app.settings_save_msg.clone(), |this, msg| {
                                this.child(div().text_xs().child(msg))
                            }),
                    )
                    .into_any_element()
            }))
            .child(section("Detected accounts", cx, |_| {
                v_flex()
                    .gap_1()
                    .children(self.app.detected_accounts.iter().map(|a| {
                        let entity = entity.clone();
                        let id = a.steam_id64.clone();
                        h_flex()
                            .gap_2()
                            .child(
                                div()
                                    .text_sm()
                                    .child(format!("{} · {}", a.persona_name, id)),
                            )
                            .child(
                                Button::new(SharedString::from(format!("acct-{id}")))
                                    .xsmall()
                                    .label("Use")
                                    .tooltip("Use this account")
                                    .on_click(move |_, window, cx| {
                                        entity.update(cx, |this, cx| {
                                            this.app.account_edit = id.clone();
                                            set_input(&this.account_input, &id, window, cx);
                                            cx.notify();
                                        });
                                    }),
                            )
                    }))
                    .into_any_element()
            }))
            .child(section("Write safety", cx, |_| {
                h_flex()
                    .gap_2()
                    .child(
                        Checkbox::new("allow-steam")
                            .label("Allow writes while Steam is running")
                            .checked(self.app.allow_steam_running)
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.allow_steam_running =
                                            !this.app.allow_steam_running;
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .into_any_element()
            }))
            .child(section("Setup diagnostics", cx, |_| {
                v_flex()
                    .gap_2()
                    .child(
                        Button::new("diag-run")
                            .small()
                            .label("Run setup check")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    entity.update(cx, |this, cx| {
                                        this.app.run_setup_diagnostics();
                                        cx.notify();
                                    });
                                }
                            }),
                    )
                    .when_some(self.app.setup_diagnostics.clone(), |this, text| {
                        this.child(div().text_xs().child(text))
                    })
                    .child(
                        Button::new("diag-export")
                            .small()
                            .label("Export diagnostics…")
                            .on_click({
                                let entity = entity.clone();
                                move |_, _, cx| {
                                    if let Some(path) = rfd::FileDialog::new()
                                        .add_filter("JSON", &["json"])
                                        .save_file()
                                    {
                                        entity.update(cx, |this, cx| {
                                            this.app.diagnostics_export_path =
                                                path.to_string_lossy().into();
                                            match this.app.export_diagnostics() {
                                                Ok(()) => {
                                                    this.app.success_msg =
                                                        Some("Diagnostics exported.".into());
                                                }
                                                Err(e) => this.app.error = Some(e),
                                            }
                                            cx.notify();
                                        });
                                    }
                                }
                            }),
                    )
                    .into_any_element()
            }))
            .child(self.backups(cx))
            .child(section("About", cx, |_| {
                div()
                    .text_sm()
                    .child(format!("Vapourfly {}", env!("CARGO_PKG_VERSION")))
                    .into_any_element()
            }))
    }

    pub(crate) fn backups(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        let backups = self.app.backups.clone();
        section("Backups", cx, move |cx| {
            v_flex()
                .gap_1()
                .children(backups.iter().map(|b| {
                    let entity = entity.clone();
                    let path = b.path.clone();
                    h_flex()
                        .gap_2()
                        .child(div().flex_1().text_sm().child(b.path.display().to_string()))
                        .child(
                            Button::new(SharedString::from(path.display().to_string()))
                                .xsmall()
                                .icon(Icon::new(IconName::Undo))
                                .tooltip("Restore this backup")
                                .disabled(self.app.ui_demo)
                                .on_click({
                                    let entity = entity.clone();
                                    let path = path.clone();
                                    move |_, window, cx| {
                                        entity.update(cx, |this, cx| {
                                            // Demo mode never writes: gate
                                            // before any dialog opens.
                                            if this.app.ui_demo {
                                                return;
                                            }
                                            let name = path
                                                .file_name()
                                                .map(|n| n.to_string_lossy().into_owned())
                                                .unwrap_or_else(|| path.display().to_string());
                                            open_alert(
                                                &entity,
                                                window,
                                                cx,
                                                AlertSpec {
                                                    title: format!("Restore backup “{name}”?"),
                                                    lines: vec![
                                                        path.display().to_string(),
                                                        "Steam files are replaced; a fresh \
                                                         backup is taken first"
                                                            .into(),
                                                    ],
                                                    verb: "Restore".into(),
                                                },
                                                {
                                                    let path = path.clone();
                                                    move |this, cx| {
                                                        this.app.begin_backup_restore(path.clone());
                                                        cx.notify();
                                                    }
                                                },
                                            );
                                        });
                                    }
                                }),
                        )
                }))
                .when(backups.is_empty(), |this| {
                    this.child(
                        h_flex()
                            .gap_2()
                            .text_color(cx.theme().muted_foreground)
                            .child(Icon::new(IconName::Inbox))
                            .child(div().text_sm().child("No backups yet")),
                    )
                })
                .into_any_element()
        })
    }
}
