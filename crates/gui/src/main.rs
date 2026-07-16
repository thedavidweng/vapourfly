use std::path::{Path, PathBuf};

use eframe::egui;
use egui::{Color32, RichText};
use vapourfly_core::config::VapourflyConfig;
use vapourfly_core::discover::{self, DiscoverOptions, DiscoverPick};
use vapourfly_core::display;
use vapourfly_core::disposition;
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

mod jobs;
mod theme;
use jobs::{fingerprint_u64, JobRunner, JobSlot, JobTicket, WorkflowKind};
use theme::*;

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
// Quick view (Library)
// ---------------------------------------------------------------------------

/// Quick filter preset for the Library grid. Selecting one sets the
/// appropriate filter toggles and clears the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum QuickView {
    #[default]
    All,
    Cozy,
    StoryRich,
    GreatOnDeck,
    ShortSessions,
}

impl QuickView {
    fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Cozy => "Cozy",
            Self::StoryRich => "Story-rich",
            Self::GreatOnDeck => "Great on Deck",
            Self::ShortSessions => "Short sessions",
        }
    }

    fn all() -> [Self; 5] {
        [
            Self::All,
            Self::Cozy,
            Self::StoryRich,
            Self::GreatOnDeck,
            Self::ShortSessions,
        ]
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

/// Result of a generator background job (Dynamic / Mood).
///
/// Carries the [`GeneratorIdentity`] captured at job start time so the
/// consumer can write the result to the correct stable slot even if the user
/// changes the chooser while the job is running. Without this, the poll would
/// re-read the current chooser and store the result under the wrong slot
/// (input drift). Input-drift protection is handled by the [`JobTicket`]
/// fingerprint compared in [`JobSlot::take_if`].
#[derive(Clone, Debug)]
struct GeneratorJobResult {
    identity: GeneratorIdentity,
    playlist: PlaylistFile,
}

// fingerprint_u64 lives in crate::jobs and is imported above.

// ---------------------------------------------------------------------------
// Playlists generator choosers (ticket 06)
// ---------------------------------------------------------------------------

/// Lightweight modal chooser opened from Playlists action bar.
///
/// Discover is intentionally absent — it is a top-level view (ADR-0005/0006).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum PlaylistChooser {
    #[default]
    None,
    Dynamic,
    Mood,
}

/// Right-workspace tab in the Playlists master-detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum PlaylistDetailTab {
    #[default]
    Games,
    Rules,
    Match,
}

/// Share sub-tab in the Playlists right workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum PlaylistShareTab {
    #[default]
    ShareCode,
    Json,
}

/// Match sub-tab in the Playlists right workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum PlaylistMatchTab {
    #[default]
    Owned,
    Missing,
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
// Background result channels (typed job slots with request IDs)
// ---------------------------------------------------------------------------

static SCAN_RESULT: JobSlot<ScanResult> = JobSlot::new();
static WRITE_RESULT: JobSlot<String> = JobSlot::new();
static ENRICH_RESULT: JobSlot<vapourfly_api::enrichment::EnrichmentSummary> = JobSlot::new();
static DRY_RUN_RESULT: JobSlot<vapourfly_core::models::WritePlan> = JobSlot::new();
static JUNK_PREVIEW_RESULT: JobSlot<Vec<JunkDecision>> = JobSlot::new();
static RECOMMEND_RESULT: JobSlot<Vec<Recommendation>> = JobSlot::new();
static DISCOVER_RESULT: JobSlot<(Vec<DiscoverPick>, PlaylistFile)> = JobSlot::new();
static DYNAMIC_RESULT: JobSlot<GeneratorJobResult> = JobSlot::new();
static MOOD_RESULT: JobSlot<GeneratorJobResult> = JobSlot::new();
static PLAYLIST_MATCH_RESULT: JobSlot<PlaylistMatchReport> = JobSlot::new();
/// Background-prepared library snapshot (hydrated games, pre-junk-classification).
/// Produced off the egui frame so the Library view does not re-hydrate from
/// the disk cache every frame.
static PREPARED_LIBRARY_RESULT: JobSlot<PreparedLibrarySnapshot> = JobSlot::new();

/// Cached library snapshot: hydrated games + the manual overrides that were
/// loaded alongside them, plus the fingerprint identifying the inputs used to
/// produce them. The games are pre-junk-classification so different JunkMode
/// callers can classify on top without re-hydrating; the overrides are captured
/// here so `prepared_games` never reads the overrides file on the egui frame.
#[derive(Clone, Debug)]
struct PreparedLibrarySnapshot {
    fingerprint: u64,
    games: Vec<Game>,
    overrides: ManualOverrides,
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

    /// Demo mode (`--ui-demo`): deterministic fixture data, no real Steam writes.
    ui_demo: bool,

    /// Light or dark visual system (ADR-0006).
    theme_mode: ThemeMode,

    // Config
    config: Option<VapourflyConfig>,

    /// Optional override for the playlist store directory (tests inject a temp dir).
    playlist_store_dir: Option<PathBuf>,
    /// Cache directory (temp dir in --ui-demo mode, default otherwise).
    cache_dir: PathBuf,
    /// Optional override for the manual overrides JSON path. In --ui-demo mode
    /// this points inside the demo temp root so the real platform default path
    /// is never read.
    manual_overrides_path: Option<PathBuf>,
    /// Root of the --ui-demo temp tree (unique per launch). `None` outside demo.
    /// Kept so tests can assert demo I/O stays inside this root.
    #[allow(dead_code)]
    demo_root: Option<PathBuf>,

    // Library view
    search_query: String,
    /// When true, only installed games appear in the grid.
    filter_installed_only: bool,
    /// When true, hidden games are excluded.
    filter_not_hidden: bool,
    /// When true, junk-flagged games are excluded.
    filter_not_junk: bool,
    /// Advanced filter: genre text match (case-insensitive substring).
    filter_genre: String,
    /// Advanced filter: ProtonDB tier threshold (show games at or above).
    filter_proton_tier: Option<ProtonTier>,
    /// Advanced filter: only games with full controller support (PCGW).
    filter_deck_compatible: bool,
    /// Advanced filter: only unplayed games (0 playtime minutes).
    filter_unplayed_only: bool,
    /// Quick view selector for the Library grid.
    library_quick_view: QuickView,
    /// "Load more" pagination: how many games to show (incremented by 48).
    library_visible_count: usize,
    /// Selected game card AppID (enables Recommend without hover).
    library_selected_app_id: Option<u32>,
    /// Junk is a Library panel (not a sidebar destination).
    show_junk_panel: bool,

    // Junk panel (opened from Library)
    junk_mode: JunkModeChoice,
    junk_results: Vec<JunkDecision>,
    junk_selected: std::collections::HashSet<u32>,
    junk_collection_name: String,
    junk_show_all_evaluated: bool,

    // Recommendations view
    recommend_minutes: String,
    recommend_count: String,
    recommend_seed: String,
    recommend_deck: bool,
    recommend_installed_only: bool,
    recommend_results: Vec<Recommendation>,
    /// The `RecommendRequest` captured when the current preview was started,
    /// so Match % is computed against the submitted inputs (e.g. Deck mode)
    /// rather than the current inputs which may have changed mid-job.
    recommend_request_at_start: Option<RecommendRequest>,
    /// Selected recommendation AppID for "Why this pick?" panel.
    recommend_selected: Option<u32>,
    /// Search filter for the seed autocomplete.
    recommend_seed_search: String,

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
    /// Ids present in the local playlist store (for Load existing).
    playlist_store_ids: Vec<String>,
    /// Whether [`playlist_store_ids`] has been loaded at least once this session.
    playlist_store_ids_loaded: bool,
    /// Selected id in the Load existing combo (empty = none).
    playlist_load_selected: String,
    /// Open generator chooser (Dynamic / Mood only).
    playlist_chooser: PlaylistChooser,
    /// Master-detail: active tab in the right workspace (Games/Rules/Match).
    playlist_detail_tab: PlaylistDetailTab,
    /// Master-detail: game search query for Add/Remove in Games tab.
    playlist_game_search: String,
    /// Master-detail: show Advanced JSON editor instead of visual rules.
    playlist_show_advanced_json: bool,
    /// Master-detail: pending duplicate-ID replacement (for confirm dialog).
    playlist_dup_id_confirm: Option<(String, PlaylistFile)>,
    /// Master-detail: show Import sub-route panel.
    playlist_show_import: bool,
    /// Master-detail: active share tab (ShareCode / Json).
    playlist_share_tab: PlaylistShareTab,
    /// Master-detail: active match sub-tab (Owned / Missing).
    playlist_match_sub_tab: PlaylistMatchTab,
    dynamic_template: String,
    dynamic_minutes: String,
    dynamic_count: String,
    editorial_mood: String,

    // Discover view (top-level; no longer nested under Playlists)
    discover_seed: String,
    discover_count: String,
    /// Last playlist generated from the Discover view (owned by Discover UI).
    discover_last_playlist: Option<PlaylistFile>,
    /// On-page Discover results with scores and reason codes.
    discover_results: Vec<DiscoverPick>,

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

    // Background job runner (request IDs + stale-result protection)
    job_runner: JobRunner,
    /// egui::Context captured each frame so non-UI methods (e.g. playlist
    /// adoption) can spawn background work and request repaints. Set at the
    /// top of `ui()`; `None` only before the first frame.
    ctx: Option<egui::Context>,
    scan_job_id: Option<JobTicket>,
    write_job_id: Option<JobTicket>,
    enrich_job_id: Option<JobTicket>,
    dry_run_job_id: Option<JobTicket>,
    junk_preview_job_id: Option<JobTicket>,
    recommend_job_id: Option<JobTicket>,
    discover_job_id: Option<JobTicket>,
    dynamic_job_id: Option<JobTicket>,
    mood_job_id: Option<JobTicket>,
    playlist_match_job_id: Option<JobTicket>,
    /// Cached library snapshot (hydrated games, pre-junk-classification).
    /// Reused across frames when the fingerprint matches so the Library view
    /// does not re-hydrate from the disk cache every frame.
    prepared_snapshot: Option<PreparedLibrarySnapshot>,
    /// JobId of an in-flight background library prepare.
    prepare_job_id: Option<JobTicket>,
    /// Fingerprint of the in-flight prepare (to set on the snapshot).
    prepare_fingerprint: Option<u64>,
    /// Increments each time a cache refresh completes, so the library snapshot
    /// is invalidated and re-hydrated with the new cache data.
    cache_refresh_generation: u64,
    /// Monotonic counter incremented every time a new scan result is accepted
    /// (real scan or demo refresh). Unlike `scan_job_id` (which is `None`
    /// after a scan completes) or the game count (which can stay the same when
    /// only content changes — playtime, hidden state, collections), this
    /// always changes, so the prepare fingerprint reliably invalidates the
    /// cached snapshot. See [`VapourflyApp::library_prepare_fingerprint`].
    scan_generation: u64,

    // Loading flags for off-frame operations
    junk_preview_loading: bool,
    recommend_loading: bool,
    discover_loading: bool,
    dynamic_loading: bool,
    mood_loading: bool,
    playlist_match_loading: bool,
}

fn mask_steam_id(id: &str) -> String {
    display::mask_id(id)
}

fn proton_tier_label(tier: ProtonTier) -> &'static str {
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
        parts.push(proton_tier_label(proton.tier).to_string());
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

/// Library grid filters. Quick-view presets set the three toggles; advanced
/// filters add genre, ProtonDB tier, deck compatibility, and unplayed.
#[derive(Clone, Debug, Default, PartialEq)]
struct LibraryFilters {
    installed_only: bool,
    not_hidden: bool,
    not_junk: bool,
    is_hidden_only: bool,
    is_junk_only: bool,
    search: String,
    genre: String,
    proton_tier: Option<ProtonTier>,
    deck_compatible: bool,
    unplayed_only: bool,
    hltb_max_minutes: Option<u32>,
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
    if filters.is_hidden_only && !game.is_hidden {
        return false;
    }
    if filters.is_junk_only && !game.is_junk {
        return false;
    }
    if filters.unplayed_only && game.playtime_minutes.unwrap_or(0) > 0 {
        return false;
    }
    if !filters.search.is_empty() {
        let q = filters.search.to_lowercase();
        if !game.name.to_lowercase().contains(&q) && !game.app_id.to_string().contains(&q) {
            return false;
        }
    }
    if !filters.genre.is_empty() {
        let g = filters.genre.to_lowercase();
        let has_genre = game
            .igdb
            .as_ref()
            .is_some_and(|i| i.genres.iter().any(|x| x.to_lowercase().contains(&g)))
            || game
                .steam_store
                .as_ref()
                .is_some_and(|s| s.genres.iter().any(|x| x.to_lowercase().contains(&g)));
        if !has_genre {
            return false;
        }
    }
    if let Some(tier) = filters.proton_tier {
        let matches = game.protondb.as_ref().is_some_and(|p| {
            // Native > Platinum > Gold > Silver > Bronze > Borked
            let order = |t: ProtonTier| match t {
                ProtonTier::Native => 6,
                ProtonTier::Platinum => 5,
                ProtonTier::Gold => 4,
                ProtonTier::Silver => 3,
                ProtonTier::Bronze => 2,
                ProtonTier::Borked => 1,
                ProtonTier::Unknown => 0,
            };
            order(p.tier) >= order(tier)
        });
        if !matches {
            return false;
        }
    }
    if filters.deck_compatible {
        let has_deck = game
            .pcgw
            .as_ref()
            .is_some_and(|p| p.controller_support == ControllerSupport::Full);
        if !has_deck {
            return false;
        }
    }
    if let Some(max_minutes) = filters.hltb_max_minutes {
        // Prefer the canonical, normalized HLTB main_story_seconds; fall back
        // to the raw IGDB time_to_beat.normally_seconds for games without HLTB.
        let completion_seconds = game
            .hltb
            .as_ref()
            .and_then(|h| h.main_story_seconds)
            .or_else(|| {
                game.igdb
                    .as_ref()
                    .and_then(|i| i.time_to_beat.as_ref())
                    .and_then(|t| t.normally_seconds)
            });
        let fits = completion_seconds.is_some_and(|secs| secs / 60 <= max_minutes);
        if !fits {
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

/// Stable hash of a playlist's full content (manual AppIDs or rules JSON), so
/// the Playlist Match fingerprint changes when the content is edited — not just
/// when the playlist id changes.
fn playlist_content_hash(pf: &PlaylistFile) -> u64 {
    match &pf.playlist.content {
        PlaylistContent::Manual { app_ids } => fingerprint_u64(&format!("manual:{app_ids:?}")),
        PlaylistContent::Rules { rules } => {
            fingerprint_u64(&format!("rules:{}", serde_json::to_string(rules).unwrap_or_default()))
        }
    }
}

/// Fingerprint for a dry-run job: the target action + all input AppIDs (junk
/// selection, recommend results, or playlist AppIDs). Used so a dry-run is
/// invalidated if the inputs change before the background job completes.
fn dry_run_fingerprint(
    action: &PendingAction,
    junk_selected: &std::collections::HashSet<u32>,
    recommend_results: &[Recommendation],
) -> String {
    let mut app_ids: Vec<u32> = match action {
        PendingAction::JunkApply | PendingAction::JunkHide => {
            junk_selected.iter().copied().collect()
        }
        PendingAction::RecommendCollection => {
            recommend_results.iter().map(|r| r.app_id).collect()
        }
        PendingAction::PlaylistSync(pf) => manual_playlist_app_ids_csv(pf)
            .split(',')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .collect(),
        PendingAction::BackupRestore(_) => vec![],
    };
    app_ids.sort_unstable();
    format!("dry_run:{action:?}:apps={app_ids:?}")
}

// ---------------------------------------------------------------------------
// Data Sources presentation helpers (ticket 08)
// ---------------------------------------------------------------------------

/// Human-facing label for an enrichment source id (`igdb` → `IGDB`).
fn source_display_name(source_id: &str) -> &'static str {
    match source_id {
        "igdb" => "IGDB",
        "rawg" => "RAWG",
        "protondb" => "ProtonDB",
        "pcgw" => "PCGW",
        "hltb" => "HLTB",
        "steam-store" => "Steam Store",
        _ => "Unknown",
    }
}

/// Credential readiness signal for the Data Sources table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CredentialSignal {
    /// IGDB/RAWG env credentials are present.
    Configured,
    /// IGDB/RAWG env credentials are missing.
    Missing,
    /// Source never needs credentials (ProtonDB, PCGW, Steam Store).
    NotRequired,
    /// HLTB is optional / feature-gated — not a hard credential failure.
    Optional,
}

impl CredentialSignal {
    fn label(self) -> &'static str {
        match self {
            CredentialSignal::Configured => "Configured",
            CredentialSignal::Missing => "Missing",
            CredentialSignal::NotRequired => "None needed",
            CredentialSignal::Optional => "Optional",
        }
    }
}

/// Map a source id to its credential signal given current env state.
fn source_credential_signal(source_id: &str, has_igdb: bool, has_rawg: bool) -> CredentialSignal {
    match source_id {
        "igdb" => {
            if has_igdb {
                CredentialSignal::Configured
            } else {
                CredentialSignal::Missing
            }
        }
        "rawg" => {
            if has_rawg {
                CredentialSignal::Configured
            } else {
                CredentialSignal::Missing
            }
        }
        "hltb" => CredentialSignal::Optional,
        "protondb" | "pcgw" | "steam-store" => CredentialSignal::NotRequired,
        _ => CredentialSignal::NotRequired,
    }
}

/// Whether a per-source refresh action should be enabled (credentials + offline).
#[allow(clippy::fn_params_excessive_bools)] // pure gate over discrete UI flags
fn source_refresh_enabled(
    source_id: &str,
    has_igdb: bool,
    has_rawg: bool,
    offline: bool,
    loading: bool,
) -> bool {
    if offline || loading {
        return false;
    }
    match source_credential_signal(source_id, has_igdb, has_rawg) {
        CredentialSignal::Missing => false,
        CredentialSignal::Configured
        | CredentialSignal::NotRequired
        | CredentialSignal::Optional => true,
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
                .color(t().text_primary),
        );
        ui.add_space(SP_1);
        ui.label(
            RichText::new(subtitle)
                .size(TS_BODY)
                .color(t().text_secondary),
        );
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
                    .color(t().text_primary),
            );
            ui.add_space(SP_1);
            ui.label(
                RichText::new(subtitle)
                    .size(TS_BODY)
                    .color(t().text_secondary),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            actions(ui);
        });
    });
    ui.add_space(SP_3);
}

fn section_card(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE
        .fill(t().surface_raised)
        .stroke(egui::Stroke::new(1.0, t().border_soft))
        .inner_margin(egui::Margin::same(m(SP_4)))
        .corner_radius(CORNER_MD)
        .show(ui, |ui| {
            ui.label(
                RichText::new(title)
                    .size(TS_MD)
                    .strong()
                    .color(t().text_primary),
            );
            ui.add_space(SP_2);
            body(ui);
        });
    ui.add_space(SP_3);
}

fn error_banner(ui: &mut egui::Ui, msg: &str) {
    egui::Frame::NONE
        .fill(t().error_soft)
        .stroke(egui::Stroke::new(1.0, t().error))
        .inner_margin(egui::Margin::symmetric(m(SP_3), m(SP_2)))
        .corner_radius(CORNER_SM)
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("\u{26A0} {msg}"))
                    .size(TS_BODY)
                    .color(t().error),
            );
        });
}

fn success_banner(ui: &mut egui::Ui, msg: &str) {
    egui::Frame::NONE
        .fill(t().success_soft)
        .stroke(egui::Stroke::new(1.0, t().success))
        .inner_margin(egui::Margin::symmetric(m(SP_3), m(SP_2)))
        .corner_radius(CORNER_SM)
        .show(ui, |ui| {
            ui.label(
                RichText::new(format!("\u{2713} {msg}"))
                    .size(TS_BODY)
                    .color(t().success),
            );
        });
}

fn metric_pill(ui: &mut egui::Ui, label: &str, value: String) {
    egui::Frame::NONE
        .fill(t().surface_sunken)
        .inner_margin(egui::Margin::symmetric(m(SP_3), m(SP_1)))
        .corner_radius(CORNER_PILL)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).size(TS_XS).color(t().text_muted));
                ui.label(
                    RichText::new(value)
                        .size(TS_SM)
                        .strong()
                        .color(t().text_primary),
                );
            });
        });
}

fn stat_inline(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(TS_SM).color(t().text_muted));
        ui.label(RichText::new(value).size(TS_BODY).color(t().text_primary));
    });
}

fn filter_toggle(ui: &mut egui::Ui, state: &mut bool, label: &str) {
    let btn = egui::Button::new(RichText::new(label).size(TS_SM).color(if *state {
        t().text_inverse
    } else {
        t().text_secondary
    }))
    .fill(if *state { t().accent } else { t().surface })
    .stroke(if *state {
        egui::Stroke::NONE
    } else {
        egui::Stroke::new(1.0, t().border_soft)
    })
    .corner_radius(CORNER_PILL);
    if ui.add(btn).clicked() {
        *state = !*state;
    }
}

fn empty_state(ui: &mut egui::Ui, icon: &str, title: &str, subtitle: &str) {
    ui.add_space(SP_6);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(icon).size(48.0).color(t().text_muted));
        ui.add_space(SP_2);
        ui.label(
            RichText::new(title)
                .size(TS_LG)
                .strong()
                .color(t().text_primary),
        );
        ui.add_space(SP_1);
        ui.label(
            RichText::new(subtitle)
                .size(TS_BODY)
                .color(t().text_secondary),
        );
    });
    ui.add_space(SP_6);
}

/// Deterministic placeholder palette for game artwork. Indexed by
/// `app_id % PALETTE.len()` so the same game always gets the same colors.
/// Each entry is (top_block, bottom_block) — two distinct shades that make
/// the placeholder visually identifiable without any network fetch.
const ARTWORK_PALETTE: [(Color32, Color32); 8] = [
    (Color32::from_rgb(0x4C, 0x6E, 0xF0), Color32::from_rgb(0x2A, 0x4A, 0xC0)),
    (Color32::from_rgb(0xE1, 0x70, 0x55), Color32::from_rgb(0xB5, 0x4A, 0x35)),
    (Color32::from_rgb(0x2E, 0xC4, 0xB6), Color32::from_rgb(0x1A, 0x9A, 0x8E)),
    (Color32::from_rgb(0xF4, 0xC4, 0x30), Color32::from_rgb(0xC0, 0x98, 0x18)),
    (Color32::from_rgb(0x9B, 0x59, 0xB6), Color32::from_rgb(0x72, 0x3C, 0x8A)),
    (Color32::from_rgb(0x34, 0x98, 0xDB), Color32::from_rgb(0x21, 0x70, 0xA8)),
    (Color32::from_rgb(0xE6, 0x7E, 0x22), Color32::from_rgb(0xB0, 0x5C, 0x12)),
    (Color32::from_rgb(0x1A, 0xBC, 0x9C), Color32::from_rgb(0x12, 0x8E, 0x76)),
];

/// Render the deterministic placeholder for a game: two color blocks (top 60%,
/// bottom 40%), the title initial centered, and the AppID in the bottom-right
/// corner. Used when CDN art is unavailable (demo mode, offline mode, or
/// network failure).
fn render_artwork_placeholder(
    ui: &mut egui::Ui,
    app_id: u32,
    name: &str,
    width: f32,
    height: f32,
) {
    let (top, bottom) = ARTWORK_PALETTE[(app_id as usize) % ARTWORK_PALETTE.len()];
    let initial = name
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_default();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
    if ui.is_rect_visible(rect) {
        let split = rect.top() + rect.height() * 0.6;
        let top_rect = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), split));
        let bot_rect = egui::Rect::from_min_max(egui::pos2(rect.left(), split), rect.max);
        ui.painter().rect_filled(top_rect, CORNER_SM, top);
        ui.painter().rect_filled(bot_rect, CORNER_SM, bottom);
        // Title initial, centered in the top block.
        let initial_galley = ui.painter().layout(
            initial,
            egui::FontId::proportional(height * 0.35),
            Color32::WHITE,
            width,
        );
        ui.painter().galley(
            top_rect.center() - egui::vec2(initial_galley.size().x * 0.5, initial_galley.size().y * 0.5),
            initial_galley,
            Color32::WHITE,
        );
        // AppID in the bottom-right corner.
        let id_galley = ui.painter().layout(
            app_id.to_string(),
            egui::FontId::monospace(height * 0.12),
            Color32::from_white_alpha(180),
            width,
        );
        ui.painter().galley(
            egui::pos2(
                bot_rect.right() - id_galley.size().x - f32::from(m(SP_1)),
                bot_rect.bottom() - id_galley.size().y - f32::from(m(SP_1)),
            ),
            id_galley,
            Color32::from_white_alpha(180),
        );
    }
}

/// Shared game artwork component. Renders CDN art when available, falling back
/// to a deterministic placeholder when:
/// - `demo_or_offline` is true (ui_demo or offline_mode — CDN is banned), or
/// - the CDN image fails to load (network error, 404, etc.).
///
/// This is the single source of truth for game art across all views (Library
/// cards, collection collages, recommendation cards, playlist heroes).
fn game_artwork(
    ui: &mut egui::Ui,
    app_id: u32,
    name: &str,
    width: f32,
    height: f32,
    demo_or_offline: bool,
    uri: &str,
) {
    if demo_or_offline {
        render_artwork_placeholder(ui, app_id, name, width, height);
        return;
    }
    // Try to load the CDN image. If it fails (network error, 404), fall back
    // to the placeholder instead of egui's default "⚠" error glyph.
    let image = egui::Image::from_uri(uri)
        .fit_to_exact_size(egui::vec2(width, height))
        .corner_radius(CORNER_SM)
        .bg_fill(t().surface_sunken)
        .show_loading_spinner(true)
        .alt_text(format!("{name} cover"));
    let load_result = image.load_for_size(ui.ctx(), egui::vec2(width, height));
    match load_result {
        Ok(egui::load::TexturePoll::Ready { .. }) | Ok(egui::load::TexturePoll::Pending { .. }) => {
            ui.add(image);
        }
        Err(_) => {
            // Network failure → deterministic placeholder.
            render_artwork_placeholder(ui, app_id, name, width, height);
        }
    }
}

/// Library card artwork (landscape capsule). Uses the shared `game_artwork`
/// component with the Steam header capsule URL.
fn game_image(ui: &mut egui::Ui, app_id: u32, name: &str, demo_or_offline: bool) {
    game_artwork(
        ui,
        app_id,
        name,
        POSTER_W,
        POSTER_H,
        demo_or_offline,
        &steam_capsule_uri(app_id),
    );
}

fn app_id_tag(ui: &mut egui::Ui, app_id: u32) {
    egui::Frame::NONE
        .fill(t().surface_sunken)
        .inner_margin(egui::Margin::symmetric(m(SP_2), m(SP_1)))
        .corner_radius(CORNER_SM)
        .show(ui, |ui| {
            ui.label(
                RichText::new(app_id.to_string())
                    .size(TS_XS)
                    .color(t().text_muted)
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
        ui.label(RichText::new(label).size(TS_BODY).color(t().text_secondary));
        field(ui);
    });
}

/// Stacked label + control + optional hint for Settings-style forms.
fn labeled_field(
    ui: &mut egui::Ui,
    label: &str,
    hint: Option<&str>,
    field: impl FnOnce(&mut egui::Ui),
) {
    ui.vertical(|ui| {
        ui.label(
            RichText::new(label)
                .size(TS_SM)
                .strong()
                .color(t().text_secondary),
        );
        ui.add_space(SP_1);
        field(ui);
        if let Some(hint) = hint {
            ui.add_space(SP_1);
            ui.label(RichText::new(hint).size(TS_XS).color(t().text_muted));
        }
    });
}

fn credential_badge(ui: &mut egui::Ui, signal: CredentialSignal) {
    match signal {
        CredentialSignal::Configured => {
            status_badge(ui, signal.label(), t().success_soft, t().success);
        }
        CredentialSignal::Missing => {
            status_badge(ui, signal.label(), t().error_soft, t().error);
        }
        CredentialSignal::NotRequired => {
            status_badge(ui, signal.label(), t().surface_sunken, t().text_muted);
        }
        CredentialSignal::Optional => {
            status_badge(ui, signal.label(), t().accent_soft, t().accent_text);
        }
    }
}

// ---------------------------------------------------------------------------
// Shared chrome primitives (primary / secondary / ghost buttons)
// ---------------------------------------------------------------------------

fn primary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(primary_button_widget(label))
}

/// The [`egui::Button`] widget used by [`primary_button`], for use with
/// `ui.add_enabled(enabled, …)` so the button can be disabled when the
/// prepared library is not yet ready.
fn primary_button_widget(label: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(label).size(TS_SM).color(t().text_inverse))
        .fill(t().accent)
        .stroke(egui::Stroke::NONE)
        .corner_radius(CORNER_SM)
}

fn secondary_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(TS_SM).color(t().text_primary))
            .fill(t().surface)
            .stroke(egui::Stroke::new(1.0, t().border_soft))
            .corner_radius(CORNER_SM),
    )
}

fn ghost_button(ui: &mut egui::Ui, label: &str) -> egui::Response {
    ui.add(
        egui::Button::new(RichText::new(label).size(TS_SM).color(t().text_secondary))
            .fill(Color32::TRANSPARENT)
            .stroke(egui::Stroke::NONE)
            .corner_radius(CORNER_SM),
    )
}

// ---------------------------------------------------------------------------
// Monochrome line icons for sidebar navigation
// ---------------------------------------------------------------------------

const NAV_ICON_SIZE: f32 = 22.0;

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
                painter.line_segment([egui::pos2(c.x - s, y), egui::pos2(c.x + s, y)], stroke);
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

/// Sidebar nav tile: a centered line icon and compact label. The visual shape
/// mirrors the reference rail while preserving normal egui hit targets.
fn nav_item(ui: &mut egui::Ui, view: View, selected: bool) -> egui::Response {
    let label = view.label();
    let row_width = SIDEBAR_WIDTH - f32::from(m(SP_3)) * 2.0;
    let desired = egui::vec2(row_width, 66.0);
    let (rect, response) = ui.allocate_exact_size(desired, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered() && !selected;
        let fill = if selected {
            t().accent_soft
        } else if hovered {
            t().surface_muted
        } else {
            Color32::TRANSPARENT
        };
        let text_color = if selected {
            t().accent_text
        } else if hovered {
            t().text_primary
        } else {
            t().text_secondary
        };
        let icon_color = if selected {
            t().accent
        } else if hovered {
            t().text_primary
        } else {
            t().text_secondary
        };

        let painter = ui.painter();
        painter.rect_filled(rect, CORNER_MD, fill);

        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + 24.0),
            egui::vec2(NAV_ICON_SIZE, NAV_ICON_SIZE),
        );
        paint_nav_icon(painter, icon_rect, view, icon_color);

        painter.text(
            egui::pos2(rect.center().x, rect.bottom() - 13.0),
            egui::Align2::CENTER_CENTER,
            label,
            egui::FontId::proportional(TS_SM),
            text_color,
        );
    }

    response
}

/// Compact value-over-label metric for the top chrome.
fn top_bar_metric(ui: &mut egui::Ui, value: impl Into<String>, label: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.label(
            RichText::new(value.into())
                .size(TS_MD)
                .strong()
                .color(t().text_primary),
        );
        ui.label(RichText::new(label).size(TS_XS).color(t().text_muted));
    });
}

/// Compact label-over-value metric for the Library insights rail.
fn insight_metric(ui: &mut egui::Ui, label: &str, value: String) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = SP_2;
        ui.label(RichText::new(label).size(TS_SM).color(t().text_secondary));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(value)
                    .size(TS_SM)
                    .strong()
                    .color(t().text_primary),
            );
        });
    });
}

/// Build a unique per-launch temp root for --ui-demo mode.
///
/// Uses nanosecond timestamp + process id so concurrent demo sessions and
/// repeated launches do not share state. The fixed `vapourfly-ui-demo` path is
/// avoided to keep demo sessions deterministic.
fn unique_demo_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("vapourfly-ui-demo-{nanos}-{pid}"))
}

impl VapourflyApp {
    fn new(fixtures_path: Option<PathBuf>, ui_demo: bool) -> Self {
        // -- Configuration ---------------------------------------------------
        // In --ui-demo mode, build an isolated in-memory config rooted at a
        // unique temp directory. The real Vapourfly config file, Steam paths,
        // account directories, and API credentials are NEVER read.
        let (config, playlist_store_dir, cache_root, manual_overrides_path, demo_root) = if ui_demo
        {
            let root = unique_demo_root();
            let _ = std::fs::create_dir_all(&root);
            let playlists = root.join("playlists");
            let cache = root.join("cache");
            let steam = root.join("steam");
            let overrides = root.join("manual_overrides.json");
            let _ = std::fs::create_dir_all(&playlists);
            let _ = std::fs::create_dir_all(&cache);
            let _ = std::fs::create_dir_all(&steam);
            let demo_config = VapourflyConfig {
                steam_dir: steam,
                account: Some("demo_user".into()),
                cache_root: root.clone(),
                app_data_root: root.clone(),
                has_igdb_credentials: false,
                has_rawg_credentials: false,
                cc: "US".into(),
                lang: "english".into(),
                backup_retention_count: 5,
            };
            (
                Some(demo_config),
                Some(playlists),
                cache,
                Some(overrides),
                Some(root),
            )
        } else {
            let cfg = VapourflyConfig::from_cli_and_env(vapourfly_core::config::CliOverrides {
                steam_dir: fixtures_path.clone(),
                account: None,
            })
            .ok();
            (
                cfg,
                None,
                vapourfly_core::config::default_cache_dir(),
                None,
                None,
            )
        };

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
        let source_statuses = vapourfly_api::enrichment::source_status(&cache_root);

        Self {
            scan_result: None,
            current_view: View::Library,
            loading: false,
            error: None,
            success_msg: None,
            fixtures_path,
            ui_demo,
            theme_mode: ThemeMode::Light,

            config,
            playlist_store_dir,
            cache_dir: cache_root,
            manual_overrides_path,
            demo_root,

            search_query: String::new(),
            filter_installed_only: false,
            filter_not_hidden: false,
            filter_not_junk: false,
            filter_genre: String::new(),
            filter_proton_tier: None,
            filter_deck_compatible: false,
            filter_unplayed_only: false,
            library_quick_view: QuickView::All,
            library_visible_count: 48,
            library_selected_app_id: None,
            show_junk_panel: false,

            junk_mode: JunkModeChoice::Default,
            junk_results: Vec::new(),
            junk_selected: std::collections::HashSet::new(),
            junk_collection_name: "junk".into(),
            junk_show_all_evaluated: false,

            recommend_minutes: "120".into(),
            recommend_count: "5".into(),
            recommend_seed: String::new(),
            recommend_deck: false,
            recommend_installed_only: false,
            recommend_results: Vec::new(),
            recommend_request_at_start: None,
            recommend_selected: None,
            recommend_seed_search: String::new(),

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
            playlist_store_ids: Vec::new(),
            playlist_store_ids_loaded: false,
            playlist_load_selected: String::new(),
            playlist_chooser: PlaylistChooser::None,
            playlist_detail_tab: PlaylistDetailTab::Games,
            playlist_game_search: String::new(),
            playlist_show_advanced_json: false,
            playlist_dup_id_confirm: None,
            playlist_show_import: false,
            playlist_share_tab: PlaylistShareTab::ShareCode,
            playlist_match_sub_tab: PlaylistMatchTab::Owned,
            dynamic_template: DynamicTemplate::DeckSession.id().into(),
            dynamic_minutes: "90".into(),
            dynamic_count: "25".into(),
            editorial_mood: EditorialMood::all().first().map_or("", |m| m.id()).into(),

            discover_seed: String::new(),
            discover_count: "20".into(),
            discover_last_playlist: None,
            discover_results: Vec::new(),

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

            job_runner: JobRunner::new(),
            ctx: None,
            scan_job_id: None,
            write_job_id: None,
            enrich_job_id: None,
            dry_run_job_id: None,
            junk_preview_job_id: None,
            recommend_job_id: None,
            discover_job_id: None,
            dynamic_job_id: None,
            mood_job_id: None,
            playlist_match_job_id: None,
            prepared_snapshot: None,
            prepare_job_id: None,
            prepare_fingerprint: None,
            cache_refresh_generation: 0,
            scan_generation: 0,
            junk_preview_loading: false,
            recommend_loading: false,
            discover_loading: false,
            dynamic_loading: false,
            mood_loading: false,
            playlist_match_loading: false,
        }
    }

    /// Populate deterministic demo data for `--ui-demo` mode.
    ///
    /// Provides enough in-memory data to render every page in a meaningful
    /// loaded state: 24 games with varied metadata, 5 Playlists, 4 Steam
    /// Collections, junk decisions, recommendation results, discover results,
    /// source statuses, accounts, and backups. No real Steam writes are
    /// possible in demo mode.
    fn populate_demo_data(&mut self) {
        use vapourfly_core::models::{
            HltbData, HltbSource, IgdbData, PcgwData, ProtonDbData, ProtonTier, RawgData,
            SteamStoreDetails, SteamStorePlatforms,
        };

        // -- 24 games with varied metadata -----------------------------------
        let demo_games: Vec<Game> = (0..24)
            .map(|i| {
                let app_id = 1000 + i;
                let name = format!("Demo Game {i:02}");
                Game {
                    app_id,
                    name,
                    app_type: SteamAppType::Game,
                    installed: i < 18,
                    install_dir: Some(format!("demo_{i}").into()),
                    library_folder: None,
                    playtime_minutes: Some(match i {
                        0 => 320,
                        1 => 180,
                        2 => 60,
                        3 => 0,
                        4 => 500,
                        5 => 15,
                        6 => 240,
                        7 => 0,
                        8 => 90,
                        9 => 0,
                        10 => 1200,
                        11 => 5,
                        12 => 0,
                        13 => 45,
                        14 => 0,
                        15 => 300,
                        16 => 0,
                        17 => 10,
                        _ => 0,
                    }),
                    playtime_2wks_minutes: if i < 6 { Some(30) } else { None },
                    playtime_disconnected_minutes: None,
                    last_played_unix: if i < 3 {
                        Some(chrono::Utc::now().timestamp() - i64::from(i) * 86_400)
                    } else {
                        None
                    },
                    steam_collections: if i % 5 == 0 {
                        vec!["Favorites".into()]
                    } else {
                        vec![]
                    },
                    is_hidden: i == 9 || i == 14,
                    is_junk: i == 11 || i == 17,
                    hltb: if i % 3 == 0 {
                        Some(HltbData {
                            main_story_seconds: Some(match i {
                                0 => 36000,  // 10h
                                3 => 7200,   // 2h
                                6 => 18000,  // 5h
                                9 => 5400,   // 1.5h
                                12 => 900,   // 15m (short sessions)
                                15 => 25200, // 7h
                                18 => 10800, // 3h
                                21 => 14400, // 4h
                                _ => 7200,
                            }),
                            main_extra_seconds: None,
                            completionist_seconds: None,
                            source: HltbSource::IgdbGameTimeToBeat,
                        })
                    } else {
                        None
                    },
                    igdb: if i % 2 == 0 {
                        Some(IgdbData {
                            igdb_id: u64::from(app_id),
                            name: format!("Demo Game {i:02}"),
                            slug: None,
                            rating_0_100: Some(match i {
                                0 => 85.0,
                                3 => 78.0,
                                6 => 92.0,
                                9 => 45.0,
                                12 => 88.0,
                                _ => 70.0,
                            }),
                            total_rating_0_100: None,
                            genres: match i {
                                0 => vec!["Shooter".into(), "Action".into()],
                                3 => vec!["Cozy".into(), "Casual".into()],
                                6 => vec!["Story Rich".into(), "Adventure".into()],
                                12 => vec!["Cozy".into(), "Farming".into()],
                                _ => vec!["Action".into()],
                            },
                            themes: if i == 6 {
                                vec!["Narrative".into()]
                            } else {
                                vec![]
                            },
                            keywords: vec![],
                            similar_game_ids: vec![],
                            steam_app_id_confirmed: true,
                            time_to_beat: None,
                        })
                    } else {
                        None
                    },
                    rawg: if i % 4 == 1 {
                        Some(RawgData {
                            rawg_id: u64::from(app_id),
                            rating_0_5: Some(match i {
                                5 => 3.5,
                                13 => 4.0,
                                _ => 3.0,
                            }),
                            ratings_count: None,
                            genres: vec![],
                            tags: match i {
                                5 => vec!["relaxing".into()],
                                13 => vec!["cozy".into()],
                                _ => vec![],
                            },
                            stores: vec![],
                        })
                    } else {
                        None
                    },
                    protondb: if i % 3 == 1 {
                        Some(ProtonDbData {
                            tier: match i {
                                1 => ProtonTier::Platinum,
                                4 => ProtonTier::Gold,
                                7 => ProtonTier::Silver,
                                10 => ProtonTier::Native,
                                13 => ProtonTier::Gold,
                                16 => ProtonTier::Platinum,
                                _ => ProtonTier::Unknown,
                            },
                            confidence: None,
                            score: None,
                        })
                    } else {
                        None
                    },
                    pcgw: if i % 3 == 2 {
                        Some(PcgwData {
                            page_name: None,
                            controller_support: match i {
                                2 => ControllerSupport::Full,
                                8 => ControllerSupport::Full,
                                14 => ControllerSupport::Partial,
                                20 => ControllerSupport::None,
                                _ => ControllerSupport::Unknown,
                            },
                            steam_deck_notes: None,
                            fixes_url: None,
                        })
                    } else {
                        None
                    },
                    steam_store: if i < 12 {
                        Some(SteamStoreDetails {
                            app_id,
                            name: format!("Demo Game {i:02}"),
                            steam_store_type: "game".into(),
                            is_free: i == 7,
                            short_description: Some(format!("Demo description {i}")),
                            header_image: None,
                            developers: vec!["Demo Studio".into()],
                            publishers: vec![],
                            genres: vec![],
                            categories: vec![],
                            release_date: None,
                            metacritic_score: None,
                            platforms: SteamStorePlatforms {
                                windows: true,
                                mac: i % 5 == 0,
                                linux: i == 10,
                            },
                            coming_soon: false,
                            price_overview: if i == 7 {
                                None
                            } else {
                                Some(vapourfly_core::models::PriceOverview {
                                    currency: "USD".into(),
                                    initial_price_cents: 1999 + i * 500,
                                    final_price_cents: 1999 + i * 500,
                                    discount_percent: 0,
                                })
                            },
                        })
                    } else {
                        None
                    },
                }
            })
            .collect();

        let scan = ScanResult {
            games: demo_games,
            warnings: vec![],
            steam_dir: "/demo/steam".into(),
            account: "demo_user".into(),
        };
        self.scan_result = Some(scan);
        // Demo refresh is a new scan result: bump the generation and drop any
        // cached snapshot so the library re-prepares from the refreshed data.
        self.scan_generation = self.scan_generation.wrapping_add(1);
        self.prepared_snapshot = None;

        // -- 4 Steam Collections ---------------------------------------------
        self.collections = vec![
            SteamCollection {
                id: "favorite".into(),
                name: "Favorites".into(),
                app_ids: vec![1000, 1005, 1010, 1015],
                removed_app_ids: vec![],
                is_hidden_collection: false,
            },
            SteamCollection {
                id: "hidden".into(),
                name: "Hidden".into(),
                app_ids: vec![1009, 1014],
                removed_app_ids: vec![],
                is_hidden_collection: true,
            },
            SteamCollection {
                id: "completed".into(),
                name: "Completed".into(),
                app_ids: vec![1000, 1004, 1010],
                removed_app_ids: vec![],
                is_hidden_collection: false,
            },
            SteamCollection {
                id: "want-to-play".into(),
                name: "Want to Play".into(),
                app_ids: vec![1003, 1007, 1012],
                removed_app_ids: vec![],
                is_hidden_collection: false,
            },
        ];

        // -- Junk decisions (mixed confidence) --------------------------------
        self.junk_results = vec![
            JunkDecision {
                app_id: 1011,
                name: "Demo Game 11".into(),
                is_junk: true,
                confidence: 1.0,
                matched: vec![JunkSignal::LowPlaytime { minutes: 5 }],
                missing: vec![],
                mode: JunkMode::Default,
            },
            JunkDecision {
                app_id: 1017,
                name: "Demo Game 17".into(),
                is_junk: true,
                confidence: 0.33,
                matched: vec![JunkSignal::LowPlaytime { minutes: 10 }],
                missing: vec![JunkSignalKind::CompletionTime, JunkSignalKind::Rating],
                mode: JunkMode::Default,
            },
            JunkDecision {
                app_id: 1005,
                name: "Demo Game 05".into(),
                is_junk: false,
                confidence: 0.66,
                matched: vec![],
                missing: vec![JunkSignalKind::CompletionTime],
                mode: JunkMode::Default,
            },
        ];
        self.junk_selected = [1011, 1017].into_iter().collect();

        // -- Recommendation results -------------------------------------------
        self.recommend_results = vec![
            Recommendation {
                app_id: 1003,
                name: "Demo Game 03".into(),
                score: 4.5,
                reasons: vec![
                    RecommendReason {
                        code: "low_playtime".into(),
                        description: "Low playtime (0 min)".into(),
                        weight: 2.0,
                    },
                    RecommendReason {
                        code: "time_match".into(),
                        description: "HLTB fits 120 min session".into(),
                        weight: 1.5,
                    },
                    RecommendReason {
                        code: "high_rating".into(),
                        description: "High rating (3.9/5)".into(),
                        weight: 1.0,
                    },
                ],
            },
            Recommendation {
                app_id: 1008,
                name: "Demo Game 08".into(),
                score: 3.0,
                reasons: vec![
                    RecommendReason {
                        code: "low_playtime".into(),
                        description: "Low playtime (0 min)".into(),
                        weight: 2.0,
                    },
                    RecommendReason {
                        code: "deck_compatible".into(),
                        description: "ProtonDB Gold".into(),
                        weight: 1.0,
                    },
                ],
            },
            Recommendation {
                app_id: 1012,
                name: "Demo Game 12".into(),
                score: 2.5,
                reasons: vec![
                    RecommendReason {
                        code: "low_playtime".into(),
                        description: "Low playtime (0 min)".into(),
                        weight: 2.0,
                    },
                    RecommendReason {
                        code: "time_match".into(),
                        description: "HLTB 15m fits session".into(),
                        weight: 0.5,
                    },
                ],
            },
        ];

        // -- Discover results -------------------------------------------------
        self.discover_results = vec![
            DiscoverPick {
                app_id: 1003,
                name: "Demo Game 03".into(),
                score: 5.2,
                reasons: vec![discover::DiscoverReason {
                    code: "taste_overlap",
                    description: "Taste vector overlap",
                    weight: 5.2,
                }],
            },
            DiscoverPick {
                app_id: 1008,
                name: "Demo Game 08".into(),
                score: 3.1,
                reasons: vec![discover::DiscoverReason {
                    code: "high_rating",
                    description: "High rating bonus",
                    weight: 3.1,
                }],
            },
        ];

        // -- Source statuses --------------------------------------------------
        self.source_statuses = vapourfly_api::enrichment::source_status(&self.cache_dir);

        // -- Detected accounts ------------------------------------------------
        self.detected_accounts = vec![SteamAccount {
            steam_id64: "76561198000000000".into(),
            account_name: "demo_user".into(),
            persona_name: "Demo Player".into(),
            most_recent: true,
        }];

        // -- Backups ----------------------------------------------------------
        self.backups = vec![BackupInfo {
            path: PathBuf::from(
                "/demo/backups/cloud-storage-namespace-1.vapourfly-backup-20260101T120000Z-abc12345.json",
            ),
            created_at: chrono::Utc::now(),
            sha256: "abc12345def67890".into(),
        }];

        // -- Playlist store ids -----------------------------------------------
        // In --ui-demo mode, write real loadable demo Playlist files to the
        // temp playlist store so "Load existing" and generator slots work.
        // All demo playlists use the canonical schema so playlist_store::put
        // succeeds and the files are genuinely loadable.
        let demo_ids: Vec<String> = vec![
            "my-favorites".into(),
            "story-games".into(),
            "discover".into(),
            "dynamic-deck-session".into(),
            "mood-quick-round".into(),
        ];
        if self.ui_demo {
            let store_path = self.playlist_store_path();
            if let Err(e) = std::fs::create_dir_all(&store_path) {
                self.error = Some(format!(
                    "demo init: failed to create playlist store {}: {e}",
                    store_path.display()
                ));
                return;
            }
            let demo_playlist =
                |id: &str, name: &str, desc: &str, app_ids: Vec<u32>| PlaylistFile {
                    vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
                    created_by: "demo".into(),
                    playlist: Playlist {
                        id: id.into(),
                        name: name.into(),
                        description: desc.into(),
                        content: PlaylistContent::Manual { app_ids },
                    },
                };
            let demo_playlists = [
                demo_playlist(
                    "my-favorites",
                    "My Favorites",
                    "Demo favorites collection",
                    vec![1000, 1002, 1005],
                ),
                demo_playlist(
                    "story-games",
                    "Story Games",
                    "Narrative-focused picks",
                    vec![1001, 1003, 1008],
                ),
                demo_playlist(
                    "discover",
                    "Discover Picks",
                    "Auto-generated discover slot",
                    vec![1004, 1006, 1010, 1012],
                ),
                demo_playlist(
                    "dynamic-deck-session",
                    "Deck Session",
                    "Dynamic template: deck-session",
                    vec![1000, 1007, 1011, 1015],
                ),
                demo_playlist(
                    "mood-quick-round",
                    "Quick Round",
                    "Editorial mood: quick-round",
                    vec![1002, 1009, 1013],
                ),
            ];
            for pf in &demo_playlists {
                if let Err(e) = playlist_store::put(&store_path, pf) {
                    // Propagate the failure instead of silently swallowing it.
                    // A demo session with missing playlist files is worse than
                    // an explicit error that surfaces in the UI banner.
                    self.error = Some(format!(
                        "demo init: failed to write playlist {}: {e}",
                        pf.playlist.id
                    ));
                    return;
                }
            }
        }
        self.playlist_store_ids = demo_ids;
        self.playlist_store_ids_loaded = true;
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

        // Demo mode: never scan the real Steam library. Re-populate the
        // deterministic demo data so "Refresh" stays a no-op on real data.
        if self.ui_demo {
            self.populate_demo_data();
            self.success_msg = Some("Demo library refreshed.".into());
            return;
        }

        self.loading = true;
        self.error = None;
        self.success_msg = None;

        // Allocate a request ID for stale-result protection.
        let job_id = self.job_runner.next_ticket(WorkflowKind::Scan, "scan");
        self.scan_job_id = Some(job_id);
        SCAN_RESULT.clear();

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
            // When offline mode is on (Data Sources toggle), the workflow skips
            // network fetches and uses cached metadata only.
            let opts = vapourfly_api::workflow::WorkflowOptions {
                steam_dir,
                account,
                fixtures,
                junk_mode: JunkMode::Default,
                offline,
            };
            let result = vapourfly_api::workflow::prepare(&opts);
            ctx.request_repaint();
            SCAN_RESULT.set(job_id, result.map_err(|e| e.to_string()));
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

        // Backup restore never uses a dry-run WritePlan. Clear any leftover plan
        // so a prior junk/playlist confirm cannot be mis-committed here.
        if matches!(action, PendingAction::BackupRestore(_)) {
            self.dry_run_plan = None;
        }

        // If we have a pre-computed plan from the dry-run step, execute it
        // directly.  Otherwise fall through to the legacy path (backup
        // restore).
        if let Some(plan) = self.dry_run_plan.take() {
            self.write_loading = true;
            self.write_result = None;
            self.success_msg = None;
            let allow_steam_running = self.allow_steam_running;
            let retention = self.backup_retention();

            let job_id = self
                .job_runner
                .next_ticket(WorkflowKind::Write, "execute_pending");
            self.write_job_id = Some(job_id);
            WRITE_RESULT.clear();

            std::thread::spawn(move || {
                let result = vapourfly_core::write::commit_with_retention(
                    &plan,
                    allow_steam_running,
                    retention,
                )
                .map_err(|e| format!("Write failed: {e}"))
                .map(|backup| format!("Write complete. Backup: {}", backup.display()));

                WRITE_RESULT.set(job_id, result);
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
        let junk_selected = self.junk_selected.clone();
        let collection_name = self.junk_collection_name.clone();
        let allow_steam_running = self.allow_steam_running;
        let retention = self.backup_retention();

        let job_id = self.job_runner.next_ticket(WorkflowKind::Write, "legacy_write");
        self.write_job_id = Some(job_id);
        WRITE_RESULT.clear();

        std::thread::spawn(move || {
            let result = match action {
                PendingAction::JunkApply => execute_junk_apply(
                    cloud_path,
                    junk_results,
                    junk_selected.clone(),
                    collection_name,
                    allow_steam_running,
                    retention,
                ),
                PendingAction::JunkHide => execute_junk_hide(
                    cloud_path,
                    junk_results,
                    junk_selected,
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

            WRITE_RESULT.set(job_id, result);
        });
    }

    /// Generate a dry-run WritePlan for the pending action and show the diff
    /// modal before committing to disk.
    fn start_dry_run(&mut self, action: PendingAction) {
        // Demo mode prohibits real Steam writes (spec: --ui-demo safety).
        if self.ui_demo {
            self.error = Some(
                "Write actions are disabled in demo mode (--ui-demo). \
                 Run without --ui-demo to modify Steam files."
                    .into(),
            );
            return;
        }

        // Rule-based Playlist Sync is resolved off-frame inside the dry-run
        // job (see `generate_dry_run_plan`), so `resolve_dry_run_action` no
        // longer matches rules on the egui frame. Capture the prepared library
        // so the background job can resolve rules there.
        let action = match self.resolve_dry_run_action(action) {
            Ok(action) => action,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let games = self.prepared_games(JunkMode::Default).unwrap_or_default();

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

        // Fingerprint covers the target action + all input AppIDs (junk
        // selection, recommend results, or playlist AppIDs) + library
        // generation, so a dry-run is invalidated if the inputs change before
        // the background job completes.
        let dry_run_fp = dry_run_fingerprint(&action, &self.junk_selected, &self.recommend_results);
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::DryRun, &dry_run_fp);
        self.dry_run_job_id = Some(job_id);
        DRY_RUN_RESULT.clear();

        let junk_results = self.junk_results.clone();
        let junk_selected = self.junk_selected.clone();
        let collection_name = self.junk_collection_name.clone();
        let recommend_results = self.recommend_results.clone();

        std::thread::spawn(move || {
            let result = generate_dry_run_plan(
                cloud_path,
                &action,
                &junk_results,
                &junk_selected,
                &collection_name,
                &recommend_results,
                &games,
            );
            DRY_RUN_RESULT.set(job_id, result);
        });
    }

    /// Start a cache refresh for the given source (or all sources).
    fn start_cache_refresh(&mut self, source: Option<String>, ctx: &egui::Context) {
        if self.cache_refresh_loading {
            return;
        }

        // Demo mode: no network fetches, no real cache writes.
        if self.ui_demo {
            self.cache_refresh_msg =
                Some("Cache refresh is disabled in demo mode (--ui-demo).".into());
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

        let fingerprint = format!("cache_refresh:{source:?}");
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::CacheRefresh, &fingerprint);
        self.enrich_job_id = Some(job_id);
        ENRICH_RESULT.clear();

        let cache_root = self.cache_dir.clone();
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
            ENRICH_RESULT.set(job_id, Ok(summary));
        });
    }

    /// Start Junk Preview in a background thread.
    fn start_junk_preview(&mut self, ctx: &egui::Context) {
        if self.junk_preview_loading {
            return;
        }
        let mode = match self.junk_mode {
            JunkModeChoice::Default => JunkMode::Default,
            JunkModeChoice::Strict => JunkMode::Strict,
            JunkModeChoice::Aggressive => JunkMode::Aggressive,
        };
        let games = match self.prepared_games(mode.clone()) {
            Some(g) => g,
            None => return,
        };

        self.junk_preview_loading = true;
        // Fingerprint covers mode + library generation + override/cache
        // generation so a rescan or cache refresh invalidates an in-flight
        // preview (the result would be computed against a stale library).
        let fingerprint = format!(
            "junk_preview:{mode:?}:lib={}:ovr={}",
            self.scan_generation, self.cache_refresh_generation
        );
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::JunkPreview, &fingerprint);
        self.junk_preview_job_id = Some(job_id);
        JUNK_PREVIEW_RESULT.clear();

        // Load overrides here (in the UI thread) so the demo path is used in
        // --ui-demo mode, not the real platform default path inside the thread.
        let overrides = self.manual_overrides();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let results = evaluate_junk(&games, &JunkRules::default(), &mode, &overrides);
            ctx.request_repaint();
            JUNK_PREVIEW_RESULT.set(job_id, Ok(results));
        });
    }

    /// Start Recommendations Preview in a background thread.
    fn start_recommend_preview(&mut self, ctx: &egui::Context) {
        if self.recommend_loading {
            return;
        }
        let request = match self.recommend_request_from_inputs() {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let games = match self.prepared_games(JunkMode::Default) {
            Some(g) => g,
            None => return,
        };

        self.recommend_loading = true;
        // Fingerprint covers the full request (minutes, count, deck,
        // installed-only, seed) + library generation. The request is also
        // captured at start time (recommend_request) so Match % is computed
        // against the request the user actually submitted, not the current
        // inputs (e.g. if Deck mode changes mid-job).
        let fingerprint = format!("recommend:{request:?}:lib={}", self.scan_generation);
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::RecommendPreview, &fingerprint);
        self.recommend_job_id = Some(job_id);
        // Keep the start-time request so Match % uses the submitted inputs.
        self.recommend_request_at_start = Some(request.clone());
        RECOMMEND_RESULT.clear();

        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let results = recommend(&games, &request);
            ctx.request_repaint();
            RECOMMEND_RESULT.set(job_id, Ok(results));
        });
    }

    /// Start Discover generate in a background thread.
    fn start_discover_generate(&mut self, ctx: &egui::Context) {
        if self.discover_loading {
            return;
        }
        let options = match self.discover_options_from_inputs() {
            Ok(o) => o,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let games = match self.prepared_games(JunkMode::Default) {
            Some(g) => g,
            None => return,
        };

        self.discover_loading = true;
        // Fingerprint covers the full options + library generation.
        let fingerprint = format!("discover:{options:?}:lib={}", self.scan_generation);
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::Discover, &fingerprint);
        self.discover_job_id = Some(job_id);
        DISCOVER_RESULT.clear();

        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let picks = discover::rank_discover_picks(&games, &options);
            let pf = discover::playlist_from_discover_picks(&games, &options, &picks);
            ctx.request_repaint();
            DISCOVER_RESULT.set(job_id, Ok((picks, pf)));
        });
    }

    /// Start Dynamic generate in a background thread.
    fn start_dynamic_generate(&mut self, ctx: &egui::Context) {
        if self.dynamic_loading {
            return;
        }
        let template = match DynamicTemplate::parse(&self.dynamic_template) {
            Some(t) => t,
            None => {
                self.error = Some("Unknown template. Use deck-session or finish-it.".into());
                return;
            }
        };
        let session_minutes = match parse_required_u32("Session minutes", &self.dynamic_minutes) {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let count = match parse_required_usize("Count", &self.dynamic_count) {
            Ok(v) => v,
            Err(e) => {
                self.error = Some(e);
                return;
            }
        };
        let games = match self.prepared_games(JunkMode::Default) {
            Some(g) => g,
            None => return,
        };

        self.dynamic_loading = true;
        let fingerprint = format!("dynamic:{}:{}:{}", template.id(), session_minutes, count);
        let job_id = self.job_runner.next_ticket(WorkflowKind::Dynamic, &fingerprint);
        self.dynamic_job_id = Some(job_id);
        // Capture the identity at start time so the consumer can write the
        // result to the correct stable slot even if the chooser changes mid-job.
        // Input-drift protection is handled by the JobTicket fingerprint
        // (compared on poll), so the result no longer needs a separate check.
        let identity = GeneratorIdentity::Dynamic(template);
        DYNAMIC_RESULT.clear();

        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let pf = dynamic::compile_dynamic_template(
                template,
                &games,
                &DynamicTemplateOptions {
                    session_minutes,
                    count,
                },
            );
            ctx.request_repaint();
            DYNAMIC_RESULT.set(
                job_id,
                Ok(GeneratorJobResult {
                    identity,
                    playlist: pf,
                }),
            );
        });
    }

    /// Start Mood generate in a background thread.
    fn start_mood_generate(&mut self, ctx: &egui::Context) {
        if self.mood_loading {
            return;
        }
        let mood = match EditorialMood::parse(&self.editorial_mood) {
            Some(m) => m,
            None => {
                self.error = Some("Unknown mood. Pick one from the list.".into());
                return;
            }
        };
        let games = match self.prepared_games(JunkMode::Default) {
            Some(g) => g,
            None => return,
        };

        self.mood_loading = true;
        let fingerprint = format!("mood:{}", mood.id());
        let job_id = self.job_runner.next_ticket(WorkflowKind::Mood, &fingerprint);
        self.mood_job_id = Some(job_id);
        // Capture identity at start time; input-drift protection is handled by
        // the JobTicket fingerprint (compared on poll).
        let identity = GeneratorIdentity::Mood(mood);
        MOOD_RESULT.clear();

        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let pf = mood::compile_editorial_mood(mood, &games, 25);
            ctx.request_repaint();
            MOOD_RESULT.set(
                job_id,
                Ok(GeneratorJobResult {
                    identity,
                    playlist: pf,
                }),
            );
        });
    }

    /// Consume finished Dynamic + Mood generator results.
    ///
    /// Each result carries the [`GeneratorIdentity`] and input fingerprint
    /// captured at job start time. The result is written to the slot named by
    /// the **start-time** identity — never the current chooser — so changing
    /// the chooser mid-job cannot redirect the result (input drift). Input-drift
    /// protection is handled by the [`JobTicket`] fingerprint compared in
    /// [`JobSlot::take_if`]: a result computed for different inputs is discarded
    /// before reaching here.
    fn poll_generator_results(&mut self) {
        // -- Dynamic ---------------------------------------------------------
        if self.dynamic_loading
            && let Some(expected) = self.dynamic_job_id
            && let Some(result) = DYNAMIC_RESULT.take_if(expected)
        {
            self.dynamic_loading = false;
            self.dynamic_job_id = None;
            match result {
                Ok(job_result) => {
                    match self
                        .store_generator_playlist(job_result.identity, job_result.playlist)
                    {
                        Ok(stored) => {
                            self.adopt_playlist_for_edit(&stored);
                            self.refresh_playlist_store_ids();
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Err(e) => self.error = Some(e),
            }
        }

        // -- Mood ------------------------------------------------------------
        if self.mood_loading
            && let Some(expected) = self.mood_job_id
            && let Some(result) = MOOD_RESULT.take_if(expected)
        {
            self.mood_loading = false;
            self.mood_job_id = None;
            match result {
                Ok(job_result) => {
                    match self
                        .store_generator_playlist(job_result.identity, job_result.playlist)
                    {
                        Ok(stored) => {
                            self.adopt_playlist_for_edit(&stored);
                            self.refresh_playlist_store_ids();
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    /// Start Playlist Match in a background thread.
    fn start_playlist_match(&mut self, ctx: &egui::Context, pf: PlaylistFile) {
        if self.playlist_match_loading {
            return;
        }
        let games = match self.prepared_games(JunkMode::Default) {
            Some(g) => g,
            None => return,
        };

        self.playlist_match_loading = true;
        // Fingerprint covers the playlist id + a hash of the full content
        // (manual AppIDs or rules) + library generation + price-cache
        // generation, so editing the playlist, rescanning, or refreshing the
        // cache invalidates an in-flight match.
        let content_hash = playlist_content_hash(&pf);
        let fingerprint = format!(
            "playlist_match:{}:content={:x}:lib={}:price={}",
            pf.playlist.id, content_hash, self.scan_generation, self.cache_refresh_generation
        );
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::PlaylistMatch, &fingerprint);
        self.playlist_match_job_id = Some(job_id);
        PLAYLIST_MATCH_RESULT.clear();

        let ctx = ctx.clone();
        let cache_dir = self.cache_dir.clone();
        std::thread::spawn(move || {
            // First pass: find missing AppIDs with empty store details.
            let empty = std::collections::HashMap::new();
            let preliminary = match playlist::match_playlist(&pf, &games, &empty) {
                Ok(r) => r,
                Err(e) => {
                    ctx.request_repaint();
                    PLAYLIST_MATCH_RESULT.set(job_id, Err(format!("Match failed: {e}")));
                    return;
                }
            };
            // Second pass: with cached store details for missing entries.
            let missing_details = if preliminary.missing.is_empty() {
                std::collections::HashMap::new()
            } else {
                let cache = vapourfly_api::cache::DiskCache::new(cache_dir);
                vapourfly_api::enrichment::missing_store_details(&preliminary.missing, &cache)
            };
            let report = match playlist::match_playlist(&pf, &games, &missing_details) {
                Ok(r) => r,
                Err(e) => {
                    ctx.request_repaint();
                    PLAYLIST_MATCH_RESULT.set(job_id, Err(format!("Match failed: {e}")));
                    return;
                }
            };
            ctx.request_repaint();
            PLAYLIST_MATCH_RESULT.set(job_id, Ok(report));
        });
    }

    fn filtered_games(&self) -> Vec<Game> {
        let games = match self.prepared_games(JunkMode::Default) {
            Some(games) => games,
            None => return Vec::new(),
        };

        let hltb_max = if self.library_quick_view == QuickView::ShortSessions {
            Some(120)
        } else {
            None
        };

        let filters = LibraryFilters {
            installed_only: self.filter_installed_only,
            not_hidden: self.filter_not_hidden,
            not_junk: self.filter_not_junk,
            is_hidden_only: false,
            is_junk_only: false,
            search: self.search_query.clone(),
            genre: self.filter_genre.clone(),
            proton_tier: self.filter_proton_tier,
            deck_compatible: self.filter_deck_compatible,
            unplayed_only: self.filter_unplayed_only,
            hltb_max_minutes: hltb_max,
        };
        project_library_games(games, &filters)
    }

    /// Reload source cache statuses from disk.
    fn reload_source_statuses(&mut self) {
        self.source_statuses = vapourfly_api::enrichment::source_status(&self.cache_dir);
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
        // Demo mode: use the demo config's steam_dir; never auto-detect real Steam.
        let steam_dir = if self.ui_demo {
            self.config.as_ref().map(|c| c.steam_dir.clone())
        } else {
            self.config
                .as_ref()
                .map(|c| c.steam_dir.clone())
                .or_else(VapourflyConfig::detect_steam_dir)
        };

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

        let cache_dir = &self.cache_dir;
        lines.push(format!("Cache root: {}", redact_path(cache_dir)));
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

    /// Load manual overrides from the configured path. In --ui-demo mode this
    /// is the demo temp root; otherwise the platform default path. Keeps demo
    /// mode from reading the user's real overrides file.
    fn manual_overrides(&self) -> ManualOverrides {
        match &self.manual_overrides_path {
            Some(p) => vapourfly_core::junk::load_manual_overrides_or_default(p),
            None => load_default_manual_overrides(),
        }
    }

    /// Fingerprint identifying the current library snapshot inputs. Changes
    /// when: a new scan result is accepted (`scan_generation` bumps), the cache
    /// is refreshed, or the cache / overrides paths change. Two matching
    /// fingerprints mean the cached hydrated games are still valid.
    ///
    /// Uses `scan_generation` rather than `scan_job_id` + game count: the job
    /// id is `None` after a scan completes, and the game count can stay the
    /// same when only content changes (playtime, hidden state, collections),
    /// so those would let a stale snapshot survive a rescan. `scan_generation`
    /// increments on every accepted scan result regardless of content.
    fn library_prepare_fingerprint(&self) -> u64 {
        let scan_gen = self.scan_generation;
        let refresh_gen = self.cache_refresh_generation;
        let cache_dir = self.cache_dir.to_string_lossy();
        let overrides = self
            .manual_overrides_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        fingerprint_u64(&format!(
            "prepare:scan_gen={scan_gen}:gen={refresh_gen}:cache={cache_dir}:ovr={overrides}"
        ))
    }

    /// Ensure a background library prepare is in flight if the snapshot is
    /// stale. Called once per frame from the UI update. The result is polled
    /// in [`poll_library_prepare`].
    fn ensure_library_prepared(&mut self, ctx: &egui::Context) {
        // No scan result yet — nothing to prepare.
        if self.scan_result.is_none() {
            return;
        }
        // Already have a fresh snapshot — no work needed.
        let fp = self.library_prepare_fingerprint();
        if let Some(snap) = &self.prepared_snapshot {
            if snap.fingerprint == fp {
                return;
            }
        }
        // A prepare is already in flight for this fingerprint — wait for it.
        if self.prepare_job_id.is_some() && self.prepare_fingerprint == Some(fp) {
            return;
        }
        // Start a new background prepare.
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::Prepare, &format!("prepare:{fp}"));
        self.prepare_job_id = Some(job_id);
        self.prepare_fingerprint = Some(fp);
        PREPARED_LIBRARY_RESULT.clear();

        let games = self
            .scan_result
            .as_ref()
            .map(|s| s.games.clone())
            .unwrap_or_default();
        let cache_dir = self.cache_dir.clone();
        let overrides_path = self.manual_overrides_path.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let mut games = games;
            let cache = vapourfly_api::cache::DiskCache::new(cache_dir);
            vapourfly_api::enrichment::hydrate_from_cache(&mut games, &cache);
            // Load overrides off-frame so prepared_games never touches the
            // overrides file on the egui frame.
            let overrides = match &overrides_path {
                Some(p) => vapourfly_core::junk::load_manual_overrides_or_default(p),
                None => load_default_manual_overrides(),
            };
            ctx.request_repaint();
            PREPARED_LIBRARY_RESULT.set(
                job_id,
                Ok(PreparedLibrarySnapshot {
                    fingerprint: fp,
                    games,
                    overrides,
                }),
            );
        });
    }

    /// Consume a finished background library prepare result.
    fn poll_library_prepare(&mut self) {
        if let Some(expected) = self.prepare_job_id
            && let Some(result) = PREPARED_LIBRARY_RESULT.take_if(expected)
        {
            self.prepare_job_id = None;
            self.prepare_fingerprint = None;
            // Hydration errors are non-fatal: keep the old snapshot.
            if let Ok(snap) = result {
                self.prepared_snapshot = Some(snap);
            }
        }
    }

    /// Hydrated + junk-classified games for the requested mode, sourced from
    /// the background-prepared snapshot. Returns `None` when the snapshot is not
    /// yet ready (first frames after a scan/rescan, or while rehydrating after a
    /// cache refresh): callers must show a loading state and disable dependent
    /// operations instead of synchronously hydrating on the egui frame.
    ///
    /// Tests inject a snapshot via [`VapourflyApp::inject_prepared_snapshot`]
    /// rather than relying on a production fallback.
    fn prepared_games(&self, junk_mode: JunkMode) -> Option<Vec<Game>> {
        // Only the snapshot path remains: no on-frame hydration fallback.
        let current_fp = self.library_prepare_fingerprint();
        let snap = self.prepared_snapshot.as_ref()?;
        if snap.fingerprint != current_fp {
            return None;
        }
        let mut games = snap.games.clone();
        apply_junk_flags(
            &mut games,
            &JunkRules::default(),
            &junk_mode,
            &snap.overrides,
        );
        Some(games)
    }

    /// Whether the prepared library snapshot is fresh and ready to serve
    /// `prepared_games`. UI that depends on the prepared library should show a
    /// loading state and disable actions while this is false.
    fn library_ready(&self) -> bool {
        let current_fp = self.library_prepare_fingerprint();
        self.prepared_snapshot
            .as_ref()
            .is_some_and(|s| s.fingerprint == current_fp)
    }

    /// Test helper: synchronously build and install a prepared snapshot from the
    /// current scan result + overrides, so tests can exercise `prepared_games`
    /// and its consumers without running the UI loop or the background thread.
    #[cfg(test)]
    fn inject_prepared_snapshot(&mut self) {
        let fp = self.library_prepare_fingerprint();
        let mut games = self
            .scan_result
            .as_ref()
            .map(|s| s.games.clone())
            .unwrap_or_default();
        let cache = vapourfly_api::cache::DiskCache::new(self.cache_dir.clone());
        vapourfly_api::enrichment::hydrate_from_cache(&mut games, &cache);
        let overrides = self.manual_overrides();
        self.prepared_snapshot = Some(PreparedLibrarySnapshot {
            fingerprint: fp,
            games,
            overrides,
        });
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
        // Demo mode: do not scan the real Steam account directories.
        if self.ui_demo {
            self.account_list_msg =
                Some("Account detection is disabled in demo mode (--ui-demo).".into());
            return;
        }
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
        // Playlist Match runs entirely off-frame: clear any stale report and
        // launch a background match (no on-frame first pass).
        self.playlist_match_report = None;
        self.start_playlist_match_from_stored_ctx(pf);
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
        // Rule-based Playlist Sync is now resolved off-frame inside the
        // background dry-run job (`generate_dry_run_plan`), so this no longer
        // matches rules on the egui frame. The action passes through unchanged;
        // any rule-resolution error surfaces as `dry_run_error` from the job.
        match action {
            PendingAction::PlaylistSync(pf) => {
                if matches!(pf.playlist.content, PlaylistContent::Rules { .. })
                    && !self.library_ready()
                {
                    return Err(
                        "Scan your library before syncing a rule-based playlist.".into(),
                    );
                }
                Ok(PendingAction::PlaylistSync(pf))
            }
            other => Ok(other),
        }
    }

    /// Launch a background Playlist Match using the ctx captured on the last
    /// frame. Used by non-UI methods (e.g. `adopt_playlist_for_edit`) that do
    /// not receive an `egui::Context` directly. No-op before the first frame.
    fn start_playlist_match_from_stored_ctx(&mut self, pf: &PlaylistFile) {
        if let Some(ctx) = self.ctx.clone() {
            self.start_playlist_match(&ctx, pf.clone());
        }
    }

    /// Playlist Match with cache lookup, called from UI handlers that have ctx.
    /// Runs entirely off-frame (no on-frame first pass): clears any stale report
    /// and launches the background match.
    fn match_playlist_against_library_background(
        &mut self,
        ctx: &egui::Context,
        pf: &PlaylistFile,
    ) {
        self.playlist_match_report = None;
        self.start_playlist_match(ctx, pf.clone());
    }

    /// Refresh the Load existing combo from the local playlist store.
    fn refresh_playlist_store_ids(&mut self) {
        // Always mark loaded so a transient failure does not re-list every frame.
        self.playlist_store_ids_loaded = true;
        match playlist_store::list_ids(&self.playlist_store_path()) {
            Ok(ids) => self.playlist_store_ids = ids,
            Err(e) => self.error = Some(format!("Failed to list playlists: {e}")),
        }
    }

    /// Load a playlist id from the store into the edit/match surface.
    fn load_playlist_from_store(&mut self, id: &str) -> Result<(), String> {
        let pf = playlist_store::get(&self.playlist_store_path(), id).map_err(|e| e.to_string())?;
        self.adopt_playlist_for_edit(&pf);
        self.playlist_load_selected = id.to_string();
        Ok(())
    }

    /// Compile the Dynamic template chosen in the chooser into its stable slot.
    #[allow(dead_code)]
    fn run_dynamic_generate(&mut self) -> Result<PlaylistFile, String> {
        let template = DynamicTemplate::parse(&self.dynamic_template)
            .ok_or_else(|| "Unknown template. Use deck-session or finish-it.".to_string())?;
        let session_minutes = parse_required_u32("Session minutes", &self.dynamic_minutes)?;
        let count = parse_required_usize("Count", &self.dynamic_count)?;
        let games = self
            .prepared_games(JunkMode::Default)
            .ok_or_else(|| "Scan your library before generating.".to_string())?;
        let pf = dynamic::compile_dynamic_template(
            template,
            &games,
            &DynamicTemplateOptions {
                session_minutes,
                count,
            },
        );
        let stored = self.store_generator_playlist(GeneratorIdentity::Dynamic(template), pf)?;
        self.adopt_playlist_for_edit(&stored);
        self.refresh_playlist_store_ids();
        Ok(stored)
    }

    /// Compile the Editorial Mood chosen in the chooser into its stable slot.
    #[allow(dead_code)]
    fn run_mood_generate(&mut self) -> Result<PlaylistFile, String> {
        let mood = EditorialMood::parse(&self.editorial_mood)
            .ok_or_else(|| "Unknown mood. Pick one from the list.".to_string())?;
        let games = self
            .prepared_games(JunkMode::Default)
            .ok_or_else(|| "Scan your library before generating.".to_string())?;
        let pf = mood::compile_editorial_mood(mood, &games, 25);
        let stored = self.store_generator_playlist(GeneratorIdentity::Mood(mood), pf)?;
        self.adopt_playlist_for_edit(&stored);
        self.refresh_playlist_store_ids();
        Ok(stored)
    }

    /// Generate Discover playlist into the stable slot and populate on-page results.
    #[allow(dead_code)]
    fn run_discover_generate(&mut self) -> Result<PlaylistFile, String> {
        let options = self.discover_options_from_inputs()?;
        let games = self
            .prepared_games(JunkMode::Default)
            .ok_or_else(|| "Scan your library before generating.".to_string())?;
        let picks = discover::rank_discover_picks(&games, &options);
        let pf = discover::playlist_from_discover_picks(&games, &options, &picks);
        let stored = self.store_generator_playlist(GeneratorIdentity::Discover, pf)?;
        self.discover_results = picks;
        self.discover_last_playlist = Some(stored.clone());
        // Load into Playlists state so "Open in Playlists" is seamless.
        self.adopt_playlist_for_edit(&stored);
        self.refresh_playlist_store_ids();
        Ok(stored)
    }
}

// ---------------------------------------------------------------------------
// Write operation helpers (run in background threads)
// ---------------------------------------------------------------------------

/// Generate a [`WritePlan`] without executing it, so the GUI can display a
/// dry-run diff before the user confirms.
///
/// Rule-based Playlist Sync is resolved here (off the egui frame) using the
/// prepared library `games`: the rule playlist is matched against the library
/// to produce the owned AppID set, which is then turned into a write operation.
fn generate_dry_run_plan(
    cloud_path: PathBuf,
    action: &PendingAction,
    junk_results: &[JunkDecision],
    junk_selected: &std::collections::HashSet<u32>,
    collection_name: &str,
    recommend_results: &[Recommendation],
    games: &[Game],
) -> Result<vapourfly_core::models::WritePlan, String> {
    let cloud = vapourfly_core::steam::read_cloud_storage(&cloud_path)
        .map_err(|e| format!("Failed to read cloud storage: {e}"))?;

    // Filter junk results to only selected items. Empty selection = 0 targets.
    let effective_junk: Vec<JunkDecision> = junk_results
        .iter()
        .filter(|d| junk_selected.contains(&d.app_id))
        .cloned()
        .collect();

    let op = match action {
        PendingAction::JunkApply => {
            if effective_junk.is_empty() {
                return Err("No junk candidates selected.".into());
            }
            let junk_app_ids = disposition::junk_app_ids_from_decisions(&effective_junk);
            disposition::junk_apply(collection_name, junk_app_ids).map_err(|e| e.to_string())?
        }
        PendingAction::JunkHide => {
            if effective_junk.is_empty() {
                return Err("No junk candidates selected.".into());
            }
            let junk_app_ids = disposition::junk_app_ids_from_decisions(&effective_junk);
            disposition::junk_hide(junk_app_ids).map_err(|e| e.to_string())?
        }
        PendingAction::RecommendCollection => {
            let app_ids: Vec<u32> = recommend_results.iter().map(|r| r.app_id).collect();
            disposition::recommend_to_collection(app_ids).map_err(|e| e.to_string())?
        }
        PendingAction::PlaylistSync(pf) => {
            // Resolve rule-based playlists off-frame: match the rules against
            // the prepared library to get the owned AppID set, then build a
            // manual-equivalent sync operation.
            let resolved_pf = match &pf.playlist.content {
                PlaylistContent::Manual { .. } => pf.clone(),
                PlaylistContent::Rules { .. } => {
                    let empty = std::collections::HashMap::new();
                    let report = playlist::match_playlist(pf, games, &empty)
                        .map_err(|e| format!("Match failed: {e}"))?;
                    PlaylistFile {
                        vapourfly_schema: pf.vapourfly_schema.clone(),
                        created_by: pf.created_by.clone(),
                        playlist: Playlist {
                            id: pf.playlist.id.clone(),
                            name: pf.playlist.name.clone(),
                            description: pf.playlist.description.clone(),
                            content: PlaylistContent::Manual {
                                app_ids: report.owned,
                            },
                        },
                    }
                }
            };
            let app_ids = disposition::playlist_sync_app_ids(&resolved_pf, None)
                .map_err(|e| e.to_string())?;
            disposition::playlist_sync(&resolved_pf, app_ids).map_err(|e| e.to_string())?
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
    junk_selected: std::collections::HashSet<u32>,
    collection_name: String,
    allow_steam_running: bool,
    retention: u32,
) -> Result<String, String> {
    // Filter to selected items only. Empty selection = 0 targets.
    let effective_junk: Vec<JunkDecision> = junk_results
        .iter()
        .filter(|d| junk_selected.contains(&d.app_id))
        .cloned()
        .collect();
    let junk_app_ids = disposition::junk_app_ids_from_decisions(&effective_junk);
    if junk_app_ids.is_empty() {
        return Err("No junk candidates selected.".into());
    }

    let cloud = vapourfly_core::steam::read_cloud_storage(&cloud_path)
        .map_err(|e| format!("Failed to read cloud storage: {e}"))?;

    let op = disposition::junk_apply(&collection_name, junk_app_ids.clone())
        .map_err(|e| e.to_string())?;
    let plan = vapourfly_core::write::preview(&cloud, vec![op], cloud_path.clone())
        .map_err(|e| format!("Failed to generate write plan: {e}"))?;

    let backup =
        vapourfly_core::write::commit_with_retention(&plan, allow_steam_running, retention)
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
    junk_selected: std::collections::HashSet<u32>,
    allow_steam_running: bool,
    retention: u32,
) -> Result<String, String> {
    // Filter to selected items only. Empty selection = 0 targets.
    let effective_junk: Vec<JunkDecision> = junk_results
        .iter()
        .filter(|d| junk_selected.contains(&d.app_id))
        .cloned()
        .collect();
    let junk_app_ids = disposition::junk_app_ids_from_decisions(&effective_junk);
    if junk_app_ids.is_empty() {
        return Err("No junk candidates selected.".into());
    }

    let cloud = vapourfly_core::steam::read_cloud_storage(&cloud_path)
        .map_err(|e| format!("Failed to read cloud storage: {e}"))?;

    let op = disposition::junk_hide(junk_app_ids.clone()).map_err(|e| e.to_string())?;
    let plan = vapourfly_core::write::preview(&cloud, vec![op], cloud_path.clone())
        .map_err(|e| format!("Failed to generate write plan: {e}"))?;

    let backup =
        vapourfly_core::write::commit_with_retention(&plan, allow_steam_running, retention)
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
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // Persist theme preference via eframe storage, not domain config
        // (ADR-0006: appearance persistence is a GUI-only concern).
        storage.set_string("vapourfly.theme", self.theme_mode.as_u8().to_string());
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Capture ctx so non-UI methods (e.g. playlist adoption) can spawn
        // background work and request repaints without a ctx parameter.
        self.ctx = Some(ctx.clone());

        // Keep free-function tokens and egui visuals aligned with the selected theme.
        set_active_theme(self.theme_mode);
        configure_ui(&ctx, self.theme_mode);

        // Poll background scan result (stale-result protected via JobId).
        if self.loading
            && let Some(expected) = self.scan_job_id
            && let Some(result) = SCAN_RESULT.take_if(expected)
        {
            self.loading = false;
            self.scan_job_id = None;
            match result {
                Ok(scan) => {
                    self.scan_result = Some(scan);
                    // New scan result accepted: bump the scan generation so the
                    // prepare fingerprint changes and any cached snapshot is
                    // treated as stale (even if the game count is unchanged).
                    self.scan_generation = self.scan_generation.wrapping_add(1);
                    self.prepared_snapshot = None;
                    match self.load_collections_from_cloud() {
                        Ok(collections) => self.collections = collections,
                        Err(e) => self.error = Some(e),
                    }
                }
                Err(e) => self.error = Some(e),
            }
        }

        // Poll background write result.
        if self.write_loading
            && let Some(expected) = self.write_job_id
            && let Some(result) = WRITE_RESULT.take_if(expected)
        {
            self.write_loading = false;
            self.write_job_id = None;
            match result {
                Ok(msg) => {
                    self.success_msg = Some(msg);
                    // Auto-re-scan to pick up changes
                    self.start_scan(&ctx);
                }
                Err(e) => self.error = Some(e),
            }
        }

        // Poll background cache refresh result.
        if self.cache_refresh_loading
            && let Some(expected) = self.enrich_job_id
            && let Some(result) = ENRICH_RESULT.take_if(expected)
        {
            self.cache_refresh_loading = false;
            self.enrich_job_id = None;
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
                    // Invalidate the library snapshot so the next frame
                    // re-hydrates from the freshly-written cache.
                    self.cache_refresh_generation = self.cache_refresh_generation.wrapping_add(1);
                }
                Err(e) => self.cache_refresh_msg = Some(format!("Error: {e}")),
            }
        }

        // -- Generator choosers (Playlists Dynamic / Mood) ---------------------
        self.render_playlist_choosers(&ctx);

        // -- Confirmation dialog -----------------------------------------------
        // Poll background dry-run result.
        if self.dry_run_loading
            && let Some(expected) = self.dry_run_job_id
            && let Some(result) = DRY_RUN_RESULT.take_if(expected)
        {
            self.dry_run_loading = false;
            self.dry_run_job_id = None;
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

        // Poll background junk preview result.
        if self.junk_preview_loading
            && let Some(expected) = self.junk_preview_job_id
            && let Some(result) = JUNK_PREVIEW_RESULT.take_if(expected)
        {
            self.junk_preview_loading = false;
            self.junk_preview_job_id = None;
            match result {
                Ok(results) => {
                    self.junk_results = results;
                    // Auto-select all junk candidates after Preview.
                    self.junk_selected = self
                        .junk_results
                        .iter()
                        .filter(|d| d.is_junk)
                        .map(|d| d.app_id)
                        .collect();
                }
                Err(e) => self.error = Some(e),
            }
        }

        // Poll background recommend preview result.
        if self.recommend_loading
            && let Some(expected) = self.recommend_job_id
            && let Some(result) = RECOMMEND_RESULT.take_if(expected)
        {
            self.recommend_loading = false;
            self.recommend_job_id = None;
            match result {
                Ok(results) => {
                    self.recommend_results = results;
                    self.recommend_selected = self.recommend_results.first().map(|r| r.app_id);
                }
                Err(e) => self.error = Some(e),
            }
        }

        // Poll background discover result.
        if self.discover_loading
            && let Some(expected) = self.discover_job_id
            && let Some(result) = DISCOVER_RESULT.take_if(expected)
        {
            self.discover_loading = false;
            self.discover_job_id = None;
            match result {
                Ok((picks, pf)) => {
                    let stored =
                        match self.store_generator_playlist(GeneratorIdentity::Discover, pf) {
                            Ok(s) => s,
                            Err(e) => {
                                self.error = Some(e);
                                return;
                            }
                        };
                    self.discover_results = picks;
                    self.discover_last_playlist = Some(stored.clone());
                    self.adopt_playlist_for_edit(&stored);
                    self.refresh_playlist_store_ids();
                }
                Err(e) => self.error = Some(e),
            }
        }

        // Poll background Dynamic + Mood generator results. Extracted so tests
        // can exercise the input-drift protection without a full egui frame.
        self.poll_generator_results();

        // Poll background library prepare (off-frame hydration) and kick off a
        // new one if the snapshot is stale (e.g. after a scan or cache refresh).
        self.poll_library_prepare();
        self.ensure_library_prepared(&ctx);

        // Poll background playlist match result.
        if self.playlist_match_loading
            && let Some(expected) = self.playlist_match_job_id
            && let Some(result) = PLAYLIST_MATCH_RESULT.take_if(expected)
        {
            self.playlist_match_loading = false;
            self.playlist_match_job_id = None;
            match result {
                Ok(report) => self.playlist_match_report = Some(report),
                Err(e) => self.error = Some(e),
            }
        }

        self.render_confirm_dialog(&ctx);

        let shell_games = self.scan_result.as_ref().map_or(0, |scan| scan.games.len());
        let shell_hidden = self.scan_result.as_ref().map_or(0, |scan| {
            scan.games.iter().filter(|game| game.is_hidden).count()
        });
        let shell_playtime = self.scan_result.as_ref().map_or(0, |scan| {
            scan.games
                .iter()
                .map(|game| game.playtime_minutes.unwrap_or(0))
                .sum::<u32>()
        });
        let shell_playtime = format_playtime(shell_playtime);
        let shell_playlists = self.playlist_store_ids.len();

        // -- Top chrome ------------------------------------------------------
        egui::Panel::top("top_chrome")
            .exact_size(TOPBAR_HEIGHT)
            .frame(
                egui::Frame::NONE
                    .fill(t().surface)
                    .stroke(egui::Stroke::new(1.0, t().border_soft))
                    .inner_margin(egui::Margin::symmetric(m(SP_3), m(SP_2))),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Native OS window controls remain authoritative (ADR-0006).
                    // No app-drawn traffic-light circles.
                    ui.add_space(SP_2);
                    ui.label(
                        RichText::new("Vapourfly")
                            .size(TS_MD)
                            .strong()
                            .color(t().text_primary),
                    );
                    ui.label(RichText::new("›").size(TS_LG).color(t().text_muted));
                    ui.label(
                        RichText::new(self.current_view.label())
                            .size(TS_MD)
                            .color(t().text_primary),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let theme_label = if self.theme_mode.is_dark() {
                            "☀ Light"
                        } else {
                            "☾ Dark"
                        };
                        if secondary_button(ui, theme_label).clicked() {
                            self.theme_mode = self.theme_mode.toggle();
                            set_active_theme(self.theme_mode);
                            configure_ui(&ctx, self.theme_mode);
                        }
                        ui.add_space(SP_4);
                        top_bar_metric(ui, "Ready", "Synced");
                        ui.add_space(SP_6);
                        top_bar_metric(ui, shell_playlists.to_string(), "Playlists");
                        ui.add_space(SP_6);
                        top_bar_metric(ui, shell_hidden.to_string(), "Hidden");
                        ui.add_space(SP_6);
                        top_bar_metric(ui, shell_playtime, "Play time");
                        ui.add_space(SP_6);
                        top_bar_metric(ui, shell_games.to_string(), "Games");
                    });
                });
            });

        // -- Left panel: navigation -----------------------------------------
        egui::Panel::left("nav_panel")
            .resizable(false)
            .exact_size(SIDEBAR_WIDTH)
            .frame(
                egui::Frame::NONE
                    .fill(t().surface)
                    .stroke(egui::Stroke::new(1.0, t().border_soft))
                    .inner_margin(egui::Margin::same(m(SP_2))),
            )
            .show(ui, |ui| {
                ui.add_space(SP_2);

                // Library-facing destinations form the primary group. The
                // maintenance destinations stay at the bottom like the
                // reference application rail.
                for &view in &View::ALL[..5] {
                    let selected = self.current_view == view;
                    if nav_item(ui, view, selected).clicked() {
                        self.current_view = view;
                    }
                    ui.add_space(3.0);
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                    ui.add_space(SP_1);
                    ui.label(
                        RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                            .size(TS_XS)
                            .color(t().text_muted),
                    );
                    ui.add_space(SP_2);
                    for &view in View::ALL[5..].iter().rev() {
                        let selected = self.current_view == view;
                        if nav_item(ui, view, selected).clicked() {
                            self.current_view = view;
                        }
                        ui.add_space(3.0);
                    }
                });
            });

        // -- Central panel: current view ------------------------------------
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(t().canvas)
                    .inner_margin(egui::Margin::same(m(SP_6))),
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
                    .fill(t().surface_raised)
                    .stroke(egui::Stroke::new(1.0, t().border))
                    .corner_radius(CORNER_LG)
                    .inner_margin(egui::Margin::same(m(SP_4))),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("Confirm Action")
                        .size(TS_XL)
                        .strong()
                        .color(t().text_primary),
                );
                ui.add_space(SP_3);

                // -- Dry-run diff (junk apply / hide) --------------------------
                if let Some(plan) = &self.dry_run_plan {
                    let diff = &plan.diff;

                    ui.label(
                        RichText::new("Dry-Run Diff")
                            .size(TS_MD)
                            .strong()
                            .color(t().text_primary),
                    );
                    ui.label(
                        RichText::new(format!("Target: {}", plan.target_path.display()))
                            .size(TS_BODY)
                            .color(t().text_secondary),
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
                                        .color(t().text_muted),
                                );
                                let names: Vec<&str> = diff
                                    .collections_changed
                                    .iter()
                                    .map(|c| c.id.as_str())
                                    .collect();
                                ui.label(
                                    RichText::new(names.join(", "))
                                        .size(TS_BODY)
                                        .color(t().text_primary),
                                );
                                ui.end_row();
                            }

                            if !diff.app_ids_added.is_empty() {
                                ui.label(
                                    RichText::new("AppIDs added:")
                                        .size(TS_SM)
                                        .color(t().text_muted),
                                );
                                ui.label(
                                    RichText::new(format!("{} games", diff.app_ids_added.len()))
                                        .size(TS_BODY)
                                        .color(t().text_primary),
                                );
                                ui.end_row();
                            }

                            if !diff.app_ids_removed.is_empty() {
                                ui.label(
                                    RichText::new("AppIDs removed:")
                                        .size(TS_SM)
                                        .color(t().text_muted),
                                );
                                ui.label(
                                    RichText::new(format!("{} games", diff.app_ids_removed.len()))
                                        .size(TS_BODY)
                                        .color(t().text_primary),
                                );
                                ui.end_row();
                            }

                            if !diff.hidden_app_ids_added.is_empty() {
                                ui.label(
                                    RichText::new("Hidden AppIDs added:")
                                        .size(TS_SM)
                                        .color(t().text_muted),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{} games",
                                        diff.hidden_app_ids_added.len()
                                    ))
                                    .size(TS_BODY)
                                    .color(t().text_primary),
                                );
                                ui.end_row();
                            }

                            ui.label(
                                RichText::new("Unchanged entries:")
                                    .size(TS_SM)
                                    .color(t().text_muted),
                            );
                            ui.label(
                                RichText::new(diff.unchanged_count.to_string())
                                    .size(TS_BODY)
                                    .color(t().text_primary),
                            );
                            ui.end_row();

                            if diff.skipped_deleted_count > 0 {
                                ui.label(
                                    RichText::new("Skipped deleted:")
                                        .size(TS_SM)
                                        .color(t().text_muted),
                                );
                                ui.label(
                                    RichText::new(diff.skipped_deleted_count.to_string())
                                        .size(TS_BODY)
                                        .color(t().text_primary),
                                );
                                ui.end_row();
                            }
                        });

                    ui.add_space(SP_2);
                    ui.label(
                        RichText::new("\u{26A0} A safety backup will be created before writing.")
                            .size(TS_BODY)
                            .color(t().warning),
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
                            .color(t().text_primary),
                    );
                    ui.add_space(SP_2);
                    ui.label(
                        RichText::new(
                            "\u{26A0} This will overwrite your current cloud storage. A safety backup will be created first.",
                        )
                        .size(TS_BODY)
                        .color(t().warning),
                    );
                }

                if self.write_loading {
                    ui.add_space(SP_2);
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("Writing").size(TS_BODY).color(t().text_secondary));
                    });
                } else {
                    ui.add_space(SP_3);
                    ui.horizontal(|ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Confirm")
                                        .size(TS_BODY)
                                        .color(t().text_inverse),
                                )
                                .fill(t().accent)
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
                                        .color(t().text_primary),
                                )
                                .fill(t().surface)
                                .stroke(egui::Stroke::new(1.0, t().border))
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

        // Closing via the window chrome (X) must clear confirm state the same
        // way Cancel does, so a leftover dry_run_plan cannot hijack a later
        // BackupRestore confirm.
        if !open && !self.write_loading {
            self.pending_action = None;
            self.dry_run_plan = None;
            self.dry_run_error = None;
        }
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
        let all_games = self
            .scan_result
            .as_ref()
            .map(|s| s.games.as_slice())
            .unwrap_or(&[]);

        // Compute extended summary metrics for the insights rail.
        let installed_count = all_games.iter().filter(|g| g.installed).count();
        let unplayed_count = all_games
            .iter()
            .filter(|g| g.playtime_minutes.unwrap_or(0) == 0)
            .count();
        let hidden_count = all_games.iter().filter(|g| g.is_hidden).count();
        let junk_count = all_games.iter().filter(|g| g.is_junk).count();
        let total_playtime: u32 = all_games
            .iter()
            .map(|g| g.playtime_minutes.unwrap_or(0))
            .sum();

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
                            RichText::new("Refresh").size(TS_SM).color(t().text_inverse),
                        )
                        .fill(t().accent)
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
                    ui.label(
                        RichText::new("Scanning")
                            .size(TS_SM)
                            .color(t().text_secondary),
                    );
                }
            },
        );

        // Quick view pills: All / Installed / Unplayed / Hidden / Junk.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
            for qv in QuickView::all() {
                let is_active = self.library_quick_view == qv;
                let btn =
                    egui::Button::new(RichText::new(qv.label()).size(TS_SM).color(if is_active {
                        t().text_inverse
                    } else {
                        t().text_secondary
                    }))
                    .fill(if is_active { t().accent } else { t().surface })
                    .stroke(egui::Stroke::new(
                        1.0,
                        if is_active { t().accent } else { t().border },
                    ))
                    .corner_radius(CORNER_PILL);
                if ui.add(btn).clicked() {
                    self.apply_quick_view(qv);
                }
            }
        });

        ui.add_space(SP_3);

        // Core filters (always visible).
        section_card(ui, "Core Filters", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_2);
                let prev_installed = self.filter_installed_only;
                let prev_hidden = self.filter_not_hidden;
                let prev_junk = self.filter_not_junk;
                filter_toggle(ui, &mut self.filter_installed_only, "Installed only");
                filter_toggle(ui, &mut self.filter_not_hidden, "Not hidden");
                filter_toggle(ui, &mut self.filter_not_junk, "Not junk");
                if prev_installed != self.filter_installed_only
                    || prev_hidden != self.filter_not_hidden
                    || prev_junk != self.filter_not_junk
                {
                    self.library_visible_count = 48;
                }
            });
        });

        ui.add_space(SP_3);

        // Search & advanced filters card.
        section_card(ui, "Search & Advanced Filters", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                ui.label(
                    RichText::new("Search")
                        .size(TS_SM)
                        .color(t().text_secondary),
                );
                let prev_search = self.search_query.clone();
                ui.add_sized(
                    [260.0, 30.0],
                    egui::TextEdit::singleline(&mut self.search_query).hint_text("Title or AppID"),
                );
                if prev_search != self.search_query {
                    self.library_visible_count = 48;
                }
                ui.separator();
                ui.label(RichText::new("Genre").size(TS_SM).color(t().text_secondary));
                let prev_genre = self.filter_genre.clone();
                ui.add_sized(
                    [140.0, 30.0],
                    egui::TextEdit::singleline(&mut self.filter_genre).hint_text("e.g. Cozy"),
                );
                if prev_genre != self.filter_genre {
                    self.library_visible_count = 48;
                }
                ui.separator();
                // ProtonDB tier dropdown.
                ui.label(
                    RichText::new("ProtonDB")
                        .size(TS_SM)
                        .color(t().text_secondary),
                );
                let prev_tier = self.filter_proton_tier;
                let tier_label = match self.filter_proton_tier {
                    None => "Any".to_string(),
                    Some(t) => proton_tier_label(t).to_string(),
                };
                egui::ComboBox::from_id_salt("lib_proton_tier")
                    .selected_text(tier_label)
                    .width(100.0)
                    .show_ui(ui, |ui| {
                        let current = self.filter_proton_tier;
                        if ui.selectable_label(current.is_none(), "Any").clicked() {
                            self.filter_proton_tier = None;
                        }
                        for tier in [
                            ProtonTier::Native,
                            ProtonTier::Platinum,
                            ProtonTier::Gold,
                            ProtonTier::Silver,
                            ProtonTier::Bronze,
                        ] {
                            if ui
                                .selectable_label(current == Some(tier), proton_tier_label(tier))
                                .clicked()
                            {
                                self.filter_proton_tier = Some(tier);
                            }
                        }
                    });
                if prev_tier != self.filter_proton_tier {
                    self.library_visible_count = 48;
                }
                ui.separator();
                let prev_deck = self.filter_deck_compatible;
                let prev_unplayed = self.filter_unplayed_only;
                filter_toggle(ui, &mut self.filter_deck_compatible, "Deck");
                filter_toggle(ui, &mut self.filter_unplayed_only, "Unplayed");
                if prev_deck != self.filter_deck_compatible
                    || prev_unplayed != self.filter_unplayed_only
                {
                    self.library_visible_count = 48;
                }
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

        // Scan result exists but the background prepare (hydration + overrides)
        // has not completed yet: show a loading skeleton instead of hydrating
        // synchronously on the egui frame.
        if !self.library_ready() {
            ui.add_space(SP_4);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Preparing library\u{2026}")
                        .size(TS_MD)
                        .color(t().text_secondary),
                );
            });
            ui.add_space(SP_2);
            ui.label(
                RichText::new("Hydrating covers and metadata in the background.")
                    .size(TS_SM)
                    .color(t().text_secondary),
            );
            return;
        }

        // Two-column layout: game grid (left) + insights rail (right).
        ui.horizontal(|ui| {
            // Main grid column.
            ui.vertical(|ui| {
                if games.is_empty() {
                    empty_state(
                        ui,
                        "\u{1F50D}",
                        "No games match these filters",
                        "Clear search or turn off a filter to bring games back.",
                    );
                    return;
                }

                // "Load more" pagination: show 48 initially, +48 each click.
                const LOAD_INCREMENT: usize = 48;
                let visible = self.library_visible_count.min(games.len());
                let page_games = &games[..visible];

                ui.with_layout(
                    egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
                    |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_3);
                        for game in page_games {
                            self.render_game_card(ui, game);
                        }
                    },
                );

                // Load more button + count.
                if visible < games.len() {
                    ui.add_space(SP_2);
                    ui.horizontal(|ui| {
                        let remaining = games.len() - visible;
                        if ui
                            .button(
                                RichText::new(format!(
                                    "Load more (+{} of {} remaining)",
                                    LOAD_INCREMENT.min(remaining),
                                    remaining
                                ))
                                .size(TS_SM),
                            )
                            .clicked()
                        {
                            self.library_visible_count += LOAD_INCREMENT;
                        }
                    });
                }
                ui.add_space(SP_1);
                ui.label(
                    RichText::new(format!("Showing {} of {}", visible, games.len()))
                        .size(TS_XS)
                        .color(t().text_muted),
                );
            });

            // Insights rail.
            ui.add_space(SP_4);
            ui.allocate_ui_with_layout(
                egui::vec2(200.0, ui.available_height()),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    egui::Frame::group(ui.style())
                        .fill(t().surface)
                        .stroke(egui::Stroke::new(1.0, t().border_soft))
                        .corner_radius(CORNER_MD)
                        .inner_margin(egui::Margin::same(m(SP_3)))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Insights")
                                    .size(TS_MD)
                                    .strong()
                                    .color(t().text_primary),
                            );
                            ui.add_space(SP_2);
                            ui.separator();
                            ui.add_space(SP_2);
                            ui.spacing_mut().item_spacing.y = SP_3;
                            insight_metric(ui, "Total", total_games.to_string());
                            insight_metric(ui, "Installed", installed_count.to_string());
                            insight_metric(ui, "Unplayed", unplayed_count.to_string());
                            insight_metric(ui, "Hidden", hidden_count.to_string());
                            insight_metric(ui, "Junk", junk_count.to_string());
                            ui.separator();
                            insight_metric(ui, "Playtime", format_playtime(total_playtime));
                            insight_metric(ui, "Matching", games.len().to_string());
                        });
                },
            );
        });
    }

    /// Apply a quick-view preset by setting/clearing the filter toggles.
    fn apply_quick_view(&mut self, qv: QuickView) {
        self.library_quick_view = qv;
        self.library_visible_count = 48;
        // Core filters are always shown and user-controllable; quick views
        // set the advanced filters on top of whatever core filters are active.
        match qv {
            QuickView::All => {
                self.filter_genre.clear();
                self.filter_proton_tier = None;
                self.filter_deck_compatible = false;
                self.filter_unplayed_only = false;
            }
            QuickView::Cozy => {
                self.filter_genre = "Cozy".into();
                self.filter_proton_tier = None;
                self.filter_deck_compatible = false;
                self.filter_unplayed_only = false;
            }
            QuickView::StoryRich => {
                self.filter_genre = "Story Rich".into();
                self.filter_proton_tier = None;
                self.filter_deck_compatible = false;
                self.filter_unplayed_only = false;
            }
            QuickView::GreatOnDeck => {
                self.filter_genre.clear();
                self.filter_proton_tier = Some(ProtonTier::Gold);
                self.filter_deck_compatible = true;
                self.filter_unplayed_only = false;
            }
            QuickView::ShortSessions => {
                self.filter_genre.clear();
                self.filter_proton_tier = None;
                self.filter_deck_compatible = false;
                self.filter_unplayed_only = false;
                // Short sessions: HLTB normally <= 120 minutes.
                // Stored via hltb_max_minutes in LibraryFilters.
            }
        }
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
            egui::Stroke::new(1.5, t().accent)
        } else {
            egui::Stroke::new(1.0, t().border)
        };

        let response = ui
            .allocate_ui_with_layout(
                egui::vec2(GAME_CARD_W, GAME_CARD_H),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    ui.set_width(GAME_CARD_W);
                    ui.set_height(GAME_CARD_H);

                    egui::Frame::NONE
                        .fill(t().surface_raised)
                        .stroke(border)
                        .inner_margin(egui::Margin::same(m(SP_3)))
                        .corner_radius(CORNER_MD)
                        .show(ui, |ui| {
                            ui.set_width(GAME_CARD_W - f32::from(m(SP_3)) * 2.0);
                            ui.set_height(GAME_CARD_H - f32::from(m(SP_3)) * 2.0);

                            ui.horizontal(|ui| {
                                let (label, fill, text) = game_primary_badge(game);
                                status_badge(ui, label, fill, text);
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.label(
                                            RichText::new("•••").size(TS_SM).color(t().text_muted),
                                        );
                                    },
                                );
                            });

                            ui.add_space(SP_2);
                            ui.vertical_centered(|ui| {
                                game_image(ui, game.app_id, &game.name, self.ui_demo || self.offline_mode);
                            });

                            ui.add_space(SP_2);
                            ui.add_sized(
                                [GAME_CARD_W - f32::from(m(SP_3)) * 2.0, 24.0],
                                egui::Label::new(
                                    RichText::new(&game.name)
                                        .size(TS_MD)
                                        .strong()
                                        .color(t().text_primary),
                                )
                                .wrap(),
                            );

                            // Secondary badges: Proton tier / Deck when hydrated.
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(SP_1, SP_1);
                                if let Some(proton) = &game.protondb {
                                    status_badge(
                                        ui,
                                        proton_tier_label(proton.tier),
                                        t().accent_soft,
                                        t().accent_text,
                                    );
                                }
                                if game_shows_deck_badge(game) {
                                    status_badge(ui, "Deck", t().success_soft, t().success);
                                }
                                if game.protondb.is_none() && !game_shows_deck_badge(game) {
                                    ui.label(
                                        RichText::new(game_card_detail(game))
                                            .size(TS_XS)
                                            .color(t().text_muted),
                                    );
                                }
                            });

                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format_playtime(
                                        game.playtime_minutes.unwrap_or(0),
                                    ))
                                    .size(TS_SM)
                                    .color(t().text_muted),
                                );
                                if show_recommend {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new("▶  Recommend")
                                                            .size(TS_SM)
                                                            .color(t().text_inverse),
                                                    )
                                                    .fill(t().accent)
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
                ui.label(
                    RichText::new("Detection")
                        .size(TS_SM)
                        .color(t().text_secondary),
                );
                for mode in &[
                    JunkModeChoice::Default,
                    JunkModeChoice::Strict,
                    JunkModeChoice::Aggressive,
                ] {
                    let selected = self.junk_mode == *mode;
                    let btn = egui::Button::new(RichText::new(mode.label()).size(TS_SM).color(
                        if selected {
                            t().text_inverse
                        } else {
                            t().text_secondary
                        },
                    ))
                    .fill(if selected { t().accent } else { t().surface })
                    .stroke(if selected {
                        egui::Stroke::NONE
                    } else {
                        egui::Stroke::new(1.0, t().border_soft)
                    })
                    .corner_radius(CORNER_PILL);
                    if ui.add(btn).clicked() {
                        self.junk_mode = *mode;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.junk_preview_loading {
                        ui.spinner();
                    }
                    if ui
                        .add_enabled(self.library_ready(), primary_button_widget("Preview"))
                        .clicked()
                    {
                        self.start_junk_preview(ui.ctx());
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
        let selected_count = self.junk_selected.len();
        let selected_junk_count = self
            .junk_results
            .iter()
            .filter(|d| self.junk_selected.contains(&d.app_id) && d.is_junk)
            .count();

        // Two-column layout: table (left) + summary rail (right).
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                // Bulk selection bar + show-all toggle.
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                    if ui
                        .button(RichText::new("Select all junk").size(TS_SM))
                        .clicked()
                    {
                        for d in &self.junk_results {
                            if d.is_junk {
                                self.junk_selected.insert(d.app_id);
                            }
                        }
                    }
                    if ui.button(RichText::new("Clear").size(TS_SM)).clicked() {
                        self.junk_selected.clear();
                    }
                    ui.separator();
                    ui.checkbox(&mut self.junk_show_all_evaluated, "Show all evaluated");
                    ui.label(
                        RichText::new(format!("{selected_count} selected"))
                            .size(TS_SM)
                            .color(t().text_secondary),
                    );
                });
                ui.add_space(SP_2);

                // Preview: candidate table with selection checkboxes.
                // Default: only show junk. Toggle to show all evaluated.
                let visible_decisions: Vec<&JunkDecision> = if self.junk_show_all_evaluated {
                    self.junk_results.iter().collect()
                } else {
                    self.junk_results.iter().filter(|d| d.is_junk).collect()
                };

                section_card(ui, "Preview", |ui| {
                    let text_height = TS_BODY;
                    egui_extras::TableBuilder::new(ui)
                        .striped(true)
                        .resizable(true)
                        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                        .column(egui_extras::Column::auto().at_least(28.0))
                        .column(egui_extras::Column::auto().at_least(60.0))
                        .column(egui_extras::Column::remainder().at_least(150.0))
                        .column(egui_extras::Column::auto().at_least(60.0))
                        .column(egui_extras::Column::auto().at_least(80.0))
                        .column(egui_extras::Column::remainder().at_least(200.0))
                        .header(text_height * 1.4, |mut header| {
                            header.col(|ui| {
                                let _ = ui.checkbox(&mut false, "");
                            });
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("ID")
                                        .size(TS_SM)
                                        .strong()
                                        .color(t().text_secondary),
                                );
                            });
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("Name")
                                        .size(TS_SM)
                                        .strong()
                                        .color(t().text_secondary),
                                );
                            });
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("Junk?")
                                        .size(TS_SM)
                                        .strong()
                                        .color(t().text_secondary),
                                );
                            });
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("Confidence")
                                        .size(TS_SM)
                                        .strong()
                                        .color(t().text_secondary),
                                );
                            });
                            header.col(|ui| {
                                ui.label(
                                    RichText::new("Signals")
                                        .size(TS_SM)
                                        .strong()
                                        .color(t().text_secondary),
                                );
                            });
                        })
                        .body(|mut body| {
                            for decision in visible_decisions {
                                body.row(text_height * 1.2, |mut row| {
                                    let app_id = decision.app_id;
                                    let mut checked = self.junk_selected.contains(&app_id);
                                    row.col(|ui| {
                                        if ui.checkbox(&mut checked, "").changed() {
                                            if checked {
                                                self.junk_selected.insert(app_id);
                                            } else {
                                                self.junk_selected.remove(&app_id);
                                            }
                                        }
                                    });
                                    row.col(|ui| {
                                        ui.label(
                                            RichText::new(decision.app_id.to_string())
                                                .size(TS_BODY)
                                                .color(t().text_primary),
                                        );
                                    });
                                    row.col(|ui| {
                                        ui.label(
                                            RichText::new(&decision.name)
                                                .size(TS_BODY)
                                                .color(t().text_primary),
                                        );
                                    });
                                    row.col(|ui| {
                                        if decision.is_junk {
                                            status_badge(ui, "Yes", t().error_soft, t().error);
                                        } else {
                                            status_badge(
                                                ui,
                                                "No",
                                                t().surface_sunken,
                                                t().text_muted,
                                            );
                                        }
                                    });
                                    row.col(|ui| {
                                        ui.label(
                                            RichText::new(format!(
                                                "{:.0}%",
                                                decision.confidence * 100.0
                                            ))
                                            .size(TS_BODY)
                                            .color(t().text_primary),
                                        );
                                    });
                                    row.col(|ui| {
                                        let signals: Vec<String> = decision
                                            .matched
                                            .iter()
                                            .map(format_junk_signal)
                                            .collect();
                                        ui.label(
                                            RichText::new(if signals.is_empty() {
                                                empty_value_label().to_string()
                                            } else {
                                                signals.join(", ")
                                            })
                                            .size(TS_BODY)
                                            .color(t().text_secondary),
                                        );
                                    });
                                });
                            }
                        });
                });
            });

            // Summary rail.
            ui.add_space(SP_4);
            ui.allocate_ui_with_layout(
                egui::vec2(200.0, ui.available_height()),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    egui::Frame::group(ui.style())
                        .fill(t().surface)
                        .stroke(egui::Stroke::new(1.0, t().border_soft))
                        .corner_radius(CORNER_MD)
                        .inner_margin(egui::Margin::same(m(SP_3)))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Summary")
                                    .size(TS_MD)
                                    .strong()
                                    .color(t().text_primary),
                            );
                            ui.add_space(SP_2);
                            ui.separator();
                            ui.add_space(SP_2);
                            ui.spacing_mut().item_spacing.y = SP_3;
                            insight_metric(ui, "Evaluated", self.junk_results.len().to_string());
                            insight_metric(ui, "Candidates", junk_count.to_string());
                            insight_metric(ui, "Selected", selected_count.to_string());
                            insight_metric(ui, "Sel. junk", selected_junk_count.to_string());
                        });
                },
            );
        });

        // Write actions — selected-only, dry-run/confirm gated.
        // Empty selection = 0 targets; buttons are disabled.
        let has_selection = selected_count > 0;
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
                let apply_enabled = !busy && has_selection && !self.junk_collection_name.is_empty();
                let action_label = if has_selection {
                    format!("Apply {selected_count} selected")
                } else {
                    "Apply (no selection)".to_string()
                };
                if ui
                    .add_enabled(apply_enabled, {
                        egui::Button::new(
                            RichText::new(action_label)
                                .size(TS_SM)
                                .color(t().text_inverse),
                        )
                        .fill(t().accent)
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(CORNER_SM)
                    })
                    .clicked()
                {
                    self.start_dry_run(PendingAction::JunkApply);
                }

                let hide_label = if has_selection {
                    format!("Hide {selected_count} selected")
                } else {
                    "Hide (no selection)".to_string()
                };
                if ui
                    .add_enabled(!busy && has_selection, {
                        egui::Button::new(
                            RichText::new(hide_label)
                                .size(TS_SM)
                                .color(t().text_primary),
                        )
                        .fill(t().surface)
                        .stroke(egui::Stroke::new(1.0, t().border_soft))
                        .corner_radius(CORNER_SM)
                    })
                    .clicked()
                {
                    self.start_dry_run(PendingAction::JunkHide);
                }

                if self.write_loading {
                    ui.spinner();
                    ui.label(
                        RichText::new("Writing")
                            .size(TS_SM)
                            .color(t().text_secondary),
                    );
                }
                if self.dry_run_loading {
                    ui.spinner();
                    ui.label(
                        RichText::new("Preparing diff")
                            .size(TS_SM)
                            .color(t().text_secondary),
                    );
                }
            });
            ui.add_space(SP_1);
            ui.label(
                RichText::new(if has_selection {
                    "Selected-only write plan. Writes require dry-run confirmation and respect write safety."
                } else {
                    "Select junk candidates to enable write actions. Empty selection means 0 targets."
                })
                .size(TS_XS)
                .color(t().text_muted),
            );
        });
    }

    // -- Recommend view -----------------------------------------------------

    fn render_recommend(&mut self, ui: &mut egui::Ui) {
        view_header(
            ui,
            "Recommendations",
            "Shape a play session, preview scored picks, then write them to vapourfly-picks after confirmation.",
        );

        // Session planner card.
        section_card(ui, "Session Planner", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_2);
                form_field(ui, "Available minutes", |ui| {
                    ui.add_sized(
                        [88.0, 28.0],
                        egui::TextEdit::singleline(&mut self.recommend_minutes).hint_text("120"),
                    );
                });
                // Quick duration buttons.
                for &mins in [30u32, 60, 120, 240].iter() {
                    let label = if mins < 60 {
                        format!("{mins}m")
                    } else if mins % 60 == 0 {
                        format!("{}h", mins / 60)
                    } else {
                        format!("{}h{}m", mins / 60, mins % 60)
                    };
                    if ui
                        .add(
                            egui::Button::new(RichText::new(label).size(TS_XS))
                                .fill(t().surface)
                                .stroke(egui::Stroke::new(1.0, t().border_soft))
                                .corner_radius(CORNER_PILL),
                        )
                        .clicked()
                    {
                        self.recommend_minutes = mins.to_string();
                    }
                }
            });
            ui.add_space(SP_2);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_2);
                form_field(ui, "Count", |ui| {
                    ui.add_sized(
                        [64.0, 28.0],
                        egui::TextEdit::singleline(&mut self.recommend_count).hint_text("5"),
                    );
                });
                // Quick count buttons.
                for &n in [3u32, 5, 10].iter() {
                    if ui
                        .add(
                            egui::Button::new(RichText::new(n.to_string()).size(TS_XS))
                                .fill(t().surface)
                                .stroke(egui::Stroke::new(1.0, t().border_soft))
                                .corner_radius(CORNER_PILL),
                        )
                        .clicked()
                    {
                        self.recommend_count = n.to_string();
                    }
                }
            });
            ui.add_space(SP_2);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_2);
                form_field(ui, "Seed AppID", |ui| {
                    ui.add_sized(
                        [100.0, 28.0],
                        egui::TextEdit::singleline(&mut self.recommend_seed).hint_text("optional"),
                    );
                });
                // Searchable seed autocomplete from library.
                if let Some(scan) = &self.scan_result {
                    ui.label(
                        RichText::new("Search:")
                            .size(TS_SM)
                            .color(t().text_secondary),
                    );
                    ui.add_sized(
                        [120.0, 28.0],
                        egui::TextEdit::singleline(&mut self.recommend_seed_search)
                            .hint_text("library search"),
                    );
                    let q = self.recommend_seed_search.to_lowercase();
                    let seed_games: Vec<(u32, String)> = scan
                        .games
                        .iter()
                        .filter(|g| q.is_empty() || g.name.to_lowercase().contains(&q))
                        .take(20)
                        .map(|g| (g.app_id, g.name.clone()))
                        .collect();
                    if !seed_games.is_empty() {
                        egui::ComboBox::from_id_salt("rec_seed_picker")
                            .selected_text("Pick from library\u{2026}")
                            .width(200.0)
                            .show_ui(ui, |ui| {
                                for (app_id, name) in seed_games {
                                    if ui
                                        .selectable_label(false, format!("{app_id} — {name}"))
                                        .clicked()
                                    {
                                        self.recommend_seed = app_id.to_string();
                                    }
                                }
                            });
                    }
                }
            });
            ui.add_space(SP_2);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                filter_toggle(ui, &mut self.recommend_installed_only, "Installed only");
                filter_toggle(ui, &mut self.recommend_deck, "Deck mode");
                if self.recommend_loading {
                    ui.spinner();
                }
                if ui
                    .add_enabled(self.library_ready(), primary_button_widget("Preview"))
                    .clicked()
                {
                    self.start_recommend_preview(ui.ctx());
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

        // Match percent formula: deck mode → score/7.5, normal → score/5.5.
        // Rounded and clamped to 0–100. Uses the Deck mode captured at preview
        // start time (recommend_request_at_start) so a result computed for one
        // mode is not re-scored against the current mode if the user toggled it
        // mid-job.
        let deck_mode = self
            .recommend_request_at_start
            .as_ref()
            .map(|r| r.deck_mode)
            .unwrap_or(self.recommend_deck);
        let max_score = if deck_mode { 7.5 } else { 5.5 };
        let match_pct = |score: f32| -> u32 {
            let pct = (score / max_score * 100.0).round() as i32;
            pct.clamp(0, 100) as u32
        };

        // Top-3 highlight cards.
        let top3: Vec<&Recommendation> = self.recommend_results.iter().take(3).collect();
        if !top3.is_empty() {
            ui.add_space(SP_3);
            ui.label(
                RichText::new("Top picks")
                    .size(TS_MD)
                    .strong()
                    .color(t().text_primary),
            );
            ui.add_space(SP_2);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_3);
                for (rank, rec) in top3.iter().enumerate() {
                    let medal = match rank {
                        0 => "\u{1F947}", // 🥇
                        1 => "\u{1F948}", // 🥈
                        _ => "\u{1F949}", // 🥉
                    };
                    let is_selected = self.recommend_selected == Some(rec.app_id);
                    egui::Frame::group(ui.style())
                        .fill(if is_selected {
                            t().accent_soft
                        } else {
                            t().surface
                        })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if is_selected {
                                t().accent
                            } else {
                                t().border_soft
                            },
                        ))
                        .corner_radius(CORNER_MD)
                        .inner_margin(egui::Margin::same(m(SP_3)))
                        .show(ui, |ui| {
                            ui.set_min_width(200.0);
                            ui.vertical(|ui| {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(medal).size(TS_LG));
                                    ui.label(
                                        RichText::new(&rec.name)
                                            .size(TS_MD)
                                            .strong()
                                            .color(t().text_primary),
                                    );
                                });
                                ui.add_space(SP_1);
                                ui.horizontal(|ui| {
                                    app_id_tag(ui, rec.app_id);
                                    status_badge(
                                        ui,
                                        &format!("{}% match", match_pct(rec.score)),
                                        t().accent_soft,
                                        t().accent_text,
                                    );
                                });
                                ui.add_space(SP_1);
                                ui.label(
                                    RichText::new(format!("Score: {:.2}", rec.score))
                                        .size(TS_SM)
                                        .color(t().text_secondary),
                                );
                                if ui
                                    .button(RichText::new("Why this pick?").size(TS_XS))
                                    .clicked()
                                {
                                    self.recommend_selected = Some(rec.app_id);
                                }
                            });
                        });
                }
            });
        }

        ui.add_space(SP_3);

        // Two-column: compact results list (left) + explanation rail (right).
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                // Write action bar.
                let busy = self.write_loading || self.dry_run_loading;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                    if ui
                        .add_enabled(
                            !busy,
                            egui::Button::new(
                                RichText::new("Write to vapourfly-picks")
                                    .size(TS_SM)
                                    .color(t().text_inverse),
                            )
                            .fill(t().accent)
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
                            .color(t().text_secondary),
                        );
                    }
                });
                ui.label(
                    RichText::new(
                        "Requires dry-run confirmation. Targets the vapourfly-picks Steam Collection.",
                    )
                    .size(TS_XS)
                    .color(t().text_muted),
                );
                ui.add_space(SP_3);

                // Compact results list (after top 3): selectable rows.
                for rec in self.recommend_results.iter().skip(3) {
                    let is_selected = self.recommend_selected == Some(rec.app_id);
                    egui::Frame::group(ui.style())
                        .fill(if is_selected { t().accent_soft } else { t().surface })
                        .stroke(egui::Stroke::new(
                            1.0,
                            if is_selected { t().accent } else { t().border_soft },
                        ))
                        .corner_radius(CORNER_SM)
                        .inner_margin(egui::Margin::same(m(SP_2)))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                                app_id_tag(ui, rec.app_id);
                                ui.label(
                                    RichText::new(&rec.name)
                                        .size(TS_BODY)
                                        .color(t().text_primary),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        status_badge(
                                            ui,
                                            &format!("{}% match", match_pct(rec.score)),
                                            t().accent_soft,
                                            t().accent_text,
                                        );
                                        ui.label(
                                            RichText::new(format!("{:.2}", rec.score))
                                                .size(TS_SM)
                                                .color(t().text_secondary),
                                        );
                                    },
                                );
                            });
                            if ui.button(RichText::new("Why?").size(TS_XS)).clicked() {
                                self.recommend_selected = Some(rec.app_id);
                            }
                        });
                }
            });

            // Explanation rail: "Why this pick?" for selected, or general scoring.
            ui.add_space(SP_4);
            ui.allocate_ui_with_layout(
                egui::vec2(220.0, ui.available_height()),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    if let Some(selected_id) = self.recommend_selected {
                        if let Some(rec) = self
                            .recommend_results
                            .iter()
                            .find(|r| r.app_id == selected_id)
                        {
                            egui::Frame::group(ui.style())
                                .fill(t().surface)
                                .stroke(egui::Stroke::new(1.0, t().border_soft))
                                .corner_radius(CORNER_MD)
                                .inner_margin(egui::Margin::same(m(SP_3)))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("Why this pick?")
                                            .size(TS_MD)
                                            .strong()
                                            .color(t().text_primary),
                                    );
                                    ui.add_space(SP_1);
                                    ui.label(
                                        RichText::new(&rec.name)
                                            .size(TS_BODY)
                                            .color(t().text_primary),
                                    );
                                    ui.horizontal(|ui| {
                                        app_id_tag(ui, rec.app_id);
                                        status_badge(
                                            ui,
                                            &format!("{}% match", match_pct(rec.score)),
                                            t().accent_soft,
                                            t().accent_text,
                                        );
                                        ui.label(
                                            RichText::new(format!("Score: {:.2}", rec.score))
                                                .size(TS_SM)
                                                .color(t().text_secondary),
                                        );
                                    });
                                    ui.add_space(SP_2);
                                    ui.separator();
                                    ui.add_space(SP_2);
                                    ui.label(
                                        RichText::new("Reason codes")
                                            .size(TS_SM)
                                            .strong()
                                            .color(t().text_secondary),
                                    );
                                    ui.add_space(SP_1);
                                    for reason in &rec.reasons {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().item_spacing =
                                                egui::vec2(SP_1, SP_1);
                                            status_badge(
                                                ui,
                                                &reason.code,
                                                t().surface_muted,
                                                t().text_secondary,
                                            );
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} ({:+.1})",
                                                    reason.description, reason.weight
                                                ))
                                                .size(TS_SM)
                                                .color(t().text_secondary),
                                            );
                                        });
                                    }
                                    // Game metadata: cover, HLTB, playtime, Deck.
                                    if let Some(scan) = &self.scan_result {
                                        if let Some(game) =
                                            scan.games.iter().find(|g| g.app_id == rec.app_id)
                                        {
                                            ui.add_space(SP_2);
                                            ui.separator();
                                            ui.add_space(SP_2);
                                            ui.label(
                                                RichText::new("Game details")
                                                    .size(TS_SM)
                                                    .strong()
                                                    .color(t().text_secondary),
                                            );
                                            ui.add_space(SP_1);
                                            if let Some(pt) = game.playtime_minutes {
                                                insight_metric(
                                                    ui,
                                                    "Playtime",
                                                    format!("{}h", pt / 60),
                                                );
                                            }
                                            if let Some(igdb) = &game.igdb {
                                                if let Some(ttb) = &igdb.time_to_beat {
                                                    if let Some(norm) = ttb.normally_seconds {
                                                        insight_metric(
                                                            ui,
                                                            "HLTB",
                                                            format!("{}h", norm / 3600),
                                                        );
                                                    }
                                                }
                                            }
                                            if let Some(pcgw) = &game.pcgw {
                                                let deck = match pcgw.controller_support {
                                                    ControllerSupport::Full => "Full",
                                                    ControllerSupport::Partial => "Partial",
                                                    ControllerSupport::None => "None",
                                                    ControllerSupport::Unknown => "Unknown",
                                                };
                                                insight_metric(ui, "Deck", deck.to_string());
                                            }
                                        }
                                    }
                                });
                        }
                    } else {
                        // General scoring explanation.
                        egui::Frame::group(ui.style())
                            .fill(t().surface)
                            .stroke(egui::Stroke::new(1.0, t().border_soft))
                            .corner_radius(CORNER_MD)
                            .inner_margin(egui::Margin::same(m(SP_3)))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("How scoring works")
                                        .size(TS_MD)
                                        .strong()
                                        .color(t().text_primary),
                                );
                                ui.add_space(SP_2);
                                ui.separator();
                                ui.add_space(SP_2);
                                ui.spacing_mut().item_spacing.y = SP_3;
                                insight_metric(
                                    ui,
                                    "Results",
                                    self.recommend_results.len().to_string(),
                                );
                                let avg_score = if self.recommend_results.is_empty() {
                                    0.0
                                } else {
                                    self.recommend_results.iter().map(|r| r.score).sum::<f32>()
                                        / self.recommend_results.len() as f32
                                };
                                insight_metric(ui, "Avg score", format!("{avg_score:.2}"));
                                let top_score = self
                                    .recommend_results
                                    .first()
                                    .map(|r| r.score)
                                    .unwrap_or(0.0);
                                insight_metric(ui, "Top score", format!("{top_score:.2}"));
                                ui.separator();
                                ui.add_space(SP_2);
                                ui.label(
                                    RichText::new(format!(
                                        "Match % = score / {:.1} ({} mode). Scores combine playtime fit, deck compatibility, ProtonDB tier, and rating signals.",
                                        max_score,
                                        if deck_mode { "deck" } else { "normal" }
                                    ))
                                    .size(TS_XS)
                                    .color(t().text_muted),
                                );
                                ui.add_space(SP_2);
                                ui.label(
                                    RichText::new("Click \"Why this pick?\" on any result to see its reason codes.")
                                        .size(TS_XS)
                                        .color(t().text_muted),
                                );
                            });
                    }
                },
            );
        });
    }

    // -- Playlists view -----------------------------------------------------

    fn render_playlists(&mut self, ui: &mut egui::Ui) {
        // Refresh store list once when first entering Playlists (or after generate/save).
        if !self.playlist_store_ids_loaded {
            self.refresh_playlist_store_ids();
        }

        view_header_with_actions(
            ui,
            "Playlists",
            "Create, load, match, and sync Vapourfly playlists. Dynamic and Mood generate into stable store slots.",
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_1);
                    if secondary_button(ui, "Dynamic").clicked() {
                        self.playlist_chooser = PlaylistChooser::Dynamic;
                    }
                    if secondary_button(ui, "Mood").clicked() {
                        self.playlist_chooser = PlaylistChooser::Mood;
                    }
                    if secondary_button(ui, "Import").clicked() {
                        self.playlist_show_import = !self.playlist_show_import;
                    }
                });
            },
        );

        // -- Duplicate ID Replace confirm dialog ------------------------------
        if let Some((existing_id, pending_pf)) = self.playlist_dup_id_confirm.clone() {
            egui::Window::new("Duplicate Playlist ID")
                .fixed_size([360.0, 160.0])
                .collapsible(false)
                .resizable(false)
                .show(ui.ctx(), |ui| {
                    ui.label(
                        RichText::new(format!(
                            "A playlist with ID '{existing_id}' already exists. Replace it?"
                        ))
                        .size(TS_BODY)
                        .color(t().text_primary),
                    );
                    ui.add_space(SP_2);
                    ui.label(
                        RichText::new(format!(
                            "Incoming: '{}' ({})",
                            pending_pf.playlist.name, pending_pf.playlist.id
                        ))
                        .size(TS_SM)
                        .color(t().text_secondary),
                    );
                    ui.add_space(SP_3);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                        if primary_button(ui, "Replace").clicked() {
                            match self.store_playlist(&pending_pf) {
                                Ok(()) => {
                                    self.adopt_playlist_for_edit(&pending_pf);
                                    self.match_playlist_against_library_background(
                                        ui.ctx(),
                                        &pending_pf,
                                    );
                                    self.refresh_playlist_store_ids();
                                    self.playlist_load_selected = pending_pf.playlist.id.clone();
                                    self.success_msg = Some(format!(
                                        "Replaced playlist '{}'",
                                        pending_pf.playlist.name
                                    ));
                                }
                                Err(e) => self.error = Some(e),
                            }
                            self.playlist_dup_id_confirm = None;
                        }
                        if ghost_button(ui, "Cancel").clicked() {
                            self.playlist_dup_id_confirm = None;
                        }
                    });
                });
        }

        // -- Import sub-route panel ------------------------------------------
        if self.playlist_show_import {
            section_card(ui, "Import", |ui| {
                form_field(ui, "File path", |ui| {
                    ui.add_sized(
                        [280.0, 28.0],
                        egui::TextEdit::singleline(&mut self.playlist_import_path)
                            .hint_text("/path/to/playlist.json"),
                    );
                    if secondary_button(ui, "Import File").clicked()
                        && !self.playlist_import_path.is_empty()
                    {
                        match playlist::import_playlist(Path::new(&self.playlist_import_path)) {
                            Ok(pf) => {
                                // Check for duplicate ID.
                                if self.playlist_store_ids.contains(&pf.playlist.id) {
                                    self.playlist_dup_id_confirm =
                                        Some((pf.playlist.id.clone(), pf));
                                } else if let Err(e) = self.store_playlist(&pf) {
                                    self.error = Some(e);
                                } else {
                                    self.adopt_playlist_for_edit(&pf);
                                    self.refresh_playlist_store_ids();
                                    self.playlist_load_selected = pf.playlist.id.clone();
                                    self.success_msg =
                                        Some(format!("Imported playlist '{}'", pf.playlist.name));
                                }
                            }
                            Err(e) => self.error = Some(format!("Import failed: {e}")),
                        }
                    }
                });
                ui.add_space(SP_2);
                form_field(ui, "Share code", |ui| {
                    ui.add_sized(
                        [280.0, 28.0],
                        egui::TextEdit::singleline(&mut self.playlist_share_code_input)
                            .hint_text("VF1:…"),
                    );
                    if secondary_button(ui, "Import Code").clicked()
                        && !self.playlist_share_code_input.is_empty()
                    {
                        match share_code::decode_share_code(&self.playlist_share_code_input) {
                            Ok(pf) => {
                                if self.playlist_store_ids.contains(&pf.playlist.id) {
                                    self.playlist_dup_id_confirm =
                                        Some((pf.playlist.id.clone(), pf));
                                } else if let Err(e) = self.store_playlist(&pf) {
                                    self.error = Some(e);
                                } else {
                                    self.adopt_playlist_for_edit(&pf);
                                    self.refresh_playlist_store_ids();
                                    self.playlist_load_selected = pf.playlist.id.clone();
                                    self.success_msg = Some(format!(
                                        "Imported share code as '{}'",
                                        pf.playlist.name
                                    ));
                                }
                            }
                            Err(e) => self.error = Some(format!("Share code import failed: {e}")),
                        }
                    }
                });
                ui.add_space(SP_2);
                if ghost_button(ui, "Close").clicked() {
                    self.playlist_show_import = false;
                }
            });
        }

        // -- Master-detail layout --------------------------------------------
        ui.horizontal(|ui| {
            // Left rail: playlist list.
            ui.vertical(|ui| {
                ui.set_min_width(220.0);
                ui.set_max_width(260.0);
                ui.label(
                    RichText::new("Playlists")
                        .size(TS_SM)
                        .strong()
                        .color(t().text_secondary),
                );
                ui.add_space(SP_1);

                if self.playlist_store_ids.is_empty() {
                    ui.label(
                        RichText::new("No playlists yet.")
                            .size(TS_SM)
                            .color(t().text_muted),
                    );
                    if ghost_button(ui, "Refresh list").clicked() {
                        self.refresh_playlist_store_ids();
                    }
                } else {
                    // "+ New" entry.
                    let is_new_selected = self.playlist_load_selected.is_empty()
                        && self.playlist_last_import.is_none();
                    if ui
                        .selectable_label(is_new_selected, "+ New playlist")
                        .clicked()
                    {
                        self.playlist_edit_id = String::new();
                        self.playlist_edit_name = String::new();
                        self.playlist_edit_description = String::new();
                        self.playlist_edit_app_ids = String::new();
                        self.playlist_edit_rules = String::new();
                        self.playlist_last_import = None;
                        self.playlist_match_report = None;
                        self.playlist_load_selected = String::new();
                        self.playlist_detail_tab = PlaylistDetailTab::Games;
                    }
                    ui.separator();
                    ui.add_space(SP_1);

                    let ids = self.playlist_store_ids.clone();
                    for id in &ids {
                        let is_selected = self.playlist_load_selected == *id;
                        let label = if let Some(pf) = &self.playlist_last_import {
                            if pf.playlist.id == *id {
                                format!("{} · {}", id, pf.playlist.name)
                            } else {
                                id.clone()
                            }
                        } else {
                            id.clone()
                        };
                        if ui.selectable_label(is_selected, &label).clicked() {
                            match self.load_playlist_from_store(id) {
                                Ok(()) => {
                                    self.playlist_load_selected = id.clone();
                                }
                                Err(e) => self.error = Some(e),
                            }
                        }
                    }
                    ui.add_space(SP_2);
                    if ghost_button(ui, "Refresh list").clicked() {
                        self.refresh_playlist_store_ids();
                    }
                }
            });

            ui.separator();

            // Right workspace.
            ui.vertical(|ui| {
                self.render_playlist_workspace(ui);
            });
        });
    }

    /// Render the right workspace of the Playlists master-detail.
    fn render_playlist_workspace(&mut self, ui: &mut egui::Ui) {
        // Hero: ID, Name, Description fields + Save.
        section_card(ui, "Playlist", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_2);
                form_field(ui, "ID", |ui| {
                    ui.add_sized(
                        [160.0, 28.0],
                        egui::TextEdit::singleline(&mut self.playlist_edit_id).hint_text("my-list"),
                    );
                });
                form_field(ui, "Name", |ui| {
                    ui.add_sized(
                        [180.0, 28.0],
                        egui::TextEdit::singleline(&mut self.playlist_edit_name)
                            .hint_text("Display name"),
                    );
                });
            });
            ui.add_space(SP_2);
            form_field(ui, "Description", |ui| {
                ui.add_sized(
                    [360.0, 28.0],
                    egui::TextEdit::singleline(&mut self.playlist_edit_description)
                        .hint_text("Optional"),
                );
            });
            ui.add_space(SP_3);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                if primary_button(ui, "Save Playlist").clicked() {
                    match self.build_playlist_from_edit_fields() {
                        Ok(pf) => {
                            // Check for duplicate ID.
                            if self.playlist_store_ids.contains(&pf.playlist.id)
                                && self.playlist_load_selected != pf.playlist.id
                            {
                                self.playlist_dup_id_confirm = Some((pf.playlist.id.clone(), pf));
                            } else {
                                match self.store_playlist(&pf) {
                                    Ok(()) => {
                                        self.playlist_last_import = Some(pf.clone());
                                        self.match_playlist_against_library_background(
                                            ui.ctx(),
                                            &pf,
                                        );
                                        self.refresh_playlist_store_ids();
                                        self.playlist_load_selected = pf.playlist.id.clone();
                                        self.success_msg =
                                            Some(format!("Saved playlist '{}'", pf.playlist.name));
                                    }
                                    Err(e) => self.error = Some(e),
                                }
                            }
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
                // Share code / JSON tabs.
                ui.separator();
                let share_tab = &mut self.playlist_share_tab;
                ui.selectable_value(share_tab, PlaylistShareTab::ShareCode, "Share Code");
                ui.selectable_value(share_tab, PlaylistShareTab::Json, "JSON");
                if let Some(pf) = &self.playlist_last_import {
                    match self.playlist_share_tab {
                        PlaylistShareTab::ShareCode => {
                            if secondary_button(ui, "Copy Share Code").clicked() {
                                match share_code::encode_share_code(pf) {
                                    Ok(code) => {
                                        self.playlist_share_code_output = Some(code.clone());
                                        ui.ctx().copy_text(code);
                                        self.success_msg =
                                            Some("Share code copied to clipboard (VF1).".into());
                                    }
                                    Err(e) => self.error = Some(format!("Share code failed: {e}")),
                                }
                            }
                            if let Some(code) = &self.playlist_share_code_output {
                                ui.label(
                                    RichText::new(format!("VF1: {code}"))
                                        .size(TS_SM)
                                        .color(t().text_muted)
                                        .monospace(),
                                );
                            }
                        }
                        PlaylistShareTab::Json => {
                            if secondary_button(ui, "Export…").clicked() {
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_file_name(format!("{}.json", pf.playlist.id))
                                    .add_filter("JSON", &["json"])
                                    .save_file()
                                {
                                    self.playlist_export_path = path.display().to_string();
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
                            }
                        }
                    }
                    // Sync button.
                    let busy = self.write_loading || self.dry_run_loading;
                    if ui
                        .add_enabled(
                            !busy,
                            egui::Button::new(
                                RichText::new("Sync to Steam Collection")
                                    .size(TS_SM)
                                    .color(t().text_inverse),
                            )
                            .fill(t().accent)
                            .stroke(egui::Stroke::NONE)
                            .corner_radius(CORNER_SM),
                        )
                        .clicked()
                    {
                        self.start_dry_run(PendingAction::PlaylistSync(pf.clone()));
                    }
                    if self.dry_run_loading {
                        ui.spinner();
                    }
                }
            });
        });

        // Tabs: Games / Rules / Match.
        ui.add_space(SP_3);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SP_1, SP_1);
            let tab = &mut self.playlist_detail_tab;
            ui.selectable_value(tab, PlaylistDetailTab::Games, "Games");
            ui.selectable_value(tab, PlaylistDetailTab::Rules, "Rules");
            ui.selectable_value(tab, PlaylistDetailTab::Match, "Match");
        });
        ui.separator();
        ui.add_space(SP_2);

        match self.playlist_detail_tab {
            PlaylistDetailTab::Games => self.render_playlist_games_tab(ui),
            PlaylistDetailTab::Rules => self.render_playlist_rules_tab(ui),
            PlaylistDetailTab::Match => self.render_playlist_match_tab(ui),
        }
    }

    /// Games tab: App IDs CSV editor + game search Add/Remove.
    fn render_playlist_games_tab(&mut self, ui: &mut egui::Ui) {
        section_card(ui, "App IDs", |ui| {
            form_field(ui, "App IDs", |ui| {
                ui.add_sized(
                    [360.0, 28.0],
                    egui::TextEdit::singleline(&mut self.playlist_edit_app_ids)
                        .hint_text("730, 440, …"),
                );
            });
            ui.label(
                RichText::new("Comma-separated Steam AppIDs for manual playlists.")
                    .size(TS_XS)
                    .color(t().text_muted),
            );
        });

        // Game search Add/Remove.
        ui.add_space(SP_3);
        section_card(ui, "Search & Add", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                ui.label(
                    RichText::new("Search:")
                        .size(TS_SM)
                        .color(t().text_secondary),
                );
                ui.add_sized(
                    [200.0, 28.0],
                    egui::TextEdit::singleline(&mut self.playlist_game_search)
                        .hint_text("game name or AppID"),
                );
            });
            ui.add_space(SP_2);

            // Parse current App IDs into a set.
            let mut current_ids: std::collections::HashSet<u32> = self
                .playlist_edit_app_ids
                .split(',')
                .filter_map(|s| s.trim().parse::<u32>().ok())
                .collect();

            // Show matching games from library.
            if let Some(scan) = &self.scan_result {
                let q = self.playlist_game_search.to_lowercase();
                let matches: Vec<&Game> = scan
                    .games
                    .iter()
                    .filter(|g| {
                        q.is_empty()
                            || g.name.to_lowercase().contains(&q)
                            || g.app_id.to_string().contains(&q)
                    })
                    .take(12)
                    .collect();
                if matches.is_empty() && !q.is_empty() {
                    ui.label(
                        RichText::new("No matching games in library.")
                            .size(TS_SM)
                            .color(t().text_muted),
                    );
                }
                for game in matches {
                    let already = current_ids.contains(&game.app_id);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_1);
                        app_id_tag(ui, game.app_id);
                        ui.label(
                            RichText::new(&game.name)
                                .size(TS_SM)
                                .color(t().text_primary),
                        );
                        if already {
                            if ghost_button(ui, "Remove").clicked() {
                                current_ids.remove(&game.app_id);
                                self.playlist_edit_app_ids = current_ids
                                    .iter()
                                    .map(|id| id.to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ");
                            }
                            status_badge(ui, "added", t().accent_soft, t().accent_text);
                        } else if secondary_button(ui, "Add").clicked() {
                            current_ids.insert(game.app_id);
                            self.playlist_edit_app_ids = current_ids
                                .iter()
                                .map(|id| id.to_string())
                                .collect::<Vec<_>>()
                                .join(", ");
                        }
                    });
                }
            } else {
                ui.label(
                    RichText::new("Scan your library to search for games to add.")
                        .size(TS_SM)
                        .color(t().text_muted),
                );
            }
        });
    }

    /// Rules tab: visual rule editor + Advanced JSON toggle.
    fn render_playlist_rules_tab(&mut self, ui: &mut egui::Ui) {
        section_card(ui, "Rules", |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                ui.label(
                    RichText::new(if self.playlist_show_advanced_json {
                        "Advanced JSON"
                    } else {
                        "Visual editor"
                    })
                    .size(TS_SM)
                    .strong()
                    .color(t().text_secondary),
                );
                if ghost_button(ui, "Toggle Advanced JSON").clicked() {
                    self.playlist_show_advanced_json = !self.playlist_show_advanced_json;
                }
            });
            ui.add_space(SP_2);

            if self.playlist_show_advanced_json {
                // Advanced JSON editor.
                form_field(ui, "Rules JSON", |ui| {
                    ui.add_sized(
                        [360.0, 120.0],
                        egui::TextEdit::multiline(&mut self.playlist_edit_rules)
                            .code_editor()
                            .desired_width(360.0)
                            .hint_text(r#"[{"op":"Installed"}]"#),
                    );
                });
                ui.label(
                    RichText::new(
                        "When provided, App IDs are ignored and a rule-based playlist is created.",
                    )
                    .size(TS_XS)
                    .color(t().text_muted),
                );
            } else {
                // Visual rule editor: show current rules as badges + quick-add.
                if self.playlist_edit_rules.is_empty() {
                    ui.label(
                        RichText::new(
                            "No rules yet. Add quick rules below or toggle Advanced JSON.",
                        )
                        .size(TS_SM)
                        .color(t().text_muted),
                    );
                } else {
                    // Parse and display rules.
                    match serde_json::from_str::<Vec<PlaylistRule>>(&self.playlist_edit_rules) {
                        Ok(rules) => {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(SP_1, SP_1);
                                for rule in &rules {
                                    let label = match rule {
                                        PlaylistRule::Installed => "Installed".into(),
                                        PlaylistRule::NotJunk => "NotJunk".into(),
                                        PlaylistRule::NotHidden => "NotHidden".into(),
                                        PlaylistRule::ControllerSupportFull => {
                                            "ControllerSupportFull".into()
                                        }
                                        PlaylistRule::ProtonAtLeast { tier } => {
                                            format!("ProtonAtLeast({tier:?})")
                                        }
                                        PlaylistRule::HltbMaxMinutes { minutes } => {
                                            format!("HltbMaxMinutes({minutes})")
                                        }
                                        PlaylistRule::PlaytimeBetween { min, max } => {
                                            format!("PlaytimeBetween({min}-{max})")
                                        }
                                        PlaylistRule::RatingAtLeast { rating_0_5 } => {
                                            format!("RatingAtLeast({rating_0_5})")
                                        }
                                        PlaylistRule::HasGenre { genre } => {
                                            format!("HasGenre({genre})")
                                        }
                                        PlaylistRule::HasTag { tag } => {
                                            format!("HasTag({tag})")
                                        }
                                        PlaylistRule::And(_) => "And(…)".into(),
                                        PlaylistRule::Or(_) => "Or(…)".into(),
                                        PlaylistRule::Not(_) => "Not(…)".into(),
                                    };
                                    status_badge(ui, &label, t().surface_muted, t().text_secondary);
                                }
                            });
                        }
                        Err(_) => {
                            ui.label(
                                RichText::new("Invalid JSON. Toggle Advanced JSON to edit.")
                                    .size(TS_SM)
                                    .color(t().error),
                            );
                        }
                    }
                }
                // Quick-add rule buttons.
                ui.add_space(SP_2);
                ui.label(
                    RichText::new("Quick add:")
                        .size(TS_SM)
                        .color(t().text_secondary),
                );
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SP_1, SP_1);
                    let quick_rules: &[(&str, PlaylistRule)] = &[
                        ("Installed", PlaylistRule::Installed),
                        ("NotHidden", PlaylistRule::NotHidden),
                        ("NotJunk", PlaylistRule::NotJunk),
                        ("ControllerSupportFull", PlaylistRule::ControllerSupportFull),
                    ];
                    for (label, rule) in quick_rules {
                        if ui
                            .add(
                                egui::Button::new(RichText::new(*label).size(TS_XS))
                                    .fill(t().surface)
                                    .stroke(egui::Stroke::new(1.0, t().border_soft))
                                    .corner_radius(CORNER_PILL),
                            )
                            .clicked()
                        {
                            // Add rule to JSON.
                            let mut rules: Vec<PlaylistRule> =
                                serde_json::from_str(&self.playlist_edit_rules).unwrap_or_default();
                            rules.push(rule.clone());
                            self.playlist_edit_rules =
                                serde_json::to_string_pretty(&rules).unwrap_or_default();
                        }
                    }
                });
            }
        });
    }

    /// Match tab: Owned/Missing tabs + completion price + match summary.
    fn render_playlist_match_tab(&mut self, ui: &mut egui::Ui) {
        if self.playlist_match_loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(
                    RichText::new("Matching against library…")
                        .size(TS_SM)
                        .color(t().text_secondary),
                );
            });
            return;
        }

        let Some(report) = &self.playlist_match_report else {
            empty_state(
                ui,
                "\u{1F50D}",
                "No match report",
                "Save or load a playlist to see the match report.",
            );
            return;
        };

        // Match summary metrics.
        section_card(ui, "Match summary", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                metric_pill(ui, "Owned", report.owned.len().to_string());
                metric_pill(ui, "Missing", report.missing.len().to_string());
                metric_pill(ui, "Played", report.played.len().to_string());
                metric_pill(ui, "Unplayed", report.unplayed.len().to_string());
                metric_pill(ui, "Hidden", report.hidden.len().to_string());
                metric_pill(ui, "Junk", report.junk.len().to_string());
            });

            // Completion price.
            if let Some(price) = &report.completion_price {
                ui.add_space(SP_2);
                ui.label(
                    RichText::new(format!("Completion price: {}", price.format()))
                        .size(TS_BODY)
                        .color(t().text_secondary),
                );
                if let Some(coverage) = &report.price_coverage
                    && let Some(ratio) = coverage.ratio()
                    && ratio < 1.0
                {
                    ui.label(
                        RichText::new(format!(
                            "Price coverage: {}/{} confirmed non-free priced ({:.0}%), {} free, {} unknown",
                            coverage.confirmed_non_free_priced,
                            coverage.confirmed_non_free(),
                            ratio * 100.0,
                            coverage.confirmed_free,
                            coverage.unknown
                        ))
                        .size(TS_SM)
                        .color(t().text_muted),
                    );
                }
            } else if report.missing.is_empty() {
                ui.add_space(SP_2);
                ui.label(
                    RichText::new("No missing entries — library is complete.")
                        .size(TS_SM)
                        .color(t().text_muted),
                );
            } else {
                ui.add_space(SP_2);
                ui.label(
                    RichText::new(
                        "Completion price unavailable — run cache refresh --source steam-store when online.",
                    )
                    .size(TS_SM)
                    .color(t().text_muted),
                );
            }
        });

        // Owned / Missing sub-tabs.
        ui.add_space(SP_3);
        let owned_ids = report.owned.clone();
        let missing_ids = report.missing.clone();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SP_1, SP_1);
            ui.selectable_value(
                &mut self.playlist_match_sub_tab,
                PlaylistMatchTab::Owned,
                "Owned",
            );
            ui.selectable_value(
                &mut self.playlist_match_sub_tab,
                PlaylistMatchTab::Missing,
                "Missing",
            );
        });
        ui.separator();
        ui.add_space(SP_2);

        let ids = match self.playlist_match_sub_tab {
            PlaylistMatchTab::Owned => &owned_ids,
            PlaylistMatchTab::Missing => &missing_ids,
        };

        if ids.is_empty() {
            ui.label(
                RichText::new(match self.playlist_match_sub_tab {
                    PlaylistMatchTab::Owned => "No owned entries.",
                    PlaylistMatchTab::Missing => "No missing entries — library is complete.",
                })
                .size(TS_SM)
                .color(t().text_muted),
            );
        } else {
            let names = self.playlist_owned_preview_labels(ids);
            for line in names {
                ui.label(RichText::new(line).size(TS_SM).color(t().text_secondary));
            }
            if ids.len() > 12 {
                ui.label(
                    RichText::new(format!("… and {} more", ids.len() - 12))
                        .size(TS_XS)
                        .color(t().text_muted),
                );
            }
        }
    }

    /// Human-readable preview lines for owned match AppIDs (max 12).
    fn playlist_owned_preview_labels(&self, owned_ids: &[u32]) -> Vec<String> {
        let games = self.scan_result.as_ref().map(|s| s.games.as_slice());
        owned_ids
            .iter()
            .take(12)
            .map(|id| {
                let name = games
                    .and_then(|gs| gs.iter().find(|g| g.app_id == *id))
                    .map(|g| g.name.as_str())
                    .unwrap_or("unknown");
                format!("{id} · {name}")
            })
            .collect()
    }

    // -- Discover view (top-level; ADR-0005 / ADR-0006) ----------------------

    fn render_discover(&mut self, ui: &mut egui::Ui) {
        view_header(
            ui,
            "Discover",
            "Generate similar unplayed picks from an optional seed AppID. Results write the stable discover playlist slot.",
        );

        section_card(ui, "Generate", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_2);
                form_field(ui, "Seed AppID", |ui| {
                    ui.add_sized(
                        [100.0, 28.0],
                        egui::TextEdit::singleline(&mut self.discover_seed).hint_text("optional"),
                    );
                });
                form_field(ui, "Count", |ui| {
                    ui.add_sized(
                        [64.0, 28.0],
                        egui::TextEdit::singleline(&mut self.discover_count).hint_text("20"),
                    );
                });
                if self.discover_loading {
                    ui.spinner();
                }
                if ui
                    .add_enabled(self.library_ready(), primary_button_widget("Generate"))
                    .clicked()
                {
                    self.start_discover_generate(ui.ctx());
                }
            });
            ui.add_space(SP_2);
            ui.label(
                RichText::new(
                    "Regenerate overwrites the stable discover slot. Change id/name and Save in Playlists to keep a long-term copy.",
                )
                .size(TS_XS)
                .color(t().text_muted),
            );
        });

        if self.discover_results.is_empty() && self.discover_last_playlist.is_none() {
            empty_state(
                ui,
                "\u{1F50D}",
                "No Discover results yet",
                "Set an optional seed AppID and count, then Generate.",
            );
            return;
        }

        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
            metric_pill(ui, "Results", self.discover_results.len().to_string());
            if let Some(pf) = &self.discover_last_playlist {
                metric_pill(ui, "Slot", pf.playlist.id.clone());
            }
        });
        ui.add_space(SP_3);

        // Continuation into Playlists; optional sync remains confirmation-gated.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
            if secondary_button(ui, "Open in Playlists").clicked() {
                if let Some(pf) = self.discover_last_playlist.clone() {
                    self.adopt_playlist_for_edit(&pf);
                }
                self.current_view = View::Playlists;
            }
            if let Some(pf) = self.discover_last_playlist.clone() {
                let busy = self.write_loading || self.dry_run_loading;
                if ui
                    .add_enabled(
                        !busy,
                        egui::Button::new(
                            RichText::new("Sync to Steam Collection")
                                .size(TS_SM)
                                .color(t().text_primary),
                        )
                        .fill(t().surface)
                        .stroke(egui::Stroke::new(1.0, t().border_soft))
                        .corner_radius(CORNER_SM),
                    )
                    .clicked()
                {
                    self.start_dry_run(PendingAction::PlaylistSync(pf));
                }
            }
        });
        ui.label(
            RichText::new("Sync requires dry-run confirmation before any Steam write.")
                .size(TS_XS)
                .color(t().text_muted),
        );
        ui.add_space(SP_3);

        // Result cards: name, score, reason codes (same structural pattern as Recommendations).
        for pick in &self.discover_results {
            section_card(ui, &pick.name, |ui| {
                ui.horizontal(|ui| {
                    app_id_tag(ui, pick.app_id);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        status_badge(
                            ui,
                            &format!("{:.2}", pick.score),
                            t().accent_soft,
                            t().accent_text,
                        );
                    });
                });
                if !pick.reasons.is_empty() {
                    ui.add_space(SP_1);
                    ui.indent("discover_reasons", |ui| {
                        for reason in &pick.reasons {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(SP_1, SP_1);
                                status_badge(
                                    ui,
                                    reason.code,
                                    t().surface_muted,
                                    t().text_secondary,
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{} ({:+.2})",
                                        reason.description, reason.weight
                                    ))
                                    .size(TS_SM)
                                    .color(t().text_secondary),
                                );
                            });
                        }
                    });
                }
            });
        }
    }

    // -- Playlist generator chooser modals ----------------------------------

    fn render_playlist_choosers(&mut self, ctx: &egui::Context) {
        match self.playlist_chooser {
            PlaylistChooser::None => {}
            PlaylistChooser::Dynamic => self.render_dynamic_chooser(ctx),
            PlaylistChooser::Mood => self.render_mood_chooser(ctx),
        }
    }

    fn render_dynamic_chooser(&mut self, ctx: &egui::Context) {
        let mut open = true;
        egui::Window::new("Dynamic template")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::NONE
                    .fill(t().surface_raised)
                    .stroke(egui::Stroke::new(1.0, t().border))
                    .corner_radius(CORNER_LG)
                    .inner_margin(egui::Margin::same(m(SP_4))),
            )
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                ui.label(
                    RichText::new("Dynamic")
                        .size(TS_XL)
                        .strong()
                        .color(t().text_primary),
                );
                ui.add_space(SP_1);
                ui.label(
                    RichText::new(
                        "Pick deck-session or finish-it, set parameters, then generate into a stable store slot.",
                    )
                    .size(TS_SM)
                    .color(t().text_secondary),
                );
                ui.add_space(SP_3);

                ui.label(
                    RichText::new("Template")
                        .size(TS_SM)
                        .strong()
                        .color(t().text_primary),
                );
                ui.add_space(SP_1);
                for template in [DynamicTemplate::DeckSession, DynamicTemplate::FinishIt] {
                    let selected = self.dynamic_template == template.id();
                    let btn = egui::Button::new(
                        RichText::new(template.label()).size(TS_SM).color(if selected {
                            t().text_inverse
                        } else {
                            t().text_secondary
                        }),
                    )
                    .fill(if selected { t().accent } else { t().surface })
                    .stroke(if selected {
                        egui::Stroke::NONE
                    } else {
                        egui::Stroke::new(1.0, t().border_soft)
                    })
                    .corner_radius(CORNER_PILL);
                    if ui.add(btn).clicked() {
                        self.dynamic_template = template.id().into();
                    }
                }

                ui.add_space(SP_3);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_2);
                    form_field(ui, "Session minutes", |ui| {
                        ui.add_sized(
                            [80.0, 28.0],
                            egui::TextEdit::singleline(&mut self.dynamic_minutes).hint_text("90"),
                        );
                    });
                    form_field(ui, "Count", |ui| {
                        ui.add_sized(
                            [64.0, 28.0],
                            egui::TextEdit::singleline(&mut self.dynamic_count).hint_text("25"),
                        );
                    });
                });
                ui.label(
                    RichText::new(
                        "Session minutes applies to Deck Session (HLTB cap). Count caps Finish It results.",
                    )
                    .size(TS_XS)
                    .color(t().text_muted),
                );

                ui.add_space(SP_4);
                ui.horizontal(|ui| {
                    if self.dynamic_loading {
                        ui.spinner();
                    }
                    if ui
                        .add_enabled(self.library_ready(), primary_button_widget("Generate"))
                        .clicked()
                    {
                        self.start_dynamic_generate(ui.ctx());
                        self.playlist_chooser = PlaylistChooser::None;
                    }
                    if ghost_button(ui, "Cancel").clicked() {
                        self.playlist_chooser = PlaylistChooser::None;
                    }
                });
                if let Some(err) = &self.error {
                    ui.add_space(SP_2);
                    ui.label(RichText::new(err).size(TS_SM).color(t().error));
                }
            });
        if !open {
            self.playlist_chooser = PlaylistChooser::None;
        }
    }

    fn render_mood_chooser(&mut self, ctx: &egui::Context) {
        let mut open = true;
        egui::Window::new("Editorial Mood")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .frame(
                egui::Frame::NONE
                    .fill(t().surface_raised)
                    .stroke(egui::Stroke::new(1.0, t().border))
                    .corner_radius(CORNER_LG)
                    .inner_margin(egui::Margin::same(m(SP_4))),
            )
            .show(ctx, |ui| {
                ui.set_min_width(380.0);
                ui.label(
                    RichText::new("Mood")
                        .size(TS_XL)
                        .strong()
                        .color(t().text_primary),
                );
                ui.add_space(SP_1);
                ui.label(
                    RichText::new(
                        "Seven canonical Editorial Moods with opaque criteria (ADR-0004). Generates into mood-<id>.",
                    )
                    .size(TS_SM)
                    .color(t().text_secondary),
                );
                ui.add_space(SP_3);

                egui::ScrollArea::vertical()
                    .max_height(280.0)
                    .show(ui, |ui| {
                        for mood in EditorialMood::all() {
                            let selected = self.editorial_mood == mood.id();
                            let fill = if selected {
                                t().accent_soft
                            } else {
                                t().surface_sunken
                            };
                            let stroke = if selected {
                                egui::Stroke::new(1.0, t().accent)
                            } else {
                                egui::Stroke::new(1.0, t().border_soft)
                            };
                            let response = egui::Frame::NONE
                                .fill(fill)
                                .stroke(stroke)
                                .inner_margin(egui::Margin::same(m(SP_3)))
                                .corner_radius(CORNER_MD)
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(
                                        RichText::new(mood.name())
                                            .size(TS_MD)
                                            .strong()
                                            .color(t().text_primary),
                                    );
                                    ui.label(
                                        RichText::new(mood.description())
                                            .size(TS_SM)
                                            .color(t().text_secondary),
                                    );
                                })
                                .response
                                .interact(egui::Sense::click());
                            if response.clicked() {
                                self.editorial_mood = mood.id().into();
                            }
                            ui.add_space(SP_2);
                        }
                    });

                ui.add_space(SP_3);
                ui.horizontal(|ui| {
                    if self.mood_loading {
                        ui.spinner();
                    }
                    if ui
                        .add_enabled(self.library_ready(), primary_button_widget("Generate"))
                        .clicked()
                    {
                        self.start_mood_generate(ui.ctx());
                        self.playlist_chooser = PlaylistChooser::None;
                    }
                    if ghost_button(ui, "Cancel").clicked() {
                        self.playlist_chooser = PlaylistChooser::None;
                    }
                });
                if let Some(err) = &self.error {
                    ui.add_space(SP_2);
                    ui.label(RichText::new(err).size(TS_SM).color(t().error));
                }
            });
        if !open {
            self.playlist_chooser = PlaylistChooser::None;
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
                    render_collection_card(ui, coll, self.ui_demo || self.offline_mode);
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

    // -- Data Sources view (ticket 08) --------------------------------------

    fn render_data_sources(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let refresh_all_enabled = !self.cache_refresh_loading && !self.offline_mode;
        let mut refresh_all = false;
        view_header_with_actions(
            ui,
            "Data Sources",
            "Credential, cache, and refresh status for enrichment APIs. Offline uses cache-only hydration (ADR-0002).",
            |ui| {
                if ui
                    .add_enabled(refresh_all_enabled, {
                        egui::Button::new(RichText::new("Refresh All").size(TS_SM).color(
                            if refresh_all_enabled {
                                t().text_inverse
                            } else {
                                t().text_muted
                            },
                        ))
                        .fill(if refresh_all_enabled {
                            t().accent
                        } else {
                            t().surface_muted
                        })
                        .stroke(egui::Stroke::NONE)
                        .corner_radius(CORNER_SM)
                    })
                    .clicked()
                {
                    refresh_all = true;
                }
            },
        );
        if refresh_all {
            self.start_cache_refresh(None, ctx);
        }

        // Offline control — primary home for cache-only mode (issue 08).
        section_card(ui, "Offline mode", |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.offline_mode, "Offline mode (cache only)");
                if self.offline_mode {
                    status_badge(ui, "Cache only", t().warning_soft, t().warning);
                } else {
                    status_badge(ui, "Network allowed", t().success_soft, t().success);
                }
            });
            ui.add_space(SP_1);
            ui.label(
                RichText::new(if self.offline_mode {
                    "Cache refresh is disabled while offline mode is on. Library workflows hydrate from cache only."
                } else {
                    "When offline is enabled, network refresh and workflow hydration skip the network and use cache only."
                })
                .size(TS_SM)
                .color(t().text_muted),
            );
        });

        // Unified source table: credential + entries + stale + last success + refresh.
        let has_igdb = self.has_igdb;
        let has_rawg = self.has_rawg;
        let offline = self.offline_mode;
        let loading = self.cache_refresh_loading;
        let mut refresh_source: Option<String> = None;

        section_card(ui, "Sources", |ui| {
            if loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("Refreshing cache…")
                            .size(TS_SM)
                            .color(t().text_secondary),
                    );
                });
                ui.add_space(SP_2);
            }
            if let Some(msg) = &self.cache_refresh_msg {
                ui.label(RichText::new(msg).size(TS_SM).color(t().text_secondary));
                ui.add_space(SP_2);
            }

            if self.source_statuses.is_empty() {
                empty_state(
                    ui,
                    "\u{1F4E1}",
                    "No source status yet",
                    "Statuses load from the local cache root. Scan and refresh to populate entries.",
                );
            } else {
                let text_height = TS_BODY;
                egui_extras::TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(egui_extras::Column::auto().at_least(100.0))
                    .column(egui_extras::Column::auto().at_least(100.0))
                    .column(egui_extras::Column::auto().at_least(70.0))
                    .column(egui_extras::Column::auto().at_least(60.0))
                    .column(egui_extras::Column::auto().at_least(120.0))
                    .column(egui_extras::Column::auto().at_least(80.0))
                    .header(text_height * 1.5, |mut header| {
                        for title in [
                            "Source",
                            "Credential",
                            "Entries",
                            "Stale",
                            "Last success",
                            "Action",
                        ] {
                            header.col(|ui| {
                                ui.label(
                                    RichText::new(title)
                                        .size(TS_SM)
                                        .strong()
                                        .color(t().text_secondary),
                                );
                            });
                        }
                    })
                    .body(|mut body| {
                        for status in &self.source_statuses {
                            let source_id = status.name.as_str();
                            let display = source_display_name(source_id);
                            let signal = source_credential_signal(source_id, has_igdb, has_rawg);
                            let can_refresh = source_refresh_enabled(
                                source_id, has_igdb, has_rawg, offline, loading,
                            );
                            let last = status.last_success.map_or_else(
                                || "—".into(),
                                |dt| dt.format("%Y-%m-%d %H:%M").to_string(),
                            );
                            let entries = status.cache_entries;
                            let stale = status.stale_entries;
                            let source_id_owned = status.name.clone();

                            body.row(text_height * 1.6, |mut row| {
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(display)
                                            .size(TS_BODY)
                                            .strong()
                                            .color(t().text_primary),
                                    );
                                });
                                row.col(|ui| {
                                    credential_badge(ui, signal);
                                });
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(entries.to_string())
                                            .size(TS_BODY)
                                            .color(t().text_primary),
                                    );
                                });
                                row.col(|ui| {
                                    let color = if stale > 0 {
                                        t().warning
                                    } else {
                                        t().text_primary
                                    };
                                    ui.label(
                                        RichText::new(stale.to_string()).size(TS_BODY).color(color),
                                    );
                                });
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(last)
                                            .size(TS_SM)
                                            .color(t().text_secondary)
                                            .monospace(),
                                    );
                                });
                                row.col(|ui| {
                                    if ui
                                        .add_enabled(
                                            can_refresh,
                                            egui::Button::new(
                                                RichText::new("Refresh").size(TS_SM).color(
                                                    if can_refresh {
                                                        t().text_primary
                                                    } else {
                                                        t().text_muted
                                                    },
                                                ),
                                            )
                                            .fill(t().surface)
                                            .stroke(egui::Stroke::new(1.0, t().border_soft))
                                            .corner_radius(CORNER_SM),
                                        )
                                        .clicked()
                                    {
                                        refresh_source = Some(source_id_owned.clone());
                                    }
                                });
                            });
                        }
                    });
            }

            ui.add_space(SP_2);
            ui.label(
                RichText::new(
                    "Credentials: VAPOURFLY_IGDB_CLIENT_ID + VAPOURFLY_IGDB_CLIENT_SECRET, VAPOURFLY_RAWG_KEY. Set env vars before launch.",
                )
                .size(TS_XS)
                .color(t().text_muted),
            );
        });

        if let Some(source) = refresh_source {
            self.start_cache_refresh(Some(source), ctx);
        }

        // Cache health summary rail.
        ui.add_space(SP_3);
        let total_entries: usize = self.source_statuses.iter().map(|s| s.cache_entries).sum();
        let total_stale: usize = self.source_statuses.iter().map(|s| s.stale_entries).sum();
        let healthy_sources = self
            .source_statuses
            .iter()
            .filter(|s| s.stale_entries == 0)
            .count();
        let total_sources = self.source_statuses.len();
        let health_pct = if total_sources > 0 {
            (healthy_sources as f64 / total_sources as f64 * 100.0) as u32
        } else {
            0
        };

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                section_card(ui, "Cache Health", |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SP_3, SP_2);
                        // Health gauge.
                        let health_color = if health_pct >= 80 {
                            t().success
                        } else if health_pct >= 50 {
                            t().warning
                        } else {
                            t().error
                        };
                        ui.label(
                            RichText::new(format!("{health_pct}%"))
                                .size(TS_2XL)
                                .strong()
                                .color(health_color),
                        );
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            ui.label(
                                RichText::new("cache health")
                                    .size(TS_SM)
                                    .color(t().text_secondary),
                            );
                            ui.label(
                                RichText::new(format!(
                                    "{healthy_sources}/{total_sources} sources fresh"
                                ))
                                .size(TS_XS)
                                .color(t().text_muted),
                            );
                        });
                        ui.separator();
                        insight_metric(ui, "Entries", total_entries.to_string());
                        insight_metric(ui, "Stale", total_stale.to_string());
                        insight_metric(ui, "Sources", total_sources.to_string());
                    });
                });
            });
        });
    }

    // -- Backups section (embedded under Settings; ADR-0006 / ticket 09) ----

    fn render_backups_section(&mut self, ui: &mut egui::Ui) {
        let mut refresh = false;
        let mut restore_path: Option<PathBuf> = None;

        section_card(ui, "Backups", |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(
                        "Safety backups of Steam cloud storage. Restore is confirmation-gated.",
                    )
                    .size(TS_SM)
                    .color(t().text_secondary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if secondary_button(ui, "Refresh Backups").clicked() {
                        refresh = true;
                    }
                });
            });
            ui.add_space(SP_2);

            if self.backups.is_empty() {
                ui.label(
                    RichText::new(
                        "No backups found. Click Refresh Backups after a write creates one.",
                    )
                    .size(TS_BODY)
                    .color(t().text_muted),
                );
            } else {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                    metric_pill(ui, "Backups", self.backups.len().to_string());
                });
                ui.add_space(SP_2);

                let text_height = TS_BODY;
                let write_loading = self.write_loading;
                egui_extras::TableBuilder::new(ui)
                    .striped(true)
                    .resizable(true)
                    .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
                    .column(egui_extras::Column::remainder().at_least(220.0))
                    .column(egui_extras::Column::auto().at_least(140.0))
                    .column(egui_extras::Column::auto().at_least(90.0))
                    .column(egui_extras::Column::auto().at_least(80.0))
                    .header(text_height * 1.5, |mut header| {
                        for title in ["Filename", "Created", "SHA256", "Action"] {
                            header.col(|ui| {
                                ui.label(
                                    RichText::new(title)
                                        .size(TS_SM)
                                        .strong()
                                        .color(t().text_secondary),
                                );
                            });
                        }
                    })
                    .body(|mut body| {
                        for backup in &self.backups {
                            let filename = backup
                                .path
                                .file_name()
                                .map(|n| n.to_string_lossy().to_string())
                                .unwrap_or_default();
                            let created = backup.created_at.format("%Y-%m-%d %H:%M:%S").to_string();
                            let sha = backup.sha256[..8.min(backup.sha256.len())].to_string();
                            let path = backup.path.clone();

                            body.row(text_height * 1.5, |mut row| {
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(&filename)
                                            .size(TS_BODY)
                                            .color(t().text_primary),
                                    );
                                });
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(created)
                                            .size(TS_SM)
                                            .color(t().text_secondary)
                                            .monospace(),
                                    );
                                });
                                row.col(|ui| {
                                    ui.label(
                                        RichText::new(sha)
                                            .size(TS_SM)
                                            .color(t().text_muted)
                                            .monospace(),
                                    );
                                });
                                row.col(|ui| {
                                    ui.add_enabled_ui(!write_loading, |ui| {
                                        if secondary_button(ui, "Restore").clicked() {
                                            restore_path = Some(path.clone());
                                        }
                                    });
                                });
                            });
                        }
                    });
            }

            if self.write_loading {
                ui.add_space(SP_2);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(
                        RichText::new("Restoring…")
                            .size(TS_SM)
                            .color(t().text_secondary),
                    );
                });
            }
        });

        if refresh {
            // Demo mode: do not scan the real Steam cloud storage for backups.
            if self.ui_demo {
                self.account_list_msg =
                    Some("Backup refresh is disabled in demo mode (--ui-demo).".into());
            } else {
                self.backups.clear();
                match self.cloud_storage_path() {
                    Ok(cloud_path) => {
                        if cloud_path.exists() {
                            match list_backups(&cloud_path) {
                                Ok(backups) => self.backups = backups,
                                Err(e) => self.error = Some(format!("Failed to list backups: {e}")),
                            }
                        }
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }
        if let Some(path) = restore_path {
            if self.ui_demo {
                self.error = Some("Backup restore is disabled in demo mode (--ui-demo).".into());
                return;
            }
            // Clear any leftover dry-run plan so Confirm runs restore, not an
            // earlier junk/playlist write (ticket 09 write-safety).
            self.dry_run_plan = None;
            self.dry_run_error = None;
            self.pending_action = Some(PendingAction::BackupRestore(path));
            self.show_confirm_dialog = true;
        }
    }

    // -- Settings view (ticket 09) ------------------------------------------

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        view_header(
            ui,
            "Settings",
            "Steam install, accounts, locale, write safety, diagnostics, and backups.",
        );

        // Two-column layout: settings cards (left) + summary rail (right).
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                // Appearance: theme toggle.
                section_card(ui, "Appearance", |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SP_2, SP_2);
                        ui.label(
                            RichText::new("Theme")
                                .size(TS_SM)
                                .color(t().text_secondary),
                        );
                        for mode in [ThemeMode::Light, ThemeMode::Dark] {
                            let is_active = self.theme_mode == mode;
                            let btn = egui::Button::new(
                                RichText::new(mode.label()).size(TS_SM).color(if is_active {
                                    t().text_inverse
                                } else {
                                    t().text_secondary
                                }),
                            )
                            .fill(if is_active { t().accent } else { t().surface })
                            .stroke(egui::Stroke::new(
                                1.0,
                                if is_active { t().accent } else { t().border },
                            ))
                            .corner_radius(CORNER_PILL);
                            if ui.add(btn).clicked() {
                                self.theme_mode = mode;
                            }
                        }
                        ui.label(
                            RichText::new("Persisted across launches via local storage.")
                                .size(TS_XS)
                                .color(t().text_muted),
                        );
                    });
                });

                // Configuration group: directory, account, locale, retention.
                section_card(ui, "Configuration", |ui| {
                    labeled_field(
                        ui,
                        "Steam directory",
                        Some("Leave empty for auto-detection."),
                        |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.steam_dir_edit)
                                    .desired_width(f32::INFINITY)
                                    .hint_text("/path/to/Steam"),
                            );
                        },
                    );
                    ui.add_space(SP_3);
                    labeled_field(
                        ui,
                        "Account override",
                        Some("Leave empty for auto-selection (most recent account)."),
                        |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.account_edit)
                                    .desired_width(280.0)
                                    .hint_text("account name"),
                            );
                        },
                    );
                    ui.add_space(SP_3);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(SP_4, SP_2);
                        labeled_field(ui, "Store country (cc)", None, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.cc_edit)
                                    .desired_width(72.0)
                                    .hint_text("us"),
                            );
                        });
                        labeled_field(ui, "Store language", None, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.lang_edit)
                                    .desired_width(140.0)
                                    .hint_text("english"),
                            );
                        });
                        labeled_field(
                            ui,
                            "Backup retention",
                            Some("Rolling backups kept per file."),
                            |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.backup_retention_edit)
                                        .desired_width(64.0)
                                        .hint_text("5"),
                                );
                            },
                        );
                    });
                    ui.add_space(SP_3);
                    ui.horizontal(|ui| {
                        if primary_button(ui, "Save Settings").clicked() {
                            self.save_settings();
                        }
                        if let Some(msg) = &self.settings_save_msg {
                            ui.label(RichText::new(msg).size(TS_SM).color(t().success));
                        }
                    });
                });

                // Detected accounts with one-click override.
                section_card(ui, "Detected accounts", |ui| {
                    ui.horizontal(|ui| {
                        if secondary_button(ui, "Refresh Accounts").clicked() {
                            self.refresh_detected_accounts();
                        }
                        if let Some(msg) = &self.account_list_msg {
                            ui.label(RichText::new(msg).size(TS_SM).color(t().text_secondary));
                        }
                    });
                    ui.add_space(SP_2);

                    if self.detected_accounts.is_empty() {
                        ui.label(
                            RichText::new("No accounts loaded. Set Steam directory and refresh.")
                                .size(TS_BODY)
                                .color(t().text_muted),
                        );
                    } else {
                        let mut selected_account = None;
                        egui::Grid::new("detected_accounts_grid")
                            .num_columns(5)
                            .spacing([SP_3, SP_2])
                            .striped(true)
                            .show(ui, |ui| {
                                for title in ["Persona", "Account", "Steam ID", "Most recent", "Action"] {
                                    ui.label(
                                        RichText::new(title)
                                            .size(TS_SM)
                                            .strong()
                                            .color(t().text_secondary),
                                    );
                                }
                                ui.end_row();

                                for account in &self.detected_accounts {
                                    ui.label(
                                        RichText::new(&account.persona_name)
                                            .size(TS_BODY)
                                            .color(t().text_primary),
                                    );
                                    ui.label(
                                        RichText::new(&account.account_name)
                                            .size(TS_BODY)
                                            .color(t().text_primary),
                                    );
                                    ui.label(
                                        RichText::new(mask_steam_id(&account.steam_id64))
                                            .size(TS_SM)
                                            .color(t().text_muted)
                                            .monospace(),
                                    );
                                    if account.most_recent {
                                        status_badge(ui, "yes", t().success_soft, t().success);
                                    } else {
                                        status_badge(ui, "no", t().surface_sunken, t().text_muted);
                                    }
                                    if secondary_button(ui, "Use").clicked() {
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

                section_card(ui, "Write safety", |ui| {
                    ui.checkbox(
                        &mut self.allow_steam_running,
                        "Allow writes while Steam is running",
                    );
                    ui.add_space(SP_1);
                    ui.label(
                        RichText::new(
                            "Enable with caution. Steam may overwrite cloud-storage changes (ADR-0001).",
                        )
                        .size(TS_SM)
                        .color(t().warning),
                    );
                });

                section_card(ui, "Setup diagnostics", |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(
                                "Steam paths, accounts, libraries, cloud storage, cache, and credentials.",
                            )
                            .size(TS_SM)
                            .color(t().text_secondary),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if secondary_button(ui, "Run Setup Check").clicked() {
                                self.run_setup_diagnostics();
                            }
                        });
                    });
                    if let Some(report) = &self.setup_diagnostics {
                        ui.add_space(SP_2);
                        egui::Frame::NONE
                            .fill(t().surface_sunken)
                            .inner_margin(egui::Margin::same(m(SP_3)))
                            .corner_radius(CORNER_SM)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(report)
                                        .size(TS_XS)
                                        .color(t().text_primary)
                                        .monospace(),
                                );
                            });
                    }
                });

                section_card(ui, "Diagnostics export", |ui| {
                    ui.label(
                        RichText::new("Export sanitized support data for bug reports.")
                            .size(TS_SM)
                            .color(t().text_secondary),
                    );
                    ui.add_space(SP_2);
                    labeled_field(ui, "Export path", None, |ui| {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.diagnostics_export_path)
                                    .desired_width(320.0)
                                    .hint_text("diagnostics.json"),
                            );
                            if secondary_button(ui, "Export Diagnostics").clicked() {
                                match self.export_diagnostics() {
                                    Ok(()) => {
                                        self.success_msg = Some(format!(
                                            "Diagnostics exported to {}",
                                            self.diagnostics_export_path.trim()
                                        ));
                                    }
                                    Err(e) => {
                                        self.error = Some(format!("Diagnostics export failed: {e}"));
                                    }
                                }
                            }
                        });
                    });
                });

                // Backups / restore live under Settings (not a top-level nav item).
                self.render_backups_section(ui);

                section_card(ui, "About", |ui| {
                    stat_inline(ui, "Version:", &format!("v{}", env!("CARGO_PKG_VERSION")));
                    ui.label(
                        RichText::new("A local-first CLI/GUI tool for managing Steam game libraries.")
                            .size(TS_BODY)
                            .color(t().text_secondary),
                    );
                    ui.label(
                        RichText::new("Licensed under MIT OR Apache-2.0.")
                            .size(TS_SM)
                            .color(t().text_muted),
                    );
                });
            });

            // Summary rail.
            ui.add_space(SP_4);
            ui.allocate_ui_with_layout(
                egui::vec2(200.0, ui.available_height()),
                egui::Layout::top_down(egui::Align::LEFT),
                |ui| {
                    egui::Frame::group(ui.style())
                        .fill(t().surface)
                        .stroke(egui::Stroke::new(1.0, t().border_soft))
                        .corner_radius(CORNER_MD)
                        .inner_margin(egui::Margin::same(m(SP_3)))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Summary")
                                    .size(TS_MD)
                                    .strong()
                                    .color(t().text_primary),
                            );
                            ui.add_space(SP_2);
                            ui.separator();
                            ui.add_space(SP_2);
                            ui.spacing_mut().item_spacing.y = SP_3;
                            insight_metric(
                                ui,
                                "Theme",
                                self.theme_mode.label().to_string(),
                            );
                            insight_metric(
                                ui,
                                "Accounts",
                                self.detected_accounts.len().to_string(),
                            );
                            insight_metric(
                                ui,
                                "Steam dir",
                                if self.steam_dir_edit.trim().is_empty() {
                                    "auto".to_string()
                                } else {
                                    "set".to_string()
                                },
                            );
                            insight_metric(
                                ui,
                                "Retention",
                                if self.backup_retention_edit.trim().is_empty() {
                                    "default".to_string()
                                } else {
                                    self.backup_retention_edit.trim().to_string()
                                },
                            );
                            insight_metric(
                                ui,
                                "Write safety",
                                if self.allow_steam_running {
                                    "relaxed".to_string()
                                } else {
                                    "strict".to_string()
                                },
                            );
                            ui.separator();
                            insight_metric(
                                ui,
                                "Version",
                                format!("v{}", env!("CARGO_PKG_VERSION")),
                            );
                        });
                },
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

        // Demo mode: never write the real config.toml.
        if self.ui_demo {
            self.settings_save_msg =
                Some("Saving settings is disabled in demo mode (--ui-demo).".into());
            return;
        }

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

fn game_primary_badge(game: &Game) -> (&'static str, Color32, Color32) {
    if game.is_junk {
        ("Junk", t().error_soft, t().error)
    } else if game.is_hidden {
        ("Hidden", t().surface_muted, t().text_secondary)
    } else if game.installed {
        ("Installed", t().success_soft, t().success)
    } else {
        ("Library", t().accent_soft, t().accent)
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
fn render_collection_card(ui: &mut egui::Ui, coll: &SteamCollection, demo_or_offline: bool) {
    ui.allocate_ui_with_layout(
        egui::vec2(COLLECTION_CARD_W, COLLECTION_CARD_H),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            ui.set_width(COLLECTION_CARD_W);
            ui.set_height(COLLECTION_CARD_H);

            egui::Frame::NONE
                .fill(t().surface_raised)
                .stroke(egui::Stroke::new(1.0, t().border_soft))
                .inner_margin(egui::Margin::same(m(SP_3)))
                .corner_radius(CORNER_MD)
                .show(ui, |ui| {
                    ui.set_width(COLLECTION_CARD_W - f32::from(m(SP_3)) * 2.0);

                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&coll.name)
                                .size(TS_MD)
                                .strong()
                                .color(t().text_primary),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if coll.is_hidden_collection {
                                status_badge(ui, "Hidden", t().surface_muted, t().text_secondary);
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
                                .fill(t().surface_sunken)
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
                                                .color(t().text_muted),
                                        );
                                    });
                                });
                        } else {
                            for app_id in shown {
                                game_artwork(
                                    ui,
                                    app_id,
                                    "",
                                    COLLAGE_POSTER_W,
                                    COLLAGE_POSTER_H,
                                    demo_or_offline,
                                    &steam_poster_uri(app_id),
                                );
                            }
                            if coll.app_ids.len() > COLLAGE_MAX {
                                ui.label(
                                    RichText::new(format!("+{}", coll.app_ids.len() - COLLAGE_MAX))
                                        .size(TS_SM)
                                        .color(t().text_muted),
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

/// Steam's universally available header capsule has the landscape ratio used
/// by the primary Library cards. Poster art remains in use for collection
/// collages where the tall composition is more useful.
fn steam_capsule_uri(app_id: u32) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{app_id}/header.jpg")
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
    let args: Vec<String> = std::env::args().collect();
    let fixtures_path = args
        .windows(2)
        .find(|w| w[0] == "--fixtures")
        .map(|w| PathBuf::from(&w[1]));

    let ui_demo = args.iter().any(|a| a == "--ui-demo");

    let app = VapourflyApp::new(fixtures_path, ui_demo);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 960.0])
            .with_min_inner_size([1024.0, 700.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Vapourfly",
        native_options,
        Box::new(|cc| {
            // Restore persisted theme from eframe storage (ADR-0006: theme
            // preference is a GUI-only concern, not domain config).
            let stored_theme = cc
                .storage
                .and_then(|s| s.get_string("vapourfly.theme"))
                .and_then(|v| v.parse::<u8>().ok())
                .map(ThemeMode::from_u8)
                .unwrap_or(ThemeMode::Light);
            configure_ui(&cc.egui_ctx, stored_theme);
            egui_extras::install_image_loaders(&cc.egui_ctx);
            let mut app = app;
            app.theme_mode = stored_theme;
            if ui_demo {
                app.populate_demo_data();
            }
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
        let app = VapourflyApp::new(None, false);
        assert!(app.scan_result.is_none());
        assert_eq!(app.current_view, View::Library);
        assert!(!app.loading);
        assert!(app.error.is_none());
    }

    #[test]
    fn ui_demo_flag_is_stored() {
        let app = VapourflyApp::new(None, true);
        assert!(app.ui_demo);
        let app = VapourflyApp::new(None, false);
        assert!(!app.ui_demo);
    }

    #[test]
    fn populate_demo_data_provides_all_pages() {
        let mut app = VapourflyApp::new(None, true);
        app.populate_demo_data();
        // Library: at least 24 games
        let scan = app.scan_result.expect("demo scan result");
        assert!(scan.games.len() >= 24);
        // Collections: at least 4
        assert!(app.collections.len() >= 4);
        // Junk decisions with mixed confidence
        assert!(!app.junk_results.is_empty());
        assert!(app.junk_results.iter().any(|d| d.confidence < 1.0));
        // Recommendations
        assert!(!app.recommend_results.is_empty());
        // Discover results
        assert!(!app.discover_results.is_empty());
        // Playlist store ids (at least 5 including generated slots)
        assert!(app.playlist_store_ids.len() >= 5);
        assert!(app.playlist_store_ids.contains(&"discover".to_string()));
        // Accounts
        assert!(!app.detected_accounts.is_empty());
        // Backups
        assert!(!app.backups.is_empty());
    }

    /// Demo playlists must be real, loadable files with the canonical schema.
    /// Regression for the "1.0" schema bug that silently failed to write.
    #[test]
    #[serial]
    fn populate_demo_data_writes_loadable_playlists() {
        let mut app = VapourflyApp::new(None, true);
        app.populate_demo_data();

        let store = app.playlist_store_path();
        let expected = [
            ("my-favorites", "My Favorites", &[1000u32, 1002, 1005][..]),
            ("story-games", "Story Games", &[1001, 1003, 1008]),
            ("discover", "Discover Picks", &[1004, 1006, 1010, 1012]),
            (
                "dynamic-deck-session",
                "Deck Session",
                &[1000, 1007, 1011, 1015],
            ),
            ("mood-quick-round", "Quick Round", &[1002, 1009, 1013]),
        ];

        for (id, name, app_ids) in expected {
            let pf = playlist_store::get(&store, id)
                .unwrap_or_else(|e| panic!("demo playlist {id} should be loadable: {e}"));
            assert_eq!(
                pf.vapourfly_schema, VAPOURFLY_PLAYLIST_SCHEMA,
                "demo playlist {id} has wrong schema"
            );
            assert_eq!(pf.playlist.id, id);
            assert_eq!(pf.playlist.name, name);
            match &pf.playlist.content {
                PlaylistContent::Manual { app_ids: got } => {
                    let mut want: Vec<u32> = app_ids.to_vec();
                    want.sort_unstable();
                    assert_eq!(got, &want, "demo playlist {id} app_ids mismatch");
                }
                other => panic!("demo playlist {id} expected Manual, got {other:?}"),
            }
        }
    }

    /// --ui-demo mode must isolate all I/O inside a unique temp root and never
    /// touch the real config, playlist, cache, or overrides paths.
    #[test]
    #[serial]
    fn ui_demo_isolates_io_from_real_user_paths() {
        let app = VapourflyApp::new(None, true);

        let demo_root = app
            .demo_root
            .clone()
            .expect("demo_root must be set in --ui-demo mode");
        // Unique per launch — must NOT be the old fixed path.
        let fixed = std::env::temp_dir().join("vapourfly-ui-demo");
        assert_ne!(
            demo_root, fixed,
            "demo root must be unique per launch, not the fixed shared path"
        );
        assert!(demo_root.starts_with(std::env::temp_dir()));

        // All demo paths live inside the demo root.
        let cache = &app.cache_dir;
        let store = app.playlist_store_path();
        let overrides = app
            .manual_overrides_path
            .clone()
            .expect("manual_overrides_path must be set in demo mode");
        assert!(
            cache.starts_with(&demo_root),
            "cache must be inside demo root"
        );
        assert!(
            store.starts_with(&demo_root),
            "playlist store must be inside demo root"
        );
        assert!(
            overrides.starts_with(&demo_root),
            "manual overrides must be inside demo root"
        );

        // Demo paths must differ from the real platform defaults.
        let real_cache = vapourfly_core::config::default_cache_dir();
        let real_playlists = vapourfly_core::config::default_playlists_dir();
        let real_overrides = vapourfly_core::config::default_manual_overrides_path();
        assert_ne!(cache, &real_cache);
        assert_ne!(store, real_playlists);
        assert_ne!(overrides, real_overrides);

        // Demo config must not carry real credentials.
        let cfg = app.config.as_ref().expect("demo config must be set");
        assert!(!cfg.has_igdb_credentials);
        assert!(!cfg.has_rawg_credentials);
        assert!(cfg.steam_dir.starts_with(&demo_root));
    }

    /// populate_demo_data writes only into the demo playlist store, never the
    /// real default playlists directory.
    #[test]
    #[serial]
    fn ui_demo_populate_does_not_write_real_playlist_dir() {
        let real_playlists = vapourfly_core::config::default_playlists_dir();
        // Snapshot any pre-existing ids in the real dir (we must not add to it).
        let before = vapourfly_core::playlist_store::list_ids(&real_playlists).unwrap_or_default();

        let mut app = VapourflyApp::new(None, true);
        app.populate_demo_data();

        let after = vapourfly_core::playlist_store::list_ids(&real_playlists).unwrap_or_default();
        assert_eq!(
            before, after,
            "demo populate must not create files in the real playlist dir"
        );
    }

    /// The background library prepare populates the snapshot, after which
    /// `prepared_games` uses the snapshot instead of re-hydrating from disk.
    #[test]
    #[serial]
    fn library_prepare_snapshot_populates_and_is_reused() {
        PREPARED_LIBRARY_RESULT.clear();
        let mut app = VapourflyApp::new(None, true);
        app.populate_demo_data();

        // Before prepare: snapshot is None, prepared_games returns None
        // (no on-frame fallback; callers show a loading state).
        assert!(app.prepared_snapshot.is_none());
        assert!(
            app.prepared_games(JunkMode::Default).is_none(),
            "prepared_games must return None before the snapshot is ready"
        );
        assert!(!app.library_ready());

        // Inject a snapshot synchronously (test helper) so we have a reference
        // game count to compare against the background-prepared snapshot.
        app.inject_prepared_snapshot();
        let games_injected = app
            .prepared_games(JunkMode::Default)
            .expect("injected snapshot should serve prepared_games");
        assert!(!games_injected.is_empty());
        // Drop the injected snapshot so the background prepare is the source.
        app.prepared_snapshot = None;

        // Start background prepare.
        let ctx = egui::Context::default();
        app.ensure_library_prepared(&ctx);
        assert!(app.prepare_job_id.is_some(), "prepare should be in flight");

        // Wait for the background thread to complete.
        let job_id = app.prepare_job_id.unwrap();
        let mut tries = 0;
        while app.prepare_job_id.is_some() && tries < 1000 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            app.poll_library_prepare();
            tries += 1;
        }
        assert!(app.prepare_job_id.is_none(), "prepare should complete");
        let _ = job_id;

        // Snapshot is now populated.
        let snap = app
            .prepared_snapshot
            .as_ref()
            .expect("snapshot should be populated after prepare");
        assert!(!snap.games.is_empty());

        // prepared_games now uses the snapshot (fast path).
        let games_snap = app
            .prepared_games(JunkMode::Default)
            .expect("snapshot path should work");
        assert_eq!(games_snap.len(), games_injected.len());
        assert!(app.library_ready());
    }

    /// Cache refresh generation bump invalidates the snapshot.
    #[test]
    #[serial]
    fn cache_refresh_generation_invalidates_snapshot() {
        PREPARED_LIBRARY_RESULT.clear();
        let mut app = VapourflyApp::new(None, true);
        app.populate_demo_data();

        // Manually set a snapshot.
        let fp = app.library_prepare_fingerprint();
        app.prepared_snapshot = Some(PreparedLibrarySnapshot {
            fingerprint: fp,
            games: vec![],
            overrides: ManualOverrides::default(),
        });

        // Bump the generation — fingerprint changes, snapshot is stale.
        app.cache_refresh_generation = app.cache_refresh_generation.wrapping_add(1);
        let new_fp = app.library_prepare_fingerprint();
        assert_ne!(
            fp, new_fp,
            "fingerprint must change when cache_refresh_generation bumps"
        );
        assert!(
            app.prepared_snapshot
                .as_ref()
                .is_some_and(|s| s.fingerprint != new_fp),
            "snapshot must be stale after generation bump"
        );
    }

    /// Regression: a rescan that produces the same game count but changes
    /// content (playtime, hidden state, collections) must invalidate the cached
    /// snapshot. Previously the fingerprint used `scan_job_id` (None after
    /// completion) + game count, so an unchanged count left the snapshot live.
    /// `scan_generation` bumps on every accepted scan result, so the
    /// fingerprint changes and `prepared_games` rejects the stale snapshot.
    #[test]
    #[serial]
    fn rescan_with_same_game_count_invalidates_snapshot() {
        PREPARED_LIBRARY_RESULT.clear();
        let mut app = VapourflyApp::new(None, true);
        app.populate_demo_data();
        let game_count = app.scan_result.as_ref().unwrap().games.len();

        // Build a snapshot holding a sentinel game that is NOT in the real
        // library, tagged with the current fingerprint.
        let fp = app.library_prepare_fingerprint();
        let mut sentinel = app
            .scan_result
            .as_ref()
            .unwrap()
            .games
            .first()
            .unwrap()
            .clone();
        sentinel.app_id = 999_999;
        sentinel.name = "STALE SENTINEL".into();
        app.prepared_snapshot = Some(PreparedLibrarySnapshot {
            fingerprint: fp,
            games: vec![sentinel.clone()],
            overrides: ManualOverrides::default(),
        });

        // Simulate a rescan accepted with the SAME game count but changed
        // content (scan_generation bumps on acceptance).
        app.scan_generation = app.scan_generation.wrapping_add(1);
        assert_eq!(
            app.scan_result.as_ref().unwrap().games.len(),
            game_count,
            "test precondition: game count unchanged"
        );

        let new_fp = app.library_prepare_fingerprint();
        assert_ne!(
            fp, new_fp,
            "fingerprint must change when scan_generation bumps even with same game count"
        );
        assert!(
            app.prepared_snapshot
                .as_ref()
                .is_some_and(|s| s.fingerprint != new_fp),
            "snapshot must be stale after scan_generation bump"
        );

        // prepared_games must NOT serve the stale snapshot: with no on-frame
        // fallback it returns None while the snapshot is stale (library not
        // ready).
        assert!(
            app.prepared_games(JunkMode::Default).is_none(),
            "prepared_games must return None while the snapshot is stale (no fallback)"
        );
        assert!(!app.library_ready());

        // After re-preparing (injected here as a synchronous stand-in for the
        // background prepare), the fresh snapshot is built from the real scan
        // result and must NOT contain the stale sentinel.
        app.inject_prepared_snapshot();
        assert!(app.library_ready());
        let games = app
            .prepared_games(JunkMode::Default)
            .expect("fresh snapshot should serve prepared_games");
        assert!(
            !games.iter().any(|g| g.app_id == sentinel.app_id),
            "prepared_games must not use the stale snapshot (sentinel leaked)"
        );
    }

    #[test]
    fn theme_mode_round_trips_through_u8() {
        assert_eq!(
            ThemeMode::from_u8(ThemeMode::Light.as_u8()),
            ThemeMode::Light
        );
        assert_eq!(ThemeMode::from_u8(ThemeMode::Dark.as_u8()), ThemeMode::Dark);
        assert_eq!(ThemeMode::from_u8(99), ThemeMode::Light); // unknown → Light
    }

    #[test]
    fn app_created_with_fixtures_path() {
        let path = PathBuf::from("/tmp/fix");
        let app = VapourflyApp::new(Some(path.clone()), false);
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
        let app = VapourflyApp::new(None, false);
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
    fn artwork_palette_is_deterministic_by_app_id() {
        // Same app_id → same palette entry.
        assert_eq!(
            ARTWORK_PALETTE[(730u32 as usize) % ARTWORK_PALETTE.len()],
            ARTWORK_PALETTE[(730u32 as usize) % ARTWORK_PALETTE.len()]
        );
        // Different app_ids → different palette entries (for small ids).
        let a = ARTWORK_PALETTE[(1000u32 as usize) % ARTWORK_PALETTE.len()];
        let b = ARTWORK_PALETTE[(1001u32 as usize) % ARTWORK_PALETTE.len()];
        assert_ne!(a, b, "adjacent app_ids should get different palette entries");
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
    fn library_filters_advanced_genre_match() {
        let mut game = test_game(1, "A");
        game.igdb = Some(vapourfly_core::models::IgdbData {
            igdb_id: 1,
            name: "A".into(),
            slug: None,
            rating_0_100: None,
            total_rating_0_100: None,
            genres: vec!["Cozy".into(), "Puzzle".into()],
            themes: vec![],
            keywords: vec![],
            similar_game_ids: vec![],
            steam_app_id_confirmed: false,
            time_to_beat: None,
        });
        let filters = LibraryFilters {
            genre: "cozy".into(),
            ..Default::default()
        };
        assert!(game_matches_library_filters(&game, &filters));

        let filters_no_match = LibraryFilters {
            genre: "shooter".into(),
            ..Default::default()
        };
        assert!(!game_matches_library_filters(&game, &filters_no_match));
    }

    #[test]
    fn library_filters_unplayed_only_excludes_played() {
        let mut unplayed = test_game(1, "A");
        unplayed.playtime_minutes = Some(0);
        let mut played = test_game(2, "B");
        played.playtime_minutes = Some(120);

        let filters = LibraryFilters {
            unplayed_only: true,
            ..Default::default()
        };
        assert!(game_matches_library_filters(&unplayed, &filters));
        assert!(!game_matches_library_filters(&played, &filters));
    }

    #[test]
    fn library_filters_proton_tier_threshold() {
        let mut platinum = test_game(1, "A");
        platinum.protondb = Some(vapourfly_core::models::ProtonDbData {
            tier: ProtonTier::Platinum,
            confidence: None,
            score: None,
        });
        let mut borked = test_game(2, "B");
        borked.protondb = Some(vapourfly_core::models::ProtonDbData {
            tier: ProtonTier::Borked,
            confidence: None,
            score: None,
        });

        // Gold threshold: Platinum passes, Borked doesn't.
        let filters = LibraryFilters {
            proton_tier: Some(ProtonTier::Gold),
            ..Default::default()
        };
        assert!(game_matches_library_filters(&platinum, &filters));
        assert!(!game_matches_library_filters(&borked, &filters));
    }

    #[test]
    fn library_filters_short_sessions_prefers_hltb_main_story() {
        // Game with HLTB main_story_seconds but NO igdb.time_to_beat.
        // Short sessions must use hltb.main_story_seconds (the canonical,
        // normalized field), not only igdb.time_to_beat.normally_seconds.
        let mut short_hltb = test_game(100, "Short HLTB");
        short_hltb.hltb = Some(vapourfly_core::models::HltbData {
            main_story_seconds: Some(4800), // 80 min — fits a 120 min session
            main_extra_seconds: None,
            completionist_seconds: None,
            source: vapourfly_core::models::HltbSource::IgdbGameTimeToBeat,
        });
        // No igdb.time_to_beat — the old code would exclude this game.
        short_hltb.igdb = Some(vapourfly_core::models::IgdbData {
            igdb_id: 100,
            name: "Short HLTB".into(),
            slug: None,
            rating_0_100: None,
            total_rating_0_100: None,
            genres: vec![],
            themes: vec![],
            keywords: vec![],
            similar_game_ids: vec![],
            steam_app_id_confirmed: true,
            time_to_beat: None,
        });

        let mut long_hltb = test_game(101, "Long HLTB");
        long_hltb.hltb = Some(vapourfly_core::models::HltbData {
            main_story_seconds: Some(14_400), // 240 min — too long
            main_extra_seconds: None,
            completionist_seconds: None,
            source: vapourfly_core::models::HltbSource::IgdbGameTimeToBeat,
        });

        let filters = LibraryFilters {
            hltb_max_minutes: Some(120),
            ..Default::default()
        };
        assert!(
            game_matches_library_filters(&short_hltb, &filters),
            "short HLTB game (80 min) must match a 120 min session filter"
        );
        assert!(
            !game_matches_library_filters(&long_hltb, &filters),
            "long HLTB game (240 min) must not match a 120 min session filter"
        );
    }

    #[test]
    fn library_filters_short_sessions_igdb_time_to_beat_is_fallback() {
        // A game with only igdb.time_to_beat (no HLTB) still matches via fallback.
        let mut igdb_only = test_game(200, "IGDB only");
        igdb_only.igdb = Some(vapourfly_core::models::IgdbData {
            igdb_id: 200,
            name: "IGDB only".into(),
            slug: None,
            rating_0_100: None,
            total_rating_0_100: None,
            genres: vec![],
            themes: vec![],
            keywords: vec![],
            similar_game_ids: vec![],
            steam_app_id_confirmed: true,
            time_to_beat: Some(vapourfly_core::models::IgdbTimeToBeat {
                normally_seconds: Some(3600), // 60 min
                hastily_seconds: None,
                completely_seconds: None,
                submission_count: None,
            }),
        });

        let filters = LibraryFilters {
            hltb_max_minutes: Some(120),
            ..Default::default()
        };
        assert!(
            game_matches_library_filters(&igdb_only, &filters),
            "igdb.time_to_beat fallback must still work"
        );
    }

    #[test]
    fn library_filter_fields_match_three_toggle_contract() {
        // Guard against reintroducing Unplayed / include-only Hidden or Junk toggles.
        let app = VapourflyApp::new(None, false);
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
        let mut app = VapourflyApp::new(None, false);
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
        let app = VapourflyApp::new(None, false);
        // cc and lang should have defaults
        assert!(!app.cc_edit.is_empty());
        assert!(!app.lang_edit.is_empty());
        assert!(!app.backup_retention_edit.is_empty());
        assert!(!app.allow_steam_running);
        assert!(app.settings_save_msg.is_none());
    }

    #[test]
    fn recommend_request_uses_optional_seed_input() {
        let mut app = VapourflyApp::new(None, false);
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
        let mut app = VapourflyApp::new(None, false);
        app.recommend_minutes = "soon".into();

        let err = app.recommend_request_from_inputs().unwrap_err();

        assert!(err.contains("Available minutes"));
    }

    #[test]
    fn discover_options_use_count_and_seed_inputs() {
        let mut app = VapourflyApp::new(None, false);
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

    #[test]
    fn playlist_chooser_has_no_discover_variant() {
        // Ticket 06: Dynamic + Mood only. Discover is top-level (ticket 07).
        assert_eq!(PlaylistChooser::default(), PlaylistChooser::None);
        let _ = PlaylistChooser::Dynamic;
        let _ = PlaylistChooser::Mood;
        // Compile-time exhaustiveness: only None / Dynamic / Mood.
        match PlaylistChooser::None {
            PlaylistChooser::None | PlaylistChooser::Dynamic | PlaylistChooser::Mood => {}
        }
    }

    #[test]
    fn load_playlist_from_store_adopts_edit_fields() {
        let dir = TempDir::new().unwrap();
        let mut app = VapourflyApp::new(None, false);
        app.playlist_store_dir = Some(dir.path().to_path_buf());

        let pf = sample_generator_playlist("my-list", vec![730, 440]);
        playlist_store::put(dir.path(), &pf).unwrap();

        app.load_playlist_from_store("my-list").unwrap();
        assert_eq!(app.playlist_edit_id, "my-list");
        assert_eq!(app.playlist_edit_name, "Generated");
        // Store export sorts AppIDs ascending.
        assert_eq!(app.playlist_edit_app_ids, "440, 730");
        assert_eq!(app.playlist_load_selected, "my-list");
        assert!(app.playlist_last_import.is_some());
    }

    #[test]
    fn run_discover_generate_writes_slot_and_results() {
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal");
        let dir = TempDir::new().unwrap();
        let mut app = VapourflyApp::new(Some(fixtures), false);
        app.playlist_store_dir = Some(dir.path().to_path_buf());

        // Minimal scan so prepared_games has data (empty is fine for empty picks).
        app.scan_result = Some(ScanResult {
            steam_dir: "/tmp".into(),
            account: "test".into(),
            games: Vec::new(),
            warnings: Vec::new(),
        });
        // Inject a prepared snapshot so prepared_games serves the library
        // without the (removed) on-frame fallback.
        app.inject_prepared_snapshot();
        app.discover_count = "5".into();

        let pf = app.run_discover_generate().unwrap();
        assert_eq!(pf.playlist.id, "discover");
        assert!(dir.path().join("discover.json").is_file());
        assert!(app.discover_last_playlist.is_some());
        // Empty library → empty results, but still a written slot.
        assert!(app.discover_results.is_empty());
        assert_eq!(app.playlist_edit_id, "discover");
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
        assert_eq!(
            ids,
            vec!["discover"],
            "regenerate must not create a second id"
        );
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
            vec!["discover", "dynamic-deck-session", "mood-quick-round"]
        );
    }

    #[test]
    fn app_store_generator_playlist_uses_injected_store_dir() {
        let dir = TempDir::new().unwrap();
        let mut app = VapourflyApp::new(None, false);
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

    /// Regression: a Dynamic result started with `deck-session` must land in the
    /// `dynamic-deck-session` slot even if the user switches the chooser to
    /// `finish-it` while the job is running. The result carries the identity
    /// captured at start time; the poll must use it, not the current chooser.
    #[test]
    #[serial]
    fn dynamic_result_uses_start_time_identity_not_current_chooser() {
        DYNAMIC_RESULT.clear();
        let dir = TempDir::new().unwrap();
        let mut app = VapourflyApp::new(None, true);
        app.playlist_store_dir = Some(dir.path().to_path_buf());
        app.populate_demo_data();
        // Simulate a deck-session job that already produced a result.
        app.dynamic_template = DynamicTemplate::DeckSession.id().into();
        let job_id = app
            .job_runner
            .next_ticket(WorkflowKind::Dynamic, "dynamic:deck-session:90:25");
        app.dynamic_job_id = Some(job_id);
        app.dynamic_loading = true;
        let pf = sample_generator_playlist("dynamic-deck-session", vec![1000, 1007]);
        DYNAMIC_RESULT.set(
            job_id,
            Ok(GeneratorJobResult {
                identity: GeneratorIdentity::Dynamic(DynamicTemplate::DeckSession),
                playlist: pf,
            }),
        );
        // User switches chooser to finish-it AFTER the job started.
        app.dynamic_template = DynamicTemplate::FinishIt.id().into();

        app.poll_generator_results();

        // Result must be stored under the start-time identity's slot.
        assert!(
            dir.path().join("dynamic-deck-session.json").is_file(),
            "result must land in dynamic-deck-session, not dynamic-finish-it"
        );
        assert!(
            !dir.path().join("dynamic-finish-it.json").exists(),
            "drifted chooser must not redirect the result"
        );
        assert!(!app.dynamic_loading);
    }

    /// Regression: a Mood result must use the start-time mood identity even if
    /// the user changes the mood chooser mid-job.
    #[test]
    #[serial]
    fn mood_result_uses_start_time_identity_not_current_chooser() {
        MOOD_RESULT.clear();
        let dir = TempDir::new().unwrap();
        let mut app = VapourflyApp::new(None, true);
        app.playlist_store_dir = Some(dir.path().to_path_buf());
        app.populate_demo_data();
        app.editorial_mood = EditorialMood::QuickRound.id().into();
        let job_id = app
            .job_runner
            .next_ticket(WorkflowKind::Mood, "mood:quick-round");
        app.mood_job_id = Some(job_id);
        app.mood_loading = true;
        let pf = sample_generator_playlist("mood-quick-round", vec![1002, 1009]);
        MOOD_RESULT.set(
            job_id,
            Ok(GeneratorJobResult {
                identity: GeneratorIdentity::Mood(EditorialMood::QuickRound),
                playlist: pf,
            }),
        );
        // User switches mood mid-job.
        app.editorial_mood = EditorialMood::FridayParty.id().into();

        app.poll_generator_results();

        assert!(
            dir.path().join("mood-quick-round.json").is_file(),
            "result must land in mood-quick-round, not mood-friday-party"
        );
        assert!(
            !dir.path().join("mood-friday-party.json").exists(),
            "drifted chooser must not redirect the result"
        );
        assert!(!app.mood_loading);
    }

    /// A result whose ticket fingerprint does not match the expected one is
    /// discarded by `JobSlot::take_if`. This scenario is covered at the
    /// `jobs` module level (see `jobs::tests::take_if_discards_input_drift_result`)
    /// because with `JobTicket` the thread always uses the ticket captured at
    /// start time — a drifted fingerprint cannot be produced by normal job
    /// submission, only by direct slot injection (which the jobs unit test
    /// does). The integration-level test that previously injected a drifted
    /// result here has been removed as it tested an impossible production path.


    #[test]
    #[serial]
    fn backup_restore_confirm_clears_leftover_dry_run_plan() {
        // Ticket 09: Restore must not commit a stale junk/playlist dry-run.
        WRITE_RESULT.clear();

        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("cloud-storage-namespace-1.json");
        std::fs::write(&target_path, "[]").unwrap();
        let cloud = vapourfly_core::steam::read_cloud_storage(&target_path).unwrap();
        let stale_plan = vapourfly_core::steam::generate_write_plan(
            &cloud,
            vec![WriteOp::UpsertCollection {
                id: "junk".into(),
                added: vec![730],
                removed: vec![],
            }],
            target_path.clone(),
        )
        .unwrap();

        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal");
        let mut app = VapourflyApp::new(Some(fixtures), false);
        app.dry_run_plan = Some(stale_plan);
        app.pending_action = Some(PendingAction::BackupRestore(
            temp_dir.path().join("missing-backup.json"),
        ));
        app.allow_steam_running = true;

        app.execute_pending_action();

        // Plan must be dropped so restore takes the legacy path.
        assert!(app.dry_run_plan.is_none());

        let result = poll_write_result(&app);
        // Must be a restore failure, not a successful plan commit of junk.
        let err = result.expect_err("expected restore to fail for missing backup");
        assert!(
            err.contains("Restore failed") || err.contains("Safety check"),
            "unexpected error: {err}"
        );
        // Stale plan target must remain untouched.
        assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "[]");
    }

    #[test]
    fn cache_refresh_is_blocked_in_offline_mode() {
        let mut app = VapourflyApp::new(None, false);
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

    // -- Data Sources presentation helpers (ticket 08) ----------------------

    #[test]
    fn source_display_names_match_product_table() {
        assert_eq!(source_display_name("igdb"), "IGDB");
        assert_eq!(source_display_name("rawg"), "RAWG");
        assert_eq!(source_display_name("protondb"), "ProtonDB");
        assert_eq!(source_display_name("pcgw"), "PCGW");
        assert_eq!(source_display_name("hltb"), "HLTB");
        assert_eq!(source_display_name("steam-store"), "Steam Store");
    }

    #[test]
    fn source_credential_signals_cover_configured_missing_and_optional() {
        assert_eq!(
            source_credential_signal("igdb", true, false),
            CredentialSignal::Configured
        );
        assert_eq!(
            source_credential_signal("igdb", false, true),
            CredentialSignal::Missing
        );
        assert_eq!(
            source_credential_signal("rawg", false, true),
            CredentialSignal::Configured
        );
        assert_eq!(
            source_credential_signal("rawg", true, false),
            CredentialSignal::Missing
        );
        assert_eq!(
            source_credential_signal("protondb", false, false),
            CredentialSignal::NotRequired
        );
        assert_eq!(
            source_credential_signal("hltb", false, false),
            CredentialSignal::Optional
        );
        assert_eq!(
            source_credential_signal("steam-store", false, false),
            CredentialSignal::NotRequired
        );
    }

    #[test]
    fn source_refresh_enabled_respects_offline_loading_and_credentials() {
        assert!(!source_refresh_enabled("protondb", true, true, true, false));
        assert!(!source_refresh_enabled("protondb", true, true, false, true));
        assert!(!source_refresh_enabled("igdb", false, true, false, false));
        assert!(source_refresh_enabled("igdb", true, false, false, false));
        assert!(source_refresh_enabled("hltb", false, false, false, false));
        assert!(source_refresh_enabled("pcgw", false, false, false, false));
    }

    #[test]
    fn settings_can_refresh_detected_accounts_from_fixture() {
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal");
        let mut app = VapourflyApp::new(Some(fixtures), false);

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
        let mut app = VapourflyApp::new(None, false);
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
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_id = "bad".into();
        app.playlist_edit_name = "Bad".into();
        app.playlist_edit_rules = "not json".into();
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("Invalid Rules JSON"));
    }

    #[test]
    fn build_playlist_from_edit_fields_rejects_empty_rules_array() {
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_id = "empty".into();
        app.playlist_edit_name = "Empty".into();
        app.playlist_edit_rules = "[]".into();
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("at least one rule"));
    }

    #[test]
    fn build_playlist_from_edit_fields_requires_id_and_name() {
        let mut app = VapourflyApp::new(None, false);
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

        let mut app = VapourflyApp::new(None, false);
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
            &std::collections::HashSet::new(),
            "junk",
            &[],
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
    fn playlist_sync_resolves_rule_playlist_in_background_dry_run() {
        // Rule-based Playlist Sync is resolved off-frame inside the dry-run
        // job (generate_dry_run_plan), not in resolve_dry_run_action. The
        // rule playlist passes through unchanged and is matched against the
        // prepared library inside the background job.
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal");
        let scan_result = scan_library(&ScanOptions {
            steam_dir: fixtures.clone(),
            account: None,
            fixtures: Some(fixtures),
        })
        .unwrap();
        let games = scan_result.games.clone();
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

        let mut app = VapourflyApp::new(None, false);
        app.scan_result = Some(scan_result);
        app.inject_prepared_snapshot();
        // resolve_dry_run_action no longer resolves rules on-frame: the rule
        // playlist passes through unchanged.
        let action = app
            .resolve_dry_run_action(PendingAction::PlaylistSync(playlist.clone()))
            .unwrap();
        match &action {
            PendingAction::PlaylistSync(pf) => {
                assert!(
                    matches!(pf.playlist.content, PlaylistContent::Rules { .. }),
                    "resolve_dry_run_action must pass rule playlist through unchanged"
                );
            }
            other => panic!("unexpected action: {other:?}"),
        }

        // The background dry-run job resolves the rule playlist against the
        // prepared library games.
        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("cloud-storage-namespace-1.json");
        std::fs::write(&target_path, "[]").unwrap();
        let plan = generate_dry_run_plan(
            target_path,
            &PendingAction::PlaylistSync(playlist),
            &[],
            &std::collections::HashSet::new(),
            "junk",
            &[],
            &games,
        )
        .unwrap();
        // The installed rule matches AppIDs 730 and 427520 (deduped, ordered).
        assert_eq!(plan.diff.app_ids_added, vec![730, 427520]);
    }

    #[test]
    #[serial]
    fn cached_dry_run_plan_still_checks_write_safety() {
        vapourfly_core::steam::set_steam_running_override(Some(true));
        WRITE_RESULT.clear();

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

        let mut app = VapourflyApp::new(None, false);
        app.pending_action = Some(PendingAction::JunkApply);
        app.dry_run_plan = Some(plan);
        app.allow_steam_running = false;
        app.execute_pending_action();

        let result = poll_write_result(&app);
        assert!(result.unwrap_err().contains("Steam is currently running"));
        assert_eq!(std::fs::read_to_string(&target_path).unwrap(), "[]");

        vapourfly_core::steam::set_steam_running_override(None);
    }

    fn poll_write_result(app: &VapourflyApp) -> Result<String, String> {
        let expected = app
            .write_job_id
            .expect("write_job_id must be set before polling");
        for _ in 0..100 {
            if let Some(result) = WRITE_RESULT.take_if(expected) {
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
        let app = VapourflyApp::new(Some(fixtures), false);
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
        let mut app = VapourflyApp::new(Some(fixtures), false);
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
        let mut app = VapourflyApp::new(Some(fixtures), false);
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
        let mut app = VapourflyApp::new(None, false);
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
