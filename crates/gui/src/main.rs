use std::path::{Path, PathBuf};
use std::sync::Mutex;

use eframe::egui;
use vapourfly_core::config::VapourflyConfig;
use vapourfly_core::junk::evaluate_junk;
use vapourfly_core::models::*;
use vapourfly_core::recommend::recommend;
use vapourfly_core::steam::BackupInfo;
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
// Pending action for confirmation dialog
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum PendingAction {
    JunkApply,
    JunkHide,
    BackupRestore(PathBuf),
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
// Background result channels
// ---------------------------------------------------------------------------

static SCAN_RESULT: Mutex<Option<vapourfly_core::Result<ScanResult>>> = Mutex::new(None);
static WRITE_RESULT: Mutex<Option<Result<String, String>>> = Mutex::new(None);
static ENRICH_RESULT: Mutex<Option<Result<vapourfly_api::enrichment::EnrichmentSummary, String>>> =
    Mutex::new(None);
static DRY_RUN_RESULT: Mutex<Option<Result<vapourfly_core::models::WritePlan, String>>> =
    Mutex::new(None);

// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// App state
// ---------------------------------------------------------------------------

struct VapourflyApp {
    // Core state
    scan_result: Option<ScanResult>,
    current_view: View,
    loading: bool,
    error: Option<String>,
    success_msg: Option<String>,
    fixtures_path: Option<PathBuf>,

    // Config
    config: Option<VapourflyConfig>,

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
    junk_collection_name: String,

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
    source_statuses: Vec<vapourfly_api::enrichment::SourceStatus>,

    // Backups view
    backups: Vec<BackupInfo>,

    // Settings view
    steam_dir_edit: String,
    account_edit: String,
    cc_edit: String,
    lang_edit: String,
    backup_retention_edit: String,
    allow_steam_running: bool,
    settings_save_msg: Option<String>,

    // Write operations
    write_loading: bool,
    write_result: Option<Result<String, String>>,
    show_confirm_dialog: bool,
    pending_action: Option<PendingAction>,
    dry_run_plan: Option<vapourfly_core::models::WritePlan>,
    dry_run_loading: bool,
    dry_run_error: Option<String>,

    // Cache refresh
    cache_refresh_loading: bool,
    cache_refresh_msg: Option<String>,
}

impl VapourflyApp {
    fn new(fixtures_path: Option<PathBuf>) -> Self {
        // Load configuration
        let config = VapourflyConfig::from_cli_and_env(vapourfly_core::config::CliOverrides {
            steam_dir: fixtures_path.clone(),
            account: None,
        })
        .ok();

        let steam_dir_edit = config
            .as_ref()
            .map(|c| c.steam_dir.to_string_lossy().to_string())
            .unwrap_or_default();

        let account_edit = config
            .as_ref()
            .and_then(|c| c.account.clone())
            .unwrap_or_default();

        let cc_edit = config
            .as_ref()
            .map(|c| c.cc.clone())
            .unwrap_or_else(|| "US".into());

        let lang_edit = config
            .as_ref()
            .map(|c| c.lang.clone())
            .unwrap_or_else(|| "english".into());

        let backup_retention_edit = config
            .as_ref()
            .map(|c| c.backup_retention_count.to_string())
            .unwrap_or_else(|| "5".into());

        let has_igdb = config
            .as_ref()
            .map(|c| c.has_igdb_credentials)
            .unwrap_or(false);

        let has_rawg = config
            .as_ref()
            .map(|c| c.has_rawg_credentials)
            .unwrap_or(false);

        // Load source cache statuses
        let cache_root = vapourfly_core::config::default_cache_dir();
        let source_statuses = vapourfly_api::enrichment::source_status(&cache_root);

        Self {
            scan_result: None,
            current_view: View::Library,
            loading: false,
            error: None,
            success_msg: None,
            fixtures_path,

            config,

            search_query: String::new(),
            filter_installed: false,
            filter_unplayed: false,
            filter_hidden: false,
            filter_junk: false,

            junk_mode: JunkModeChoice::Default,
            junk_results: Vec::new(),
            junk_selected: std::collections::HashSet::new(),
            junk_collection_name: "junk".into(),

            recommend_minutes: "120".into(),
            recommend_count: "5".into(),
            recommend_deck: false,
            recommend_installed_only: false,
            recommend_results: Vec::new(),

            playlist_import_path: String::new(),
            playlist_last_import: None,
            playlist_match_report: None,

            collections: Vec::new(),

            has_igdb,
            has_rawg,
            source_statuses,

            backups: Vec::new(),

            steam_dir_edit,
            account_edit,
            cc_edit,
            lang_edit,
            backup_retention_edit,
            allow_steam_running: false,
            settings_save_msg: None,

            write_loading: false,
            write_result: None,
            show_confirm_dialog: false,
            pending_action: None,
            dry_run_plan: None,
            dry_run_loading: false,
            dry_run_error: None,

            cache_refresh_loading: false,
            cache_refresh_msg: None,
        }
    }

    /// Resolve the cloud storage path for the current config.
    fn cloud_storage_path(&self) -> Result<PathBuf, String> {
        let config = self
            .config
            .as_ref()
            .ok_or("Configuration not loaded. Set Steam directory in Settings.")?;

        let accounts = vapourfly_core::steam::detect_accounts(&config.steam_dir)
            .map_err(|e| format!("Failed to detect Steam accounts: {e}"))?;

        let selected = vapourfly_core::steam::select_account(&accounts, config.account.as_deref())
            .map_err(|e| format!("Failed to select account: {e}"))?;

        Ok(
            vapourfly_core::steam::resolve_userdata_dir(&config.steam_dir, &selected.steam_id64)
                .join("config/cloudstorage/cloud-storage-namespace-1.json"),
        )
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        if self.loading {
            return;
        }
        self.loading = true;
        self.error = None;
        self.success_msg = None;

        let ctx = ctx.clone();
        let fixtures = self.fixtures_path.clone();

        // Use config steam_dir if available, otherwise auto-detect
        let steam_dir = self
            .config
            .as_ref()
            .map(|c| c.steam_dir.clone())
            .or_else(vapourfly_core::config::VapourflyConfig::detect_steam_dir)
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .unwrap_or_default()
                    .join(".steam")
                    .join("steam")
            });

        let account = self.config.as_ref().and_then(|c| c.account.clone());

        std::thread::spawn(move || {
            let opts = ScanOptions {
                steam_dir,
                account,
                fixtures,
            };

            let result = scan_library(&opts);
            ctx.request_repaint();
            SCAN_RESULT.lock().unwrap().replace(result);
        });
    }

    /// Execute a pending write action in a background thread.
    ///
    /// For junk apply/hide the [`WritePlan`] was already generated during the
    /// dry-run step and stored in `self.dry_run_plan`.  For backup restores we
    /// fall back to the original on-the-fly path.
    fn execute_pending_action(&mut self) {
        let action = match self.pending_action.take() {
            Some(a) => a,
            None => return,
        };

        self.show_confirm_dialog = false;

        // If we have a pre-computed plan from the dry-run step, execute it
        // directly.  Otherwise fall through to the legacy path (backup
        // restore).
        if let Some(plan) = self.dry_run_plan.take() {
            self.write_loading = true;
            self.write_result = None;
            self.success_msg = None;
            let allow_steam_running = self.allow_steam_running;

            std::thread::spawn(move || {
                let result = vapourfly_core::steam::check_write_safety(
                    &plan.target_path,
                    allow_steam_running,
                )
                .map_err(|e| format!("Safety check failed: {e}"))
                .and_then(|_| {
                    vapourfly_core::steam::backup::execute_write_plan(&plan, 5)
                        .map_err(|e| format!("Write failed: {e}"))
                })
                .map(|_| format!("Write complete. Backup: {}", plan.backup_path.display()));

                WRITE_RESULT.lock().unwrap().replace(result);
            });
            return;
        }

        // Legacy path for BackupRestore (no dry-run diff).
        let cloud_path = match self.cloud_storage_path() {
            Ok(p) => p,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };

        self.write_loading = true;
        self.write_result = None;
        self.success_msg = None;

        let junk_results = self.junk_results.clone();
        let collection_name = self.junk_collection_name.clone();
        let allow_steam_running = self.allow_steam_running;

        std::thread::spawn(move || {
            let result = match action {
                PendingAction::JunkApply => execute_junk_apply(
                    cloud_path,
                    junk_results,
                    collection_name,
                    allow_steam_running,
                ),
                PendingAction::JunkHide => {
                    execute_junk_hide(cloud_path, junk_results, allow_steam_running)
                }
                PendingAction::BackupRestore(backup_path) => {
                    execute_backup_restore(backup_path, cloud_path, allow_steam_running)
                }
            };

            WRITE_RESULT.lock().unwrap().replace(result);
        });
    }

    /// Generate a dry-run WritePlan for the pending action and show the diff
    /// modal before committing to disk.
    fn start_dry_run(&mut self, action: PendingAction) {
        let cloud_path = match self.cloud_storage_path() {
            Ok(p) => p,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };

        self.dry_run_loading = true;
        self.dry_run_error = None;
        self.dry_run_plan = None;
        self.pending_action = Some(action.clone());

        let junk_results = self.junk_results.clone();
        let collection_name = self.junk_collection_name.clone();

        std::thread::spawn(move || {
            let result =
                generate_dry_run_plan(cloud_path, &action, &junk_results, &collection_name);
            DRY_RUN_RESULT.lock().unwrap().replace(result);
        });
    }

    /// Start a cache refresh for the given source (or all sources).
    fn start_cache_refresh(&mut self, source: Option<String>, ctx: &egui::Context) {
        if self.cache_refresh_loading {
            return;
        }

        let games = match &self.scan_result {
            Some(scan) => scan.games.clone(),
            None => {
                self.cache_refresh_msg =
                    Some("Scan your library first to enable cache refresh.".into());
                return;
            }
        };

        self.cache_refresh_loading = true;
        self.cache_refresh_msg = None;
        self.success_msg = None;

        let cache_root = vapourfly_core::config::default_cache_dir();
        let ctx = ctx.clone();

        std::thread::spawn(move || {
            let cache = vapourfly_api::cache::DiskCache::new(cache_root);

            let sources = match source {
                Some(s) => vec![s],
                None => vapourfly_api::enrichment::ALL_SOURCES
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            };

            let options = vapourfly_api::enrichment::EnrichmentOptions {
                sources,
                offline: false,
                force: true,
            };

            let mut games = games;
            let summary = vapourfly_api::enrichment::enrich_games(&mut games, &cache, &options);

            ctx.request_repaint();
            ENRICH_RESULT.lock().unwrap().replace(Ok(summary));
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

    /// Reload source cache statuses from disk.
    fn reload_source_statuses(&mut self) {
        let cache_root = vapourfly_core::config::default_cache_dir();
        self.source_statuses = vapourfly_api::enrichment::source_status(&cache_root);
    }
}

// ---------------------------------------------------------------------------
// Write operation helpers (run in background threads)
// ---------------------------------------------------------------------------

/// Generate a [`WritePlan`] without executing it, so the GUI can display a
/// dry-run diff before the user confirms.
fn generate_dry_run_plan(
    cloud_path: PathBuf,
    action: &PendingAction,
    junk_results: &[JunkDecision],
    collection_name: &str,
) -> Result<vapourfly_core::models::WritePlan, String> {
    let mut junk_app_ids: Vec<u32> = junk_results
        .iter()
        .filter(|d| d.is_junk)
        .map(|d| d.app_id)
        .collect();
    junk_app_ids.sort();
    junk_app_ids.dedup();

    if junk_app_ids.is_empty() {
        return Err("No junk candidates found.".into());
    }

    let cloud = vapourfly_core::steam::read_cloud_storage(&cloud_path)
        .map_err(|e| format!("Failed to read cloud storage: {e}"))?;

    let op = match action {
        PendingAction::JunkApply => WriteOp::UpsertCollection {
            id: collection_name.to_string(),
            added: junk_app_ids,
            removed: vec![],
        },
        PendingAction::JunkHide => WriteOp::AddToHidden {
            app_ids: junk_app_ids,
        },
        PendingAction::BackupRestore(_) => {
            return Err("Dry-run not supported for backup restore.".into());
        }
    };

    vapourfly_core::steam::generate_write_plan(&cloud, vec![op], cloud_path)
        .map_err(|e| format!("Failed to generate write plan: {e}"))
}

fn execute_junk_apply(
    cloud_path: PathBuf,
    junk_results: Vec<JunkDecision>,
    collection_name: String,
    allow_steam_running: bool,
) -> Result<String, String> {
    let mut junk_app_ids: Vec<u32> = junk_results
        .iter()
        .filter(|d| d.is_junk)
        .map(|d| d.app_id)
        .collect();
    junk_app_ids.sort();
    junk_app_ids.dedup();

    if junk_app_ids.is_empty() {
        return Err("No junk candidates found.".into());
    }

    let cloud = vapourfly_core::steam::read_cloud_storage(&cloud_path)
        .map_err(|e| format!("Failed to read cloud storage: {e}"))?;

    let plan = vapourfly_core::steam::generate_write_plan(
        &cloud,
        vec![WriteOp::UpsertCollection {
            id: collection_name.clone(),
            added: junk_app_ids.clone(),
            removed: vec![],
        }],
        cloud_path.clone(),
    )
    .map_err(|e| format!("Failed to generate write plan: {e}"))?;

    vapourfly_core::steam::check_write_safety(&cloud_path, allow_steam_running)
        .map_err(|e| format!("Safety check failed: {e}"))?;

    vapourfly_core::steam::execute_write_plan(&plan, 5)
        .map_err(|e| format!("Write failed: {e}"))?;

    Ok(format!(
        "Applied {} junk games to collection '{}'. Backup: {}",
        junk_app_ids.len(),
        collection_name,
        plan.backup_path.display()
    ))
}

fn execute_junk_hide(
    cloud_path: PathBuf,
    junk_results: Vec<JunkDecision>,
    allow_steam_running: bool,
) -> Result<String, String> {
    let mut junk_app_ids: Vec<u32> = junk_results
        .iter()
        .filter(|d| d.is_junk)
        .map(|d| d.app_id)
        .collect();
    junk_app_ids.sort();
    junk_app_ids.dedup();

    if junk_app_ids.is_empty() {
        return Err("No junk candidates found.".into());
    }

    let cloud = vapourfly_core::steam::read_cloud_storage(&cloud_path)
        .map_err(|e| format!("Failed to read cloud storage: {e}"))?;

    let plan = vapourfly_core::steam::generate_write_plan(
        &cloud,
        vec![WriteOp::AddToHidden {
            app_ids: junk_app_ids.clone(),
        }],
        cloud_path.clone(),
    )
    .map_err(|e| format!("Failed to generate write plan: {e}"))?;

    vapourfly_core::steam::check_write_safety(&cloud_path, allow_steam_running)
        .map_err(|e| format!("Safety check failed: {e}"))?;

    vapourfly_core::steam::execute_write_plan(&plan, 5)
        .map_err(|e| format!("Write failed: {e}"))?;

    Ok(format!(
        "Added {} junk games to Hidden collection. Backup: {}",
        junk_app_ids.len(),
        plan.backup_path.display()
    ))
}

fn execute_backup_restore(
    backup_path: PathBuf,
    cloud_path: PathBuf,
    allow_steam_running: bool,
) -> Result<String, String> {
    vapourfly_core::steam::check_write_safety(&cloud_path, allow_steam_running)
        .map_err(|e| format!("Safety check failed: {e}"))?;

    vapourfly_core::steam::restore_backup(&backup_path, &cloud_path)
        .map_err(|e| format!("Restore failed: {e}"))?;

    let filename = backup_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    Ok(format!("Restored backup '{filename}'"))
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

        // Poll background write result.
        if self.write_loading {
            let mut guard = WRITE_RESULT.lock().unwrap();
            if let Some(result) = guard.take() {
                self.write_loading = false;
                match result {
                    Ok(msg) => {
                        self.success_msg = Some(msg);
                        // Auto-re-scan to pick up changes
                        self.start_scan(ctx);
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }

        // Poll background cache refresh result.
        if self.cache_refresh_loading {
            let mut guard = ENRICH_RESULT.lock().unwrap();
            if let Some(result) = guard.take() {
                self.cache_refresh_loading = false;
                match result {
                    Ok(summary) => {
                        self.cache_refresh_msg = Some(format!(
                            "Refresh complete: {} games processed, {} network fetches, {} cache hits, {} errors",
                            summary.games_processed,
                            summary.network_fetches,
                            summary.cache_hits,
                            summary.errors.len()
                        ));
                        self.reload_source_statuses();
                    }
                    Err(e) => self.cache_refresh_msg = Some(format!("Error: {e}")),
                }
            }
        }

        // -- Confirmation dialog -----------------------------------------------
        // Poll background dry-run result.
        if self.dry_run_loading {
            let mut guard = DRY_RUN_RESULT.lock().unwrap();
            if let Some(result) = guard.take() {
                self.dry_run_loading = false;
                match result {
                    Ok(plan) => {
                        self.dry_run_plan = Some(plan);
                        self.dry_run_error = None;
                        self.show_confirm_dialog = true;
                    }
                    Err(e) => {
                        self.dry_run_error = Some(e);
                        self.pending_action = None;
                    }
                }
            }
        }
        self.render_confirm_dialog(ctx);

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
            // Error and success banners (clone to avoid borrow issues)
            let mut dismiss_error = false;
            let mut dismiss_success = false;
            if let Some(err) = self.error.clone() {
                let resp = ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::RED, format!("Error: {err}"));
                    ui.button("✕").clicked()
                });
                dismiss_error = resp.inner;
                ui.separator();
            }
            if let Some(msg) = self.success_msg.clone() {
                let resp = ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(50, 180, 50), format!("✓ {msg}"));
                    ui.button("✕").clicked()
                });
                dismiss_success = resp.inner;
                ui.separator();
            }
            if dismiss_error {
                self.error = None;
            }
            if dismiss_success {
                self.success_msg = None;
            }

            match self.current_view {
                View::Library => self.render_library(ui),
                View::Junk => self.render_junk(ui),
                View::Recommend => self.render_recommend(ui),
                View::Playlists => self.render_playlists(ui),
                View::Collections => self.render_collections(ui),
                View::DataSources => self.render_data_sources(ui, ctx),
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
// Confirmation dialog
// ---------------------------------------------------------------------------

impl VapourflyApp {
    fn render_confirm_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_confirm_dialog {
            // Surface dry-run errors even when the dialog isn't open.
            if let Some(err) = self.dry_run_error.take() {
                self.error = Some(err);
            }
            return;
        }

        let mut open = self.show_confirm_dialog;
        egui::Window::new("Confirm Action")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                // -- Dry-run diff (junk apply / hide) --------------------------
                if let Some(plan) = &self.dry_run_plan {
                    let diff = &plan.diff;

                    ui.heading("Dry-Run Diff");
                    ui.label(format!(
                        "Target: {}",
                        plan.target_path.display()
                    ));
                    ui.add_space(4.0);

                    egui::Grid::new("dry_run_diff_grid")
                        .num_columns(2)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            if !diff.collections_changed.is_empty() {
                                ui.label("Collections changed:");
                                let names: Vec<&str> = diff
                                    .collections_changed
                                    .iter()
                                    .map(|c| c.id.as_str())
                                    .collect();
                                ui.label(names.join(", "));
                                ui.end_row();
                            }

                            if !diff.app_ids_added.is_empty() {
                                ui.label("AppIDs added:");
                                ui.label(format!("{} games", diff.app_ids_added.len()));
                                ui.end_row();
                            }

                            if !diff.app_ids_removed.is_empty() {
                                ui.label("AppIDs removed:");
                                ui.label(format!("{} games", diff.app_ids_removed.len()));
                                ui.end_row();
                            }

                            if !diff.hidden_app_ids_added.is_empty() {
                                ui.label("Hidden AppIDs added:");
                                ui.label(format!("{} games", diff.hidden_app_ids_added.len()));
                                ui.end_row();
                            }

                            ui.label("Unchanged entries:");
                            ui.label(diff.unchanged_count.to_string());
                            ui.end_row();

                            if diff.skipped_deleted_count > 0 {
                                ui.label("Skipped deleted:");
                                ui.label(diff.skipped_deleted_count.to_string());
                                ui.end_row();
                            }
                        });

                    ui.add_space(4.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 150, 50),
                        "A safety backup will be created before writing.",
                    );
                }
                // -- Backup restore (no dry-run diff) --------------------------
                else if let Some(PendingAction::BackupRestore(path)) = &self.pending_action {
                    let filename = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    ui.label(format!("Restore backup '{filename}'?"));
                    ui.colored_label(
                        egui::Color32::from_rgb(200, 150, 50),
                        "This will overwrite your current cloud storage. A safety backup will be created first.",
                    );
                }

                if self.write_loading {
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Writing...");
                    });
                } else {
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        if ui.button("✓ Confirm").clicked() {
                            self.execute_pending_action();
                        }
                        if ui.button("✕ Cancel").clicked() {
                            self.show_confirm_dialog = false;
                            self.pending_action = None;
                            self.dry_run_plan = None;
                        }
                    });
                }
            });

        self.show_confirm_dialog = open;
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

        // Write actions
        if junk_count > 0 {
            ui.separator();
            ui.strong("Actions");
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label("Collection name:");
                ui.text_edit_singleline(&mut self.junk_collection_name);
            });
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                let busy = self.write_loading || self.dry_run_loading;
                let apply_enabled = !busy && !self.junk_collection_name.is_empty();
                if ui
                    .add_enabled(apply_enabled, egui::Button::new("📁 Apply to Collection"))
                    .clicked()
                {
                    self.start_dry_run(PendingAction::JunkApply);
                }

                let hide_enabled = !busy;
                if ui
                    .add_enabled(hide_enabled, egui::Button::new("👁 Add to Hidden"))
                    .clicked()
                {
                    self.start_dry_run(PendingAction::JunkHide);
                }

                if self.write_loading {
                    ui.spinner();
                    ui.label("Writing...");
                }
                if self.dry_run_loading {
                    ui.spinner();
                    ui.label("Preparing diff...");
                }
            });
        }
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

    fn render_data_sources(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.heading("Data Sources");
        ui.separator();

        // Source credentials
        ui.strong("API Credentials");
        ui.add_space(4.0);

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
        ui.label("Set credentials via environment variables or ~/.config/vapourfly/config.toml.");
        ui.separator();

        // Cache refresh section
        ui.strong("Cache Refresh");
        ui.add_space(4.0);

        let refresh_enabled = !self.cache_refresh_loading;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(refresh_enabled, egui::Button::new("🔄 Refresh All"))
                .clicked()
            {
                self.start_cache_refresh(None, ctx);
            }
            if ui
                .add_enabled(refresh_enabled, egui::Button::new("ProtonDB"))
                .clicked()
            {
                self.start_cache_refresh(Some("protondb".into()), ctx);
            }
            if ui
                .add_enabled(refresh_enabled, egui::Button::new("PCGW"))
                .clicked()
            {
                self.start_cache_refresh(Some("pcgw".into()), ctx);
            }
            if ui
                .add_enabled(refresh_enabled, egui::Button::new("HLTB"))
                .clicked()
            {
                self.start_cache_refresh(Some("hltb".into()), ctx);
            }
            if ui
                .add_enabled(refresh_enabled, egui::Button::new("Steam Store"))
                .clicked()
            {
                self.start_cache_refresh(Some("steam-store".into()), ctx);
            }
            if self.has_igdb
                && ui
                    .add_enabled(refresh_enabled, egui::Button::new("IGDB"))
                    .clicked()
            {
                self.start_cache_refresh(Some("igdb".into()), ctx);
            }
            if self.has_rawg
                && ui
                    .add_enabled(refresh_enabled, egui::Button::new("RAWG"))
                    .clicked()
            {
                self.start_cache_refresh(Some("rawg".into()), ctx);
            }
        });

        if self.cache_refresh_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Refreshing cache...");
            });
        }
        if let Some(msg) = &self.cache_refresh_msg {
            ui.label(msg);
        }

        ui.separator();

        // Source cache status
        ui.strong("Source Cache Status");
        ui.add_space(4.0);

        if self.source_statuses.is_empty() {
            ui.label("No cache data found. Run a scan and refresh to populate cache.");
        } else {
            let text_height = egui::TextStyle::Body.resolve(ui.style()).size;
            egui_extras::TableBuilder::new(ui)
                .striped(true)
                .resizable(true)
                .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                .column(egui_extras::Column::auto().at_least(100.0))
                .column(egui_extras::Column::auto().at_least(100.0))
                .column(egui_extras::Column::auto().at_least(80.0))
                .column(egui_extras::Column::auto().at_least(60.0))
                .column(egui_extras::Column::auto().at_least(60.0))
                .header(text_height * 1.4, |mut header| {
                    header.col(|ui| {
                        ui.strong("Source");
                    });
                    header.col(|ui| {
                        ui.strong("Last Success");
                    });
                    header.col(|ui| {
                        ui.strong("Entries");
                    });
                    header.col(|ui| {
                        ui.strong("Stale");
                    });
                    header.col(|ui| {
                        ui.strong("Cached");
                    });
                })
                .body(|mut body| {
                    for status in &self.source_statuses {
                        body.row(text_height * 1.2, |mut row| {
                            row.col(|ui| {
                                ui.label(&status.name);
                            });
                            row.col(|ui| {
                                let last = status
                                    .last_success
                                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                                    .unwrap_or_else(|| "n/a".into());
                                ui.label(last);
                            });
                            row.col(|ui| {
                                ui.label(status.cache_entries.to_string());
                            });
                            row.col(|ui| {
                                ui.label(status.stale_entries.to_string());
                            });
                            row.col(|ui| {
                                ui.label(if status.cache_dir_exists {
                                    "✓"
                                } else {
                                    "—"
                                });
                            });
                        });
                    }
                });
        }

        // Offline mode
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
            match self.cloud_storage_path() {
                Ok(cloud_path) => {
                    if cloud_path.exists() {
                        match list_backups(&cloud_path) {
                            Ok(backups) => {
                                self.backups = backups;
                            }
                            Err(e) => {
                                self.error = Some(format!("Failed to list backups: {e}"));
                            }
                        }
                    }
                }
                Err(e) => {
                    self.error = Some(e);
                }
            }
        }

        if self.backups.is_empty() {
            ui.label("No backups found. Click 'Refresh Backups' to scan.");
            return;
        }

        ui.label(format!("{} backups found", self.backups.len()));
        ui.separator();

        let text_height = egui::TextStyle::Body.resolve(ui.style()).size;
        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::remainder().at_least(250.0))
            .column(egui_extras::Column::auto().at_least(140.0))
            .column(egui_extras::Column::auto().at_least(100.0))
            .column(egui_extras::Column::auto().at_least(80.0))
            .header(text_height * 1.4, |mut header| {
                header.col(|ui| {
                    ui.strong("Filename");
                });
                header.col(|ui| {
                    ui.strong("Created");
                });
                header.col(|ui| {
                    ui.strong("SHA256");
                });
                header.col(|ui| {
                    ui.strong("Action");
                });
            })
            .body(|mut body| {
                for backup in &self.backups {
                    let filename = backup
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    body.row(text_height * 1.2, |mut row| {
                        row.col(|ui| {
                            ui.label(&filename);
                        });
                        row.col(|ui| {
                            ui.label(backup.created_at.format("%Y-%m-%d %H:%M:%S").to_string());
                        });
                        row.col(|ui| {
                            ui.label(&backup.sha256[..8]);
                        });
                        row.col(|ui| {
                            let enabled = !self.write_loading;
                            if ui
                                .add_enabled(enabled, egui::Button::new("↩ Restore"))
                                .clicked()
                            {
                                self.pending_action =
                                    Some(PendingAction::BackupRestore(backup.path.clone()));
                                self.show_confirm_dialog = true;
                            }
                        });
                    });
                }
            });

        if self.write_loading {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Restoring...");
            });
        }
    }

    // -- Settings view ------------------------------------------------------

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        ui.heading("Settings");
        ui.separator();

        ui.group(|ui| {
            ui.strong("Steam Directory");
            ui.horizontal(|ui| {
                ui.label("Path:");
                ui.text_edit_singleline(&mut self.steam_dir_edit);
            });
            ui.label("Leave empty for auto-detection.");
        });
        ui.separator();

        ui.group(|ui| {
            ui.strong("Account Override");
            ui.horizontal(|ui| {
                ui.label("Account:");
                ui.text_edit_singleline(&mut self.account_edit);
            });
            ui.label("Leave empty for auto-selection (most recent).");
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
        });
        ui.separator();

        ui.group(|ui| {
            ui.strong("Backup Retention");
            ui.horizontal(|ui| {
                ui.label("Keep backups:");
                ui.text_edit_singleline(&mut self.backup_retention_edit);
            });
            ui.label("Number of rolling backups to keep for modified files.");
        });
        ui.separator();

        ui.group(|ui| {
            ui.strong("Write Safety");
            ui.checkbox(
                &mut self.allow_steam_running,
                "Allow writes while Steam is running",
            );
            ui.label("Enable with caution. Steam may overwrite changes.");
        });
        ui.separator();

        // Save button
        ui.horizontal(|ui| {
            if ui.button("💾 Save Settings").clicked() {
                self.save_settings();
            }
            if let Some(msg) = &self.settings_save_msg {
                ui.label(msg);
            }
        });
        ui.separator();

        ui.group(|ui| {
            ui.strong("About");
            ui.label(format!("Vapourfly v{}", env!("CARGO_PKG_VERSION")));
            ui.label("A local-first CLI/GUI tool for managing Steam game libraries.");
            ui.label("Licensed under MIT OR Apache-2.0.");
        });
    }

    /// Save settings to config.toml.
    fn save_settings(&mut self) {
        let config_path = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("vapourfly")
            .join("config.toml");

        // Create directory if needed
        if let Some(parent) = config_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                self.settings_save_msg = Some(format!("Failed to create config directory: {e}"));
                return;
            }
        }

        // Read existing config.toml to preserve fields we don't manage in the
        // GUI (e.g. igdb_client_id, igdb_client_secret, rawg_api_key).
        let mut table: toml::Value = std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or(toml::Value::Table(toml::map::Map::new()));

        // Ensure we have a top-level table.
        if !table.is_table() {
            table = toml::Value::Table(toml::map::Map::new());
        }

        let backup_retention = self.backup_retention_edit.parse::<u32>().ok();

        // Helper: set a string key, removing it when the value is None.
        let tbl = table.as_table_mut().unwrap();
        let set_str =
            |tbl: &mut toml::map::Map<String, toml::Value>, key: &str, val: Option<String>| {
                match val {
                    Some(s) => {
                        tbl.insert(key.to_string(), toml::Value::String(s));
                    }
                    None => {
                        tbl.remove(key);
                    }
                }
            };

        set_str(
            tbl,
            "steam_dir",
            if self.steam_dir_edit.is_empty() {
                None
            } else {
                Some(self.steam_dir_edit.clone())
            },
        );
        set_str(
            tbl,
            "account",
            if self.account_edit.is_empty() {
                None
            } else {
                Some(self.account_edit.clone())
            },
        );
        set_str(
            tbl,
            "cc",
            if self.cc_edit.is_empty() {
                None
            } else {
                Some(self.cc_edit.clone())
            },
        );
        set_str(
            tbl,
            "lang",
            if self.lang_edit.is_empty() {
                None
            } else {
                Some(self.lang_edit.clone())
            },
        );

        match backup_retention {
            Some(n) => {
                tbl.insert(
                    "backup_retention_count".to_string(),
                    toml::Value::Integer(n as i64),
                );
            }
            None => {
                tbl.remove("backup_retention_count");
            }
        }

        match toml::to_string_pretty(&table) {
            Ok(toml_str) => match std::fs::write(&config_path, toml_str) {
                Ok(()) => {
                    self.settings_save_msg = Some(format!("Saved to {}", config_path.display()));
                    // Reload config
                    self.config = VapourflyConfig::from_cli_and_env(
                        vapourfly_core::config::CliOverrides::default(),
                    )
                    .ok();
                }
                Err(e) => {
                    self.settings_save_msg = Some(format!("Failed to write config: {e}"));
                }
            },
            Err(e) => {
                self.settings_save_msg = Some(format!("Failed to serialize config: {e}"));
            }
        }
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
    use serial_test::serial;
    use tempfile::TempDir;

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

    #[test]
    fn pending_action_is_clone() {
        let a = PendingAction::JunkApply;
        let _b = a.clone();
        let c = PendingAction::BackupRestore(PathBuf::from("/tmp/test"));
        let _d = c.clone();
    }

    #[test]
    fn app_settings_fields_initialized() {
        let app = VapourflyApp::new(None);
        // cc and lang should have defaults
        assert!(!app.cc_edit.is_empty());
        assert!(!app.lang_edit.is_empty());
        assert!(!app.backup_retention_edit.is_empty());
        assert!(!app.allow_steam_running);
        assert!(app.settings_save_msg.is_none());
    }

    #[test]
    #[serial]
    fn cached_dry_run_plan_still_checks_write_safety() {
        vapourfly_core::steam::set_steam_running_override(Some(true));
        WRITE_RESULT.lock().unwrap().take();

        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("cloud-storage-namespace-1.json");
        std::fs::write(&target_path, "[]").unwrap();

        let cloud = vapourfly_core::steam::read_cloud_storage(&target_path).unwrap();
        let plan = vapourfly_core::steam::generate_write_plan(
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "junk".into(),
                added: vec![730],
                removed: vec![],
            }],
            target_path.clone(),
        )
        .unwrap();

        let mut app = VapourflyApp::new(None);
        app.pending_action = Some(PendingAction::JunkApply);
        app.dry_run_plan = Some(plan);
        app.allow_steam_running = false;
        app.execute_pending_action();

        let result = poll_write_result();
        assert!(result.unwrap_err().contains("Steam is currently running"));
        assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "[]");

        vapourfly_core::steam::set_steam_running_override(None);
    }

    fn poll_write_result() -> Result<String, String> {
        for _ in 0..100 {
            if let Some(result) = WRITE_RESULT.lock().unwrap().take() {
                return result;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("timed out waiting for write result");
    }
}
