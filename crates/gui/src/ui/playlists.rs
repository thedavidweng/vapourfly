//! Playlists: a rail of stored playlists beside an editor with Games,
//! Rules, Match and Share tabs.

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable, StyledExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::Input,
    menu::{DropdownMenu, PopupMenuItem},
    scroll::ScrollableElement,
    spinner::Spinner,
    switch::Switch,
    tab::{Tab, TabBar},
    tag::Tag,
    v_flex,
};
use gpui_kit::{
    Anchor, AnyElement, App, ClipboardItem, Context, InteractiveElement, IntoElement,
    ParentElement, SharedString, Styled, Window, div, prelude::*, px,
};
use vapourfly_core::dynamic::DynamicTemplate;
use vapourfly_core::models::{JunkMode, PlaylistContent, PlaylistRule, ProtonTier};
use vapourfly_core::mood::EditorialMood;
use vapourfly_core::playlist;

use super::widgets::{self, PAGE_PX};
use super::{GuiRoot, hx, set_input};
use crate::app::{
    PendingAction, PlaylistDetailTab, PlaylistMatchTab, empty_value_label, format_playtime,
    open_url_in_browser, playlist_avg_hltb, playlist_content_type_label, playlist_cover_app_id,
    playlist_game_count, proton_tier_label, rule_label,
};
use crate::theme;

const RAIL_W: f32 = 280.;
const SHARE_TAB: usize = 3;

fn parse_ids(csv: &str) -> Vec<u32> {
    csv.split(',')
        .filter_map(|s| s.trim().parse().ok())
        .collect()
}

impl GuiRoot {
    fn editing_content(&self) -> PlaylistContent {
        if self.app.playlist_edit_rules.trim().is_empty() {
            PlaylistContent::Manual {
                app_ids: parse_ids(&self.app.playlist_edit_app_ids),
            }
        } else {
            PlaylistContent::Rules {
                rules: self.app.parse_current_rules().unwrap_or_default(),
            }
        }
    }

    fn toggle_playlist_game(&mut self, id: u32, window: &mut Window, cx: &mut Context<Self>) {
        let mut ids = parse_ids(&self.app.playlist_edit_app_ids);
        if let Some(pos) = ids.iter().position(|x| *x == id) {
            ids.remove(pos);
        } else {
            ids.push(id);
        }
        self.app.playlist_edit_app_ids = ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        set_input(
            &self.playlist_csv_input,
            &self.app.playlist_edit_app_ids,
            window,
            cx,
        );
        cx.notify();
    }

    fn save_playlist(&mut self, cx: &mut Context<Self>) {
        match self.app.build_playlist_from_edit_fields() {
            Ok(pf) => match self.app.store_playlist(&pf) {
                Ok(()) => {
                    self.app.success_msg = Some(format!("Saved “{}”", pf.playlist.name));
                    self.app.refresh_playlist_store_ids();
                }
                Err(e) => self.app.error = Some(e),
            },
            Err(e) => self.app.error = Some(e),
        }
        cx.notify();
    }

    fn import_playlist_file(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .pick_file()
        {
            self.app.playlist_import_path = path.to_string_lossy().into();
            match playlist::import_playlist(&path) {
                Ok(pf) => self
                    .app
                    .adopt_imported_playlist(pf, "Imported playlist.".into()),
                Err(e) => self.app.error = Some(e.to_string()),
            }
            cx.notify();
        }
    }

    fn export_playlist_file(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("JSON", &["json"])
            .save_file()
        {
            self.app.playlist_export_path = path.to_string_lossy().into();
            match self.app.export_loaded_playlist() {
                Ok(()) => self.app.success_msg = Some("Exported playlist.".into()),
                Err(e) => self.app.error = Some(e),
            }
            cx.notify();
        }
    }

    fn copy_share_code(&mut self, cx: &mut Context<Self>) {
        match self.app.build_playlist_from_edit_fields() {
            Ok(pf) => match vapourfly_core::share_code::encode_share_code(&pf) {
                Ok(code) => {
                    self.app.playlist_share_code_output = Some(code.clone());
                    cx.write_to_clipboard(ClipboardItem::new_string(code));
                    self.app.success_msg = Some("Share code copied.".into());
                }
                Err(e) => self.app.error = Some(e.to_string()),
            },
            Err(e) => self.app.error = Some(e),
        }
        cx.notify();
    }

    fn paste_share_code(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.app.error = Some("The clipboard has no text to import.".into());
            cx.notify();
            return;
        };
        self.app.playlist_share_code_input = text.clone();
        match vapourfly_core::share_code::decode_share_code(&text) {
            Ok(pf) => self
                .app
                .adopt_imported_playlist(pf, "Imported share code.".into()),
            Err(e) => self.app.error = Some(e.to_string()),
        }
        cx.notify();
    }

    pub(super) fn playlists(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("playlists")
            .size_full()
            .items_stretch()
            .child(self.playlist_rail(cx))
            .child(self.playlist_workspace(cx))
    }

    fn new_playlist_button(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity();
        Button::new("pl-new")
            .ghost()
            .icon(IconName::Plus)
            .tooltip("New playlist")
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |menu, _, _| {
                let blank = entity.clone();
                let file = entity.clone();
                let paste = entity.clone();
                let mut menu = menu
                    .item(
                        PopupMenuItem::new("Blank playlist")
                            .icon(IconName::FilePlus)
                            .on_click(move |_, window, cx| {
                                blank.update(cx, |this, cx| {
                                    this.app.reset_playlist_editor();
                                    this.sync_playlist_inputs(window, cx);
                                    this.playlist_edit_synced = this.app.playlist_edit_generation;
                                    cx.notify();
                                });
                            }),
                    )
                    .separator()
                    .label("From a template");
                for template in [DynamicTemplate::DeckSession, DynamicTemplate::FinishIt] {
                    let entity = entity.clone();
                    menu = menu.item(
                        PopupMenuItem::new(template.label())
                            .icon(IconName::WandSparkles)
                            .on_click(move |_, _, cx| {
                                entity.update(cx, |this, cx| {
                                    this.app.dynamic_template = template.id().to_string();
                                    this.app.start_dynamic_generate();
                                    this.arm_poll(cx);
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu = menu.separator().label("From a mood");
                for mood in EditorialMood::all() {
                    let entity = entity.clone();
                    let id = mood.id().to_string();
                    menu = menu.item(
                        PopupMenuItem::new(mood.name())
                            .icon(IconName::Sparkle)
                            .on_click(move |_, _, cx| {
                                let id = id.clone();
                                entity.update(cx, |this, cx| {
                                    this.app.editorial_mood = id;
                                    this.app.start_mood_generate();
                                    this.arm_poll(cx);
                                    cx.notify();
                                });
                            }),
                    );
                }
                menu.separator()
                    .item(
                        PopupMenuItem::new("Import from file…")
                            .icon(IconName::Download)
                            .on_click(move |_, _, cx| {
                                file.update(cx, |this, cx| this.import_playlist_file(cx));
                            }),
                    )
                    .item(
                        PopupMenuItem::new("Paste share code")
                            .icon(IconName::ClipboardPaste)
                            .on_click(move |_, _, cx| {
                                paste.update(cx, |this, cx| this.paste_share_code(cx));
                            }),
                    )
            })
    }

    fn playlist_rail(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current = self.app.playlist_edit_id.clone();
        let report = self.app.playlist_match_report.as_ref();
        let entries: Vec<AnyElement> = self
            .app
            .playlist_rail_entries
            .iter()
            .map(|(id, entry)| {
                let active = *id == current;
                let load_id = id.clone();
                let (title, meta, cover) = match entry {
                    Ok(pf) => (
                        pf.playlist.name.clone(),
                        format!(
                            "{} · {} games",
                            playlist_content_type_label(&pf.playlist.content),
                            playlist_game_count(&pf.playlist.content, report)
                        ),
                        Some(playlist_cover_app_id(&pf.playlist.content)),
                    ),
                    Err(e) => (id.clone(), format!("Could not load: {e}"), None),
                };
                let content = h_flex().w_full().gap_3();
                widgets::focus_row(SharedString::from(format!("rail-{id}")), active, cx)
                    .px_2()
                    .py_2()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Err(e) = this.app.load_playlist_from_store(&load_id) {
                            this.app.error = Some(e);
                        } else {
                            this.sync_playlist_inputs(window, cx);
                            this.playlist_edit_synced = this.app.playlist_edit_generation;
                            this.arm_poll(cx);
                        }
                        cx.notify();
                    }))
                    .child(
                        content
                            .child(
                                div()
                                    .flex_none()
                                    .w(px(80.))
                                    .h(px(37.))
                                    .rounded(px(theme::CORNER_SM))
                                    .overflow_hidden()
                                    .bg(cx.theme().muted)
                                    .when_some(cover, |this, cover| {
                                        this.child(widgets::cover(
                                            self.artwork.header(cover),
                                            cover,
                                            &title,
                                            None,
                                        ))
                                    }),
                            )
                            .child(
                                v_flex()
                                    .min_w_0()
                                    .child(
                                        div()
                                            .text_size(px(15.))
                                            .font_medium()
                                            .truncate()
                                            .child(title),
                                    )
                                    .child(widgets::caption(meta, cx).truncate()),
                            ),
                    )
                    .into_any_element()
            })
            .collect();

        v_flex()
            .w(px(RAIL_W))
            .flex_none()
            .h_full()
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                h_flex()
                    .justify_between()
                    .px_4()
                    .pt_5()
                    .pb_3()
                    .child(div().text_size(px(21.)).font_semibold().child("Playlists"))
                    .child(self.new_playlist_button(cx)),
            )
            .child(
                v_flex()
                    .id("pl-rail")
                    .flex_1()
                    .min_h_0()
                    .px_2()
                    .gap_0p5()
                    .overflow_y_scrollbar()
                    .children(entries),
            )
    }

    fn playlist_workspace(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = if self.playlist_share_tab_open {
            SHARE_TAB
        } else {
            match self.app.playlist_detail_tab {
                PlaylistDetailTab::Games => 0,
                PlaylistDetailTab::Rules => 1,
                PlaylistDetailTab::Match => 2,
            }
        };
        let body = match tab {
            0 => self.playlist_games(cx).into_any_element(),
            1 => self.playlist_rules(cx).into_any_element(),
            2 => self.playlist_match(cx).into_any_element(),
            _ => self.playlist_share(cx).into_any_element(),
        };

        v_flex()
            .flex_1()
            .min_w_0()
            .h_full()
            .child(self.playlist_hero(cx))
            .child(
                div().px(px(PAGE_PX)).child(
                    TabBar::new("pl-tabs")
                        .underline()
                        .large()
                        .selected_index(tab)
                        .on_click(cx.listener(|this, ix: &usize, _, cx| {
                            this.playlist_share_tab_open = *ix == SHARE_TAB;
                            if *ix < SHARE_TAB {
                                this.app.playlist_detail_tab = match ix {
                                    1 => PlaylistDetailTab::Rules,
                                    2 => PlaylistDetailTab::Match,
                                    _ => PlaylistDetailTab::Games,
                                };
                            }
                            if *ix == 2 {
                                if let Ok(pf) = this.app.build_playlist_from_edit_fields() {
                                    this.app.match_playlist_against_library_background(&pf);
                                    this.arm_poll(cx);
                                }
                            }
                            cx.notify();
                        }))
                        .child(Tab::new().label("Games"))
                        .child(Tab::new().label("Rules"))
                        .child(Tab::new().label("Match"))
                        .child(Tab::new().label("Share")),
                ),
            )
            .child(
                v_flex()
                    .id("pl-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .child(div().px(px(PAGE_PX)).py_5().child(body)),
            )
    }

    fn playlist_hero(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let content = self.editing_content();
        let cover = playlist_cover_app_id(&content);
        let games = self.app.prepared_games(JunkMode::Default);
        let avg = games
            .as_ref()
            .and_then(|g| playlist_avg_hltb(&content, self.app.playlist_match_report.as_ref(), g));
        let count = playlist_game_count(&content, self.app.playlist_match_report.as_ref());
        let entity = cx.entity();

        h_flex()
            .px(px(PAGE_PX))
            .pt_6()
            .pb_4()
            .gap_5()
            .items_start()
            .child(
                div()
                    .flex_none()
                    .w(px(240.))
                    .h(px(112.))
                    .rounded(cx.theme().radius_lg)
                    .overflow_hidden()
                    .border_1()
                    .border_color(cx.theme().border)
                    .child(widgets::cover(
                        self.artwork.header(cover),
                        cover,
                        &self.app.playlist_edit_name,
                        None,
                    )),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(widgets::caption(
                        format!("{} playlist", playlist_content_type_label(&content)),
                        cx,
                    ))
                    .child(
                        Input::new(&self.playlist_name_input)
                            .appearance(false)
                            .large()
                            .text_size(px(26.))
                            .font_bold()
                            .w_full(),
                    )
                    .child(
                        Input::new(&self.playlist_desc_input)
                            .appearance(false)
                            .w_full(),
                    )
                    .child(
                        h_flex()
                            .gap_3()
                            .pt_1()
                            .child(Tag::secondary().child(format!("{count} games")))
                            .child(Tag::secondary().child(format!(
                                "Avg {}",
                                avg.map_or_else(|| "—".into(), format_playtime)
                            )))
                            .child(
                                Input::new(&self.playlist_id_input)
                                    .prefix(widgets::caption("ID", cx))
                                    .small()
                                    .w(px(220.)),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .child(
                        Button::new("pl-more")
                            .ghost()
                            .icon(IconName::Ellipsis)
                            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                                let export = entity.clone();
                                let import = entity.clone();
                                let share = entity.clone();
                                menu.item(
                                    PopupMenuItem::new("Export JSON…")
                                        .icon(IconName::Upload)
                                        .on_click(move |_, _, cx| {
                                            export.update(cx, |this, cx| {
                                                this.export_playlist_file(cx);
                                            });
                                        }),
                                )
                                .item(
                                    PopupMenuItem::new("Import JSON…")
                                        .icon(IconName::Download)
                                        .on_click(move |_, _, cx| {
                                            import.update(cx, |this, cx| {
                                                this.import_playlist_file(cx);
                                            });
                                        }),
                                )
                                .separator()
                                .item(
                                    PopupMenuItem::new("Copy share code")
                                        .icon(IconName::Share2)
                                        .on_click(move |_, _, cx| {
                                            share.update(cx, |this, cx| this.copy_share_code(cx));
                                        }),
                                )
                            }),
                    )
                    .child(
                        Button::new("pl-sync")
                            .outline()
                            .icon(IconName::Upload)
                            .label("Sync to Steam")
                            .disabled(self.app.ui_demo)
                            .on_click(cx.listener(|this, _, _, cx| {
                                match this.app.build_playlist_from_edit_fields() {
                                    Ok(pf) => {
                                        this.app.start_dry_run(PendingAction::PlaylistSync(pf));
                                        this.arm_poll(cx);
                                    }
                                    Err(e) => this.app.error = Some(e),
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("pl-save")
                            .primary()
                            .label("Save")
                            .on_click(cx.listener(|this, _, _, cx| this.save_playlist(cx))),
                    ),
            )
    }

    fn game_line(
        &self,
        id: u32,
        name: String,
        trailing: impl IntoElement,
        cx: &App,
    ) -> impl IntoElement {
        h_flex()
            .h(px(60.))
            .px_2()
            .gap_3()
            .rounded(cx.theme().radius)
            .hover(|s| s.bg(cx.theme().list_hover))
            .child(
                div()
                    .flex_none()
                    .w(px(86.))
                    .h(px(40.))
                    .rounded(px(theme::CORNER_SM))
                    .overflow_hidden()
                    .child(widgets::cover(self.artwork.header(id), id, &name, None)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(15.))
                    .truncate()
                    .child(name),
            )
            .child(trailing)
    }

    fn playlist_games(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let ids = parse_ids(&self.app.playlist_edit_app_ids);
        let rule_based = !self.app.playlist_edit_rules.trim().is_empty();
        let library = self.app.prepared_games(JunkMode::Default);
        let name_of = |id: u32| {
            library
                .as_ref()
                .and_then(|g| g.iter().find(|g| g.app_id == id))
                .map_or_else(|| format!("App {id}"), |g| g.name.clone())
        };
        let query = self.app.playlist_game_search.to_lowercase();
        let candidates: Vec<(u32, String)> = library
            .as_ref()
            .map(|games| {
                games
                    .iter()
                    .filter(|g| {
                        query.is_empty()
                            || g.name.to_lowercase().contains(&query)
                            || g.app_id.to_string().contains(&query)
                    })
                    .take(30)
                    .map(|g| (g.app_id, g.name.clone()))
                    .collect()
            })
            .unwrap_or_default();

        let current = v_flex()
            .flex_1()
            .min_w_0()
            .gap_2()
            .child(widgets::section_title(
                format!("In this playlist · {}", ids.len()),
                cx,
            ))
            .child(if ids.is_empty() {
                widgets::panel(cx)
                    .p_6()
                    .items_center()
                    .child(widgets::caption(
                        "No games yet. Add some from your library.",
                        cx,
                    ))
                    .into_any_element()
            } else {
                v_flex()
                    .children(ids.iter().map(|id| {
                        let id = *id;
                        self.game_line(
                            id,
                            name_of(id),
                            Button::new(("pl-rm", id as usize))
                                .ghost()
                                .small()
                                .icon(IconName::X)
                                .tooltip("Remove")
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.toggle_playlist_game(id, window, cx);
                                })),
                            cx,
                        )
                    }))
                    .into_any_element()
            });

        let add = v_flex()
            .flex_1()
            .min_w_0()
            .gap_2()
            .child(widgets::section_title("Add from library", cx))
            .child(
                Input::new(&self.playlist_search_input)
                    .prefix(widgets::inline_icon(
                        IconName::Search,
                        cx.theme().muted_foreground,
                    ))
                    .cleanable(true),
            )
            .child(v_flex().children(candidates.into_iter().map(|(id, name)| {
                let added = ids.contains(&id);
                self.game_line(
                    id,
                    name,
                    Button::new(("pl-add", id as usize))
                        .small()
                        .when(added, |b| b.ghost().icon(IconName::Check).label("Added"))
                        .when(!added, |b| b.outline().icon(IconName::Plus).label("Add"))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.toggle_playlist_game(id, window, cx);
                        })),
                    cx,
                )
            })));

        v_flex()
            .gap_4()
            .when(rule_based, |this| {
                this.child(
                    h_flex()
                        .gap_2()
                        .p_3()
                        .rounded(cx.theme().radius)
                        .bg(cx.theme().muted)
                        .text_size(px(15.))
                        .text_color(cx.theme().muted_foreground)
                        .child(widgets::inline_icon(
                            IconName::Info,
                            cx.theme().muted_foreground,
                        ))
                        .child("This playlist is driven by rules. Its games are matched automatically; see Match."),
                )
            })
            .child(h_flex().items_start().gap_8().child(current).child(add))
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        Switch::new("pl-adv-csv")
                            .checked(self.app.playlist_show_advanced_csv)
                            .label("Edit App IDs directly")
                            .on_click(cx.listener(|this, v: &bool, _, cx| {
                                this.app.playlist_show_advanced_csv = *v;
                                cx.notify();
                            })),
                    )
                    .when(self.app.playlist_show_advanced_csv, |this| {
                        this.child(Input::new(&self.playlist_csv_input).w_full())
                    }),
            )
    }

    fn rule_row(
        &self,
        label: &'static str,
        control: impl IntoElement,
        add_id: &'static str,
        on_add: impl Fn(&Self) -> Option<PlaylistRule> + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let valid = on_add(self).is_some();
        h_flex()
            .gap_4()
            .py_2p5()
            .px_4()
            .border_b_1()
            .border_color(cx.theme().border.opacity(0.6))
            .child(div().w(px(170.)).text_size(px(15.)).child(label))
            .child(div().flex_1().child(control))
            .child(
                Button::new(add_id)
                    .outline()
                    .small()
                    .icon(IconName::Plus)
                    .label("Add")
                    .disabled(!valid)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(rule) = on_add(this) {
                            if let Err(e) = this.app.append_rule_to_json(rule) {
                                this.app.error = Some(e);
                            }
                        }
                        cx.notify();
                    })),
            )
    }

    fn playlist_rules(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let rules = self.app.parse_current_rules().unwrap_or_default();
        let tiers = [
            ProtonTier::Bronze,
            ProtonTier::Silver,
            ProtonTier::Gold,
            ProtonTier::Platinum,
            ProtonTier::Native,
        ];
        let tier = self.app.playlist_rule_proton_tier;

        let active = v_flex()
            .gap_2()
            .child(widgets::section_title(
                format!("Active rules · {}", rules.len()),
                cx,
            ))
            .child(if rules.is_empty() {
                widgets::caption(
                    "No rules yet. Without rules this playlist uses its hand-picked games.",
                    cx,
                )
                .into_any_element()
            } else {
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .children(rules.iter().enumerate().map(|(i, rule)| {
                        Button::new(("rm-rule", i))
                            .secondary()
                            .label(rule_label(rule))
                            .icon(IconName::X)
                            .tooltip("Remove rule")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Ok(mut rs) = this.app.parse_current_rules() {
                                    if i < rs.len() {
                                        rs.remove(i);
                                        this.app.playlist_edit_rules =
                                            serde_json::to_string_pretty(&rs).unwrap_or_default();
                                    }
                                }
                                cx.notify();
                            }))
                    }))
                    .into_any_element()
            });

        let quick = v_flex()
            .gap_2()
            .child(widgets::section_title("Quick add", cx))
            .child(
                h_flex().gap_1p5().flex_wrap().children(
                    [
                        ("Installed", PlaylistRule::Installed),
                        ("Not hidden", PlaylistRule::NotHidden),
                        ("Not junk", PlaylistRule::NotJunk),
                        ("Full controller", PlaylistRule::ControllerSupportFull),
                    ]
                    .into_iter()
                    .map(|(label, rule)| {
                        Button::new(label)
                            .outline()
                            .small()
                            .icon(IconName::Plus)
                            .label(label)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Err(e) = this.app.append_rule_to_json(rule.clone()) {
                                    this.app.error = Some(e);
                                }
                                cx.notify();
                            }))
                    }),
                ),
            );

        let builder = v_flex()
            .gap_2()
            .child(widgets::section_title("Build a rule", cx))
            .child(
                widgets::panel(cx)
                    .overflow_hidden()
                    .child(self.rule_row(
                        "Has genre",
                        Input::new(&self.rule_genre_input).w(px(260.)),
                        "add-genre",
                        |this| {
                            let genre = this.app.playlist_rule_genre.trim().to_string();
                            (!genre.is_empty()).then_some(PlaylistRule::HasGenre { genre })
                        },
                        cx,
                    ))
                    .child(self.rule_row(
                        "Has tag",
                        Input::new(&self.rule_tag_input).w(px(260.)),
                        "add-tag",
                        |this| {
                            let tag = this.app.playlist_rule_tag.trim().to_string();
                            (!tag.is_empty()).then_some(PlaylistRule::HasTag { tag })
                        },
                        cx,
                    ))
                    .child(self.rule_row(
                        "Max time to beat (min)",
                        Input::new(&self.rule_hltb_input).w(px(120.)),
                        "add-hltb",
                        |this| {
                            this.app
                                .playlist_rule_hltb_max
                                .trim()
                                .parse::<u32>()
                                .ok()
                                .map(|minutes| PlaylistRule::HltbMaxMinutes { minutes })
                        },
                        cx,
                    ))
                    .child(
                        self.rule_row(
                            "Minimum ProtonDB tier",
                            widgets::segmented(
                                "rule-proton",
                                tiers
                                    .iter()
                                    .map(|t| Tab::new().label(proton_tier_label(*t))),
                                tiers.iter().position(|t| tier == Some(*t)),
                            )
                            .small()
                            .on_click(cx.listener(
                                move |this, ix: &usize, _, cx| {
                                    this.app.playlist_rule_proton_tier = tiers.get(*ix).copied();
                                    cx.notify();
                                },
                            )),
                            "add-proton",
                            |this| {
                                this.app
                                    .playlist_rule_proton_tier
                                    .map(|tier| PlaylistRule::ProtonAtLeast { tier })
                            },
                            cx,
                        ),
                    )
                    .child(
                        self.rule_row(
                            "Playtime between (min)",
                            h_flex()
                                .gap_2()
                                .child(Input::new(&self.rule_playtime_min_input).w(px(100.)))
                                .child(widgets::caption("to", cx))
                                .child(Input::new(&self.rule_playtime_max_input).w(px(100.))),
                            "add-playtime",
                            |this| {
                                let min = this
                                    .app
                                    .playlist_rule_playtime_min
                                    .trim()
                                    .parse::<u32>()
                                    .unwrap_or(0);
                                let max = this
                                    .app
                                    .playlist_rule_playtime_max
                                    .trim()
                                    .parse::<u32>()
                                    .ok()?;
                                (min <= max).then_some(PlaylistRule::PlaytimeBetween { min, max })
                            },
                            cx,
                        ),
                    )
                    .child(self.rule_row(
                        "Minimum rating",
                        Input::new(&self.rule_rating_input).w(px(100.)),
                        "add-rating",
                        |this| {
                            this.app
                                .playlist_rule_rating_min
                                .trim()
                                .parse::<f32>()
                                .ok()
                                .filter(|r| (0.0..=5.0).contains(r))
                                .map(|rating_0_5| PlaylistRule::RatingAtLeast { rating_0_5 })
                        },
                        cx,
                    )),
            );

        v_flex()
            .gap_6()
            .child(active)
            .child(quick)
            .child(builder)
            .child(
                v_flex()
                    .gap_2()
                    .child(
                        Switch::new("pl-adv-json")
                            .checked(self.app.playlist_show_advanced_json)
                            .label("Show rules as JSON")
                            .on_click(cx.listener(|this, v: &bool, _, cx| {
                                this.app.playlist_show_advanced_json = *v;
                                cx.notify();
                            })),
                    )
                    .when(self.app.playlist_show_advanced_json, |this| {
                        this.child(code_block(
                            if self.app.playlist_edit_rules.trim().is_empty() {
                                "[]".to_string()
                            } else {
                                self.app.playlist_edit_rules.clone()
                            },
                            cx,
                        ))
                    }),
            )
    }

    fn playlist_match(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme::t();
        let Some(report) = self.app.playlist_match_report.as_ref() else {
            return if self.app.playlist_match_loading {
                h_flex()
                    .gap_2()
                    .text_size(px(15.))
                    .text_color(cx.theme().muted_foreground)
                    .child(Spinner::new())
                    .child("Matching against your library…")
                    .into_any_element()
            } else {
                widgets::empty_state(
                    IconName::ListChecks,
                    "No match report yet",
                    "Open this tab after saving to see which games you own.",
                    None,
                )
                .into_any_element()
            };
        };
        let missing_tab = self.app.playlist_match_sub_tab == PlaylistMatchTab::Missing;
        let ids = if missing_tab {
            &report.missing
        } else {
            &report.owned
        };
        let library = self.app.prepared_games(JunkMode::Default);

        v_flex()
            .gap_5()
            .child(
                h_flex()
                    .gap_3()
                    .child(widgets::stat_tile(
                        "Owned",
                        report.owned.len().to_string(),
                        None,
                        cx,
                    ))
                    .child(widgets::stat_tile(
                        "Missing",
                        report.missing.len().to_string(),
                        Some(hx(t.warning)),
                        cx,
                    ))
                    .child(widgets::stat_tile(
                        "Played",
                        report.played.len().to_string(),
                        None,
                        cx,
                    ))
                    .child(widgets::stat_tile(
                        "Unplayed",
                        report.unplayed.len().to_string(),
                        None,
                        cx,
                    ))
                    .child(widgets::stat_tile(
                        "Hidden",
                        report.hidden.len().to_string(),
                        None,
                        cx,
                    ))
                    .child(widgets::stat_tile(
                        "Junk",
                        report.junk.len().to_string(),
                        None,
                        cx,
                    )),
            )
            .when_some(report.completion_price.clone(), |this, price| {
                this.child(widgets::caption(
                    format!(
                        "Buying the missing games would cost about {}",
                        price.format()
                    ),
                    cx,
                ))
            })
            .child(
                widgets::segmented(
                    "match-sub",
                    [Tab::new().label("Owned"), Tab::new().label("Missing")],
                    Some(usize::from(missing_tab)),
                )
                .on_click(cx.listener(|this, ix: &usize, _, cx| {
                    this.app.playlist_match_sub_tab = if *ix == 1 {
                        PlaylistMatchTab::Missing
                    } else {
                        PlaylistMatchTab::Owned
                    };
                    cx.notify();
                })),
            )
            .child(if ids.is_empty() {
                widgets::caption(empty_value_label(), cx).into_any_element()
            } else {
                v_flex()
                    .children(ids.iter().map(|id| {
                        let id = *id;
                        let name = library
                            .as_ref()
                            .and_then(|g| g.iter().find(|g| g.app_id == id))
                            .map_or_else(|| format!("App {id}"), |g| g.name.clone());
                        self.game_line(
                            id,
                            name,
                            Button::new(("m-store", id as usize))
                                .ghost()
                                .small()
                                .icon(IconName::ExternalLink)
                                .tooltip("Open store page")
                                .on_click(move |_, _, _| {
                                    open_url_in_browser(&format!(
                                        "https://store.steampowered.com/app/{id}"
                                    ));
                                }),
                            cx,
                        )
                    }))
                    .into_any_element()
            })
            .into_any_element()
    }

    fn playlist_share(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let json = self
            .app
            .build_playlist_from_edit_fields()
            .ok()
            .and_then(|pf| serde_json::to_string_pretty(&pf).ok());
        v_flex()
            .gap_5()
            .child(
                widgets::panel(cx)
                    .p_5()
                    .gap_3()
                    .child(div().text_base().font_semibold().child("Share code"))
                    .child(widgets::caption(
                        "A compact code friends can paste into Vapourfly to get this playlist.",
                        cx,
                    ))
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("share-copy")
                                    .primary()
                                    .icon(IconName::Copy)
                                    .label("Copy share code")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.copy_share_code(cx)),
                                    ),
                            )
                            .child(
                                Button::new("share-paste")
                                    .outline()
                                    .icon(IconName::ClipboardPaste)
                                    .label("Import from clipboard")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.paste_share_code(cx)),
                                    ),
                            ),
                    )
                    .when_some(self.app.playlist_share_code_output.clone(), |this, code| {
                        this.child(code_block(code, cx))
                    }),
            )
            .child(
                widgets::panel(cx)
                    .p_5()
                    .gap_3()
                    .child(
                        h_flex()
                            .justify_between()
                            .child(div().text_base().font_semibold().child("JSON"))
                            .child(
                                Button::new("share-export")
                                    .outline()
                                    .icon(IconName::Upload)
                                    .label("Export…")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.export_playlist_file(cx);
                                    })),
                            ),
                    )
                    .when(!self.app.playlist_export_path.is_empty(), |this| {
                        this.child(widgets::caption(
                            format!("Last exported to {}", self.app.playlist_export_path),
                            cx,
                        ))
                    })
                    .child(code_block(
                        json.unwrap_or_else(|| "Fill in a name and id to preview the file.".into()),
                        cx,
                    )),
            )
    }
}

fn code_block(text: String, cx: &App) -> impl IntoElement {
    div()
        .w_full()
        .p_3()
        .rounded(cx.theme().radius)
        .bg(cx.theme().muted)
        .border_1()
        .border_color(cx.theme().border.opacity(0.6))
        .font_family(cx.theme().mono_font_family.clone())
        .text_size(px(13.))
        .line_height(px(18.))
        .text_color(cx.theme().muted_foreground)
        .child(text)
}
