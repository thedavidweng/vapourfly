use std::path::{Path, PathBuf};
use std::sync::Mutex;

use eframe::egui;
use egui::{Color32, RichText};
use vapourfly_core::config::VapourflyConfig;
use vapourfly_core::discover::{self, DiscoverOptions};
use vapourfly_core::disposition;
use vapourfly_core::display;
use vapourfly_core::dynamic::{self, DynamicTemplate, DynamicTemplateOptions};
use vapourfly_core::junk::{apply_junk_flags, evaluate_junk, load_default_manual_overrides};
use vapourfly_core::models::*;
use vapourfly_core::mood::{self, EditorialMood};
use vapourfly_core::playlist;
use vapourfly_core::playlist_store;
use vapourfly_core::recommend::recommend;
use vapourfly_core::share_code;
use vapourfly_core::steam::BackupInfo;
use vapourfly_core::steam::backup::list_backups;
#[cfg(test)]
use vapourfly_core::steam::scan::{ScanOptions, scan_library};
use vapourfly_core::steam::{
    SteamAccount, detect_accounts, detect_library_folders, read_cloud_storage,
    read_user_collections, redact_path, select_account,
};

// ---------------------------------------------------------------------------
// View enum
// ---------------------------------------------------------------------------

/// Top-level destinations shown in the sidebar (ADR-0006).
/// Junk and Backups are intentionally absent — they live under Library and
/// Settings respectively.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Library,
    Collections,
    Recommendations,
    Playlists,
    Discover,
    DataSources,
    Settings,
}

impl View {
    /// Canonical sidebar order. Tests lock this set and ordering.
    const ALL: &'static [View] = &[
        View::Library,
        View::Collections,
        View::Recommendations,
        View::Playlists,
        View::Discover,
        View::DataSources,
        View::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            View::Library => "Library",
            View::Collections => "Collections",
            View::Recommendations => "Recommendations",
            View::Playlists => "Playlists",
            View::Discover => "Discover",
            View::DataSources => "Data Sources",
            View::Settings => "Settings",
        }
    }
}

// ---------------------------------------------------------------------------
// Generator playlist slots (ADR-0007)
// ---------------------------------------------------------------------------

/// GUI-owned generator identity for playlist-store slots.
///
/// Core engines produce playlists; the GUI assigns a **stable playlist id**
/// per identity and overwrites that slot on regenerate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GeneratorIdentity {
    /// Single Discover slot (seed is presentation-only; id does not vary).
    Discover,
    Dynamic(DynamicTemplate),
    Mood(EditorialMood),
}

impl GeneratorIdentity {
    /// Stable, readable playlist id for this generator slot.
    fn slot_id(self) -> String {
        match self {
            Self::Discover => "discover".into(),
            Self::Dynamic(template) => format!("dynamic-{}", template.id()),
            Self::Mood(mood) => format!("mood-{}", mood.id()),
        }
    }
}

/// Assign the stable slot id and write the playlist to `store_dir`.
///
/// Returns the playlist as stored (id rewritten to the slot id). Content
/// (name, description, app ids / rules) is otherwise left unchanged.
fn put_generator_slot(
    store_dir: &Path,
    identity: GeneratorIdentity,
    mut playlist: PlaylistFile,
) -> Result<PlaylistFile, String> {
    playlist.playlist.id = identity.slot_id();
    playlist_store::put(store_dir, &playlist).map_err(|e| e.to_string())?;
    Ok(playlist)
}

// ---------------------------------------------------------------------------
// Pending action for confirmation dialog
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum PendingAction {
    JunkApply,
    JunkHide,
    RecommendCollection,
    PlaylistSync(PlaylistFile),
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
    fn label(self) -> &'static str {
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
// Visual system — dark design tokens (ADR-0006)
// ---------------------------------------------------------------------------

// Dark surface hierarchy with a cool violet brand accent.

const SURFACE: Color32 = Color32::from_rgb(18, 20, 26);
const SURFACE_RAISED: Color32 = Color32::from_rgb(26, 29, 36);
const SURFACE_MUTED: Color32 = Color32::from_rgb(34, 38, 48);
const SURFACE_SUNKEN: Color32 = Color32::from_rgb(14, 16, 22);
const BORDER: Color32 = Color32::from_rgb(52, 58, 70);
const BORDER_SOFT: Color32 = Color32::from_rgb(42, 46, 56);

const TEXT_PRIMARY: Color32 = Color32::from_rgb(236, 238, 244);
const TEXT_SECONDARY: Color32 = Color32::from_rgb(158, 166, 180);
const TEXT_MUTED: Color32 = Color32::from_rgb(110, 118, 132);
const TEXT_INVERSE: Color32 = Color32::from_rgb(18, 20, 26);

const ACCENT: Color32 = Color32::from_rgb(156, 110, 220);
const ACCENT_SOFT: Color32 = Color32::from_rgb(48, 36, 72);
const ACCENT_TEXT: Color32 = Color32::from_rgb(196, 168, 255);

const SUCCESS: Color32 = Color32::from_rgb(72, 180, 120);
const SUCCESS_SOFT: Color32 = Color32::from_rgb(28, 52, 40);
const ERROR: Color32 = Color32::from_rgb(220, 90, 90);
const ERROR_SOFT: Color32 = Color32::from_rgb(56, 28, 28);
const WARNING: Color32 = Color32::from_rgb(220, 170, 70);

// Type scale (8px grid aligned, 1.25 ratio between steps)

const TS_XS: f32 = 11.0;
const TS_SM: f32 = 12.0;
const TS_BODY: f32 = 13.5;
const TS_MD: f32 = 15.0;
const TS_LG: f32 = 18.0;
const TS_XL: f32 = 22.0;
const TS_2XL: f32 = 28.0;

// Spacing scale (4px grid) — f32 for layout, cast to i8 for Margin

const SP_1: f32 = 4.0;
const SP_2: f32 = 8.0;
const SP_3: f32 = 12.0;
const SP_4: f32 = 16.0;
const SP_6: f32 = 24.0;

// Layout constants

const SIDEBAR_WIDTH: f32 = 208.0;
const CORNER_SM: f32 = 6.0;
const CORNER_MD: f32 = 10.0;
const CORNER_LG: f32 = 14.0;
const CORNER_PILL: f32 = 20.0;

const POSTER_W: f32 = 132.0;
const POSTER_H: f32 = 180.0;
const GAME_CARD_W: f32 = 190.0;
/// Tall enough for poster + title + optional Proton/Deck badges + playtime + hover Recommend.
const GAME_CARD_H: f32 = 360.0;

/// Cast f32 spacing to i8 for egui::Margin.
const fn m(v: f32) -> i8 {
    v as i8
}

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

    /// Optional override for the playlist store directory (tests inject a temp dir).
    playlist_store_dir: Option<PathBuf>,

    // Library view
    search_query: String,
    /// When true, only installed games appear in the grid.
    filter_installed_only: bool,
    /// When true, hidden games are excluded.
    filter_not_hidden: bool,
    /// When true, junk-flagged games are excluded.
    filter_not_junk: bool,
    /// Selected game card AppID (enables Recommend without hover).
    library_selected_app_id: Option<u32>,
    /// Junk is a Library panel (not a sidebar destination).
    show_junk_panel: bool,

    // Junk panel (opened from Library)
    junk_mode: JunkModeChoice,
    junk_results: Vec<JunkDecision>,
    junk_selected: std::collections::HashSet<u32>,
    junk_collection_name: String,

    // Recommendations view
    recommend_minutes: String,
    recommend_count: String,
    recommend_seed: String,
    recommend_deck: bool,
    recommend_installed_only: bool,
    recommend_results: Vec<Recommendation>,

    // Playlists view
    playlist_import_path: String,
    playlist_export_path: String,
    playlist_share_code_input: String,
    playlist_share_code_output: Option<String>,
    playlist_edit_id: String,
    playlist_edit_name: String,
    playlist_edit_description: String,
    playlist_edit_app_ids: String,
    /// Optional JSON rules array. When non-empty, "Save Playlist" creates a
    /// rule-based playlist instead of a manual one.
    playlist_edit_rules: String,
    playlist_last_import: Option<PlaylistFile>,
    playlist_match_report: Option<PlaylistMatchReport>,
    dynamic_template: String,
    dynamic_minutes: String,
    editorial_mood: String,

    // Discover view (top-level; no longer nested under Playlists)
    discover_seed: String,
    discover_count: String,
    /// Last playlist generated from the Discover view (owned by Discover UI).
    discover_last_playlist: Option<PlaylistFile>,

    // Collections view
    collections: Vec<SteamCollection>,
    collections_export_path: String,

    // Setup / diagnostics (Settings view)
    setup_diagnostics: Option<String>,
    diagnostics_export_path: String,

    // Data Sources view
    has_igdb: bool,
    has_rawg: bool,
    source_statuses: Vec<vapourfly_api::enrichment::SourceStatus>,
    offline_mode: bool,

    // Backups (listed under Settings; not a top-level view)
    backups: Vec<BackupInfo>,

    // Settings view
    steam_dir_edit: String,
    account_edit: String,
    detected_accounts: Vec<SteamAccount>,
    account_list_msg: Option<String>,
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

fn mask_steam_id(id: &str) -> String {
    display::mask_id(id)
}

fn proton_tier_label(tier: &ProtonTier) -> &'static str {
    match tier {
        ProtonTier::Borked => "Borked",
        ProtonTier::Bronze => "Bronze",
        ProtonTier::Silver => "Silver",
        ProtonTier::Gold => "Gold",
        ProtonTier::Platinum => "Platinum",
        ProtonTier::Native => "Native",
        ProtonTier::Unknown => "Unknown",
    }
}

fn format_hltb_seconds(seconds: u32) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

fn game_metadata_summary(game: &Game) -> String {
    let mut parts = Vec::new();

    if let Some(proton) = &game.protondb {
        parts.push(proton_tier_label(&proton.tier).to_string());
    }

    if let Some(hltb) = &game.hltb {
        if let Some(seconds) = hltb.main_story_seconds {
            parts.push(format_hltb_seconds(seconds));
        }
    }

    if let Some(rating) = game
        .rawg
        .as_ref()
        .and_then(|rawg| rawg.rating_0_5)
        .or_else(|| {
            game.igdb
                .as_ref()
                .and_then(|igdb| igdb.rating_0_100)
                .map(|rating| rating / 20.0)
        })
    {
        parts.push(format!("{rating:.1}/5"));
    }

    if let Some(genre) = game
        .igdb
        .as_ref()
        .and_then(|igdb| igdb.genres.first())
        .or_else(|| game.rawg.as_ref().and_then(|rawg| rawg.genres.first()))
    {
        parts.push(genre.clone());
    }

    if parts.is_empty() {
        String::new()
    } else {
        parts.join(" | ")
    }
}

// ---------------------------------------------------------------------------
// Library filter / projection (pure helpers — ticket 03)
// ---------------------------------------------------------------------------

/// Library grid filters. Only three toggles are exposed in the UI (ADR-0006):
/// Installed only, Not hidden, Not junk. Search is free-text title/AppID.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct LibraryFilters {
    installed_only: bool,
    not_hidden: bool,
    not_junk: bool,
    search: String,
}

/// Whether a single game matches the Library filters and search query.
fn game_matches_library_filters(game: &Game, filters: &LibraryFilters) -> bool {
    if filters.installed_only && !game.installed {
        return false;
    }
    if filters.not_hidden && game.is_hidden {
        return false;
    }
    if filters.not_junk && game.is_junk {
        return false;
    }
    if !filters.search.is_empty() {
        let q = filters.search.to_lowercase();
        if !game.name.to_lowercase().contains(&q) && !game.app_id.to_string().contains(&q) {
            return false;
        }
    }
    true
}

/// Filter + sort games for the Library poster grid.
fn project_library_games(games: Vec<Game>, filters: &LibraryFilters) -> Vec<Game> {
    let mut games: Vec<Game> = games
        .into_iter()
        .filter(|g| game_matches_library_filters(g, filters))
        .collect();

    games.sort_by(|a, b| {
        b.installed
            .cmp(&a.installed)
            .then_with(|| {
                b.playtime_minutes
                    .unwrap_or(0)
                    .cmp(&a.playtime_minutes.unwrap_or(0))
            })
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });

    games
}

/// Summary counts for the Library footer/metrics region.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LibrarySummary {
    matching: usize,
    total: usize,
    installed: usize,
}

fn library_summary(all: &[Game], matching: &[Game]) -> LibrarySummary {
    LibrarySummary {
        matching: matching.len(),
        total: all.len(),
        installed: matching.iter().filter(|g| g.installed).count(),
    }
}

fn manual_playlist_app_ids_csv(pf: &PlaylistFile) -> String {
    match &pf.playlist.content {
        PlaylistContent::Manual { app_ids } => app_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(", "),
        PlaylistContent::Rules { .. } => String::new(),
    }
}

/// Render a playlist's rules as a pretty-printed JSON array string, or an
/// empty string for manual playlists. Used to populate the Rules JSON edit
/// field when loading an existing playlist.
fn playlist_rules_json(pf: &PlaylistFile) -> String {
    match &pf.playlist.content {
        PlaylistContent::Rules { rules } => serde_json::to_string_pretty(rules).unwrap_or_default(),
        PlaylistContent::Manual { .. } => String::new(),
    }
}

// ---------------------------------------------------------------------------
// Reusable UI helpers (design-token based)
// ---------------------------------------------------------------------------

fn view_header(ui: &mut egui::Ui, title: &str, subtitle: &str) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(title)
                .size(TS_2XL)
                .strong()
                .color(TEXT_PRIMARY),
        );
        ui.add_space(SP_1);
        ui.label(RichText::new(subtitle).size(TS_BODY).color(TEXT_SECONDARY));
    });
    ui.add_space(SP_3);
}

fn view_header_with_actions(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    actions: impl FnOnce(&mut egui::Ui),
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(title)
                    .size(TS_2XL)
                    .strong()
                    .color(TEXT_PRIMARY),
            );
            ui.add_space(SP_1);
            ui.label(RichText::new(subtitle).size(TS_BODY).color(TEXT_SECONDARY));
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            actions(ui);
        });
    });
    ui.add_space(SP_3);
}

fn section_card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(SURFACE_RAISED)
        .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
        .inner_margin(egui::Margin::same(m(SP_4)))
        .corner_radius(CORNER_MD)
        .show(ui, |ui| {
            ui.label(
                RichText::new(title)
                    .size(TS_MD)
                    .strong()
                    .color(TEXT_PRIMARY),
            );
            ui.add_space(SP_2);
            body(ui);
        });
    ui.add_space(SP_3);
}

fn error_banner(ui: &mut egui::Ui, msg: &str) {
    egui::Frame::NONE
        .fill(ERROR_SOFT)
        .stroke(egui::Stroke::new(1.0, ERROR))
        .inner_margin(egui::Margin::symmetric(m(SP_3), m(SP_2)))
        .corner_radius(CORNER_SM)
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("\u{26A0} {msg}"))
                    .size(TS_BODY)
                    .color(ERROR),
            );
        });
}

fn success_banner(ui: &mut egui::Ui, msg: &str) {
    egui::Frame::NONE
        .fill(SUCCESS_SOFT)
        .stroke(egui::Stroke::new(1.0, SUCCESS))
        .inner_margin(egui::Margin::symmetric(m(SP_3), m(SP_2)))
        .corner_radius(CORNER_SM)
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("\u{2713} {msg}"))
                    .size(TS_BODY)
                    .color(SUCCESS),
            );
        });
}

fn metric_pill(ui: &mut egui::Ui, label: &str, value: String) {
    egui::Frame::NONE
        .fill(SURFACE_SUNKEN)
        .inner_margin(egui::Margin::symmetric(m(SP_3), m(SP_1)))
        .corner_radius(CORNER_PILL)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(TS_XS).color(TEXT_MUTED));
                ui.label(
                    RichText::new(value)
                        .size(TS_SM)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
            });
        });
}

fn stat_inline(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(TS_SM).color(TEXT_MUTED));
        ui.label(RichText::new(value).size(TS_BODY).color(TEXT_PRIMARY));
    });
}

fn filter_toggle(ui: &mut egui::Ui, state: &mut bool, label: &str) {
    let btn = egui::Button::new(RichText::new(label).size(TS_SM).color(if *state {
        TEXT_INVERSE
    } else {
        TEXT_SECONDARY
    }))
    .fill(if *state { ACCENT } else { SURFACE })
    .stroke(if *state {
        egui::Stroke::NONE
    } else {
        egui::Stroke::new(1.0, BORDER_SOFT)
    })
    .corner_radius(CORNER_PILL);
    if ui.add(btn).clicked() {
        *state = !*state;
    }
}

fn empty_state(ui: &mut egui::Ui, icon: &str, title: &str, subtitle: &str) {
    ui.add_space(SP_6);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(icon).size(48.0).color(TEXT_MUTED));
        ui.add_space(SP_2);
        ui.label(
            RichText::new(title)
                .size(TS_LG)
                .strong()
                .color(TEXT_PRIMARY),
        );
        ui.add_space(SP_1);
        ui.label(RichText::new(subtitle).size(TS_BODY).color(TEXT_SECONDARY));
    });
    ui.add_space(SP_6);
}

fn game_image(ui: &mut egui::Ui, app_id: u32, name: &str) {
    ui.add(
        egui::Image::from_uri(steam_poster_uri(app_id))
            .fit_to_exact_size(egui::vec2(POSTER_W, POSTER_H))
            .corner_radius(CORNER_SM)
            .bg_fill(SURFACE_SUNKEN)
            .show_loading_spinner(true)
            .alt_text(format!("{name} cover")),
    );
}

fn app_id_tag(ui: &mut egui::Ui, app_id: u32) {
    egui::Frame::NONE
        .fill(SURFACE_SUNKEN)
        .inner_margin(egui::Margin::symmetric(m(SP_2), m(SP_1)))
        .corner_radius(CORNER_SM)
        .show(ui, |ui| {
            ui.label(
                RichText::new(app_id.to_string())
                    .size(TS_XS)
                    .color(TEXT_MUTED)
                    .monospace(),
            );
        });
}

fn status_badge(ui: &mut egui::Ui, label: &str, fill: Color32, text: Color32) {
    egui::Frame::NONE
        .fill(fill)
        .inner_margin(egui::Margin::symmetric(m(SP_2), m(SP_1)))
        .corner_radius(CORNER_PILL)
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(TS_XS).strong().color(text));
        });
}

fn form_field(ui: &mut egui::Ui, label: &str, field: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(TS_BODY).color(TEXT_SECONDARY));
        field(ui);
    });
}

// ---------------------------------------------------------------------------
// Shared chrome primitives (primary / secondary / ghost buttons)
// ---------------------------------------------------------------------------

fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(TS_SM).color(TEXT_INVERSE))
            .fill(ACCENT)
            .stroke(egui::Stroke::NONE)
            .corner_radius(CORNER_SM),
    )
}

fn secondary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(TS_SM).color(TEXT_PRIMARY))
            .fill(SURFACE)
            .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
            .corner_radius(CORNER_SM),
    )
}

fn ghost_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(TS_SM).color(TEXT_SECONDARY))
            .fill(Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE)
            .corner_radius(CORNER_SM),
    )
}

// ---------------------------------------------------------------------------
// Monochrome line icons for sidebar navigation
// ---------------------------------------------------------------------------

const NAV_ICON_SIZE: f32 = 16.0;

/// Draw a monochrome stroke icon for a top-level nav destination.
fn paint_nav_icon(painter: &egui::Painter, rect: egui::Rect, view: View, color: Color32) {
    let stroke = egui::Stroke::new(1.4, color);
    let c = rect.center();
    let s = rect.width().min(rect.height()) * 0.42;

    match view {
        View::Library => {
            // 2×2 grid of small squares
            let gap = s * 0.28;
            let cell = (s * 2.0 - gap) / 2.0;
            let origin = c - egui::vec2(s, s);
            for (dx, dy) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                let min = origin + egui::vec2(dx * (cell + gap), dy * (cell + gap));
                painter.rect_stroke(
                    egui::Rect::from_min_size(min, egui::vec2(cell, cell)),
                    1.0,
                    stroke,
                    egui::StrokeKind::Middle,
                );
            }
        }
        View::Collections => {
            // Folder outline
            let left = c.x - s;
            let right = c.x + s;
            let top = c.y - s * 0.55;
            let bottom = c.y + s * 0.75;
            let tab_w = s * 0.7;
            let tab_h = s * 0.35;
            let points = vec![
                egui::pos2(left, top + tab_h),
                egui::pos2(left, top),
                egui::pos2(left + tab_w, top),
                egui::pos2(left + tab_w + tab_h * 0.5, top + tab_h),
                egui::pos2(right, top + tab_h),
                egui::pos2(right, bottom),
                egui::pos2(left, bottom),
                egui::pos2(left, top + tab_h),
            ];
            painter.add(egui::Shape::closed_line(points, stroke));
        }
        View::Recommendations => {
            // Target: concentric circles + centre dot
            painter.circle_stroke(c, s, stroke);
            painter.circle_stroke(c, s * 0.55, stroke);
            painter.circle_filled(c, s * 0.18, color);
        }
        View::Playlists => {
            // Three horizontal lines (list)
            for i in 0..3 {
                let y = c.y - s * 0.7 + i as f32 * s * 0.7;
                painter.line_segment(
                    [egui::pos2(c.x - s, y), egui::pos2(c.x + s, y)],
                    stroke,
                );
            }
        }
        View::Discover => {
            // Compass: circle + diagonal needle
            painter.circle_stroke(c, s, stroke);
            painter.line_segment(
                [
                    egui::pos2(c.x - s * 0.35, c.y + s * 0.35),
                    egui::pos2(c.x + s * 0.45, c.y - s * 0.45),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    egui::pos2(c.x - s * 0.15, c.y - s * 0.15),
                    egui::pos2(c.x + s * 0.15, c.y + s * 0.15),
                ],
                stroke,
            );
        }
        View::DataSources => {
            // Stacked horizontal layers
            for i in 0..3 {
                let y = c.y - s * 0.65 + i as f32 * s * 0.65;
                let half = s * (1.0 - i as f32 * 0.08);
                painter.line_segment(
                    [egui::pos2(c.x - half, y), egui::pos2(c.x + half, y)],
                    stroke,
                );
            }
        }
        View::Settings => {
            // Simple gear: outer circle + inner circle + four spokes
            painter.circle_stroke(c, s, stroke);
            painter.circle_stroke(c, s * 0.35, stroke);
            for (dx, dy) in [(0.0, 1.0), (1.0, 0.0), (0.0, -1.0), (-1.0, 0.0)] {
                painter.line_segment(
                    [
                        c + egui::vec2(dx, dy) * s * 0.45,
                        c + egui::vec2(dx, dy) * s * 1.05,
                    ],
                    stroke,
                );
            }
        }
    }
}

/// Sidebar nav row: monochrome line icon + label, with selected accent fill.
fn nav_item(ui: &mut egui::Ui, view: View, selected: bool) -> egui::Response {
    let label = view.label();
    let row_width = SIDEBAR_WIDTH - f32::from(m(SP_3)) * 2.0 - SP_3;
    let desired = egui::vec2(row_width, 32.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered() && !selected;
        let fill = if selected {
            ACCENT
        } else if hovered {
            SURFACE_MUTED
        } else {
            Color32::TRANSPARENT
        };
        let text_color = if selected {
            TEXT_INVERSE
        } else if hovered {
            TEXT_PRIMARY
        } else {
            TEXT_SECONDARY
        };
        let icon_color = if selected {
            TEXT_INVERSE
        } else if hovered {
            TEXT_SECONDARY
        } else {
            TEXT_MUTED
        };

        let painter = ui.painter();
        painter.rect_filled(rect, CORNER_SM, fill);

        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + SP_3 + NAV_ICON_SIZE * 0.5, rect.center().y),
            egui::vec2(NAV_ICON_SIZE, NAV_ICON_SIZE),
        );
        paint_nav_icon(painter, icon_rect, view, icon_color);

        painter.text(
            egui::pos2(rect.left() + SP_3 + NAV_ICON_SIZE + SP_2, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(TS_BODY),
            text_color,
        );
    }

    response
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
            .map_or_else(|| "US".into(), |c| c.cc.clone());

        let lang_edit = config
            .as_ref()
            .map_or_else(|| "english".into(), |c| c.lang.clone());

        let backup_retention_edit = config
            .as_ref()
            .map_or_else(|| "5".into(), |c| c.backup_retention_count.to_string());

        let has_igdb = config.as_ref().is_some_and(|c| c.has_igdb_credentials);

        let has_rawg = config.as_ref().is_some_and(|c| c.has_rawg_credentials);

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
            playlist_store_dir: None,

            search_query: String::new(),
            filter_installed_only: false,
            filter_not_hidden: false,
            filter_not_junk: false,
            library_selected_app_id: None,
            show_junk_panel: false,

            junk_mode: JunkModeChoice::Default,
            junk_results: Vec::new(),
            junk_selected: std::collections::HashSet::new(),
            junk_collection_name: "junk".into(),

            recommend_minutes: "120".into(),
            recommend_count: "5".into(),
            recommend_seed: String::new(),
            recommend_deck: false,
            recommend_installed_only: false,
            recommend_results: Vec::new(),

            playlist_import_path: String::new(),
            playlist_export_path: String::new(),
            playlist_share_code_input: String::new(),
            playlist_share_code_output: None,
            playlist_edit_id: String::new(),
            playlist_edit_name: String::new(),
            playlist_edit_description: String::new(),
            playlist_edit_app_ids: String::new(),
            playlist_edit_rules: String::new(),
            playlist_last_import: None,
            playlist_match_report: None,
            dynamic_template: "deck-session".into(),
            dynamic_minutes: "90".into(),
            editorial_mood: EditorialMood::all().first().map_or("", |m| m.id()).into(),

            discover_seed: String::new(),
            discover_count: "20".into(),
            discover_last_playlist: None,

            collections: Vec::new(),
            collections_export_path: String::new(),

            setup_diagnostics: None,
            diagnostics_export_path: String::new(),

            has_igdb,
            has_rawg,
            source_statuses,
            offline_mode: false,

            backups: Vec::new(),

            steam_dir_edit,
            account_edit,
            detected_accounts: Vec::new(),
            account_list_msg: None,
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

        Ok(vapourfly_core::steam::cloud_storage_path(
            &config.steam_dir,
            &selected.steam_id64,
        ))
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
        let offline = self.offline_mode;

        std::thread::spawn(move || {
            // Use the Workflow module (ADR-0002 lazy hydration + junk classification).
            // When the user has enabled offline mode in Settings, the workflow
            // skips network fetches and uses cached metadata only.
            let opts = vapourfly_api::workflow::WorkflowOptions {
                steam_dir,
                account,
                fixtures,
                junk_mode: JunkMode::Default,
                offline,
            };
            let result = vapourfly_api::workflow::prepare(&opts);
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
            let retention = self.backup_retention();

            std::thread::spawn(move || {
                let result = vapourfly_core::write::commit_with_retention(
                    &plan,
                    allow_steam_running,
                    retention,
                )
                .map_err(|e| format!("Write failed: {e}"))
                .map(|backup| format!("Write complete. Backup: {}", backup.display()));

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
        let retention = self.backup_retention();

        std::thread::spawn(move || {
            let result = match action {
                PendingAction::JunkApply => execute_junk_apply(
                    cloud_path,
                    junk_results,
                    collection_name,
                    allow_steam_running,
                    retention,
                ),
                PendingAction::JunkHide => execute_junk_hide(
                    cloud_path,
                    junk_results,
                    allow_steam_running,
                    retention,
                ),
                PendingAction::BackupRestore(backup_path) => {
                    execute_backup_restore(backup_path, cloud_path, allow_steam_running)
                }
                PendingAction::RecommendCollection => {
                    Err("Recommendation collection writes require a dry-run plan.".into())
                }
                PendingAction::PlaylistSync(_) => {
                    Err("Playlist sync writes require a dry-run plan.".into())
                }
            };

            WRITE_RESULT.lock().unwrap().replace(result);
        });
    }

    /// Generate a dry-run WritePlan for the pending action and show the diff
    /// modal before committing to disk.
    fn start_dry_run(&mut self, action: PendingAction) {
        let action = match self.resolve_dry_run_action(action) {
            Ok(action) => action,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };

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
        let recommend_results = self.recommend_results.clone();

        std::thread::spawn(move || {
            let result = generate_dry_run_plan(
                cloud_path,
                &action,
                &junk_results,
                &collection_name,
                &recommend_results,
            );
            DRY_RUN_RESULT.lock().unwrap().replace(result);
        });
    }

    /// Start a cache refresh for the given source (or all sources).
    fn start_cache_refresh(&mut self, source: Option<String>, ctx: &egui::Context) {
        if self.cache_refresh_loading {
            return;
        }

        if self.offline_mode {
            self.cache_refresh_msg =
                Some("Offline mode is on. Cache refresh requires network access.".into());
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
                    .map(|s| (*s).to_string())
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

    fn filtered_games(&self) -> Vec<Game> {
        let games = match self.prepared_games(JunkMode::Default) {
            Some(games) => games,
            None => return Vec::new(),
        };

        let filters = LibraryFilters {
            installed_only: self.filter_installed_only,
            not_hidden: self.filter_not_hidden,
            not_junk: self.filter_not_junk,
            search: self.search_query.clone(),
        };
        project_library_games(games, &filters)
    }

    /// Reload source cache statuses from disk.
    fn reload_source_statuses(&mut self) {
        let cache_root = vapourfly_core::config::default_cache_dir();
        self.source_statuses = vapourfly_api::enrichment::source_status(&cache_root);
    }

    /// Hydrate cached external metadata and annotate junk flags for workflows.
    fn load_collections_from_cloud(&self) -> Result<Vec<SteamCollection>, String> {
        let cloud_path = self.cloud_storage_path()?;
        if !cloud_path.exists() {
            return Ok(Vec::new());
        }

        let cloud = read_cloud_storage(&cloud_path)
            .map_err(|e| format!("Failed to read cloud storage: {e}"))?;
        read_user_collections(&cloud).map_err(|e| format!("Failed to read collections: {e}"))
    }

    fn export_collections(&self) -> Result<(), String> {
        if self.collections_export_path.trim().is_empty() {
            return Err("Choose an export path before exporting.".into());
        }

        let collections = self.load_collections_from_cloud()?;
        let json = serde_json::to_string_pretty(&collections)
            .map_err(|e| format!("Failed to serialize collections: {e}"))?;
        std::fs::write(self.collections_export_path.trim(), json)
            .map_err(|e| format!("Failed to write collections export: {e}"))
    }

    fn run_setup_diagnostics(&mut self) {
        let steam_dir = self
            .config
            .as_ref()
            .map(|c| c.steam_dir.clone())
            .or_else(VapourflyConfig::detect_steam_dir);

        let mut lines = vec!["Vapourfly Setup Diagnostics".to_string()];

        match steam_dir {
            Some(dir) => {
                lines.push(format!("Steam dir: {}", redact_path(&dir)));

                let accounts = detect_accounts(&dir).unwrap_or_default();
                let selected = select_account(
                    &accounts,
                    self.config.as_ref().and_then(|c| c.account.as_deref()),
                )
                .ok();
                lines.push(format!("Accounts: {} detected", accounts.len()));
                if let Some(acc) = selected {
                    lines.push(format!(
                        "Selected: {} (***) [{}]",
                        acc.persona_name,
                        mask_steam_id(&acc.steam_id64)
                    ));

                    let cloud_path =
                        vapourfly_core::steam::cloud_storage_path(&dir, &acc.steam_id64);
                    lines.push(format!(
                        "Cloud storage: {}",
                        if cloud_path.exists() {
                            "available"
                        } else {
                            "not found"
                        }
                    ));
                } else {
                    lines.push("Cloud storage: (no account selected)".into());
                }

                let folders = detect_library_folders(&dir).unwrap_or_default();
                lines.push(format!("Libraries: {}", folders.len()));
            }
            None => {
                lines.push("Steam dir: (not detected)".into());
                lines.push("Hint: set Steam directory in Settings.".into());
            }
        }

        let cache_dir = vapourfly_core::config::default_cache_dir();
        lines.push(format!("Cache root: {}", redact_path(&cache_dir)));
        lines.push(String::new());
        lines.push("Credentials".into());
        lines.push(format!(
            "IGDB: {}",
            if self.has_igdb {
                "configured"
            } else {
                "not configured"
            }
        ));
        lines.push(format!(
            "RAWG: {}",
            if self.has_rawg {
                "configured"
            } else {
                "not configured"
            }
        ));

        if self.fixtures_path.is_some() {
            lines.push("Fixtures: enabled".into());
        }

        self.setup_diagnostics = Some(lines.join("\n"));
    }

    fn export_diagnostics(&self) -> Result<(), String> {
        if self.diagnostics_export_path.trim().is_empty() {
            return Err("Choose an export path before exporting diagnostics.".into());
        }

        let diagnostics = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "sources": {
                "IGDB": if self.has_igdb { "configured" } else { "not configured" },
                "RAWG": if self.has_rawg { "configured" } else { "not configured" },
            },
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        let json = serde_json::to_string_pretty(&diagnostics)
            .map_err(|e| format!("Failed to serialize diagnostics: {e}"))?;
        std::fs::write(self.diagnostics_export_path.trim(), json)
            .map_err(|e| format!("Failed to write diagnostics export: {e}"))
    }

    /// Backup retention for write commits: Settings edit field when valid,
    /// else resolved config, else write default. Keeps UI and write path aligned.
    fn backup_retention(&self) -> u32 {
        if let Ok(n) = self.backup_retention_edit.trim().parse::<u32>() {
            return n;
        }
        self.config
            .as_ref()
            .map(|c| c.backup_retention_count)
            .unwrap_or(vapourfly_core::write::DEFAULT_BACKUP_RETENTION)
    }

    fn prepared_games(&self, junk_mode: JunkMode) -> Option<Vec<Game>> {
        let scan = self.scan_result.as_ref()?;
        let mut games = scan.games.clone();
        // Re-hydrate from cache so that a cache refresh (which writes to disk but
        // does not update scan_result) is reflected in views without a re-scan.
        // hydrate_from_cache is idempotent: it only fills in fields that have
        // cached data, never overwriting with None.
        let cache =
            vapourfly_api::cache::DiskCache::new(vapourfly_core::config::default_cache_dir());
        vapourfly_api::enrichment::hydrate_from_cache(&mut games, &cache);
        // Same overrides as workflow::prepare so re-classification does not wipe
        // force-include / force-exclude / manual HLTB / rating.
        let overrides = load_default_manual_overrides();
        apply_junk_flags(
            &mut games,
            &JunkRules::default(),
            &junk_mode,
            &overrides,
        );
        Some(games)
    }

    fn recommend_request_from_inputs(&self) -> Result<RecommendRequest, String> {
        Ok(RecommendRequest {
            available_minutes: parse_required_u32("Available minutes", &self.recommend_minutes)?,
            count: parse_required_usize("Count", &self.recommend_count)?,
            deck_mode: self.recommend_deck,
            include_installed_only: self.recommend_installed_only,
            seed: parse_optional_u64("Seed", &self.recommend_seed)?,
            exclude_collections: vec![],
        })
    }

    fn discover_options_from_inputs(&self) -> Result<DiscoverOptions, String> {
        Ok(DiscoverOptions {
            seed_app_id: parse_optional_u32("Discover seed AppID", &self.discover_seed)?,
            count: parse_required_usize("Discover count", &self.discover_count)?,
        })
    }

    fn refresh_detected_accounts(&mut self) {
        let steam_dir = self
            .config
            .as_ref()
            .map(|c| c.steam_dir.clone())
            .or_else(VapourflyConfig::detect_steam_dir);

        let Some(steam_dir) = steam_dir else {
            self.error = Some("Steam directory not detected. Set it in Settings first.".into());
            return;
        };

        match detect_accounts(&steam_dir) {
            Ok(accounts) => {
                let count = accounts.len();
                self.detected_accounts = accounts;
                self.account_list_msg = Some(format!("{count} account(s) detected."));
            }
            Err(e) => {
                self.error = Some(format!("Failed to detect Steam accounts: {e}"));
            }
        }
    }

    fn playlist_store_path(&self) -> PathBuf {
        self.playlist_store_dir
            .clone()
            .unwrap_or_else(vapourfly_core::config::default_playlists_dir)
    }

    fn store_playlist(&self, pf: &PlaylistFile) -> Result<(), String> {
        playlist_store::put(&self.playlist_store_path(), pf).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Write a generator result to its stable playlist slot (ADR-0007).
    fn store_generator_playlist(
        &self,
        identity: GeneratorIdentity,
        playlist: PlaylistFile,
    ) -> Result<PlaylistFile, String> {
        put_generator_slot(&self.playlist_store_path(), identity, playlist)
    }

    /// Load a playlist into the Playlists edit/match surface (and last-import).
    fn adopt_playlist_for_edit(&mut self, pf: &PlaylistFile) {
        self.playlist_last_import = Some(pf.clone());
        self.playlist_edit_id = pf.playlist.id.clone();
        self.playlist_edit_name = pf.playlist.name.clone();
        self.playlist_edit_description = pf.playlist.description.clone();
        self.playlist_edit_app_ids = manual_playlist_app_ids_csv(pf);
        self.playlist_edit_rules = playlist_rules_json(pf);
        self.match_playlist_against_library(pf);
    }

    /// Build a `PlaylistFile` from the current edit fields.
    ///
    /// When `playlist_edit_rules` is non-empty, it is parsed as a JSON rules
    /// array and a rule-based playlist is produced (App IDs are ignored).
    /// Otherwise the App IDs field is parsed into a manual playlist.
    fn build_playlist_from_edit_fields(&self) -> Result<PlaylistFile, String> {
        let id = self.playlist_edit_id.trim();
        if id.is_empty() {
            return Err("Playlist ID is required.".into());
        }
        let name = self.playlist_edit_name.trim();
        if name.is_empty() {
            return Err("Playlist name is required.".into());
        }

        let content = if self.playlist_edit_rules.trim().is_empty() {
            let app_ids: Vec<u32> = self
                .playlist_edit_app_ids
                .split(',')
                .filter_map(|part| part.trim().parse().ok())
                .collect();
            PlaylistContent::Manual { app_ids }
        } else {
            let rules: Vec<PlaylistRule> = serde_json::from_str(self.playlist_edit_rules.trim())
                .map_err(|e| format!("Invalid Rules JSON: {e}"))?;
            if rules.is_empty() {
                return Err("Rules JSON must contain at least one rule.".into());
            }
            PlaylistContent::Rules { rules }
        };

        Ok(PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "user".into(),
            playlist: Playlist {
                id: id.to_string(),
                name: name.to_string(),
                description: self.playlist_edit_description.clone(),
                content,
            },
        })
    }

    fn export_loaded_playlist(&self) -> Result<(), String> {
        let pf = self
            .playlist_last_import
            .as_ref()
            .ok_or("Load or save a playlist before exporting.")?;
        if self.playlist_export_path.trim().is_empty() {
            return Err("Choose an export path before exporting.".into());
        }

        playlist::export_playlist(pf, Path::new(self.playlist_export_path.trim()))
            .map_err(|e| e.to_string())
    }

    fn resolve_dry_run_action(&self, action: PendingAction) -> Result<PendingAction, String> {
        match action {
            PendingAction::PlaylistSync(pf) => {
                if matches!(pf.playlist.content, PlaylistContent::Manual { .. }) {
                    return Ok(PendingAction::PlaylistSync(pf));
                }

                let games = self
                    .prepared_games(JunkMode::Default)
                    .ok_or("Scan your library before syncing a rule-based playlist.")?;
                let report = playlist::match_playlist(&pf, &games)
                    .map_err(|e| format!("Match failed: {e}"))?;

                Ok(PendingAction::PlaylistSync(PlaylistFile {
                    vapourfly_schema: pf.vapourfly_schema,
                    created_by: pf.created_by,
                    playlist: Playlist {
                        id: pf.playlist.id,
                        name: pf.playlist.name,
                        description: pf.playlist.description,
                        content: PlaylistContent::Manual {
                            app_ids: report.owned,
                        },
                    },
                }))
            }
            other => Ok(other),
        }
    }

    fn match_playlist_against_library(&mut self, pf: &PlaylistFile) {
        if let Some(games) = self.prepared_games(JunkMode::Default) {
            match playlist::match_playlist(pf, &games) {
                Ok(report) => self.playlist_match_report = Some(report),
                Err(e) => self.error = Some(format!("Match failed: {e}")),
            }
        }
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
    recommend_results: &[Recommendation],
) -> Result<vapourfly_core::models::WritePlan, String> {
    let cloud = vapourfly_core::steam::read_cloud_storage(&cloud_path)
        .map_err(|e| format!("Failed to read cloud storage: {e}"))?;

    let op = match action {
        PendingAction::JunkApply => {
            let junk_app_ids = disposition::junk_app_ids_from_decisions(junk_results);
            disposition::junk_apply(collection_name, junk_app_ids).map_err(|e| e.to_string())?
        }
        PendingAction::JunkHide => {
            let junk_app_ids = disposition::junk_app_ids_from_decisions(junk_results);
            disposition::junk_hide(junk_app_ids).map_err(|e| e.to_string())?
        }
        PendingAction::RecommendCollection => {
            let app_ids: Vec<u32> = recommend_results.iter().map(|r| r.app_id).collect();
            disposition::recommend_to_collection(app_ids).map_err(|e| e.to_string())?
        }
        PendingAction::PlaylistSync(pf) => {
            let app_ids = disposition::playlist_sync_app_ids(pf, None).map_err(|e| e.to_string())?;
            disposition::playlist_sync(pf, app_ids).map_err(|e| e.to_string())?
        }
        PendingAction::BackupRestore(_) => {
            return Err("Dry-run not supported for backup restore.".into());
        }
    };

    vapourfly_core::write::preview(&cloud, vec![op], cloud_path)
        .map_err(|e| format!("Failed to generate write plan: {e}"))
}

fn execute_junk_apply(
    cloud_path: PathBuf,
    junk_results: Vec<JunkDecision>,
    collection_name: String,
    allow_steam_running: bool,
    retention: u32,
) -> Result<String, String> {
    let junk_app_ids = disposition::junk_app_ids_from_decisions(&junk_results);
    if junk_app_ids.is_empty() {
        return Err("No junk candidates found.".into());
    }

    let cloud = vapourfly_core::steam::read_cloud_storage(&cloud_path)
        .map_err(|e| format!("Failed to read cloud storage: {e}"))?;

    let op = disposition::junk_apply(&collection_name, junk_app_ids.clone())
        .map_err(|e| e.to_string())?;
    let plan = vapourfly_core::write::preview(&cloud, vec![op], cloud_path.clone())
        .map_err(|e| format!("Failed to generate write plan: {e}"))?;

    let backup = vapourfly_core::write::commit_with_retention(
        &plan,
        allow_steam_running,
        retention,
    )
    .map_err(|e| format!("Write failed: {e}"))?;

    Ok(format!(
        "Applied {} junk games to collection '{}'. Backup: {}",
        junk_app_ids.len(),
        collection_name,
        backup.display()
    ))
}

fn execute_junk_hide(
    cloud_path: PathBuf,
    junk_results: Vec<JunkDecision>,
    allow_steam_running: bool,
    retention: u32,
) -> Result<String, String> {
    let junk_app_ids = disposition::junk_app_ids_from_decisions(&junk_results);
    if junk_app_ids.is_empty() {
        return Err("No junk candidates found.".into());
    }

    let cloud = vapourfly_core::steam::read_cloud_storage(&cloud_path)
        .map_err(|e| format!("Failed to read cloud storage: {e}"))?;

    let op = disposition::junk_hide(junk_app_ids.clone()).map_err(|e| e.to_string())?;
    let plan = vapourfly_core::write::preview(&cloud, vec![op], cloud_path.clone())
        .map_err(|e| format!("Failed to generate write plan: {e}"))?;

    let backup = vapourfly_core::write::commit_with_retention(
        &plan,
        allow_steam_running,
        retention,
    )
    .map_err(|e| format!("Write failed: {e}"))?;

    Ok(format!(
        "Added {} junk games to Hidden collection. Backup: {}",
        junk_app_ids.len(),
        backup.display()
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
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Poll background scan result.
        if self.loading {
            let mut guard = SCAN_RESULT.lock().unwrap();
            if let Some(result) = guard.take() {
                self.loading = false;
                match result {
                    Ok(scan) => {
                        self.scan_result = Some(scan);
                        match self.load_collections_from_cloud() {
                            Ok(collections) => self.collections = collections,
                            Err(e) => self.error = Some(e),
                        }
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
                        self.start_scan(&ctx);
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
        self.render_confirm_dialog(&ctx);

        // -- Left panel: navigation -----------------------------------------
        egui::Panel::left("nav_panel")
            .resizable(false)
            .default_size(SIDEBAR_WIDTH)
            .frame(
                egui::Frame::NONE
                    .fill(SURFACE_RAISED)
                    .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                    .inner_margin(egui::Margin::same(m(SP_3))),
            )
            .show(ui, |ui| {
                // Brand header
                ui.add_space(SP_2);
                ui.label(
                    RichText::new("Vapourfly")
                        .size(TS_XL)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.add_space(SP_1);
                ui.label(
                    RichText::new("Steam library curator")
                        .size(TS_SM)
                        .color(TEXT_MUTED),
                );
                ui.add_space(SP_4);

                // Navigation items (dark shell + monochrome line icons)
                for &view in View::ALL {
                    let selected = self.current_view == view;
                    if nav_item(ui, view, selected).clicked() {
                        self.current_view = view;
                    }
                    ui.add_space(SP_1);
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(TS_XS)
                            .color(TEXT_MUTED),
                    );
                    ui.add_space(SP_2);
                    if self.loading {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("Scanning").size(TS_SM).color(TEXT_MUTED));
                        });
                    } else if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Refresh").size(TS_SM).color(TEXT_SECONDARY),
                            )
                            .fill(SURFACE)
                            .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                            .corner_radius(CORNER_SM),
                        )
                        .clicked()
                    {
                        self.start_scan(&ctx);
                    }
                    if let Some(scan) = &self.scan_result {
                        ui.label(
                            RichText::new(format!("{} games", scan.games.len()))
                                .size(TS_XS)
                                .color(TEXT_MUTED),
                        );
                    }
                });
            });

        // -- Central panel: current view ------------------------------------
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(SURFACE)
                    .inner_margin(egui::Margin::same(m(SP_4))),
            )
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Error and success banners (clone to avoid borrow issues)
                        let mut dismiss_error = false;
                        let mut dismiss_success = false;
                        if let Some(err) = self.error.clone() {
                            ui.horizontal(|ui| {
                                error_banner(ui, &format!("Error: {err}"));
                                if ghost_button(ui, "Dismiss").clicked() {
                                    dismiss_error = true;
                                }
                            });
                            ui.add_space(SP_2);
                        }
                        if let Some(msg) = self.success_msg.clone() {
                            ui.horizontal(|ui| {
                                success_banner(ui, &msg);
                                if ghost_button(ui, "Dismiss").clicked() {
                                    dismiss_success = true;
                                }
                            });
                            ui.add_space(SP_2);
                        }
                        if dismiss_error {
                            self.error = None;
                        }
                        if dismiss_success {
                            self.success_msg = None;
                        }

                        match self.current_view {
                            View::Library => {
                                if self.show_junk_panel {
                                    self.render_junk(ui);
                                } else {
                                    self.render_library(ui, &ctx);
                                }
                            }
                            View::Collections => self.render_collections(ui),
                            View::Recommendations => self.render_recommend(ui),
                            View::Playlists => self.render_playlists(ui),
                            View::Discover => self.render_discover(ui),
                            View::DataSources => self.render_data_sources(ui, &ctx),
                            View::Settings => self.render_settings(ui),
                        }
                    });
            });

        // Kick off initial scan on first frame.
        if self.scan_result.is_none() && !self.loading && self.error.is_none() {
            self.start_scan(&ctx);
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
            .frame(
                egui::Frame::NONE
                    .fill(SURFACE_RAISED)
                    .stroke(egui::Stroke::new(1.0, BORDER))
                    .corner_radius(CORNER_LG)
                    .inner_margin(egui::Margin::same(m(SP_4))),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Confirm Action")
                        .size(TS_XL)
                        .strong()
                        .color(TEXT_PRIMARY),
                );
                ui.add_space(SP_3);

                // -- Dry-run diff (junk apply / hide) --------------------------
                if let Some(plan) = &self.dry_run_plan {
                    let diff = &plan.diff;

                    ui.label(
                        RichText::new("Dry-Run Diff")
                            .size(TS_MD)
                            .strong()
                            .color(TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new(format!("Target: {}", plan.target_path.display()))
                            .size(TS_BODY)
                            .color(TEXT_SECONDARY),
                    );
                    ui.add_space(SP_1);

                    egui::Grid::new("dry_run_diff_grid")
                        .num_columns(2)
                        .spacing([SP_3, SP_1])
                        .show(ui, |ui| {
                            if !diff.collections_changed.is_empty() {
                                ui.label(
                                    RichText::new("Collections changed:")
                                        .size(TS_SM)
                                        .color(TEXT_MUTED),
                                );
                                let names: Vec<&str> = diff
                                    .collections_changed
                                    .iter()
                                    .map(|c| c.id.as_str())
                                    .collect();
                                ui.label(
                                    RichText::new(names.join(", "))
                                        .size(TS_BODY)
                                        .color(TEXT_PRIMARY),
                                );
                                ui.end_row();
                            }

                            if !diff.app_ids_added.is_empty() {
                                ui.label(
                                    RichText::new("AppIDs added:")
                                        .size(TS_SM)
                                        .color(TEXT_MUTED),
                                );
                                ui.label(
                                    RichText::new(format!("{} games", diff.app_ids_added.len()))
                                        .size(TS_BODY)
                                        .color(TEXT_PRIMARY),
                                );
                                ui.end_row();
                            }

                            if !diff.app_ids_removed.is_empty() {
                                ui.label(
                                    RichText::new("AppIDs removed:")
                                        .size(TS_SM)
                                        .color(TEXT_MUTED),
                                );
                                ui.label(
                                    RichText::new(format!("{} games", diff.app_ids_removed.len()))
                                        .size(TS_BODY)
                                        .color(TEXT_PRIMARY),
                                );
                                ui.end_row();
                            }

                            if !diff.hidden_app_ids_added.is_empty() {
                                ui.label(
                                    RichText::new("Hidden AppIDs added:")
                                        .size(TS_SM)
                                        .color(TEXT_MUTED),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{} games",
                                        diff.hidden_app_ids_added.len()
                                    ))
                                    .size(TS_BODY)
                                    .color(TEXT_PRIMARY),
                                );
                                ui.end_row();
                            }

                            ui.label(
                                RichText::new("Unchanged entries:")
                                    .size(TS_SM)
                                    .color(TEXT_MUTED),
                            );
                            ui.label(
                                RichText::new(diff.unchanged_count.to_string())
                                    .size(TS_BODY)
                                    .color(TEXT_PRIMARY),
                            );
                            ui.end_row();

                            if diff.skipped_deleted_count > 0 {
                                ui.label(
                                    RichText::new("Skipped deleted:")
                                        .size(TS_SM)
                                        .color(TEXT_MUTED),
                                );
                                ui.label(
                                    RichText::new(diff.skipped_deleted_count.to_string())
                                        .size(TS_BODY)
                                        .color(TEXT_PRIMARY),
                                );
                                ui.end_row();
                            }
                        });

                    ui.add_space(SP_2);
                    ui.label(
                        RichText::new("\u{26A0} A safety backup will be created before writing.")
                            .size(TS_BODY)
                            .color(WARNING),
                    );
                }
                // -- Backup restore (no dry-run diff) --------------------------
                else if let Some(PendingAction::BackupRestore(path)) = &self.pending_action {
                    let filename = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    ui.label(
                        RichText::new(format!("Restore backup '{filename}'?"))
                            .size(TS_BODY)
                            .color(TEXT_PRIMARY),
                    );
                    ui.add_space(SP_2);
                    ui.label(
                        RichText::new(
                            "\u{26A0} This will overwrite your current cloud storage. A safety backup will be created first.",
                        )
                        .size(TS_BODY)
                        .color(WARNING),
                    );
                }

                if self.write_loading {
                    ui.add_space(SP_2);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Writing").size(TS_BODY).color(TEXT_SECONDARY));
                    });
                } else {
                    ui.add_space(SP_3);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Confirm")
                                        .size(TS_BODY)
                                        .color(TEXT_INVERSE),
                                )
                                .fill(ACCENT)
                                .stroke(egui::Stroke::NONE)
                                .corner_radius(CORNER_SM),
                            )
                            .clicked()
                        {
                            self.execute_pending_action();
                        }
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Cancel")
                                        .size(TS_BODY)
                                        .color(TEXT_PRIMARY),
                                )
                                .fill(SURFACE)
                                .stroke(egui::Stroke::new(1.0, BORDER))
                                .corner_radius(CORNER_SM),
                            )
                            .clicked()
                        {
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

    fn render_library(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let total_games = self.scan_result.as_ref().map_or(0, |scan| scan.games.len());
        let games = self.filtered_games();
        let summary = library_summary(
            self.scan_result
                .as_ref()
                .map(|s| s.games.as_slice())
                .unwrap_or(&[]),
            &games,
        );
        // Keep totals consistent when scan is empty vs unprepared filters.
        let summary = LibrarySummary {
            matching: games.len(),
            total: total_games,
            installed: summary.installed,
        };

        view_header_with_actions(
            ui,
            "Library",
            "Browse your Steam games visually, then turn that library into clean playlists and recommendations.",
            |ui| {
                // right_to_left: first widget is rightmost
                if ui
                    .add_enabled(
                        !self.loading,
                        egui::Button::new(
                            RichText::new("Refresh").size(TS_SM).color(TEXT_INVERSE),
                        )
                        .fill(ACCENT)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(CORNER_SM),
                    )
                    .clicked()
                {
                    self.start_scan(ctx);
                }
                if secondary_button(ui, "Junk\u{2026}").clicked() {
                    self.show_junk_panel = true;
                }
                if self.loading {
                    ui.spinner();
                    ui.label(RichText::new("Scanning").size(TS_SM).color(TEXT_SECONDARY));
                }
            },
        );

        // Summary region: matching count + installed among matches + full library size.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
            metric_pill(ui, "Matching", summary.matching.to_string());
            metric_pill(ui, "Installed", summary.installed.to_string());
            metric_pill(ui, "Library", summary.total.to_string());
        });

        ui.add_space(SP_3);

        section_card(ui, "Search & Filter", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                ui.label(RichText::new("Search").size(TS_SM).color(TEXT_SECONDARY));
                ui.add_sized(
                    [320.0, 30.0],
                    egui::TextEdit::singleline(&mut self.search_query).hint_text("Title or AppID"),
                );
                ui.separator();
                // Exactly three filters (ticket 03 / ADR-0006).
                filter_toggle(ui, &mut self.filter_installed_only, "Installed only");
                filter_toggle(ui, &mut self.filter_not_hidden, "Not hidden");
                filter_toggle(ui, &mut self.filter_not_junk, "Not junk");
            });
        });

        if self.scan_result.is_none() {
            if self.loading {
                empty_state(
                    ui,
                    "\u{23F3}",
                    "Scanning your Steam library",
                    "Covers and metadata will appear here as soon as the scan finishes.",
                );
            } else {
                empty_state(
                    ui,
                    "\u{1F3AE}",
                    "No library loaded",
                    "Refresh the library or set your Steam directory in Settings.",
                );
            }
            return;
        }

        if games.is_empty() {
            empty_state(
                ui,
                "\u{1F50D}",
                "No games match these filters",
                "Clear search or turn off a filter to bring games back.",
            );
            return;
        }

        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
            |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_3);
                for game in &games {
                    self.render_game_card(ui, game);
                }
            },
        );
    }

    fn render_game_card(&mut self, ui: &mut egui::Ui, game: &Game) {
        let is_selected = self.library_selected_app_id == Some(game.app_id);
        let hover_id = egui::Id::new(("library_card_hover", game.app_id));
        let was_hovered = ui
            .ctx()
            .data(|d| d.get_temp::<bool>(hover_id).unwrap_or(false));
        // Approved deviation: Recommend is revealed on hover or selection.
        let show_recommend = was_hovered || is_selected;
        let border = if is_selected {
            egui::Stroke::new(1.5, ACCENT)
        } else {
            egui::Stroke::new(1.0, BORDER_SOFT)
        };

        let response = ui
            .allocate_ui_with_layout(
                egui::vec2(GAME_CARD_W, GAME_CARD_H),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.set_width(GAME_CARD_W);
                    ui.set_height(GAME_CARD_H);

                    egui::Frame::NONE
                        .fill(SURFACE_RAISED)
                        .stroke(border)
                        .inner_margin(egui::Margin::same(m(SP_3)))
                        .corner_radius(CORNER_MD)
                        .show(ui, |ui| {
                            ui.set_width(GAME_CARD_W - f32::from(m(SP_3)) * 2.0);
                            ui.set_height(GAME_CARD_H - f32::from(m(SP_3)) * 2.0);

                            ui.horizontal(|ui| {
                                app_id_tag(ui, game.app_id);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let (label, fill, text) = game_primary_badge(game);
                                        status_badge(ui, label, fill, text);
                                    },
                                );
                            });

                            ui.add_space(SP_2);
                            ui.vertical_centered(|ui| {
                                game_image(ui, game.app_id, &game.name);
                            });

                            ui.add_space(SP_2);
                            ui.add_sized(
                                [GAME_CARD_W - f32::from(m(SP_3)) * 2.0, 36.0],
                                egui::Label::new(
                                    RichText::new(&game.name)
                                        .size(TS_MD)
                                        .strong()
                                        .color(TEXT_PRIMARY),
                                )
                                .wrap(),
                            );

                            // Secondary badges: Proton tier / Deck when hydrated.
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(SP_1, SP_1);
                                if let Some(proton) = &game.protondb {
                                    status_badge(
                                        ui,
                                        proton_tier_label(&proton.tier),
                                        ACCENT_SOFT,
                                        ACCENT_TEXT,
                                    );
                                }
                                if game_shows_deck_badge(game) {
                                    status_badge(ui, "Deck", SUCCESS_SOFT, SUCCESS);
                                }
                            });

                            ui.add_sized(
                                [GAME_CARD_W - f32::from(m(SP_3)) * 2.0, 24.0],
                                egui::Label::new(
                                    RichText::new(game_card_detail(game))
                                        .size(TS_SM)
                                        .color(TEXT_SECONDARY),
                                )
                                .wrap(),
                            );

                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format_playtime(
                                        game.playtime_minutes.unwrap_or(0),
                                    ))
                                    .size(TS_SM)
                                    .color(TEXT_MUTED),
                                );
                                if show_recommend {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new("Recommend")
                                                            .size(TS_SM)
                                                            .color(ACCENT_TEXT),
                                                    )
                                                    .fill(ACCENT_SOFT)
                                                    .stroke(egui::Stroke::NONE)
                                                    .corner_radius(CORNER_SM),
                                                )
                                                .clicked()
                                            {
                                                self.recommend_seed = game.app_id.to_string();
                                                self.current_view = View::Recommendations;
                                            }
                                        },
                                    );
                                }
                            });
                        });
                },
            )
            .response;

        if response.clicked() {
            self.library_selected_app_id = Some(game.app_id);
        }
        ui.ctx()
            .data_mut(|d| d.insert_temp(hover_id, response.hovered()));
    }

    // -- Junk panel (opened from Library toolbar) ---------------------------

    fn render_junk(&mut self, ui: &mut egui::Ui) {
        view_header_with_actions(
            ui,
            "Junk",
            "Preview candidates with explainable signals, then apply to a collection or hide after dry-run confirmation.",
            |ui| {
                if secondary_button(ui, "Back to Library").clicked() {
                    self.show_junk_panel = false;
                }
            },
        );

        section_card(ui, "Mode", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                ui.label(RichText::new("Detection").size(TS_SM).color(TEXT_SECONDARY));
                for mode in &[
                    JunkModeChoice::Default,
                    JunkModeChoice::Strict,
                    JunkModeChoice::Aggressive,
                ] {
                    let selected = self.junk_mode == *mode;
                    let btn = egui::Button::new(RichText::new(mode.label()).size(TS_SM).color(
                        if selected {
                            TEXT_INVERSE
                        } else {
                            TEXT_SECONDARY
                        },
                    ))
                    .fill(if selected { ACCENT } else { SURFACE })
                    .stroke(if selected {
                        egui::Stroke::NONE
                    } else {
                        egui::Stroke::new(1.0, BORDER_SOFT)
                    })
                    .corner_radius(CORNER_PILL);
                    if ui.add(btn).clicked() {
                        self.junk_mode = *mode;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if primary_button(ui, "Preview").clicked() {
                        let mode = match self.junk_mode {
                            JunkModeChoice::Default => JunkMode::Default,
                            JunkModeChoice::Strict => JunkMode::Strict,
                            JunkModeChoice::Aggressive => JunkMode::Aggressive,
                        };
                        if let Some(games) = self.prepared_games(mode.clone()) {
                            let overrides = load_default_manual_overrides();
                            self.junk_results = evaluate_junk(
                                &games,
                                &JunkRules::default(),
                                &mode,
                                &overrides,
                            );
                            self.junk_selected.clear();
                        }
                    }
                });
            });
        });

        if self.junk_results.is_empty() {
            empty_state(
                ui,
                "\u{1F9F9}",
                "No junk preview yet",
                "Choose Default, Strict, or Aggressive, then click Preview.",
            );
            return;
        }

        let junk_count = self.junk_results.iter().filter(|d| d.is_junk).count();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
            metric_pill(ui, "Candidates", junk_count.to_string());
            metric_pill(ui, "Evaluated", self.junk_results.len().to_string());
        });
        ui.add_space(SP_3);

        // Preview: candidate cards with confidence + signals (not only a table).
        section_card(ui, "Preview", |ui| {
            let text_height = TS_BODY;
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
                        ui.label(
                            RichText::new("ID")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                    });
                    header.col(|ui| {
                        ui.label(
                            RichText::new("Name")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                    });
                    header.col(|ui| {
                        ui.label(
                            RichText::new("Junk?")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                    });
                    header.col(|ui| {
                        ui.label(
                            RichText::new("Confidence")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                    });
                    header.col(|ui| {
                        ui.label(
                            RichText::new("Signals")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                    });
                })
                .body(|mut body| {
                    for decision in &self.junk_results {
                        body.row(text_height * 1.2, |mut row| {
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(decision.app_id.to_string())
                                        .size(TS_BODY)
                                        .color(TEXT_PRIMARY),
                                );
                            });
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(&decision.name)
                                        .size(TS_BODY)
                                        .color(TEXT_PRIMARY),
                                );
                            });
                            row.col(|ui| {
                                if decision.is_junk {
                                    status_badge(ui, "Yes", ERROR_SOFT, ERROR);
                                } else {
                                    status_badge(ui, "No", SURFACE_SUNKEN, TEXT_MUTED);
                                }
                            });
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(format!("{:.0}%", decision.confidence * 100.0))
                                        .size(TS_BODY)
                                        .color(TEXT_PRIMARY),
                                );
                            });
                            row.col(|ui| {
                                let signals: Vec<String> =
                                    decision.matched.iter().map(format_junk_signal).collect();
                                ui.label(
                                    RichText::new(if signals.is_empty() {
                                        empty_value_label().to_string()
                                    } else {
                                        signals.join(", ")
                                    })
                                    .size(TS_BODY)
                                    .color(TEXT_SECONDARY),
                                );
                            });
                        });
                    }
                });
        });

        // Write actions — still dry-run/confirm gated.
        if junk_count > 0 {
            ui.add_space(SP_3);
            section_card(ui, "Actions", |ui| {
                form_field(ui, "Collection name", |ui| {
                    ui.add_sized(
                        [220.0, 28.0],
                        egui::TextEdit::singleline(&mut self.junk_collection_name)
                            .hint_text("e.g. junk"),
                    );
                });
                ui.add_space(SP_2);

                ui.horizontal(|ui| {
                    let busy = self.write_loading || self.dry_run_loading;
                    let apply_enabled = !busy && !self.junk_collection_name.is_empty();
                    if ui
                        .add_enabled(apply_enabled, {
                            egui::Button::new(
                                RichText::new("Apply to collection")
                                    .size(TS_SM)
                                    .color(TEXT_INVERSE),
                            )
                            .fill(ACCENT)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(CORNER_SM)
                        })
                        .clicked()
                    {
                        self.start_dry_run(PendingAction::JunkApply);
                    }

                    if ui
                        .add_enabled(!busy, {
                            egui::Button::new(
                                RichText::new("Hide")
                                    .size(TS_SM)
                                    .color(TEXT_PRIMARY),
                            )
                            .fill(SURFACE)
                            .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                            .corner_radius(CORNER_SM)
                        })
                        .clicked()
                    {
                        self.start_dry_run(PendingAction::JunkHide);
                    }

                    if self.write_loading {
                        ui.spinner();
                        ui.label(RichText::new("Writing").size(TS_SM).color(TEXT_SECONDARY));
                    }
                    if self.dry_run_loading {
                        ui.spinner();
                        ui.label(
                            RichText::new("Preparing diff")
                                .size(TS_SM)
                                .color(TEXT_SECONDARY),
                        );
                    }
                });
                ui.add_space(SP_1);
                ui.label(
                    RichText::new("Writes require dry-run confirmation and respect write safety.")
                        .size(TS_XS)
                        .color(TEXT_MUTED),
                );
            });
        }
    }

    // -- Recommend view -----------------------------------------------------

    fn render_recommend(&mut self, ui: &mut egui::Ui) {
        view_header(
            ui,
            "Recommendations",
            "Shape a play session, preview scored picks, then write them to vapourfly-picks after confirmation.",
        );

        section_card(ui, "Session", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_2);
                form_field(ui, "Available minutes", |ui| {
                    ui.add_sized(
                        [88.0, 28.0],
                        egui::TextEdit::singleline(&mut self.recommend_minutes)
                            .hint_text("120"),
                    );
                });
                form_field(ui, "Count", |ui| {
                    ui.add_sized(
                        [64.0, 28.0],
                        egui::TextEdit::singleline(&mut self.recommend_count).hint_text("5"),
                    );
                });
                form_field(ui, "Seed AppID", |ui| {
                    ui.add_sized(
                        [100.0, 28.0],
                        egui::TextEdit::singleline(&mut self.recommend_seed)
                            .hint_text("optional"),
                    );
                });
            });
            ui.add_space(SP_2);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                filter_toggle(ui, &mut self.recommend_installed_only, "Installed only");
                filter_toggle(ui, &mut self.recommend_deck, "Deck mode");
                if primary_button(ui, "Preview").clicked() {
                    match self.recommend_request_from_inputs() {
                        Ok(request) => {
                            if let Some(games) = self.prepared_games(JunkMode::Default) {
                                self.recommend_results = recommend(&games, &request);
                            }
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
            });
        });

        if self.recommend_results.is_empty() {
            empty_state(
                ui,
                "\u{1F3AF}",
                "No recommendations yet",
                "Set minutes and count, then click Preview. Seed can be filled from Library Recommend.",
            );
            return;
        }

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
            metric_pill(ui, "Results", self.recommend_results.len().to_string());
        });
        ui.add_space(SP_3);

        let busy = self.write_loading || self.dry_run_loading;
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !busy,
                    egui::Button::new(
                        RichText::new("Write to vapourfly-picks")
                            .size(TS_SM)
                            .color(TEXT_INVERSE),
                    )
                    .fill(ACCENT)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(CORNER_SM),
                )
                .clicked()
            {
                self.start_dry_run(PendingAction::RecommendCollection);
            }
            if self.write_loading || self.dry_run_loading {
                ui.spinner();
                ui.label(
                    RichText::new(if self.dry_run_loading {
                        "Preparing diff"
                    } else {
                        "Writing"
                    })
                    .size(TS_SM)
                    .color(TEXT_SECONDARY),
                );
            }
        });
        ui.label(
            RichText::new("Requires dry-run confirmation. Targets the vapourfly-picks Steam Collection.")
                .size(TS_XS)
                .color(TEXT_MUTED),
        );
        ui.add_space(SP_3);

        // Preview list: score + human-readable reason codes.
        for rec in &self.recommend_results {
            section_card(ui, &rec.name, |ui| {
                ui.horizontal(|ui| {
                    app_id_tag(ui, rec.app_id);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        status_badge(
                            ui,
                            &format!("{:.2}", rec.score),
                            ACCENT_SOFT,
                            ACCENT_TEXT,
                        );
                    });
                });
                if !rec.reasons.is_empty() {
                    ui.add_space(SP_1);
                    ui.indent("rec_reasons", |ui| {
                        for reason in &rec.reasons {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(SP_1, SP_1);
                                status_badge(ui, &reason.code, SURFACE_MUTED, TEXT_SECONDARY);
                                ui.label(
                                    RichText::new(format!(
                                        "{} ({:+.1})",
                                        reason.description, reason.weight
                                    ))
                                    .size(TS_SM)
                                    .color(TEXT_SECONDARY),
                                );
                            });
                        }
                    });
                }
            });
        }
    }

    // -- Playlists view -----------------------------------------------------

    fn render_playlists(&mut self, ui: &mut egui::Ui) {
        view_header(
            ui,
            "Playlists",
            "Create, import, and sync curated game playlists to your Steam collections.",
        );

        section_card(ui, "Create / Edit Playlist", |ui| {
            form_field(ui, "ID:", |ui| {
                ui.add_sized(
                    [200.0, 20.0],
                    egui::TextEdit::singleline(&mut self.playlist_edit_id),
                );
            });
            form_field(ui, "Name:", |ui| {
                ui.add_sized(
                    [200.0, 20.0],
                    egui::TextEdit::singleline(&mut self.playlist_edit_name),
                );
            });
            form_field(ui, "Description:", |ui| {
                ui.add_sized(
                    [300.0, 20.0],
                    egui::TextEdit::singleline(&mut self.playlist_edit_description),
                );
            });
            form_field(ui, "App IDs:", |ui| {
                ui.add_sized(
                    [300.0, 20.0],
                    egui::TextEdit::singleline(&mut self.playlist_edit_app_ids),
                );
            });
            ui.label(
                RichText::new("Comma-separated Steam AppIDs for manual playlists.")
                    .size(TS_SM)
                    .color(TEXT_MUTED),
            );
            ui.add_space(SP_2);
            form_field(ui, "Rules JSON:", |ui| {
                ui.add_sized(
                    [360.0, 80.0],
                    egui::TextEdit::multiline(&mut self.playlist_edit_rules)
                        .code_editor()
                        .desired_width(360.0),
                );
            });
            ui.label(
                RichText::new(
                    "Optional. A JSON rules array (e.g. \
                     `[{\"op\":\"Installed\"},{\"op\":\"NotHidden\"}]`). When \
                     provided, App IDs are ignored and a rule-based playlist is \
                     created.",
                )
                .size(TS_SM)
                .color(TEXT_MUTED),
            );
            ui.add_space(SP_2);
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Save Playlist")
                            .size(TS_SM)
                            .color(TEXT_INVERSE),
                    )
                    .fill(ACCENT)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(CORNER_SM),
                )
                .clicked()
            {
                match self.build_playlist_from_edit_fields() {
                    Ok(pf) => match self.store_playlist(&pf) {
                        Ok(()) => {
                            self.playlist_last_import = Some(pf.clone());
                            self.match_playlist_against_library(&pf);
                            self.success_msg =
                                Some(format!("Saved playlist '{}'", pf.playlist.name));
                        }
                        Err(e) => self.error = Some(e),
                    },
                    Err(e) => {
                        self.error = Some(e);
                    }
                }
            }
        });

        section_card(ui, "Import Playlist", |ui| {
            form_field(ui, "Path:", |ui| {
                ui.add_sized(
                    [300.0, 20.0],
                    egui::TextEdit::singleline(&mut self.playlist_import_path),
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Import File").size(TS_SM).color(TEXT_PRIMARY),
                        )
                        .fill(SURFACE)
                        .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                        .corner_radius(CORNER_SM),
                    )
                    .clicked()
                    && !self.playlist_import_path.is_empty()
                {
                    match playlist::import_playlist(Path::new(&self.playlist_import_path)) {
                        Ok(pf) => {
                            if let Err(e) = self.store_playlist(&pf) {
                                self.error = Some(e);
                            } else {
                                self.playlist_last_import = Some(pf.clone());
                                self.match_playlist_against_library(&pf);
                                self.playlist_edit_id = pf.playlist.id.clone();
                                self.playlist_edit_name = pf.playlist.name.clone();
                                self.playlist_edit_description = pf.playlist.description.clone();
                                self.playlist_edit_app_ids = manual_playlist_app_ids_csv(&pf);
                                self.playlist_edit_rules = playlist_rules_json(&pf);
                            }
                        }
                        Err(e) => self.error = Some(format!("Import failed: {e}")),
                    }
                }
            });
            form_field(ui, "Share code:", |ui| {
                ui.add_sized(
                    [300.0, 20.0],
                    egui::TextEdit::singleline(&mut self.playlist_share_code_input),
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Import Code").size(TS_SM).color(TEXT_PRIMARY),
                        )
                        .fill(SURFACE)
                        .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                        .corner_radius(CORNER_SM),
                    )
                    .clicked()
                    && !self.playlist_share_code_input.is_empty()
                {
                    match share_code::decode_share_code(&self.playlist_share_code_input) {
                        Ok(pf) => {
                            if let Err(e) = self.store_playlist(&pf) {
                                self.error = Some(e);
                            } else {
                                self.playlist_last_import = Some(pf.clone());
                                self.match_playlist_against_library(&pf);
                                self.playlist_edit_id = pf.playlist.id.clone();
                                self.playlist_edit_name = pf.playlist.name.clone();
                                self.playlist_edit_description = pf.playlist.description.clone();
                                self.playlist_edit_app_ids = manual_playlist_app_ids_csv(&pf);
                                self.playlist_edit_rules = playlist_rules_json(&pf);
                            }
                        }
                        Err(e) => self.error = Some(format!("Share code import failed: {e}")),
                    }
                }
            });
        });

        // Dynamic templates stay on Playlists; Discover is its own top-level view.
        section_card(ui, "Dynamic Templates", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_1);
                ui.label(RichText::new("Template:").size(TS_SM).color(TEXT_SECONDARY));
                ui.add_sized(
                    [120.0, 20.0],
                    egui::TextEdit::singleline(&mut self.dynamic_template),
                );
                ui.label(RichText::new("Minutes:").size(TS_SM).color(TEXT_SECONDARY));
                ui.add_sized(
                    [60.0, 20.0],
                    egui::TextEdit::singleline(&mut self.dynamic_minutes),
                );
            });
            ui.add_space(SP_2);
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Compile Dynamic Template")
                            .size(TS_SM)
                            .color(TEXT_INVERSE),
                    )
                    .fill(ACCENT)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(CORNER_SM),
                )
                .clicked()
            {
                if let Some(template) = DynamicTemplate::parse(&self.dynamic_template) {
                    if let Some(games) = self.prepared_games(JunkMode::Default) {
                        let pf = dynamic::compile_dynamic_template(
                            template,
                            &games,
                            &DynamicTemplateOptions {
                                session_minutes: self.dynamic_minutes.parse().unwrap_or(90),
                                count: 25,
                            },
                        );
                        match self.store_generator_playlist(
                            GeneratorIdentity::Dynamic(template),
                            pf,
                        ) {
                            Ok(pf) => {
                                self.adopt_playlist_for_edit(&pf);
                                self.success_msg = Some(format!(
                                    "Compiled dynamic template '{}' (slot {})",
                                    pf.playlist.name, pf.playlist.id
                                ));
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                } else {
                    self.error = Some("Unknown template. Use deck-session or finish-it.".into());
                }
            }
        });

        // Editorial Moods (ADR-0004): named, curated playlists with hidden
        // selection criteria. The user picks a vibe; Vapourfly compiles the
        // playlist. Criteria are opaque to the user.
        section_card(ui, "Editorial Moods", |ui| {
            ui.label(
                RichText::new(
                    "Named, curated playlists with hidden selection criteria — \
                     pick a vibe and Vapourfly does the rest.",
                )
                .size(TS_SM)
                .color(TEXT_MUTED),
            );
            ui.add_space(SP_2);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_1);
                ui.label(RichText::new("Mood:").size(TS_SM).color(TEXT_SECONDARY));
                egui::ComboBox::from_id_salt("editorial_mood_combo")
                    .selected_text(
                        EditorialMood::parse(&self.editorial_mood)
                            .map_or("Select mood", |m| m.name()),
                    )
                    .width(180.0)
                    .show_ui(ui, |ui| {
                        for m in EditorialMood::all() {
                            ui.selectable_value(&mut self.editorial_mood, m.id().into(), m.name());
                        }
                    });
            });
            ui.add_space(SP_2);
            if let Some(mood) = EditorialMood::parse(&self.editorial_mood) {
                ui.label(
                    RichText::new(mood.description())
                        .size(TS_SM)
                        .color(TEXT_SECONDARY),
                );
                ui.add_space(SP_2);
            }
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Compile Editorial Mood")
                            .size(TS_SM)
                            .color(TEXT_INVERSE),
                    )
                    .fill(ACCENT)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(CORNER_SM),
                )
                .clicked()
            {
                if let Some(mood) = EditorialMood::parse(&self.editorial_mood) {
                    if let Some(games) = self.prepared_games(JunkMode::Default) {
                        let pf = mood::compile_editorial_mood(mood, &games, 25);
                        match self
                            .store_generator_playlist(GeneratorIdentity::Mood(mood), pf)
                        {
                            Ok(pf) => {
                                self.adopt_playlist_for_edit(&pf);
                                self.success_msg = Some(format!(
                                    "Compiled editorial mood '{}' (slot {})",
                                    pf.playlist.name, pf.playlist.id
                                ));
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                } else {
                    self.error = Some("Unknown mood. Pick one from the dropdown.".into());
                }
            }
        });

        // Show imported playlist info
        if let Some(pf) = self.playlist_last_import.clone() {
            section_card(ui, &format!("Playlist: {}", pf.playlist.name), |ui| {
                stat_inline(ui, "ID:", &pf.playlist.id);
                stat_inline(ui, "Description:", &pf.playlist.description);
                stat_inline(ui, "Schema:", &pf.vapourfly_schema);

                match &pf.playlist.content {
                    PlaylistContent::Manual { app_ids } => {
                        stat_inline(
                            ui,
                            "Type:",
                            &format!("Manual playlist with {} AppIDs", app_ids.len()),
                        );
                    }
                    PlaylistContent::Rules { rules } => {
                        stat_inline(
                            ui,
                            "Type:",
                            &format!("Rule-based playlist with {} rules", rules.len()),
                        );
                    }
                }

                ui.add_space(SP_2);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Copy Share Code")
                                .size(TS_SM)
                                .color(TEXT_PRIMARY),
                        )
                        .fill(SURFACE)
                        .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                        .corner_radius(CORNER_SM),
                    )
                    .clicked()
                {
                    match share_code::encode_share_code(&pf) {
                        Ok(code) => {
                            self.playlist_share_code_output = Some(code.clone());
                            ui.ctx().copy_text(code);
                            self.success_msg = Some("Share code copied to clipboard.".into());
                        }
                        Err(e) => self.error = Some(format!("Share code failed: {e}")),
                    }
                }
                if let Some(code) = &self.playlist_share_code_output {
                    ui.label(
                        RichText::new(format!("Share code: {code}"))
                            .size(TS_SM)
                            .color(TEXT_MUTED)
                            .monospace(),
                    );
                }

                ui.add_space(SP_2);
                form_field(ui, "Export path:", |ui| {
                    ui.add_sized(
                        [250.0, 20.0],
                        egui::TextEdit::singleline(&mut self.playlist_export_path),
                    );
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Export Playlist")
                                    .size(TS_SM)
                                    .color(TEXT_PRIMARY),
                            )
                            .fill(SURFACE)
                            .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                            .corner_radius(CORNER_SM),
                        )
                        .clicked()
                    {
                        match self.export_loaded_playlist() {
                            Ok(()) => {
                                self.success_msg = Some(format!(
                                    "Exported playlist '{}' to {}",
                                    pf.playlist.name,
                                    self.playlist_export_path.trim()
                                ));
                            }
                            Err(e) => self.error = Some(format!("Export failed: {e}")),
                        }
                    }
                });

                ui.add_space(SP_2);
                let busy = self.write_loading || self.dry_run_loading;
                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(
                            RichText::new("Sync to Steam Collection")
                                .size(TS_SM)
                                .color(TEXT_INVERSE),
                        )
                        .fill(ACCENT)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(CORNER_SM),
                    )
                    .clicked()
                {
                    self.start_dry_run(PendingAction::PlaylistSync(pf.clone()));
                }
                if self.dry_run_loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(
                            RichText::new("Preparing diff")
                                .size(TS_SM)
                                .color(TEXT_SECONDARY),
                        );
                    });
                }
            });
        }

        // Match report
        if let Some(report) = &self.playlist_match_report {
            section_card(ui, "Match Report", |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                    metric_pill(ui, "Owned", report.owned.len().to_string());
                    metric_pill(ui, "Missing", report.missing.len().to_string());
                    metric_pill(ui, "Played", report.played.len().to_string());
                    metric_pill(ui, "Unplayed", report.unplayed.len().to_string());
                    metric_pill(ui, "Hidden", report.hidden.len().to_string());
                    metric_pill(ui, "Junk", report.junk.len().to_string());
                });
                if let Some(price) = &report.completion_price {
                    ui.add_space(SP_2);
                    ui.label(
                        RichText::new(format!("Completion price: {}", price.format()))
                            .size(TS_BODY)
                            .color(TEXT_SECONDARY),
                    );
                } else {
                    ui.add_space(SP_2);
                    ui.label(
                        RichText::new(
                            "Completion price: (no Steam Store price cached; run \
                             'vapourfly cache refresh --source steam-store')",
                        )
                        .size(TS_SM)
                        .color(TEXT_MUTED),
                    );
                }
            });
        }
    }

    // -- Discover view (top-level; ADR-0005 / ADR-0006) ----------------------

    fn render_discover(&mut self, ui: &mut egui::Ui) {
        view_header(
            ui,
            "Discover",
            "Generate similar-game playlists from taste similarity with an optional seed AppID.",
        );

        section_card(ui, "Generate", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_1);
                ui.label(
                    RichText::new("Seed AppID:")
                        .size(TS_SM)
                        .color(TEXT_SECONDARY),
                );
                ui.add_sized(
                    [80.0, 20.0],
                    egui::TextEdit::singleline(&mut self.discover_seed),
                );
                ui.label(RichText::new("Count:").size(TS_SM).color(TEXT_SECONDARY));
                ui.add_sized(
                    [60.0, 20.0],
                    egui::TextEdit::singleline(&mut self.discover_count),
                );
                if primary_button(ui, "Generate Discover").clicked() {
                    match self.discover_options_from_inputs() {
                        Ok(options) => {
                            if let Some(games) = self.prepared_games(JunkMode::Default) {
                                let pf = discover::generate_discover_playlist(&games, &options);
                                match self
                                    .store_generator_playlist(GeneratorIdentity::Discover, pf)
                                {
                                    Ok(pf) => {
                                        self.discover_last_playlist = Some(pf.clone());
                                        // Also load into Playlists state so "Open in Playlists" is seamless.
                                        self.adopt_playlist_for_edit(&pf);
                                        self.success_msg = Some(format!(
                                            "Generated Discover playlist '{}' (slot {})",
                                            pf.playlist.name, pf.playlist.id
                                        ));
                                    }
                                    Err(e) => self.error = Some(e),
                                }
                            }
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
            });
            ui.add_space(SP_2);
            ui.label(
                RichText::new(
                    "Results overwrite the stable 'discover' playlist slot in the local store. Open Playlists to edit, share, or sync — change id/name and Save to keep a copy.",
                )
                .size(TS_SM)
                .color(TEXT_MUTED),
            );
            if self.discover_last_playlist.is_some() {
                ui.add_space(SP_2);
                if secondary_button(ui, "Open in Playlists").clicked() {
                    self.current_view = View::Playlists;
                }
            }
        });

        if let Some(pf) = self.discover_last_playlist.clone() {
            section_card(ui, &format!("Playlist: {}", pf.playlist.name), |ui| {
                stat_inline(ui, "ID:", &pf.playlist.id);
                match &pf.playlist.content {
                    PlaylistContent::Manual { app_ids } => {
                        stat_inline(
                            ui,
                            "Type:",
                            &format!("Manual playlist with {} AppIDs", app_ids.len()),
                        );
                        if !app_ids.is_empty() {
                            ui.add_space(SP_1);
                            ui.label(
                                RichText::new(format!(
                                    "AppIDs: {}",
                                    app_ids
                                        .iter()
                                        .map(ToString::to_string)
                                        .collect::<Vec<_>>()
                                        .join(", ")
                                ))
                                .size(TS_SM)
                                .color(TEXT_SECONDARY),
                            );
                        }
                    }
                    PlaylistContent::Rules { rules } => {
                        stat_inline(
                            ui,
                            "Type:",
                            &format!("Rule-based playlist with {} rules", rules.len()),
                        );
                    }
                }
            });

            if let Some(report) = &self.playlist_match_report {
                section_card(ui, "Match Report", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                        metric_pill(ui, "Owned", report.owned.len().to_string());
                        metric_pill(ui, "Missing", report.missing.len().to_string());
                        metric_pill(ui, "Played", report.played.len().to_string());
                        metric_pill(ui, "Unplayed", report.unplayed.len().to_string());
                    });
                });
            }
        }
    }

    // -- Collections view ---------------------------------------------------

    fn render_collections(&mut self, ui: &mut egui::Ui) {
        view_header_with_actions(
            ui,
            "Collections",
            "Read-only overview of your Steam collections. Membership edits stay in Steam or Playlist sync.",
            |ui| {
                if primary_button(ui, "Export all").clicked() {
                    self.export_all_collections_action();
                }
            },
        );

        if self.collections.is_empty() {
            empty_state(
                ui,
                "\u{1F4C2}",
                "No collections found",
                "Run a scan first to load your Steam collections.",
            );
            return;
        }

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
            metric_pill(ui, "Collections", self.collections.len().to_string());
            let total_games: usize = self.collections.iter().map(|c| c.app_ids.len()).sum();
            metric_pill(ui, "Memberships", total_games.to_string());
        });
        ui.add_space(SP_3);

        // Read-only card grid: name, count, poster collage (best-effort).
        // No drill-in member editor; no per-card export.
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
            |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_3);
                for coll in &self.collections {
                    render_collection_card(ui, coll);
                }
            },
        );
    }

    /// Page-level Export all via native save dialog (ticket 04).
    ///
    /// Programmatic/tests use [`Self::export_collections`] with a pre-set path.
    fn export_all_collections_action(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_file_name("collections.json")
            .add_filter("JSON", &["json"])
            .save_file()
        else {
            // User cancelled — no write, no error banner.
            return;
        };

        self.collections_export_path = path.display().to_string();
        match self.export_collections() {
            Ok(()) => {
                self.success_msg = Some(format!(
                    "Exported {} collections to {}",
                    self.collections.len(),
                    self.collections_export_path.trim()
                ));
            }
            Err(e) => self.error = Some(format!("Collections export failed: {e}")),
        }
    }

    // -- Data Sources view --------------------------------------------------

    fn render_data_sources(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        view_header(
            ui,
            "Data Sources",
            "Manage enrichment API credentials, cache refresh, and offline mode.",
        );

        // Source credentials
        section_card(ui, "API Credentials", |ui| {
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
                    if *available {
                        status_badge(ui, "Configured", SUCCESS_SOFT, SUCCESS);
                    } else {
                        status_badge(ui, "Missing", ERROR_SOFT, ERROR);
                    }
                    ui.label(RichText::new(*name).size(TS_BODY).color(TEXT_PRIMARY));
                    ui.add_space(SP_2);
                    ui.label(RichText::new(*note).size(TS_SM).color(TEXT_MUTED));
                });
            }

            ui.add_space(SP_2);
            ui.label(
                RichText::new(
                    "Set credentials via environment variables before launching Vapourfly.",
                )
                .size(TS_SM)
                .color(TEXT_SECONDARY),
            );
        });

        // Cache refresh section
        section_card(ui, "Cache Refresh", |ui| {
            ui.checkbox(&mut self.offline_mode, "Offline mode (cache only)");
            if self.offline_mode {
                ui.label(
                    RichText::new("Cache refresh is disabled while offline mode is on.")
                        .size(TS_SM)
                        .color(TEXT_MUTED),
                );
            }

            ui.add_space(SP_2);
            let refresh_enabled = !self.cache_refresh_loading && !self.offline_mode;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                let primary_btn = |label: &str| {
                    egui::Button::new(RichText::new(label).size(TS_SM).color(TEXT_INVERSE))
                        .fill(ACCENT)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(CORNER_SM)
                };
                let secondary_btn = |label: &str| {
                    egui::Button::new(RichText::new(label).size(TS_SM).color(TEXT_PRIMARY))
                        .fill(SURFACE)
                        .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                        .corner_radius(CORNER_SM)
                };
                if ui
                    .add_enabled(refresh_enabled, primary_btn("Refresh All"))
                    .clicked()
                {
                    self.start_cache_refresh(None, ctx);
                }
                if ui
                    .add_enabled(refresh_enabled, secondary_btn("ProtonDB"))
                    .clicked()
                {
                    self.start_cache_refresh(Some("protondb".into()), ctx);
                }
                if ui
                    .add_enabled(refresh_enabled, secondary_btn("PCGW"))
                    .clicked()
                {
                    self.start_cache_refresh(Some("pcgw".into()), ctx);
                }
                if ui
                    .add_enabled(refresh_enabled, secondary_btn("HLTB"))
                    .clicked()
                {
                    self.start_cache_refresh(Some("hltb".into()), ctx);
                }
                if ui
                    .add_enabled(refresh_enabled, secondary_btn("Steam Store"))
                    .clicked()
                {
                    self.start_cache_refresh(Some("steam-store".into()), ctx);
                }
                if self.has_igdb
                    && ui
                        .add_enabled(refresh_enabled, secondary_btn("IGDB"))
                        .clicked()
                {
                    self.start_cache_refresh(Some("igdb".into()), ctx);
                }
                if self.has_rawg
                    && ui
                        .add_enabled(refresh_enabled, secondary_btn("RAWG"))
                        .clicked()
                {
                    self.start_cache_refresh(Some("rawg".into()), ctx);
                }
            });

            if self.cache_refresh_loading {
                ui.add_space(SP_2);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("Refreshing cache")
                            .size(TS_SM)
                            .color(TEXT_SECONDARY),
                    );
                });
            }
            if let Some(msg) = &self.cache_refresh_msg {
                ui.add_space(SP_2);
                ui.label(RichText::new(msg).size(TS_SM).color(TEXT_SECONDARY));
            }
        });

        // Source cache status
        section_card(ui, "Source Cache Status", |ui| {
            if self.source_statuses.is_empty() {
                ui.label(
                    RichText::new("No cache data found. Run a scan and refresh to populate cache.")
                        .size(TS_BODY)
                        .color(TEXT_SECONDARY),
                );
            } else {
                let text_height = TS_BODY;
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
                            ui.label(
                                RichText::new("Source")
                                    .size(TS_SM)
                                    .strong()
                                    .color(TEXT_SECONDARY),
                            );
                        });
                        header.col(|ui| {
                            ui.label(
                                RichText::new("Last Success")
                                    .size(TS_SM)
                                    .strong()
                                    .color(TEXT_SECONDARY),
                            );
                        });
                        header.col(|ui| {
                            ui.label(
                                RichText::new("Entries")
                                    .size(TS_SM)
                                    .strong()
                                    .color(TEXT_SECONDARY),
                            );
                        });
                        header.col(|ui| {
                            ui.label(
                                RichText::new("Stale")
                                    .size(TS_SM)
                                    .strong()
                                    .color(TEXT_SECONDARY),
                            );
                        });
                        header.col(|ui| {
                            ui.label(
                                RichText::new("Cached")
                                    .size(TS_SM)
                                    .strong()
                                    .color(TEXT_SECONDARY),
                            );
                        });
                    })
                    .body(|mut body| {
                        for status in &self.source_statuses {
                            body.row(text_height * 1.2, |mut row| {
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(&status.name)
                                            .size(TS_BODY)
                                            .color(TEXT_PRIMARY),
                                    );
                                });
                                row.col(|ui| {
                                    let last = status.last_success.map_or_else(
                                        || "n/a".into(),
                                        |dt| dt.format("%Y-%m-%d %H:%M").to_string(),
                                    );
                                    ui.label(RichText::new(last).size(TS_BODY).color(TEXT_PRIMARY));
                                });
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(status.cache_entries.to_string())
                                            .size(TS_BODY)
                                            .color(TEXT_PRIMARY),
                                    );
                                });
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(status.stale_entries.to_string())
                                            .size(TS_BODY)
                                            .color(TEXT_PRIMARY),
                                    );
                                });
                                row.col(|ui| {
                                    if status.cache_dir_exists {
                                        status_badge(ui, "Yes", SUCCESS_SOFT, SUCCESS);
                                    } else {
                                        status_badge(ui, "No", SURFACE_SUNKEN, TEXT_MUTED);
                                    }
                                });
                            });
                        }
                    });
            }
        });

        // Offline mode
        section_card(ui, "Offline Mode", |ui| {
            ui.label(
                RichText::new(
                    "Use `--offline` CLI flag to prohibit network calls and use cached data only.",
                )
                .size(TS_BODY)
                .color(TEXT_SECONDARY),
            );
        });
    }

    // -- Backups section (embedded under Settings; ADR-0006) ----------------

    fn render_backups_section(&mut self, ui: &mut egui::Ui) {
        section_card(ui, "Backups", |ui| {
            ui.label(
                RichText::new(
                    "Browse and restore safety backups of your Steam cloud storage.",
                )
                .size(TS_SM)
                .color(TEXT_SECONDARY),
            );
            ui.add_space(SP_2);
            if primary_button(ui, "Refresh Backups").clicked() {
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
                ui.add_space(SP_2);
                ui.label(
                    RichText::new(
                        "No backups found. Click Refresh Backups after a write creates one.",
                    )
                    .size(TS_BODY)
                    .color(TEXT_MUTED),
                );
                return;
            }

            ui.add_space(SP_2);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                metric_pill(ui, "Backups", self.backups.len().to_string());
            });
            ui.add_space(SP_2);

            let text_height = TS_BODY;
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
                        ui.label(
                            RichText::new("Filename")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                    });
                    header.col(|ui| {
                        ui.label(
                            RichText::new("Created")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                    });
                    header.col(|ui| {
                        ui.label(
                            RichText::new("SHA256")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                    });
                    header.col(|ui| {
                        ui.label(
                            RichText::new("Action")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
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
                                ui.label(
                                    RichText::new(&filename).size(TS_BODY).color(TEXT_PRIMARY),
                                );
                            });
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(
                                        backup.created_at.format("%Y-%m-%d %H:%M:%S").to_string(),
                                    )
                                    .size(TS_BODY)
                                    .color(TEXT_PRIMARY),
                                );
                            });
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(&backup.sha256[..8])
                                        .size(TS_BODY)
                                        .color(TEXT_MUTED)
                                        .monospace(),
                                );
                            });
                            row.col(|ui| {
                                let enabled = !self.write_loading;
                                ui.add_enabled_ui(enabled, |ui| {
                                    if secondary_button(ui, "Restore").clicked() {
                                        self.pending_action =
                                            Some(PendingAction::BackupRestore(backup.path.clone()));
                                        self.show_confirm_dialog = true;
                                    }
                                });
                            });
                        });
                    }
                });

            if self.write_loading {
                ui.add_space(SP_2);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Restoring").size(TS_SM).color(TEXT_SECONDARY));
                });
            }
        });
    }

    // -- Settings view ------------------------------------------------------

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        view_header(
            ui,
            "Settings",
            "Configure Steam directory, accounts, locale, write safety, and backups.",
        );

        section_card(ui, "Steam Directory", |ui| {
            form_field(ui, "Path:", |ui| {
                ui.add_sized(
                    [300.0, 20.0],
                    egui::TextEdit::singleline(&mut self.steam_dir_edit),
                );
            });
            ui.label(
                RichText::new("Leave empty for auto-detection.")
                    .size(TS_SM)
                    .color(TEXT_MUTED),
            );
        });

        section_card(ui, "Account Override", |ui| {
            form_field(ui, "Account:", |ui| {
                ui.add_sized(
                    [200.0, 20.0],
                    egui::TextEdit::singleline(&mut self.account_edit),
                );
            });
            ui.label(
                RichText::new("Leave empty for auto-selection (most recent).")
                    .size(TS_SM)
                    .color(TEXT_MUTED),
            );
        });

        section_card(ui, "Detected Accounts", |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Refresh Accounts")
                                .size(TS_SM)
                                .color(TEXT_PRIMARY),
                        )
                        .fill(SURFACE)
                        .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                        .corner_radius(CORNER_SM),
                    )
                    .clicked()
                {
                    self.refresh_detected_accounts();
                }
            });
            if let Some(msg) = &self.account_list_msg {
                ui.label(RichText::new(msg).size(TS_SM).color(TEXT_SECONDARY));
            }

            if self.detected_accounts.is_empty() {
                ui.label(
                    RichText::new("No accounts loaded.")
                        .size(TS_BODY)
                        .color(TEXT_SECONDARY),
                );
            } else {
                let mut selected_account = None;
                egui::Grid::new("detected_accounts_grid")
                    .num_columns(5)
                    .spacing([SP_3, SP_1])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("Persona")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                        ui.label(
                            RichText::new("Account")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                        ui.label(
                            RichText::new("Steam ID")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                        ui.label(
                            RichText::new("Most recent")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                        ui.label(
                            RichText::new("Action")
                                .size(TS_SM)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                        ui.end_row();

                        for account in &self.detected_accounts {
                            ui.label(
                                RichText::new(&account.persona_name)
                                    .size(TS_BODY)
                                    .color(TEXT_PRIMARY),
                            );
                            ui.label(
                                RichText::new(&account.account_name)
                                    .size(TS_BODY)
                                    .color(TEXT_PRIMARY),
                            );
                            ui.label(
                                RichText::new(mask_steam_id(&account.steam_id64))
                                    .size(TS_BODY)
                                    .color(TEXT_MUTED)
                                    .monospace(),
                            );
                            if account.most_recent {
                                status_badge(ui, "yes", SUCCESS_SOFT, SUCCESS);
                            } else {
                                status_badge(ui, "no", SURFACE_SUNKEN, TEXT_MUTED);
                            }
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Use").size(TS_SM).color(TEXT_PRIMARY),
                                    )
                                    .fill(SURFACE)
                                    .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                                    .corner_radius(CORNER_SM),
                                )
                                .clicked()
                            {
                                selected_account = Some(account.account_name.clone());
                            }
                            ui.end_row();
                        }
                    });

                if let Some(account_name) = selected_account {
                    self.account_edit = account_name;
                }
            }
        });

        section_card(ui, "Store Locale", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_1);
                ui.label(
                    RichText::new("Country code:")
                        .size(TS_SM)
                        .color(TEXT_SECONDARY),
                );
                ui.add_sized([60.0, 20.0], egui::TextEdit::singleline(&mut self.cc_edit));
                ui.label(RichText::new("Language:").size(TS_SM).color(TEXT_SECONDARY));
                ui.add_sized(
                    [120.0, 20.0],
                    egui::TextEdit::singleline(&mut self.lang_edit),
                );
            });
        });

        section_card(ui, "Backup Retention", |ui| {
            form_field(ui, "Keep backups:", |ui| {
                ui.add_sized(
                    [60.0, 20.0],
                    egui::TextEdit::singleline(&mut self.backup_retention_edit),
                );
            });
            ui.label(
                RichText::new("Number of rolling backups to keep for modified files.")
                    .size(TS_SM)
                    .color(TEXT_MUTED),
            );
        });

        section_card(ui, "Write Safety", |ui| {
            ui.checkbox(
                &mut self.allow_steam_running,
                "Allow writes while Steam is running",
            );
            ui.label(
                RichText::new("Enable with caution. Steam may overwrite changes.")
                    .size(TS_SM)
                    .color(WARNING),
            );
        });

        section_card(ui, "Network", |ui| {
            ui.checkbox(&mut self.offline_mode, "Offline mode (cache only)");
            ui.label(
                RichText::new(
                    "Blocks all network calls: cache refresh and library scan hydration.",
                )
                .size(TS_SM)
                .color(TEXT_MUTED),
            );
        });

        // Save button
        ui.horizontal(|ui| {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Save Settings")
                            .size(TS_SM)
                            .color(TEXT_INVERSE),
                    )
                    .fill(ACCENT)
                    .stroke(egui::Stroke::NONE)
                    .corner_radius(CORNER_SM),
                )
                .clicked()
            {
                self.save_settings();
            }
            if let Some(msg) = &self.settings_save_msg {
                ui.label(RichText::new(msg).size(TS_SM).color(TEXT_SECONDARY));
            }
        });
        ui.add_space(SP_3);

        section_card(ui, "Setup Diagnostics", |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(
                        "Check Steam paths, accounts, libraries, cloud storage, cache, and credentials.",
                    )
                    .size(TS_SM)
                    .color(TEXT_SECONDARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new("Run Setup Check").size(TS_SM).color(TEXT_PRIMARY),
                            )
                            .fill(SURFACE)
                            .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                            .corner_radius(CORNER_SM),
                        )
                        .clicked()
                    {
                        self.run_setup_diagnostics();
                    }
                });
            });
            if let Some(report) = &self.setup_diagnostics {
                ui.add_space(SP_2);
                ui.label(
                    RichText::new(report)
                        .size(TS_XS)
                        .color(TEXT_PRIMARY)
                        .monospace(),
                );
            }
        });

        section_card(ui, "Diagnostics Export", |ui| {
            ui.label(
                RichText::new("Export sanitized support data for bug reports.")
                    .size(TS_SM)
                    .color(TEXT_SECONDARY),
            );
            ui.add_space(SP_2);
            form_field(ui, "Export path:", |ui| {
                ui.add_sized(
                    [300.0, 20.0],
                    egui::TextEdit::singleline(&mut self.diagnostics_export_path),
                );
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Export Diagnostics")
                                .size(TS_SM)
                                .color(TEXT_PRIMARY),
                        )
                        .fill(SURFACE)
                        .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                        .corner_radius(CORNER_SM),
                    )
                    .clicked()
                {
                    match self.export_diagnostics() {
                        Ok(()) => {
                            self.success_msg = Some(format!(
                                "Diagnostics exported to {}",
                                self.diagnostics_export_path.trim()
                            ));
                        }
                        Err(e) => self.error = Some(format!("Diagnostics export failed: {e}")),
                    }
                }
            });
        });

        // Backups / restore live under Settings (not a top-level nav item).
        self.render_backups_section(ui);

        section_card(ui, "About", |ui| {
            stat_inline(ui, "Version:", &format!("v{}", env!("CARGO_PKG_VERSION")));
            ui.label(
                RichText::new("A local-first CLI/GUI tool for managing Steam game libraries.")
                    .size(TS_BODY)
                    .color(TEXT_SECONDARY),
            );
            ui.label(
                RichText::new("Licensed under MIT OR Apache-2.0.")
                    .size(TS_SM)
                    .color(TEXT_MUTED),
            );
        });
    }

    /// Save settings to config.toml.
    ///
    /// All field updates are validated first, then applied in a single
    /// read-modify-write transaction via
    /// [`vapourfly_core::config::apply_config_updates`]. Because validation
    /// happens before any write, a validation failure never leaves the file
    /// in a partially-updated state, and the surfaced error message always
    /// reflects whether anything was actually persisted. The single write is
    /// atomic (temp file + rename), so the file is also not left truncated or
    /// corrupt if the process is interrupted mid-write.
    fn save_settings(&mut self) {
        use vapourfly_core::config::{ConfigField, ConfigUpdate, apply_config_updates};

        // Validate every input first. If any field is invalid, we abort before
        // touching the config file so the user never sees a "Failed to save"
        // message for a save that partially succeeded.
        let mut errors: Vec<String> = Vec::new();

        let backup_value = self.backup_retention_edit.trim();
        let backup_update: Option<Option<String>> = if backup_value.is_empty() {
            Some(None) // unset
        } else {
            match backup_value.parse::<u32>() {
                Ok(_) => Some(Some(backup_value.to_string())),
                Err(_) => {
                    errors.push("backup_retention_count: must be a non-negative integer".into());
                    None
                }
            }
        };

        if !errors.is_empty() {
            self.settings_save_msg = Some(format!("Failed to save: {}", errors.join("; ")));
            return;
        }

        // All inputs are valid — build the batch and persist atomically.
        let str_field = |field: ConfigField, value: &str| -> ConfigUpdate {
            if value.is_empty() {
                (field, None)
            } else {
                (field, Some(value.to_string()))
            }
        };

        let mut updates: Vec<ConfigUpdate> = vec![
            str_field(ConfigField::SteamDir, &self.steam_dir_edit),
            str_field(ConfigField::Account, &self.account_edit),
            str_field(ConfigField::Cc, &self.cc_edit),
            str_field(ConfigField::Lang, &self.lang_edit),
        ];
        if let Some(update) = backup_update {
            updates.push((ConfigField::BackupRetentionCount, update));
        }

        if let Err(e) = apply_config_updates(&updates) {
            self.settings_save_msg = Some(format!("Failed to save: {e}"));
            return;
        }

        let path = vapourfly_core::config::config_file_path();
        self.settings_save_msg = Some(match path {
            Some(p) => format!("Saved to {}", p.display()),
            None => "Saved.".into(),
        });
        // Reload config so the UI reflects the new values.
        self.config =
            VapourflyConfig::from_cli_and_env(vapourfly_core::config::CliOverrides::default()).ok();
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn configure_ui(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.spacing.item_spacing = egui::vec2(SP_2, 7.0);
    style.spacing.button_padding = egui::vec2(SP_3, SP_1);
    style.spacing.window_margin = egui::Margin::same(m(SP_3));
    style.spacing.indent = SP_3;

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = SURFACE;
    visuals.window_fill = SURFACE_RAISED;
    visuals.faint_bg_color = SURFACE_RAISED;
    visuals.extreme_bg_color = SURFACE_SUNKEN;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = ACCENT_SOFT;
    visuals.selection.stroke.color = ACCENT_TEXT;
    visuals.widgets.noninteractive.bg_fill = SURFACE;
    visuals.widgets.noninteractive.fg_stroke.color = TEXT_SECONDARY;
    visuals.widgets.inactive.bg_fill = SURFACE_MUTED;
    visuals.widgets.inactive.fg_stroke.color = TEXT_SECONDARY;
    visuals.widgets.hovered.bg_fill = ACCENT_SOFT;
    visuals.widgets.hovered.fg_stroke.color = TEXT_PRIMARY;
    visuals.widgets.active.bg_fill = ACCENT_SOFT;
    visuals.widgets.active.fg_stroke.color = TEXT_PRIMARY;
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.dark_mode = true;
    style.visuals = visuals;

    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::Theme::Dark);
}

fn game_primary_badge(game: &Game) -> (&'static str, Color32, Color32) {
    if game.is_junk {
        ("Junk", ERROR_SOFT, ERROR)
    } else if game.is_hidden {
        ("Hidden", SURFACE_MUTED, TEXT_SECONDARY)
    } else if game.installed {
        ("Installed", SUCCESS_SOFT, SUCCESS)
    } else {
        ("Library", ACCENT_SOFT, ACCENT)
    }
}

/// Deck badge when PCGW reports full controller support (hydrated cache only).
fn game_shows_deck_badge(game: &Game) -> bool {
    game.pcgw
        .as_ref()
        .is_some_and(|pcgw| pcgw.controller_support == ControllerSupport::Full)
}

const COLLECTION_CARD_W: f32 = 240.0;
const COLLECTION_CARD_H: f32 = 168.0;
const COLLAGE_POSTER_W: f32 = 44.0;
const COLLAGE_POSTER_H: f32 = 66.0;
const COLLAGE_MAX: usize = 4;

/// Read-only collection overview card: name, count, poster collage.
fn render_collection_card(ui: &mut egui::Ui, coll: &SteamCollection) {
    ui.allocate_ui_with_layout(
        egui::vec2(COLLECTION_CARD_W, COLLECTION_CARD_H),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            ui.set_width(COLLECTION_CARD_W);
            ui.set_height(COLLECTION_CARD_H);

            egui::Frame::NONE
                .fill(SURFACE_RAISED)
                .stroke(egui::Stroke::new(1.0, BORDER_SOFT))
                .inner_margin(egui::Margin::same(m(SP_3)))
                .corner_radius(CORNER_MD)
                .show(ui, |ui| {
                    ui.set_width(COLLECTION_CARD_W - f32::from(m(SP_3)) * 2.0);

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&coll.name)
                                .size(TS_MD)
                                .strong()
                                .color(TEXT_PRIMARY),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if coll.is_hidden_collection {
                                status_badge(ui, "Hidden", SURFACE_MUTED, TEXT_SECONDARY);
                            }
                            metric_pill(ui, "Games", coll.app_ids.len().to_string());
                        });
                    });

                    ui.add_space(SP_2);

                    // Poster collage — best-effort Steam CDN art for member AppIDs.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SP_1, SP_1);
                        let shown: Vec<u32> =
                            coll.app_ids.iter().copied().take(COLLAGE_MAX).collect();
                        if shown.is_empty() {
                            egui::Frame::NONE
                                .fill(SURFACE_SUNKEN)
                                .corner_radius(CORNER_SM)
                                .inner_margin(egui::Margin::same(m(SP_3)))
                                .show(ui, |ui| {
                                    ui.set_min_size(egui::vec2(
                                        COLLAGE_POSTER_W * 2.0,
                                        COLLAGE_POSTER_H,
                                    ));
                                    ui.centered_and_justified(|ui| {
                                        ui.label(
                                            RichText::new("No covers")
                                                .size(TS_XS)
                                                .color(TEXT_MUTED),
                                        );
                                    });
                                });
                        } else {
                            for app_id in shown {
                                ui.add(
                                    egui::Image::from_uri(steam_poster_uri(app_id))
                                        .fit_to_exact_size(egui::vec2(
                                            COLLAGE_POSTER_W,
                                            COLLAGE_POSTER_H,
                                        ))
                                        .corner_radius(CORNER_SM)
                                        .bg_fill(SURFACE_SUNKEN)
                                        .show_loading_spinner(true)
                                        .alt_text(format!("App {app_id} cover")),
                                );
                            }
                            if coll.app_ids.len() > COLLAGE_MAX {
                                ui.label(
                                    RichText::new(format!("+{}", coll.app_ids.len() - COLLAGE_MAX))
                                        .size(TS_SM)
                                        .color(TEXT_MUTED),
                                );
                            }
                        }
                    });
                });
        },
    );
}

fn game_card_detail(game: &Game) -> String {
    let metadata = game_metadata_summary(game);
    if !metadata.is_empty() {
        return metadata;
    }

    if !game.steam_collections.is_empty() {
        return format!("{} collection(s)", game.steam_collections.len());
    }

    if game.installed {
        "Ready to play".to_string()
    } else {
        "In your Steam library".to_string()
    }
}

fn steam_poster_uri(app_id: u32) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{app_id}/library_600x900.jpg")
}

fn empty_value_label() -> &'static str {
    "None"
}

fn parse_required_u32(label: &str, input: &str) -> Result<u32, String> {
    input
        .trim()
        .parse()
        .map_err(|_| format!("{label} must be a whole number."))
}

fn parse_required_usize(label: &str, input: &str) -> Result<usize, String> {
    input
        .trim()
        .parse()
        .map_err(|_| format!("{label} must be a whole number."))
}

fn parse_optional_u32(label: &str, input: &str) -> Result<Option<u32>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    trimmed
        .parse()
        .map(Some)
        .map_err(|_| format!("{label} must be a whole number."))
}

fn parse_optional_u64(label: &str, input: &str) -> Result<Option<u64>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    trimmed
        .parse()
        .map(Some)
        .map_err(|_| format!("{label} must be a whole number."))
}

fn format_playtime(minutes: u32) -> String {
    if minutes == 0 {
        return "0m".to_string();
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
    display::format_junk_signal(signal)
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
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([900.0, 620.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Vapourfly",
        native_options,
        Box::new(|cc| {
            configure_ui(&cc.egui_ctx);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
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

    fn test_game(app_id: u32, name: &str) -> Game {
        Game {
            app_id,
            name: name.into(),
            app_type: SteamAppType::Game,
            installed: false,
            install_dir: None,
            library_folder: None,
            playtime_minutes: None,
            playtime_2wks_minutes: None,
            playtime_disconnected_minutes: None,
            last_played_unix: None,
            steam_collections: Vec::new(),
            is_hidden: false,
            is_junk: false,
            hltb: None,
            igdb: None,
            rawg: None,
            protondb: None,
            pcgw: None,
            steam_store: None,
        }
    }

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
        assert_eq!(View::ALL.len(), 7);
        assert!(View::ALL.contains(&View::Library));
        assert!(View::ALL.contains(&View::Collections));
        assert!(View::ALL.contains(&View::Recommendations));
        assert!(View::ALL.contains(&View::Playlists));
        assert!(View::ALL.contains(&View::Discover));
        assert!(View::ALL.contains(&View::DataSources));
        assert!(View::ALL.contains(&View::Settings));
    }

    #[test]
    fn navigation_contract_matches_design_ia() {
        // Sidebar destinations only — Junk and Backups are relocated (ADR-0006).
        let labels: Vec<&str> = View::ALL.iter().map(|v| v.label()).collect();
        assert_eq!(
            labels,
            vec![
                "Library",
                "Collections",
                "Recommendations",
                "Playlists",
                "Discover",
                "Data Sources",
                "Settings",
            ]
        );
        assert!(!labels.contains(&"Junk"));
        assert!(!labels.contains(&"Backups"));
        assert!(!labels.contains(&"Recommend"));
    }

    #[test]
    fn default_landing_view_is_library() {
        let app = VapourflyApp::new(None);
        assert_eq!(app.current_view, View::Library);
        assert!(!app.show_junk_panel);
    }

    #[test]
    fn view_labels_are_distinct() {
        let labels: Vec<&str> = View::ALL.iter().map(|v| v.label()).collect();
        let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
        assert_eq!(labels.len(), unique.len());
    }

    #[test]
    fn format_playtime_zero() {
        assert_eq!(format_playtime(0), "0m");
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
    fn nav_labels_are_plain_text() {
        for view in View::ALL {
            let label = view.label();
            assert!(
                label.is_ascii(),
                "{label} should not depend on emoji glyphs"
            );
            assert!(!label.chars().any(|c| c > '\u{007f}'));
        }
    }

    #[test]
    fn empty_value_label_is_plain_text() {
        assert_eq!(empty_value_label(), "None");
        assert!(empty_value_label().is_ascii());
    }

    #[test]
    fn steam_poster_uri_uses_library_poster_endpoint() {
        assert_eq!(
            steam_poster_uri(730),
            "https://cdn.cloudflare.steamstatic.com/steam/apps/730/library_600x900.jpg"
        );
    }

    #[test]
    fn game_primary_badge_prioritizes_visible_state() {
        let mut game = test_game(730, "Counter-Strike 2");
        assert_eq!(game_primary_badge(&game).0, "Library");

        game.installed = true;
        assert_eq!(game_primary_badge(&game).0, "Installed");

        game.is_hidden = true;
        assert_eq!(game_primary_badge(&game).0, "Hidden");

        game.is_junk = true;
        assert_eq!(game_primary_badge(&game).0, "Junk");
    }

    #[test]
    fn library_filters_default_to_show_all() {
        let game = test_game(730, "Counter-Strike 2");
        let filters = LibraryFilters::default();
        assert!(game_matches_library_filters(&game, &filters));
    }

    #[test]
    fn library_filters_installed_only_excludes_unowned_installs() {
        let mut game = test_game(730, "Counter-Strike 2");
        game.installed = false;
        let filters = LibraryFilters {
            installed_only: true,
            ..LibraryFilters::default()
        };
        assert!(!game_matches_library_filters(&game, &filters));
        game.installed = true;
        assert!(game_matches_library_filters(&game, &filters));
    }

    #[test]
    fn library_filters_not_hidden_and_not_junk_exclude_flagged() {
        let mut game = test_game(999, "Demo");
        game.is_hidden = true;
        game.is_junk = true;

        let not_hidden = LibraryFilters {
            not_hidden: true,
            ..LibraryFilters::default()
        };
        assert!(!game_matches_library_filters(&game, &not_hidden));

        let not_junk = LibraryFilters {
            not_junk: true,
            ..LibraryFilters::default()
        };
        assert!(!game_matches_library_filters(&game, &not_junk));

        game.is_hidden = false;
        game.is_junk = false;
        assert!(game_matches_library_filters(&game, &not_hidden));
        assert!(game_matches_library_filters(&game, &not_junk));
    }

    #[test]
    fn library_filters_search_matches_title_or_app_id() {
        let game = test_game(730, "Counter-Strike 2");
        let by_title = LibraryFilters {
            search: "counter".into(),
            ..LibraryFilters::default()
        };
        assert!(game_matches_library_filters(&game, &by_title));
        let by_id = LibraryFilters {
            search: "730".into(),
            ..LibraryFilters::default()
        };
        assert!(game_matches_library_filters(&game, &by_id));
        let miss = LibraryFilters {
            search: "factorio".into(),
            ..LibraryFilters::default()
        };
        assert!(!game_matches_library_filters(&game, &miss));
    }

    #[test]
    fn project_library_games_sorts_installed_then_playtime() {
        let mut a = test_game(1, "Alpha");
        a.installed = false;
        a.playtime_minutes = Some(999);
        let mut b = test_game(2, "Bravo");
        b.installed = true;
        b.playtime_minutes = Some(10);
        let mut c = test_game(3, "Charlie");
        c.installed = true;
        c.playtime_minutes = Some(100);

        let projected = project_library_games(vec![a, b, c], &LibraryFilters::default());
        assert_eq!(
            projected.iter().map(|g| g.app_id).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
    }

    #[test]
    fn library_summary_counts_matching_and_installed() {
        let mut installed = test_game(1, "A");
        installed.installed = true;
        let remote = test_game(2, "B");
        let matching = vec![installed.clone(), remote];
        let all = vec![installed, test_game(3, "C")];
        assert_eq!(
            library_summary(&all, &matching),
            LibrarySummary {
                matching: 2,
                total: 2,
                installed: 1,
            }
        );
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn library_filter_fields_match_three_toggle_contract() {
        // Guard against reintroducing Unplayed / include-only Hidden or Junk toggles.
        let app = VapourflyApp::new(None);
        assert!(!app.filter_installed_only);
        assert!(!app.filter_not_hidden);
        assert!(!app.filter_not_junk);
        assert!(app.library_selected_app_id.is_none());
    }

    #[test]
    fn game_shows_deck_badge_only_with_full_controller() {
        let mut game = test_game(730, "CS2");
        assert!(!game_shows_deck_badge(&game));
        game.pcgw = Some(PcgwData {
            page_name: None,
            controller_support: ControllerSupport::Full,
            steam_deck_notes: None,
            fixes_url: None,
        });
        assert!(game_shows_deck_badge(&game));
        game.pcgw.as_mut().unwrap().controller_support = ControllerSupport::Partial;
        assert!(!game_shows_deck_badge(&game));
    }

    #[test]
    fn game_card_detail_uses_collection_or_playable_state() {
        let mut game = test_game(730, "Counter-Strike 2");
        assert_eq!(game_card_detail(&game), "In your Steam library");

        game.installed = true;
        assert_eq!(game_card_detail(&game), "Ready to play");

        game.installed = false;
        game.steam_collections.push("favorites".into());
        assert_eq!(game_card_detail(&game), "1 collection(s)");
    }

    #[test]
    fn pending_action_is_clone() {
        let a = PendingAction::JunkApply;
        let _b = a.clone();
        let c = PendingAction::BackupRestore(PathBuf::from("/tmp/test"));
        let _d = c.clone();
    }

    #[test]
    fn backup_retention_prefers_settings_edit_field() {
        let mut app = VapourflyApp::new(None);
        app.backup_retention_edit = "1".into();
        assert_eq!(app.backup_retention(), 1);
        app.backup_retention_edit = "not-a-number".into();
        // Falls back to config or default when edit is invalid.
        let expected = app
            .config
            .as_ref()
            .map(|c| c.backup_retention_count)
            .unwrap_or(vapourfly_core::write::DEFAULT_BACKUP_RETENTION);
        assert_eq!(app.backup_retention(), expected);
        app.backup_retention_edit = "7".into();
        assert_eq!(
            app.backup_retention(),
            7,
            "Settings UI retention must drive write commit retention"
        );
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
    fn recommend_request_uses_optional_seed_input() {
        let mut app = VapourflyApp::new(None);
        app.recommend_minutes = "90".into();
        app.recommend_count = "7".into();
        app.recommend_seed = "12345".into();
        app.recommend_deck = true;
        app.recommend_installed_only = true;

        let request = app.recommend_request_from_inputs().unwrap();

        assert_eq!(request.available_minutes, 90);
        assert_eq!(request.count, 7);
        assert_eq!(request.seed, Some(12345));
        assert!(request.deck_mode);
        assert!(request.include_installed_only);

        app.recommend_seed.clear();
        assert_eq!(app.recommend_request_from_inputs().unwrap().seed, None);
    }

    #[test]
    fn recommend_request_rejects_invalid_input() {
        let mut app = VapourflyApp::new(None);
        app.recommend_minutes = "soon".into();

        let err = app.recommend_request_from_inputs().unwrap_err();

        assert!(err.contains("Available minutes"));
    }

    #[test]
    fn discover_options_use_count_and_seed_inputs() {
        let mut app = VapourflyApp::new(None);
        app.discover_seed = "367520".into();
        app.discover_count = "12".into();

        let options = app.discover_options_from_inputs().unwrap();

        assert_eq!(options.seed_app_id, Some(367520));
        assert_eq!(options.count, 12);

        app.discover_seed.clear();
        assert_eq!(
            app.discover_options_from_inputs().unwrap().seed_app_id,
            None
        );
    }

    // -- Generator playlist slots (ADR-0007) --------------------------------

    fn sample_generator_playlist(id: &str, app_ids: Vec<u32>) -> PlaylistFile {
        PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "vapourfly".into(),
            playlist: Playlist {
                id: id.into(),
                name: "Generated".into(),
                description: "test".into(),
                content: PlaylistContent::Manual { app_ids },
            },
        }
    }

    #[test]
    fn generator_slot_ids_are_stable_per_identity() {
        assert_eq!(GeneratorIdentity::Discover.slot_id(), "discover");
        assert_eq!(
            GeneratorIdentity::Dynamic(DynamicTemplate::DeckSession).slot_id(),
            "dynamic-deck-session"
        );
        assert_eq!(
            GeneratorIdentity::Dynamic(DynamicTemplate::FinishIt).slot_id(),
            "dynamic-finish-it"
        );
        assert_eq!(
            GeneratorIdentity::Mood(EditorialMood::QuickRound).slot_id(),
            "mood-quick-round"
        );
        assert_eq!(
            GeneratorIdentity::Mood(EditorialMood::FridayParty).slot_id(),
            "mood-friday-party"
        );
    }

    #[test]
    fn put_generator_slot_writes_stable_id_and_overwrites_on_regenerate() {
        let dir = TempDir::new().unwrap();
        let store = dir.path();

        // Discover: one slot regardless of content/source id.
        let first = put_generator_slot(
            store,
            GeneratorIdentity::Discover,
            sample_generator_playlist("discover-taste", vec![10, 20]),
        )
        .unwrap();
        assert_eq!(first.playlist.id, "discover");
        let loaded = playlist_store::get(store, "discover").unwrap();
        match &loaded.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, &vec![10, 20]),
            _ => panic!("expected manual"),
        }

        let second = put_generator_slot(
            store,
            GeneratorIdentity::Discover,
            sample_generator_playlist("discover-367520", vec![30, 40, 50]),
        )
        .unwrap();
        assert_eq!(second.playlist.id, "discover");
        let ids = playlist_store::list_ids(store).unwrap();
        assert_eq!(ids, vec!["discover"], "regenerate must not create a second id");
        let reloaded = playlist_store::get(store, "discover").unwrap();
        match reloaded.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, vec![30, 40, 50]),
            _ => panic!("expected manual"),
        }

        // Dynamic template slot is independent of Discover.
        let dyn_pf = put_generator_slot(
            store,
            GeneratorIdentity::Dynamic(DynamicTemplate::DeckSession),
            sample_generator_playlist("deck-session", vec![1]),
        )
        .unwrap();
        assert_eq!(dyn_pf.playlist.id, "dynamic-deck-session");
        put_generator_slot(
            store,
            GeneratorIdentity::Dynamic(DynamicTemplate::DeckSession),
            sample_generator_playlist("deck-session", vec![2, 3]),
        )
        .unwrap();
        let dyn_loaded = playlist_store::get(store, "dynamic-deck-session").unwrap();
        match dyn_loaded.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, vec![2, 3]),
            _ => panic!("expected manual"),
        }

        // Editorial mood slot.
        put_generator_slot(
            store,
            GeneratorIdentity::Mood(EditorialMood::QuickRound),
            sample_generator_playlist("mood-quick-round", vec![7]),
        )
        .unwrap();
        put_generator_slot(
            store,
            GeneratorIdentity::Mood(EditorialMood::QuickRound),
            sample_generator_playlist("mood-quick-round", vec![8, 9]),
        )
        .unwrap();
        let mood_loaded = playlist_store::get(store, "mood-quick-round").unwrap();
        match mood_loaded.playlist.content {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, vec![8, 9]),
            _ => panic!("expected manual"),
        }

        let mut all = playlist_store::list_ids(store).unwrap();
        all.sort();
        assert_eq!(
            all,
            vec![
                "discover",
                "dynamic-deck-session",
                "mood-quick-round"
            ]
        );
    }

    #[test]
    fn app_store_generator_playlist_uses_injected_store_dir() {
        let dir = TempDir::new().unwrap();
        let mut app = VapourflyApp::new(None);
        app.playlist_store_dir = Some(dir.path().to_path_buf());

        let pf = app
            .store_generator_playlist(
                GeneratorIdentity::Discover,
                sample_generator_playlist("ignored-id", vec![100]),
            )
            .unwrap();
        assert_eq!(pf.playlist.id, "discover");
        assert!(dir.path().join("discover.json").is_file());
        assert!(!dir.path().join("ignored-id.json").exists());
    }

    #[test]
    fn cache_refresh_is_blocked_in_offline_mode() {
        let mut app = VapourflyApp::new(None);
        app.offline_mode = true;
        app.scan_result = Some(ScanResult {
            steam_dir: "/tmp/steam".into(),
            account: "test".into(),
            games: Vec::new(),
            warnings: Vec::new(),
        });

        app.start_cache_refresh(Some("igdb".into()), &egui::Context::default());

        assert!(!app.cache_refresh_loading);
        assert_eq!(
            app.cache_refresh_msg.as_deref(),
            Some("Offline mode is on. Cache refresh requires network access.")
        );
    }

    #[test]
    fn settings_can_refresh_detected_accounts_from_fixture() {
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal");
        let mut app = VapourflyApp::new(Some(fixtures));

        app.refresh_detected_accounts();

        assert_eq!(app.detected_accounts.len(), 1);
        assert_eq!(
            app.detected_accounts[0].account_name,
            "vapourfly_fixture_user"
        );
        assert_eq!(app.detected_accounts[0].persona_name, "Vapourfly Fixture");
        assert!(app.detected_accounts[0].most_recent);
    }

    #[test]
    fn manual_playlist_app_ids_render_as_csv_for_editing() {
        let pf = PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "test".into(),
            playlist: Playlist {
                id: "deck-shortlist".into(),
                name: "Deck Shortlist".into(),
                description: String::new(),
                content: PlaylistContent::Manual {
                    app_ids: vec![730, 427520],
                },
            },
        };

        assert_eq!(manual_playlist_app_ids_csv(&pf), "730, 427520");
    }

    #[test]
    fn playlist_rules_json_round_trips_rule_based_playlist() {
        let pf = PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "test".into(),
            playlist: Playlist {
                id: "installed-unplayed".into(),
                name: "Installed Unplayed".into(),
                description: String::new(),
                content: PlaylistContent::Rules {
                    rules: vec![PlaylistRule::Installed, PlaylistRule::NotHidden],
                },
            },
        };
        let json = playlist_rules_json(&pf);
        assert!(json.contains("\"Installed\""));
        assert!(json.contains("\"NotHidden\""));

        // Manual playlists produce an empty rules string.
        let manual = PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "test".into(),
            playlist: Playlist {
                id: "manual".into(),
                name: "Manual".into(),
                description: String::new(),
                content: PlaylistContent::Manual { app_ids: vec![1] },
            },
        };
        assert_eq!(playlist_rules_json(&manual), "");
    }

    #[test]
    fn build_playlist_from_edit_fields_creates_rule_based_playlist() {
        let mut app = VapourflyApp::new(None);
        app.playlist_edit_id = "installed-unplayed".into();
        app.playlist_edit_name = "Installed Unplayed".into();
        app.playlist_edit_rules = r#"[{"op":"Installed"},{"op":"NotHidden"}]"#.into();
        // App IDs are ignored when rules are present.
        app.playlist_edit_app_ids = "999, 9999".into();

        let pf = app.build_playlist_from_edit_fields().unwrap();
        assert_eq!(pf.playlist.id, "installed-unplayed");
        match &pf.playlist.content {
            PlaylistContent::Rules { rules } => assert_eq!(rules.len(), 2),
            PlaylistContent::Manual { .. } => panic!("expected rule-based playlist"),
        }
    }

    #[test]
    fn build_playlist_from_edit_fields_rejects_invalid_rules_json() {
        let mut app = VapourflyApp::new(None);
        app.playlist_edit_id = "bad".into();
        app.playlist_edit_name = "Bad".into();
        app.playlist_edit_rules = "not json".into();
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("Invalid Rules JSON"));
    }

    #[test]
    fn build_playlist_from_edit_fields_rejects_empty_rules_array() {
        let mut app = VapourflyApp::new(None);
        app.playlist_edit_id = "empty".into();
        app.playlist_edit_name = "Empty".into();
        app.playlist_edit_rules = "[]".into();
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("at least one rule"));
    }

    #[test]
    fn build_playlist_from_edit_fields_requires_id_and_name() {
        let mut app = VapourflyApp::new(None);
        // No id, no name.
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("ID is required"));

        app.playlist_edit_id = "has-id".into();
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("name is required"));
    }

    #[test]
    fn export_loaded_playlist_writes_selected_path() {
        let temp_dir = TempDir::new().unwrap();
        let export_path = temp_dir.path().join("deck-shortlist.json");
        let pf = PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "test".into(),
            playlist: Playlist {
                id: "deck-shortlist".into(),
                name: "Deck Shortlist".into(),
                description: "Games to play on Deck".into(),
                content: PlaylistContent::Manual {
                    app_ids: vec![427520, 730],
                },
            },
        };

        let mut app = VapourflyApp::new(None);
        app.playlist_last_import = Some(pf.clone());
        app.playlist_export_path = export_path.to_string_lossy().to_string();

        app.export_loaded_playlist().unwrap();

        let exported = playlist::import_playlist(&export_path).unwrap();
        assert_eq!(exported.playlist.id, pf.playlist.id);
        assert_eq!(manual_playlist_app_ids_csv(&exported), "730, 427520");
    }

    #[test]
    fn playlist_sync_dry_run_uses_slugged_playlist_id_and_deduped_app_ids() {
        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("cloud-storage-namespace-1.json");
        std::fs::write(&target_path, "[]").unwrap();
        let playlist = PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "test".into(),
            playlist: Playlist {
                id: "Deck Shortlist!".into(),
                name: "Deck Shortlist".into(),
                description: String::new(),
                content: PlaylistContent::Manual {
                    app_ids: vec![730, 427520, 730],
                },
            },
        };

        let plan = generate_dry_run_plan(
            target_path,
            &PendingAction::PlaylistSync(playlist),
            &[],
            "junk",
            &[],
        )
        .unwrap();

        assert_eq!(plan.diff.collections_changed[0].id, "deck-shortlist");
        assert_eq!(plan.diff.app_ids_added, vec![730, 427520]);
        match &plan.operations[0] {
            WriteOp::UpsertCollection { id, added, removed } => {
                assert_eq!(id, "deck-shortlist");
                assert_eq!(added, &vec![730, 427520]);
                assert!(removed.is_empty());
            }
            WriteOp::AddToHidden { .. } => panic!("expected collection upsert"),
        }
    }

    #[test]
    fn playlist_sync_resolves_rule_playlist_before_dry_run() {
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal");
        let scan_result = scan_library(&ScanOptions {
            steam_dir: fixtures.clone(),
            account: None,
            fixtures: Some(fixtures),
        })
        .unwrap();
        let playlist = PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "test".into(),
            playlist: Playlist {
                id: "installed-games".into(),
                name: "Installed Games".into(),
                description: String::new(),
                content: PlaylistContent::Rules {
                    rules: vec![PlaylistRule::Installed],
                },
            },
        };

        let mut app = VapourflyApp::new(None);
        app.scan_result = Some(scan_result);

        let action = app
            .resolve_dry_run_action(PendingAction::PlaylistSync(playlist))
            .unwrap();

        match action {
            PendingAction::PlaylistSync(pf) => match pf.playlist.content {
                PlaylistContent::Manual { app_ids } => {
                    assert_eq!(app_ids, vec![730, 427520]);
                }
                PlaylistContent::Rules { .. } => panic!("rule playlist should be resolved"),
            },
            other => panic!("unexpected action: {other:?}"),
        }
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

    #[test]
    fn load_collections_from_fixture_cloud_storage() {
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal");
        let app = VapourflyApp::new(Some(fixtures));
        let collections = app.load_collections_from_cloud().unwrap();
        let favorites = collections
            .iter()
            .find(|collection| collection.id == "favorite")
            .expect("favorite collection");
        assert_eq!(favorites.app_ids, vec![730, 427520]);
    }

    #[test]
    fn export_collections_writes_json_file() {
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal");
        let temp_dir = TempDir::new().unwrap();
        let export_path = temp_dir.path().join("collections.json");
        let mut app = VapourflyApp::new(Some(fixtures));
        app.collections_export_path = export_path.to_string_lossy().to_string();

        app.export_collections().unwrap();

        let exported: Vec<SteamCollection> =
            serde_json::from_str(&std::fs::read_to_string(&export_path).unwrap()).unwrap();
        assert!(
            exported
                .iter()
                .any(|collection| collection.id == "favorite")
        );
    }

    #[test]
    fn setup_diagnostics_reports_fixture_mode() {
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal");
        let mut app = VapourflyApp::new(Some(fixtures));
        app.run_setup_diagnostics();
        let report = app.setup_diagnostics.expect("diagnostics report");
        assert!(report.contains("Vapourfly Setup Diagnostics"));
        assert!(report.contains("Fixtures: enabled"));
        assert!(report.contains("Cloud storage: available"));
    }

    #[test]
    fn export_diagnostics_writes_json_file() {
        let temp_dir = TempDir::new().unwrap();
        let export_path = temp_dir.path().join("diagnostics.json");
        let mut app = VapourflyApp::new(None);
        app.diagnostics_export_path = export_path.to_string_lossy().to_string();

        app.export_diagnostics().unwrap();

        let exported: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&export_path).unwrap()).unwrap();
        assert_eq!(exported["version"], env!("CARGO_PKG_VERSION"));
        assert!(exported["timestamp"].is_string());
    }

    #[test]
    fn game_metadata_summary_formats_cached_fields() {
        let game = Game {
            app_id: 730,
            name: "Test".into(),
            app_type: SteamAppType::Game,
            installed: true,
            install_dir: None,
            library_folder: None,
            playtime_minutes: None,
            playtime_2wks_minutes: None,
            playtime_disconnected_minutes: None,
            last_played_unix: None,
            steam_collections: Vec::new(),
            is_hidden: false,
            is_junk: false,
            rawg: None,
            pcgw: None,
            steam_store: None,
            protondb: Some(ProtonDbData {
                tier: ProtonTier::Gold,
                confidence: None,
                score: None,
            }),
            hltb: Some(HltbData {
                main_story_seconds: Some(7_200),
                main_extra_seconds: None,
                completionist_seconds: None,
                source: HltbSource::IgdbGameTimeToBeat,
            }),
            igdb: Some(IgdbData {
                igdb_id: 1,
                name: "Test".into(),
                slug: None,
                rating_0_100: Some(80.0),
                total_rating_0_100: None,
                genres: vec!["RPG".into()],
                themes: Vec::new(),
                keywords: Vec::new(),
                similar_game_ids: Vec::new(),
                steam_app_id_confirmed: true,
                time_to_beat: None,
            }),
        };

        let summary = game_metadata_summary(&game);
        assert!(summary.contains("Gold"));
        assert!(summary.contains("2h 0m"));
        assert!(summary.contains("4.0/5"));
        assert!(summary.contains("RPG"));
    }
}
