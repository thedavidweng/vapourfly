//! Collections, Data Sources and Settings.

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    scroll::ScrollableElement,
    switch::Switch,
    tab::Tab,
    tag::Tag,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, Context, Div, InteractiveElement, IntoElement, ParentElement, SharedString,
    Styled, Window, div, prelude::*, px,
};
use vapourfly_core::models::SteamCollection;

use super::widgets::{self, PAGE_PX, Tone};
use super::{GuiRoot, hx, set_input};
use crate::app::{
    CredentialSignal, open_url_in_browser, relative_time_ago, source_credential_signal,
    source_display_name, source_refresh_enabled,
};
use crate::theme::{self, ThemeMode};

const COLLECTION_MIN_W: f32 = 240.;
const COLLECTION_GAP: f32 = 20.;
const SETTINGS_MAX_W: f32 = 760.;

/// One labelled row inside a settings group: title and help on the left,
/// the control on the right.
fn setting_row(
    title: impl Into<SharedString>,
    description: impl Into<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> Div {
    h_flex()
        .w_full()
        .px_4()
        .py_3p5()
        .gap_6()
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(div().text_size(px(15.)).font_medium().child(title.into()))
                .child(widgets::caption(description, cx)),
        )
        .child(h_flex().flex_none().gap_2().child(control))
}

/// A titled stack of rows separated by hairlines.
fn setting_group(
    title: impl Into<SharedString>,
    rows: impl IntoIterator<Item = AnyElement>,
    cx: &App,
) -> impl IntoElement {
    let border = cx.theme().border;
    v_flex()
        .gap_2()
        .child(widgets::section_title(title, cx).px_1())
        .child(
            widgets::panel(cx)
                .overflow_hidden()
                .children(rows.into_iter().enumerate().map(move |(i, row)| {
                    div()
                        .when(i > 0, |this| this.border_t_1().border_color(border))
                        .child(row)
                })),
        )
}

fn credential_tag(signal: CredentialSignal) -> Tag {
    match signal {
        CredentialSignal::Configured => widgets::tone_tag(Tone::Success, signal.label()),
        CredentialSignal::Missing => widgets::tone_tag(Tone::Warning, signal.label()),
        CredentialSignal::NotRequired | CredentialSignal::Optional => {
            Tag::secondary().child(signal.label())
        }
    }
}

fn backup_label(path: &std::path::Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

impl GuiRoot {
    fn page_width(&self, window: &Window) -> f32 {
        let width: f32 = window.viewport_size().width.into();
        width - self.sidebar_width(window) - 10. - PAGE_PX * 2.
    }

    // ---------------------------------------------------------------- Collections

    fn collection_card(
        &self,
        c: &SteamCollection,
        card_w: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let art_h = (card_w * 0.5).round();
        let ids: Vec<u32> = c.app_ids.iter().copied().take(4).collect();
        let tile = |id: u32, this: &Self, cx: &mut Context<Self>| {
            let name = this
                .find_game(id)
                .map_or_else(|| format!("App {id}"), |g| g.name.clone());
            div()
                .id(("collage", id as usize))
                .flex_1()
                .h_full()
                .min_w_0()
                .overflow_hidden()
                .cursor_pointer()
                .child(widgets::cover(this.artwork.header(id), id, &name, None))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.open_game_sheet(id, window, cx);
                }))
        };
        let collage = match ids.len() {
            0 => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(cx.theme().muted)
                .child(widgets::inline_icon(
                    IconName::Layers,
                    cx.theme().muted_foreground,
                ))
                .into_any_element(),
            1..=2 => h_flex()
                .size_full()
                .gap(px(2.))
                .children(ids.iter().map(|id| tile(*id, self, cx)))
                .into_any_element(),
            _ => {
                let (top, bottom) = ids.split_at(2);
                v_flex()
                    .size_full()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .flex_1()
                            .gap(px(2.))
                            .children(top.iter().map(|id| tile(*id, self, cx))),
                    )
                    .child(
                        h_flex()
                            .flex_1()
                            .gap(px(2.))
                            .children(bottom.iter().map(|id| tile(*id, self, cx))),
                    )
                    .into_any_element()
            }
        };

        let count = c.app_ids.len();
        v_flex()
            .id(SharedString::from(format!("col-{}", c.id)))
            .w(px(card_w))
            .gap_2p5()
            .child(
                div()
                    .w_full()
                    .h(px(art_h))
                    .rounded(cx.theme().radius_lg)
                    .overflow_hidden()
                    .border_1()
                    .border_color(cx.theme().border.opacity(0.6))
                    .bg(cx.theme().border.opacity(0.6))
                    .child(collage),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .text_size(px(15.))
                                    .font_medium()
                                    .truncate()
                                    .child(c.name.clone()),
                            )
                            .child(widgets::caption(
                                format!("{count} game{}", if count == 1 { "" } else { "s" }),
                                cx,
                            )),
                    )
                    .when(c.is_hidden_collection, |this| {
                        this.child(widgets::tone_tag(Tone::Warning, "Hidden"))
                    }),
            )
            .into_any_element()
    }

    pub(super) fn collections(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let total = self.app.collections.len();
        let games: usize = self.app.collections.iter().map(|c| c.app_ids.len()).sum();
        let width = self.page_width(window);
        let columns = (((width + COLLECTION_GAP) / (COLLECTION_MIN_W + COLLECTION_GAP)).floor()
            as usize)
            .max(1);
        let card_w = ((width - COLLECTION_GAP * (columns as f32 - 1.)) / columns as f32).floor();

        let export = Button::new("col-export")
            .outline()
            .icon(IconName::Download)
            .label("Export all")
            .disabled(total == 0)
            .on_click(cx.listener(|this, _, _, cx| {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("JSON", &["json"])
                    .set_file_name("steam-collections.json")
                    .save_file()
                {
                    this.app.collections_export_path = path.to_string_lossy().into();
                    match this.app.export_collections() {
                        Ok(()) => this.app.success_msg = Some("Exported collections.".into()),
                        Err(e) => this.app.error = Some(e),
                    }
                    cx.notify();
                }
            }));

        let body = if total == 0 {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .child(widgets::empty_state(
                    IconName::Layers,
                    "No collections yet",
                    "Collections you create in Steam, or sync from a playlist, show up here.",
                    None,
                ))
                .into_any_element()
        } else {
            let cards: Vec<AnyElement> = self
                .app
                .collections
                .clone()
                .iter()
                .map(|c| self.collection_card(c, card_w, cx))
                .collect();
            div()
                .id("col-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scrollbar()
                .child(
                    div()
                        .px(px(PAGE_PX))
                        .pb(px(28.))
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .gap(px(COLLECTION_GAP))
                        .children(cards),
                )
                .into_any_element()
        };

        v_flex()
            .id("collections")
            .size_full()
            .child(widgets::page_header(
                "Collections",
                format!("{total} collections · {games} games filed"),
                export,
                cx,
            ))
            .child(body)
    }

    // ---------------------------------------------------------------- Data sources

    pub(super) fn data_sources(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme::t();
        let statuses = self.app.source_statuses.clone();
        let entries: usize = statuses.iter().map(|s| s.cache_entries).sum();
        let stale: usize = statuses.iter().map(|s| s.stale_entries).sum();
        let missing = statuses
            .iter()
            .filter(|s| {
                source_credential_signal(&s.name, self.app.has_igdb, self.app.has_rawg)
                    == CredentialSignal::Missing
            })
            .count();
        let offline = self.app.offline_mode;
        let refreshing = self.app.cache_refresh_loading;

        let actions = h_flex()
            .gap_3()
            .child(
                Switch::new("offline")
                    .checked(offline)
                    .label("Offline mode")
                    .on_click(cx.listener(|this, value: &bool, _, cx| {
                        this.app.offline_mode = *value;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("refresh-all")
                    .primary()
                    .icon(IconName::RefreshCw)
                    .label("Refresh all")
                    .loading(refreshing)
                    .disabled(offline || refreshing || self.app.ui_demo)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.app.start_cache_refresh(None);
                        this.arm_poll(cx);
                        cx.notify();
                    })),
            );

        let col = |w: f32| div().flex_none().w(px(w));
        let header = h_flex()
            .px_4()
            .h(px(42.))
            .gap_4()
            .text_size(px(13.))
            .text_color(cx.theme().muted_foreground)
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().flex_1().child("Source"))
            .child(col(120.).child("Credentials"))
            .child(col(90.).text_right().child("Entries"))
            .child(col(160.).child("Freshness"))
            .child(col(120.).child("Last updated"))
            .child(col(84.));

        let rows = statuses.iter().enumerate().map(|(i, st)| {
            let id = st.name.clone();
            let signal = source_credential_signal(&id, self.app.has_igdb, self.app.has_rawg);
            let enabled = source_refresh_enabled(
                &id,
                self.app.has_igdb,
                self.app.has_rawg,
                offline,
                refreshing,
            ) && !self.app.ui_demo;
            let fresh = st.cache_entries.saturating_sub(st.stale_entries);
            let ratio = if st.cache_entries == 0 {
                0.
            } else {
                fresh as f32 / st.cache_entries as f32
            };
            let tone = if st.cache_entries == 0 {
                hx(t.text_muted)
            } else if ratio >= 0.8 {
                hx(t.success)
            } else {
                hx(t.warning)
            };
            let freshness = if st.cache_entries == 0 {
                "Empty".to_string()
            } else if st.stale_entries == 0 {
                "All fresh".to_string()
            } else {
                format!("{} stale", st.stale_entries)
            };
            h_flex()
                .id(SharedString::from(format!("src-{id}")))
                .px_4()
                .h(px(60.))
                .gap_4()
                .when(i > 0, |this| {
                    this.border_t_1().border_color(cx.theme().border)
                })
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_medium()
                                .child(source_display_name(&id)),
                        )
                        .when(!st.cache_dir_exists, |this| {
                            this.child(widgets::caption("No cache on disk yet", cx))
                        }),
                )
                .child(col(120.).flex().child(credential_tag(signal)))
                .child(
                    col(90.)
                        .text_right()
                        .text_size(px(15.))
                        .font_family(cx.theme().mono_font_family.clone())
                        .child(st.cache_entries.to_string()),
                )
                .child(col(160.).child(widgets::status(freshness, tone, cx)))
                .child(
                    col(120.).child(widgets::caption(
                        st.last_success
                            .map(|t| relative_time_ago(t.timestamp()))
                            .unwrap_or_else(|| "Never".into()),
                        cx,
                    )),
                )
                .child(
                    col(84.).flex().justify_end().child(
                        Button::new(SharedString::from(format!("rf-{id}")))
                            .ghost()
                            .small()
                            .icon(IconName::RefreshCw)
                            .label("Refresh")
                            .disabled(!enabled)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.app.start_cache_refresh(Some(id.clone()));
                                this.arm_poll(cx);
                                cx.notify();
                            })),
                    ),
                )
        });

        let subtitle = if offline {
            "Offline — Vapourfly uses cached metadata only.".to_string()
        } else {
            "Metadata that powers filters, Proton ratings and play-time estimates.".to_string()
        };

        v_flex()
            .id("sources")
            .size_full()
            .child(widgets::page_header("Data Sources", subtitle, actions, cx))
            .child(
                v_flex()
                    .id("sources-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .px(px(PAGE_PX))
                    .pb(px(28.))
                    .gap_5()
                    .child(
                        h_flex()
                            .gap_3()
                            .child(widgets::stat_tile(
                                "Sources",
                                statuses.len().to_string(),
                                None,
                                cx,
                            ))
                            .child(widgets::stat_tile(
                                "Cached entries",
                                entries.to_string(),
                                None,
                                cx,
                            ))
                            .child(widgets::stat_tile(
                                "Stale",
                                stale.to_string(),
                                (stale > 0).then(|| hx(t.warning)),
                                cx,
                            ))
                            .child(widgets::stat_tile(
                                "Missing credentials",
                                missing.to_string(),
                                (missing > 0).then(|| hx(t.warning)),
                                cx,
                            )),
                    )
                    .when_some(self.app.cache_refresh_msg.clone(), |this, msg| {
                        this.child(
                            h_flex()
                                .gap_2()
                                .child(widgets::inline_icon(
                                    IconName::Info,
                                    cx.theme().muted_foreground,
                                ))
                                .child(widgets::caption(msg, cx)),
                        )
                    })
                    .child(
                        widgets::panel(cx)
                            .overflow_hidden()
                            .child(header)
                            .children(rows),
                    )
                    .when(missing > 0, |this| {
                        this.child(widgets::caption(
                            "IGDB and RAWG read their keys from the environment: \
                             VAPOURFLY_IGDB_CLIENT_ID, VAPOURFLY_IGDB_CLIENT_SECRET \
                             and VAPOURFLY_RAWG_KEY.",
                            cx,
                        ))
                    }),
            )
    }

    // ---------------------------------------------------------------- Settings

    fn appearance_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mode = self.app.theme_mode;
        let choices = [ThemeMode::Light, ThemeMode::Dark];
        setting_group(
            "Appearance",
            [setting_row(
                "Theme",
                "Dark suits big cover art; light is easier in bright rooms.",
                widgets::segmented(
                    "theme-choice",
                    choices.iter().map(|m| Tab::new().label(m.label())),
                    choices.iter().position(|m| *m == mode),
                )
                .on_click(cx.listener(move |this, ix: &usize, window, cx| {
                    if let Some(m) = choices.get(*ix) {
                        if *m != this.app.theme_mode {
                            this.toggle_theme(window, cx);
                        }
                    }
                })),
                cx,
            )
            .into_any_element()],
            cx,
        )
    }

    fn controller_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let status = match &self.pad_name {
            Some(name) => widgets::tone_tag(Tone::Success, name.clone()).into_any_element(),
            None => Tag::secondary().child("None detected").into_any_element(),
        };
        let keyboard = if self.device.has_steam_keyboard() {
            "X opens Steam's on-screen keyboard, and A on a text field does too."
        } else {
            "On SteamOS, X opens Steam's on-screen keyboard."
        };
        setting_group(
            "Controller",
            [
                setting_row(
                    "Controller",
                    "Steam Deck controls and Xbox-style gamepads work out of the box.",
                    status,
                    cx,
                )
                .into_any_element(),
                setting_row(
                    "Session",
                    "Detected from the environment Steam sets for the app.",
                    Tag::secondary().child(self.device.label()),
                    cx,
                )
                .into_any_element(),
                div()
                    .px_4()
                    .py_3()
                    .child(widgets::caption(
                        format!(
                            "D-pad or left stick moves focus, A selects, B goes back, LB / RB \
                             switch pages, LT / RT and the right stick scroll, Y jumps to \
                             search, Start opens Settings and Select hides the sidebar. {keyboard}"
                        ),
                        cx,
                    ))
                    .into_any_element(),
            ],
            cx,
        )
    }

    fn steam_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let accounts = self.app.detected_accounts.clone();
        let current = self.app.active_account().map(|a| a.steam_id64.clone());
        let account_list = (!accounts.is_empty()).then(|| {
            v_flex()
                .px_4()
                .py_3()
                .gap_2()
                .child(widgets::caption("Accounts found on this computer", cx))
                .children(accounts.into_iter().map(|a| {
                    let id = a.steam_id64.clone();
                    let active = current.as_ref() == Some(&id);
                    h_flex()
                        .gap_3()
                        .child(widgets::avatar(
                            self.app.avatar_path(&a),
                            Some(&a.persona_name),
                            40.,
                        ))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .child(
                                            div().text_size(px(15.)).child(a.persona_name.clone()),
                                        )
                                        .when(a.most_recent, |this| {
                                            this.child(Tag::secondary().child("Last signed in"))
                                        }),
                                )
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_color(cx.theme().muted_foreground)
                                        .child(id.clone()),
                                ),
                        )
                        .child(if active {
                            widgets::tone_tag(Tone::Accent, "In use").into_any_element()
                        } else {
                            Button::new(SharedString::from(format!("acct-{id}")))
                                .outline()
                                .small()
                                .label("Use")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.app.account_edit = id.clone();
                                    set_input(&this.account_input, &id, window, cx);
                                    cx.notify();
                                }))
                                .into_any_element()
                        })
                }))
                .into_any_element()
        });

        let mut rows = vec![
            setting_row(
                "Steam folder",
                "Where Steam is installed. Detected automatically when left empty.",
                h_flex()
                    .gap_2()
                    .child(Input::new(&self.steam_dir_input).w(px(260.)))
                    .child(
                        Button::new("pick-steam")
                            .outline()
                            .icon(IconName::FolderOpen)
                            .tooltip("Choose folder")
                            .on_click(cx.listener(|this, _, window, cx| {
                                if let Some(path) = rfd::FileDialog::new().pick_folder() {
                                    let value = path.to_string_lossy().into_owned();
                                    this.app.steam_dir_edit = value.clone();
                                    set_input(&this.steam_dir_input, &value, window, cx);
                                    cx.notify();
                                }
                            })),
                    ),
                cx,
            )
            .into_any_element(),
            setting_row(
                "Account",
                "SteamID64 of the profile whose collections you edit.",
                Input::new(&self.account_input).w(px(300.)),
                cx,
            )
            .into_any_element(),
        ];
        rows.extend(account_list);
        rows.push(
            setting_row(
                "Store region",
                "Country code and language used for Steam Store metadata.",
                h_flex()
                    .gap_2()
                    .child(Input::new(&self.cc_input).w(px(72.)))
                    .child(Input::new(&self.lang_input).w(px(120.))),
                cx,
            )
            .into_any_element(),
        );
        rows.push(
            setting_row(
                "Steam Web API key",
                "Optional. Resolves every game name in a single request.",
                h_flex()
                    .gap_2()
                    .child(Input::new(&self.api_key_input).mask_toggle().w(px(240.)))
                    .child(
                        Button::new("apikey-help")
                            .ghost()
                            .icon(IconName::ExternalLink)
                            .tooltip("Get a free key")
                            .on_click(|_, _, _| {
                                open_url_in_browser("https://steamcommunity.com/dev/apikey");
                            }),
                    ),
                cx,
            )
            .into_any_element(),
        );
        setting_group("Steam", rows, cx)
    }

    fn safety_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        setting_group(
            "Write safety",
            [
                setting_row(
                    "Write while Steam is running",
                    "Steam may overwrite changes made while it is open. Leave off unless you know it is safe.",
                    Switch::new("allow-steam")
                        .checked(self.app.allow_steam_running)
                        .on_click(cx.listener(|this, value: &bool, _, cx| {
                            this.app.allow_steam_running = *value;
                            cx.notify();
                        })),
                    cx,
                )
                .into_any_element(),
                setting_row(
                    "Backups to keep",
                    "A backup of Steam's collections file is made before every write.",
                    Input::new(&self.retention_input).w(px(72.)),
                    cx,
                )
                .into_any_element(),
            ],
            cx,
        )
    }

    fn backups_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let demo = self.app.ui_demo;
        let rows: Vec<AnyElement> = if self.app.backups.is_empty() {
            vec![
                div()
                    .px_4()
                    .py_4()
                    .child(widgets::caption(
                        "No backups yet. One is created the first time Vapourfly writes to Steam.",
                        cx,
                    ))
                    .into_any_element(),
            ]
        } else {
            self.app
                .backups
                .iter()
                .map(|b| {
                    let path = b.path.clone();
                    let short_sha: String = b.sha256.chars().take(8).collect();
                    h_flex()
                        .px_4()
                        .py_3()
                        .gap_3()
                        .child(widgets::inline_icon(
                            IconName::Archive,
                            cx.theme().muted_foreground,
                        ))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().text_size(px(15.)).child(
                                    b.created_at.format("%b %-d, %Y · %H:%M UTC").to_string(),
                                ))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .truncate()
                                        .font_family(cx.theme().mono_font_family.clone())
                                        .text_color(cx.theme().muted_foreground)
                                        .child(format!("{short_sha} · {}", backup_label(&b.path))),
                                ),
                        )
                        .child(
                            Button::new(SharedString::from(format!(
                                "restore-{}",
                                b.path.display()
                            )))
                            .outline()
                            .small()
                            .icon(IconName::Undo2)
                            .label("Restore")
                            .disabled(demo)
                            .on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.app.begin_backup_restore(path.clone());
                                    this.arm_poll(cx);
                                    cx.notify();
                                },
                            )),
                        )
                        .into_any_element()
                })
                .collect()
        };
        setting_group("Backups", rows, cx)
    }

    fn diagnostics_group(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rows = vec![
            setting_row(
                "Setup check",
                "Verifies the Steam folder, account and collections file are readable.",
                h_flex()
                    .gap_2()
                    .child(
                        Button::new("diag-run")
                            .outline()
                            .icon(IconName::Stethoscope)
                            .label("Run check")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.app.run_setup_diagnostics();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("diag-export")
                            .ghost()
                            .icon(IconName::Download)
                            .label("Export")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(path) = rfd::FileDialog::new()
                                    .add_filter("JSON", &["json"])
                                    .set_file_name("vapourfly-diagnostics.json")
                                    .save_file()
                                {
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
                                }
                            })),
                    ),
                cx,
            )
            .into_any_element(),
        ];
        if let Some(text) = self.app.setup_diagnostics.clone() {
            rows.push(
                div()
                    .px_4()
                    .py_3()
                    .bg(cx.theme().muted.opacity(0.5))
                    .text_size(px(13.))
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_color(cx.theme().muted_foreground)
                    .child(text)
                    .into_any_element(),
            );
        }
        setting_group("Diagnostics", rows, cx)
    }

    pub(super) fn settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let save = h_flex()
            .gap_3()
            .when_some(self.app.settings_save_msg.clone(), |this, msg| {
                this.child(widgets::caption(msg, cx))
            })
            .child(
                Button::new("save-settings")
                    .primary()
                    .icon(IconName::Check)
                    .label("Save changes")
                    .disabled(self.app.ui_demo || !self.app.settings_dirty())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.app.save_settings();
                        cx.notify();
                    })),
            );

        let groups = [
            self.appearance_group(cx).into_any_element(),
            self.controller_group(cx).into_any_element(),
            self.steam_group(cx).into_any_element(),
            self.safety_group(cx).into_any_element(),
            self.backups_group(cx).into_any_element(),
            self.diagnostics_group(cx).into_any_element(),
        ];
        let about = setting_group(
            "About",
            [setting_row(
                format!("Vapourfly {}", env!("CARGO_PKG_VERSION")),
                "Organize your Steam library with playlists, smart filters and safe writes.",
                Button::new("about-data")
                    .ghost()
                    .icon(IconName::FolderOpen)
                    .label("Open data folder")
                    .disabled(self.app.ui_demo)
                    .on_click(cx.listener(|this, _, _, _| {
                        open_url_in_browser(&this.app.cache_dir.to_string_lossy());
                    })),
                cx,
            )
            .into_any_element()],
            cx,
        );

        div()
            .id("settings")
            .size_full()
            .overflow_y_scrollbar()
            .child(
                v_flex()
                    .max_w(px(SETTINGS_MAX_W + PAGE_PX * 2.))
                    .child(widgets::page_header(
                        "Settings",
                        "Steam connection, safety and appearance.",
                        save,
                        cx,
                    ))
                    .child(
                        v_flex()
                            .px(px(PAGE_PX))
                            .pb(px(32.))
                            .gap_6()
                            .children(groups)
                            .child(about),
                    ),
            )
    }
}
