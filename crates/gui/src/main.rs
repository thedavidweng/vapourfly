use std::path::{Path, PathBuf};
use std::sync::Mutex;

use eframe::egui;
use vapourfly_core::junk::evaluate_junk;
use vapourfly_core::models::*;
use vapourfly_core::recommend::recommend;
use vapourfly_core::steam::backup::list_backups;
use vapourfly_core::steam::scan::{ScanOptions, scan_library};

// ---------------------------------------------------------------------------
// View enum
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Library,
    Junk,
    Recommend,
    Playlists,
    Collections,
    DataSources,
    Backups,
    Settings,
}

impl View {
    const ALL: &'static [View] = &[
        View::Library,
        View::Junk,
        View::Recommend,
        View::Playlists,
        View::Collections,
        View::DataSources,
        View::Backups,
        View::Settings,
    ];

    fn label(&self) -> &'static str {
        match self {
            View::Library => "Library",
            View::Junk => "Junk",
            View::Recommend => "Recommend",
            View::Playlists => "Playlists",
            View::Collections => "Collections",
            View::DataSources => "Data Sources",
            View::Backups => "Backups",
            View::Settings => "Settings",
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            View::Library => "🎮",
            View::Junk => "🗑",
            View::Recommend => "⭐",
            View::Playlists => "📋",
            View::Collections => "📁",
            View::DataSources => "🔌",
            View::Backups => "💾",
            View::Settings => "⚙",
        }
    }
}

// ---------------------------------------------------------------------------
// Junk mode
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JunkModeChoice {
    Default,
    Strict,
    Aggressive,
}

impl JunkModeChoice {
    fn label(&self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Strict => "Strict",
            Self::Aggressive => "Aggressive",
        }
    }
}

// ---------------------------------------------------------------------------
// Background scan result channel
// ---------------------------------------------------------------------------

static SCAN_RESULT: Mutex<Option<vapourfly_core::Result<ScanResult>>> = Mutex::new(None);

// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct VapourflyApp {
    // Core state
    scan_result: Option<ScanResult>,
    current_view: View,
    loading: bool,
    error: Option<String>,
    fixtures_path: Option<PathBuf>,

    // Library view
    search_query: String,
    filter_installed: bool,
    filter_unplayed: bool,
    filter_hidden: bool,
    filter_junk: bool,

    // Junk view
    junk_mode: JunkModeChoice,
    junk_results: Vec<JunkDecision>,
    junk_selected: std::collections::HashSet<u32>,

    // Recommend view
    recommend_minutes: String,
    recommend_count: String,
    recommend_deck: bool,
    recommend_installed_only: bool,
    recommend_results: Vec<Recommendation>,

    // Playlists view
    playlist_import_path: String,
    playlist_last_import: Option<PlaylistFile>,
    playlist_match_report: Option<PlaylistMatchReport>,

    // Collections view
    collections: Vec<SteamCollection>,

    // Data Sources view
    has_igdb: bool,
    has_rawg: bool,

    // Backups view
    backups: Vec<BackupEntry>,

    // Settings view
    steam_dir_edit: String,
    account_edit: String,
    cc_edit: String,
    lang_edit: String,
}

/// A backup entry for display.
#[derive(Clone, Debug)]
struct BackupEntry {
    #[allow(dead_code)]
    path: PathBuf,
    filename: String,
    size_bytes: u64,
}

impl VapourflyApp {
    fn new(fixtures_path: Option<PathBuf>) -> Self {
        Self {
            scan_result: None,
            current_view: View::Library,
            loading: false,
            error: None,
            fixtures_path,

            search_query: String::new(),
            filter_installed: false,
            filter_unplayed: false,
            filter_hidden: false,
            filter_junk: false,

            junk_mode: JunkModeChoice::Default,
            junk_results: Vec::new(),
            junk_selected: std::collections::HashSet::new(),

            recommend_minutes: "120".into(),
            recommend_count: "5".into(),
            recommend_deck: false,
            recommend_installed_only: false,
            recommend_results: Vec::new(),

            playlist_import_path: String::new(),
            playlist_last_import: None,
            playlist_match_report: None,

            collections: Vec::new(),

            has_igdb: std::env::var("VAPOURFLY_IGDB_CLIENT_ID").is_ok()
                && std::env::var("VAPOURFLY_IGDB_CLIENT_SECRET").is_ok(),
            has_rawg: std::env::var("VAPOURFLY_RAWG_KEY").is_ok(),

            backups: Vec::new(),

            steam_dir_edit: String::new(),
            account_edit: String::new(),
            cc_edit: "us".into(),
            lang_edit: "english".into(),
        }
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;

        let ctx = ctx.clone();
        let fixtures = self.fixtures_path.clone();

        std::thread::spawn(move || {
            let opts = ScanOptions {
                steam_dir: dirs::home_dir()
                    .unwrap_or_default()
                    .join(".steam")
                    .join("steam"),
                account: None,
                fixtures,
            };

            let result = scan_library(&opts);
            ctx.request_repaint();
            SCAN_RESULT.lock().unwrap().replace(result);
        });
    }

    fn filtered_games(&self) -> Vec<&Game> {
        let games = match &self.scan_result {
            Some(scan) => &scan.games,
            None => return Vec::new(),
        };

        games
            .iter()
            .filter(|g| {
                if self.filter_installed && !g.installed {
                    return false;
                }
                if self.filter_unplayed && g.playtime_minutes.unwrap_or(0) > 0 {
                    return false;
                }
                if self.filter_hidden && !g.is_hidden {
                    return false;
                }
                if self.filter_junk && !g.is_junk {
                    return false;
                }
                if !self.search_query.is_empty() {
                    let q = self.search_query.to_lowercase();
                    if !g.name.to_lowercase().contains(&q) && !g.app_id.to_string().contains(&q) {
                        return false;
                    }
                }
                true
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// eframe::App implementation
// ---------------------------------------------------------------------------

impl eframe::App for VapourflyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll background scan result.
        if self.loading {
            let mut guard = SCAN_RESULT.lock().unwrap();
            if let Some(result) = guard.take() {
                self.loading = false;
                match result {
                    Ok(scan) => {
                        // Load collections from scan.
                        self.collections = scan
                            .games
                            .iter()
                            .flat_map(|g| g.steam_collections.clone())
                            .collect::<std::collections::HashSet<_>>()
                            .into_iter()
                            .map(|name| SteamCollection {
                                id: name.to_lowercase().replace(' ', "-"),
                                name,
                                app_ids: Vec::new(),
                                removed_app_ids: Vec::new(),
                                is_hidden_collection: false,
                            })
                            .collect();
                        self.scan_result = Some(scan);
                    }
                    Err(e) => self.error = Some(e.to_string()),
                }
            }
        }

        // -- Left panel: navigation -----------------------------------------
        egui::SidePanel::left("nav_panel")
            .resizable(false)
            .default_width(180.0)
            .show(ctx, |ui| {
                ui.heading("Vapourfly");
                ui.label(format!("v{}", env!("CARGO_PKG_VERSION")));
                ui.separator();

                for &view in View::ALL {
                    let selected = self.current_view == view;
                    let label = format!("{} {}", view.icon(), view.label());
                    if ui.selectable_label(selected, &label).clicked() {
                        self.current_view = view;
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.separator();
                    if self.loading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Scanning...");
                        });
                    } else if ui.button("🔄 Refresh").clicked() {
                        self.start_scan(ctx);
                    }
                    if let Some(scan) = &self.scan_result {
                        ui.label(format!("{} games", scan.games.len()));
                    }
                });
            });

        // -- Central panel: current view ------------------------------------
        egui::CentralPanel::default().show(ctx, |ui| {
            if let Some(err) = &self.error {
                ui.colored_label(egui::Color32::RED, format!("Error: {err}"));
                ui.separator();
            }

            match self.current_view {
                View::Library => self.render_library(ui),
                View::Junk => self.render_junk(ui),
                View::Recommend => self.render_recommend(ui),
                View::Playlists => self.render_playlists(ui),
                View::Collections => self.render_collections(ui),
                View::DataSources => self.render_data_sources(ui),
                View::Backups => self.render_backups(ui),
                View::Settings => self.render_settings(ui),
            }
        });

        // Kick off initial scan on first frame.
        if self.scan_result.is_none() && !self.loading && self.error.is_none() {
            self.start_scan(ctx);
        }
    }
}

// ---------------------------------------------------------------------------
// View renderers
// ---------------------------------------------------------------------------

impl VapourflyApp {
    // -- Library view -------------------------------------------------------

    fn render_library(&mut self, ui: &mut egui::Ui) {
        ui.heading("Library");
        ui.separator();

        // Search and filters
        ui.horizontal(|ui| {
            ui.label("Search:");
            ui.text_edit_singleline(&mut self.search_query);
            ui.separator();
            ui.checkbox(&mut self.filter_installed, "Installed");
            ui.checkbox(&mut self.filter_unplayed, "Unplayed");
            ui.checkbox(&mut self.filter_hidden, "Hidden");
            ui.checkbox(&mut self.filter_junk, "Junk");
        });
        ui.separator();

        let games = self.filtered_games();
        ui.label(format!("Showing {} games", games.len()));
        ui.separator();

        let text_height = egui::TextStyle::Body.resolve(ui.style()).size;

        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::auto().at_least(60.0))
            .column(egui_extras::Column::remainder().at_least(200.0))
            .column(egui_extras::Column::auto().at_least(80.0))
            .column(egui_extras::Column::auto().at_least(100.0))
            .column(egui_extras::Column::auto().at_least(80.0))
            .header(text_height * 1.4, |mut header| {
                header.col(|ui| {
                    ui.strong("App ID");
                });
                header.col(|ui| {
                    ui.strong("Name");
                });
                header.col(|ui| {
                    ui.strong("Installed");
                });
                header.col(|ui| {
                    ui.strong("Playtime");
                });
                header.col(|ui| {
                    ui.strong("Status");
                });
            })
            .body(|mut body| {
                for game in &games {
                    body.row(text_height * 1.2, |mut row| {
                        row.col(|ui| {
                            ui.label(game.app_id.to_string());
                        });
                        row.col(|ui| {
                            ui.label(&game.name);
                        });
                        row.col(|ui| {
                            ui.label(if game.installed { "✓" } else { "—" });
                        });
                        row.col(|ui| {
                            ui.label(format_playtime(game.playtime_minutes.unwrap_or(0)));
                        });
                        row.col(|ui| {
                            let mut status = Vec::new();
                            if game.is_hidden {
                                status.push("Hidden");
                            }
                            if game.is_junk {
                                status.push("Junk");
                            }
                            let status_str = if status.is_empty() {
                                "—".to_string()
                            } else {
                                status.join(", ")
                            };
                            ui.label(status_str);
                        });
                    });
                }
            });
    }

    // -- Junk view ----------------------------------------------------------

    fn render_junk(&mut self, ui: &mut egui::Ui) {
        ui.heading("Junk Detection");
        ui.colored_label(
            egui::Color32::from_rgb(200, 150, 50),
            "⚠ Preview mode — write actions (apply/hide) are disabled in v0.1.0. Use CLI for writes.",
        );
        ui.separator();

        // Mode selector
        ui.horizontal(|ui| {
            ui.label("Mode:");
            for mode in &[
                JunkModeChoice::Default,
                JunkModeChoice::Strict,
                JunkModeChoice::Aggressive,
            ] {
                if ui
                    .selectable_label(self.junk_mode == *mode, mode.label())
                    .clicked()
                {
                    self.junk_mode = *mode;
                }
            }
        });
        ui.separator();

        // Run junk detection
        if ui.button("🔍 Run Junk Detection").clicked() {
            if let Some(scan) = &self.scan_result {
                let mode = match self.junk_mode {
                    JunkModeChoice::Default => vapourfly_core::models::JunkMode::Default,
                    JunkModeChoice::Strict => vapourfly_core::models::JunkMode::Strict,
                    JunkModeChoice::Aggressive => vapourfly_core::models::JunkMode::Aggressive,
                };
                let overrides = vapourfly_core::junk::ManualOverrides::default();
                self.junk_results =
                    evaluate_junk(&scan.games, &JunkRules::default(), &mode, &overrides);
                self.junk_selected.clear();
            }
        }

        if self.junk_results.is_empty() {
            ui.label("No junk detection results yet. Click 'Run Junk Detection' to scan.");
            return;
        }

        let junk_count = self.junk_results.iter().filter(|d| d.is_junk).count();
        ui.label(format!(
            "Found {} junk candidates out of {} games evaluated",
            junk_count,
            self.junk_results.len()
        ));
        ui.separator();

        // Results table
        let text_height = egui::TextStyle::Body.resolve(ui.style()).size;
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::auto().at_least(60.0))
            .column(egui_extras::Column::remainder().at_least(150.0))
            .column(egui_extras::Column::auto().at_least(60.0))
            .column(egui_extras::Column::auto().at_least(80.0))
            .column(egui_extras::Column::remainder().at_least(200.0))
            .header(text_height * 1.4, |mut header| {
                header.col(|ui| {
                    ui.strong("ID");
                });
                header.col(|ui| {
                    ui.strong("Name");
                });
                header.col(|ui| {
                    ui.strong("Junk?");
                });
                header.col(|ui| {
                    ui.strong("Confidence");
                });
                header.col(|ui| {
                    ui.strong("Signals");
                });
            })
            .body(|mut body| {
                for decision in &self.junk_results {
                    body.row(text_height * 1.2, |mut row| {
                        row.col(|ui| {
                            ui.label(decision.app_id.to_string());
                        });
                        row.col(|ui| {
                            ui.label(&decision.name);
                        });
                        row.col(|ui| {
                            ui.label(if decision.is_junk { "✓" } else { "—" });
                        });
                        row.col(|ui| {
                            ui.label(format!("{:.0}%", decision.confidence * 100.0));
                        });
                        row.col(|ui| {
                            let signals: Vec<String> =
                                decision.matched.iter().map(format_junk_signal).collect();
                            ui.label(if signals.is_empty() {
                                "—".to_string()
                            } else {
                                signals.join(", ")
                            });
                        });
                    });
                }
            });
    }

    // -- Recommend view -----------------------------------------------------

    fn render_recommend(&mut self, ui: &mut egui::Ui) {
        ui.heading("Recommendations");
        ui.separator();

        // Controls
        ui.horizontal(|ui| {
            ui.label("Available minutes:");
            ui.text_edit_singleline(&mut self.recommend_minutes);
            ui.separator();
            ui.label("Count:");
            ui.text_edit_singleline(&mut self.recommend_count);
            ui.separator();
            ui.checkbox(&mut self.recommend_deck, "Deck mode");
            ui.checkbox(&mut self.recommend_installed_only, "Installed only");
        });
        ui.separator();

        if ui.button("⭐ Get Recommendations").clicked() {
            if let Some(scan) = &self.scan_result {
                let minutes: u32 = self.recommend_minutes.parse().unwrap_or(120);
                let count: usize = self.recommend_count.parse().unwrap_or(5);
                let request = RecommendRequest {
                    available_minutes: minutes,
                    count,
                    deck_mode: self.recommend_deck,
                    include_installed_only: self.recommend_installed_only,
                    seed: Some(42),
                    exclude_collections: vec!["hidden".into()],
                };
                self.recommend_results = recommend(&scan.games, &request);
            }
        }

        if self.recommend_results.is_empty() {
            ui.label(
                "No recommendations yet. Set your available time and click 'Get Recommendations'.",
            );
            return;
        }

        ui.label(format!(
            "Top {} recommendations:",
            self.recommend_results.len()
        ));
        ui.separator();

        // Recommendation cards
        egui::ScrollArea::vertical().show(ui, |ui| {
            for rec in &self.recommend_results {
                ui.group(|ui| {
                    ui.horizontal(|ui| {
                        ui.strong(&rec.name);
                        ui.label(format!("(AppID: {})", rec.app_id));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(format!("Score: {:.2}", rec.score));
                        });
                    });
                    if !rec.reasons.is_empty() {
                        ui.indent("rec_reasons", |ui| {
                            for reason in &rec.reasons {
                                ui.label(format!(
                                    "• {} ({:+.1})",
                                    reason.description, reason.weight
                                ));
                            }
                        });
                    }
                });
                ui.add_space(4.0);
            }
        });
    }

    // -- Playlists view -----------------------------------------------------

    fn render_playlists(&mut self, ui: &mut egui::Ui) {
        ui.heading("Playlists");
        ui.separator();

        // Import section
        ui.group(|ui| {
            ui.strong("Import Playlist");
            ui.horizontal(|ui| {
                ui.label("Path:");
                ui.text_edit_singleline(&mut self.playlist_import_path);
                if ui.button("Import").clicked() && !self.playlist_import_path.is_empty() {
                    match vapourfly_core::playlist::import_playlist(Path::new(
                        &self.playlist_import_path,
                    )) {
                        Ok(pf) => {
                            // Run match against library
                            if let Some(scan) = &self.scan_result {
                                match vapourfly_core::playlist::match_playlist(&pf, &scan.games) {
                                    Ok(report) => {
                                        self.playlist_match_report = Some(report);
                                    }
                                    Err(e) => {
                                        self.error = Some(format!("Match failed: {e}"));
                                    }
                                }
                            }
                            self.playlist_last_import = Some(pf);
                        }
                        Err(e) => {
                            self.error = Some(format!("Import failed: {e}"));
                        }
                    }
                }
            });
        });
        ui.separator();

        // Show imported playlist info
        if let Some(pf) = &self.playlist_last_import {
            ui.group(|ui| {
                ui.strong(format!("Playlist: {}", pf.playlist.name));
                ui.label(format!("ID: {}", pf.playlist.id));
                ui.label(format!("Description: {}", pf.playlist.description));
                ui.label(format!("Schema: {}", pf.vapourfly_schema));

                match &pf.playlist.content {
                    PlaylistContent::Manual { app_ids } => {
                        ui.label(format!("Manual playlist with {} AppIDs", app_ids.len()));
                    }
                    PlaylistContent::Rules { rules } => {
                        ui.label(format!("Rule-based playlist with {} rules", rules.len()));
                    }
                }
            });
            ui.separator();
        }

        // Match report
        if let Some(report) = &self.playlist_match_report {
            ui.group(|ui| {
                ui.strong("Match Report");
                ui.horizontal(|ui| {
                    ui.label(format!("Owned: {}", report.owned.len()));
                    ui.label(format!("Missing: {}", report.missing.len()));
                    ui.label(format!("Played: {}", report.played.len()));
                    ui.label(format!("Unplayed: {}", report.unplayed.len()));
                });
                ui.horizontal(|ui| {
                    ui.label(format!("Hidden: {}", report.hidden.len()));
                    ui.label(format!("Junk: {}", report.junk.len()));
                    if let Some(price) = &report.completion_price {
                        ui.label(format!(
                            "Completion price: {} {}",
                            price.currency, price.amount_cents
                        ));
                    }
                });
            });
        }
    }

    // -- Collections view ---------------------------------------------------

    fn render_collections(&mut self, ui: &mut egui::Ui) {
        ui.heading("Collections");
        ui.separator();

        if self.collections.is_empty() {
            ui.label("No collections found. Run a scan first.");
            return;
        }

        ui.label(format!("{} collections found", self.collections.len()));
        ui.separator();

        let text_height = egui::TextStyle::Body.resolve(ui.style()).size;
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::remainder().at_least(200.0))
            .column(egui_extras::Column::auto().at_least(80.0))
            .column(egui_extras::Column::auto().at_least(100.0))
            .header(text_height * 1.4, |mut header| {
                header.col(|ui| {
                    ui.strong("Name");
                });
                header.col(|ui| {
                    ui.strong("Games");
                });
                header.col(|ui| {
                    ui.strong("Hidden?");
                });
            })
            .body(|mut body| {
                for coll in &self.collections {
                    body.row(text_height * 1.2, |mut row| {
                        row.col(|ui| {
                            ui.label(&coll.name);
                        });
                        row.col(|ui| {
                            ui.label(coll.app_ids.len().to_string());
                        });
                        row.col(|ui| {
                            ui.label(if coll.is_hidden_collection {
                                "✓"
                            } else {
                                "—"
                            });
                        });
                    });
                }
            });
    }

    // -- Data Sources view --------------------------------------------------

    fn render_data_sources(&mut self, ui: &mut egui::Ui) {
        ui.heading("Data Sources");
        ui.separator();

        ui.label("External API credential status:");
        ui.separator();

        let sources = [
            (
                "IGDB (Twitch OAuth)",
                self.has_igdb,
                "VAPOURFLY_IGDB_CLIENT_ID + VAPOURFLY_IGDB_CLIENT_SECRET",
            ),
            ("RAWG", self.has_rawg, "VAPOURFLY_RAWG_KEY"),
            ("ProtonDB", true, "No credentials needed"),
            ("PCGamingWiki", true, "No credentials needed"),
            ("Steam Store", true, "No credentials needed"),
            (
                "HLTB",
                false,
                "Feature gate: hltb_scrape (compile with --features hltb_scrape)",
            ),
        ];

        for (name, available, note) in &sources {
            ui.horizontal(|ui| {
                let icon = if *available { "✅" } else { "❌" };
                ui.label(format!("{icon} {name}"));
                ui.label(format!("— {note}"));
            });
        }

        ui.separator();
        ui.label("Set credentials via environment variables. See docs/API_SOURCES.md for details.");
        ui.separator();

        // Cache status
        ui.strong("Cache Status");
        ui.label("Use `vapourfly sources status` and `vapourfly cache refresh` CLI commands for full cache management.");
        ui.label("GUI cache refresh will be implemented in a future release.");

        // Offline mode indicator
        ui.separator();
        ui.strong("Offline Mode");
        ui.label("Use `--offline` CLI flag to prohibit network calls and use cached data only.");
    }

    // -- Backups view -------------------------------------------------------

    fn render_backups(&mut self, ui: &mut egui::Ui) {
        ui.heading("Backups");
        ui.separator();

        // Refresh backups list
        if ui.button("🔄 Refresh Backups").clicked() {
            self.backups.clear();
            if let Some(scan) = &self.scan_result {
                // list_backups() expects the target file path (not directory),
                // and derives the backup prefix from the file name.
                let cloud_path = PathBuf::from(&scan.steam_dir)
                    .join("userdata")
                    .join(&scan.account)
                    .join("config")
                    .join("cloudstorage")
                    .join("cloud-storage-namespace-1.json");

                if cloud_path.exists() {
                    match list_backups(&cloud_path) {
                        Ok(backup_infos) => {
                            self.backups = backup_infos
                                .into_iter()
                                .map(|info| {
                                    let size =
                                        std::fs::metadata(&info.path).map(|m| m.len()).unwrap_or(0);
                                    BackupEntry {
                                        filename: info
                                            .path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .to_string(),
                                        path: info.path,
                                        size_bytes: size,
                                    }
                                })
                                .collect();
                        }
                        Err(e) => {
                            self.error = Some(format!("Failed to list backups: {e}"));
                        }
                    }
                }
            }
        }

        if self.backups.is_empty() {
            ui.label("No backups found. Click 'Refresh Backups' to scan.");
            return;
        }

        ui.label(format!("{} backups found", self.backups.len()));
        ui.separator();

        ui.colored_label(
            egui::Color32::from_rgb(200, 150, 50),
            "⚠ Backup restore is disabled in GUI preview. Use `vapourfly backup restore <file>` CLI command.",
        );
        ui.separator();

        let text_height = egui::TextStyle::Body.resolve(ui.style()).size;
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::remainder().at_least(300.0))
            .column(egui_extras::Column::auto().at_least(100.0))
            .header(text_height * 1.4, |mut header| {
                header.col(|ui| {
                    ui.strong("Filename");
                });
                header.col(|ui| {
                    ui.strong("Size");
                });
            })
            .body(|mut body| {
                for backup in &self.backups {
                    body.row(text_height * 1.2, |mut row| {
                        row.col(|ui| {
                            ui.label(&backup.filename);
                        });
                        row.col(|ui| {
                            ui.label(format_bytes(backup.size_bytes));
                        });
                    });
                }
            });
    }

    // -- Settings view ------------------------------------------------------

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.separator();

        ui.colored_label(
            egui::Color32::from_rgb(200, 150, 50),
            "⚠ Settings are display-only in v0.1.0 preview. Use CLI flags or config.toml for configuration.",
        );
        ui.separator();

        ui.group(|ui| {
            ui.strong("Steam Directory Override");
            ui.horizontal(|ui| {
                ui.label("Path:");
                ui.text_edit_singleline(&mut self.steam_dir_edit);
            });
            ui.label("Leave empty for auto-detection. CLI: --steam-dir");
        });
        ui.separator();

        ui.group(|ui| {
            ui.strong("Account Override");
            ui.horizontal(|ui| {
                ui.label("Account:");
                ui.text_edit_singleline(&mut self.account_edit);
            });
            ui.label("Leave empty for auto-selection (most recent). CLI: --account");
        });
        ui.separator();

        ui.group(|ui| {
            ui.strong("Store Locale");
            ui.horizontal(|ui| {
                ui.label("Country code:");
                ui.text_edit_singleline(&mut self.cc_edit);
                ui.label("Language:");
                ui.text_edit_singleline(&mut self.lang_edit);
            });
            ui.label("Configure in ~/.config/vapourfly/config.toml");
        });
        ui.separator();

        ui.group(|ui| {
            ui.strong("About");
            ui.label(format!("Vapourfly v{}", env!("CARGO_PKG_VERSION")));
            ui.label("A local-first CLI/GUI tool for managing Steam game libraries.");
            ui.label("Licensed under MIT OR Apache-2.0.");
        });
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_playtime(minutes: u32) -> String {
    if minutes == 0 {
        return "—".to_string();
    }
    let hours = minutes / 60;
    let mins = minutes % 60;
    if hours == 0 {
        format!("{mins}m")
    } else {
        format!("{hours}h {mins}m")
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn format_junk_signal(signal: &JunkSignal) -> String {
    match signal {
        JunkSignal::LowPlaytime { minutes } => format!("Low playtime ({minutes}m)"),
        JunkSignal::ShortCompletion { seconds, source } => {
            format!("Short completion ({}h, {:?})", seconds / 3600, source)
        }
        JunkSignal::LowRating { rating_0_5, source } => {
            format!("Low rating ({rating_0_5:.1}, {source:?})")
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> eframe::Result<()> {
    let fixtures_path = std::env::args()
        .collect::<Vec<String>>()
        .windows(2)
        .find(|w| w[0] == "--fixtures")
        .map(|w| PathBuf::from(&w[1]));

    let app = VapourflyApp::new(fixtures_path);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1024.0, 768.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Vapourfly",
        native_options,
        Box::new(|_cc| Ok(Box::new(app))),
    )
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_created_without_fixtures() {
        let app = VapourflyApp::new(None);
        assert!(app.scan_result.is_none());
        assert_eq!(app.current_view, View::Library);
        assert!(!app.loading);
        assert!(app.error.is_none());
    }

    #[test]
    fn app_created_with_fixtures_path() {
        let path = PathBuf::from("/tmp/fix");
        let app = VapourflyApp::new(Some(path.clone()));
        assert_eq!(app.fixtures_path, Some(path));
    }

    #[test]
    fn scan_with_fixtures_produces_results() {
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal");
        let opts = ScanOptions {
            steam_dir: fixtures.clone(),
            account: None,
            fixtures: Some(fixtures),
        };
        let result = scan_library(&opts).unwrap();

        assert!(!result.games.is_empty());
        assert_eq!(result.account, "vapourfly_fixture_user");

        let cs2 = result.games.iter().find(|g| g.app_id == 730).unwrap();
        assert_eq!(cs2.name, "Counter-Strike 2");
        assert!(cs2.installed);
        assert_eq!(cs2.playtime_minutes, Some(418));
    }

    #[test]
    fn view_all_contains_every_variant() {
        assert_eq!(View::ALL.len(), 8);
        assert!(View::ALL.contains(&View::Library));
        assert!(View::ALL.contains(&View::Junk));
        assert!(View::ALL.contains(&View::Recommend));
        assert!(View::ALL.contains(&View::Playlists));
        assert!(View::ALL.contains(&View::Collections));
        assert!(View::ALL.contains(&View::DataSources));
        assert!(View::ALL.contains(&View::Backups));
        assert!(View::ALL.contains(&View::Settings));
    }

    #[test]
    fn view_labels_are_distinct() {
        let labels: Vec<&str> = View::ALL.iter().map(|v| v.label()).collect();
        let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
        assert_eq!(labels.len(), unique.len());
    }

    #[test]
    fn format_playtime_zero() {
        assert_eq!(format_playtime(0), "—");
    }

    #[test]
    fn format_playtime_minutes_only() {
        assert_eq!(format_playtime(45), "45m");
    }

    #[test]
    fn format_playtime_hours_and_minutes() {
        assert_eq!(format_playtime(125), "2h 5m");
    }

    #[test]
    fn format_bytes_variants() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1536), "1.5 KB");
        assert_eq!(format_bytes(1048576), "1.0 MB");
    }

    #[test]
    fn junk_mode_labels() {
        assert_eq!(JunkModeChoice::Default.label(), "Default");
        assert_eq!(JunkModeChoice::Strict.label(), "Strict");
        assert_eq!(JunkModeChoice::Aggressive.label(), "Aggressive");
    }

    #[test]
    fn view_icons_exist() {
        for view in View::ALL {
            assert!(!view.icon().is_empty());
        }
    }
}
