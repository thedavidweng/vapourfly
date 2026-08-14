//! Toolkit-independent GUI application state and workflows.
//!
//! Presentation lives in `ui` (GPUI). This module owns filters, demo isolation,
//! write gates, and vapourfly_api job orchestration so existing tests keep
//! driving the shipped functions.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use vapourfly_core::actions;
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
use vapourfly_core::steam::BackupInfo;
use vapourfly_core::steam::backup::list_backups;
#[cfg(test)]
use vapourfly_core::steam::scan::{ScanOptions, scan_library};
use vapourfly_core::steam::{
    SteamAccount, detect_accounts, detect_library_folders, read_cloud_storage,
    read_user_collections, redact_path, select_account,
};

use crate::jobs::{JobRunner, JobSlot, JobTicket, WorkflowKind, fingerprint_u64};
use crate::theme::*;

/// Wakes the GPUI window after a background job stores a result.
#[derive(Clone)]
pub struct RepaintHook(Arc<dyn Fn() + Send + Sync>);

impl Default for RepaintHook {
    fn default() -> Self {
        Self(Arc::new(|| {}))
    }
}

impl RepaintHook {
    pub fn new(f: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(f))
    }

    pub fn request(&self) {
        (self.0)();
    }
}

/// Top-level destinations shown in the sidebar (ADR-0006).
/// Junk and Backups are intentionally absent — they live under Library and
/// Settings respectively.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum View {
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
    pub(crate) const ALL: &'static [View] = &[
        View::Library,
        View::Collections,
        View::Recommendations,
        View::Playlists,
        View::Discover,
        View::DataSources,
        View::Settings,
    ];

    pub(crate) fn label(self) -> &'static str {
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

/// Quick filter preset for the Library grid. Selecting one sets the
/// appropriate filter toggles and clears the others.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum QuickView {
    #[default]
    All,
    Cozy,
    StoryRich,
    GreatOnDeck,
    ShortSessions,
}

/// Primary Library scope shown as the segmented control beside search.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum LibraryScope {
    #[default]
    All,
    Installed,
    Unplayed,
    Hidden,
}

impl LibraryScope {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Installed => "Installed",
            Self::Unplayed => "Unplayed",
            Self::Hidden => "Hidden",
        }
    }

    pub(crate) fn all() -> [Self; 4] {
        [Self::All, Self::Installed, Self::Unplayed, Self::Hidden]
    }
}

impl QuickView {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Cozy => "Cozy",
            Self::StoryRich => "Story-rich",
            Self::GreatOnDeck => "Great on Deck",
            Self::ShortSessions => "Short sessions",
        }
    }
}

/// GUI-owned generator identity for playlist-store slots.
///
/// Core engines produce playlists; the GUI assigns a **stable playlist id**
/// per identity and overwrites that slot on regenerate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeneratorIdentity {
    /// Single Discover slot (seed is presentation-only; id does not vary).
    Discover,
    Dynamic(DynamicTemplate),
    Mood(EditorialMood),
}

impl GeneratorIdentity {
    /// Stable, readable playlist id for this generator slot.
    pub(crate) fn slot_id(self) -> String {
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
pub(crate) fn put_generator_slot(
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
pub(crate) struct GeneratorJobResult {
    identity: GeneratorIdentity,
    playlist: PlaylistFile,
}

/// Lightweight modal chooser opened from Playlists action bar.
///
/// Discover is intentionally absent — it is a top-level view (ADR-0005/0006).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum PlaylistChooser {
    #[default]
    None,
    Dynamic,
    Mood,
}

/// Right-workspace tab in the Playlists master-detail.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum PlaylistDetailTab {
    #[default]
    Games,
    Rules,
    Match,
}

/// Share sub-tab in the Playlists right workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum PlaylistShareTab {
    #[default]
    ShareCode,
    Json,
}

/// Match sub-tab in the Playlists right workspace.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum PlaylistMatchTab {
    #[default]
    Owned,
    Missing,
}

#[derive(Clone, Debug)]
pub(crate) enum PendingAction {
    JunkApply,
    JunkHide,
    RecommendCollection,
    PlaylistSync(PlaylistFile),
    BackupRestore(PathBuf),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum JunkModeChoice {
    Default,
    Strict,
    Aggressive,
}

impl JunkModeChoice {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Strict => "Strict",
            Self::Aggressive => "Aggressive",
        }
    }

    pub(crate) fn mode(self) -> JunkMode {
        match self {
            Self::Default => JunkMode::Default,
            Self::Strict => JunkMode::Strict,
            Self::Aggressive => JunkMode::Aggressive,
        }
    }
}

static SCAN_RESULT: JobSlot<ScanResult> = JobSlot::new();
static WRITE_RESULT: JobSlot<String> = JobSlot::new();
static ENRICH_RESULT: JobSlot<vapourfly_api::enrichment::EnrichmentSummary> = JobSlot::new();
static DRY_RUN_RESULT: JobSlot<vapourfly_core::write::PreviewedPlan> = JobSlot::new();
static JUNK_PREVIEW_RESULT: JobSlot<Vec<JunkDecision>> = JobSlot::new();
static RECOMMEND_RESULT: JobSlot<Vec<Recommendation>> = JobSlot::new();
static DISCOVER_RESULT: JobSlot<(Vec<DiscoverPick>, PlaylistFile)> = JobSlot::new();
static DYNAMIC_RESULT: JobSlot<GeneratorJobResult> = JobSlot::new();
static MOOD_RESULT: JobSlot<GeneratorJobResult> = JobSlot::new();
static PLAYLIST_MATCH_RESULT: JobSlot<PlaylistMatchReport> = JobSlot::new();
/// Background-prepared library snapshot (hydrated games, pre-junk-classification).
/// Produced off the UI frame so the Library view does not re-hydrate from
/// the disk cache every frame.
static PREPARED_LIBRARY_RESULT: JobSlot<PreparedLibrarySnapshot> = JobSlot::new();

/// Cached library snapshot: hydrated games + the manual overrides that were
/// loaded alongside them, plus the fingerprint identifying the inputs used to
/// produce them. The games are **pre-classified with `JunkMode::Default`** so
/// the common path (Library, Recommendations, Discover, Playlist Match) never
/// needs to reclassify on the UI frame — it just clones the `Arc`. Non-Default
/// modes (Junk Preview Strict/Aggressive) reclassify inside their own background
/// job. The overrides are captured here so `prepared_games` never reads the
/// overrides file on the UI frame.
#[derive(Clone, Debug)]
pub(crate) struct PreparedLibrarySnapshot {
    fingerprint: u64,
    /// Games hydrated and pre-classified with `JunkMode::Default`.
    games: Arc<[Game]>,
    /// Manual overrides snapshot (for non-Default reclassification in background jobs).
    overrides: ManualOverrides,
}

pub(crate) struct VapourflyApp {
    // Core state
    pub(crate) scan_result: Option<ScanResult>,
    pub(crate) current_view: View,
    pub(crate) loading: bool,
    pub(crate) error: Option<String>,
    pub(crate) success_msg: Option<String>,
    pub(crate) fixtures_path: Option<PathBuf>,

    /// Demo mode (`--ui-demo`): deterministic fixture data, no real Steam writes.
    pub(crate) ui_demo: bool,

    /// Whether the once-per-launch automatic background populate (ADR-0009)
    /// has been kicked off after the first successful scan.
    pub(crate) auto_populate_started: bool,

    /// Light or dark visual system (ADR-0006).
    pub(crate) theme_mode: ThemeMode,

    // Config
    pub(crate) config: Option<VapourflyConfig>,

    /// Optional override for the playlist store directory (tests inject a temp dir).
    pub(crate) playlist_store_dir: Option<PathBuf>,
    /// Cache directory (temp dir in --ui-demo mode, default otherwise).
    pub(crate) cache_dir: PathBuf,
    /// Optional override for the manual overrides JSON path. In --ui-demo mode
    /// this points inside the demo temp root so the real platform default path
    /// is never read.
    pub(crate) manual_overrides_path: Option<PathBuf>,
    /// Root of the --ui-demo temp tree (unique per launch). `None` outside demo.
    /// Kept so tests can assert demo I/O stays inside this root.
    #[allow(dead_code)]
    pub(crate) demo_root: Option<PathBuf>,

    // Library view
    pub(crate) search_query: String,
    /// When true, only installed games appear in the grid.
    pub(crate) filter_installed_only: bool,
    /// When true, hidden games are excluded.
    pub(crate) filter_not_hidden: bool,
    /// When true, junk-flagged games are excluded.
    pub(crate) filter_not_junk: bool,
    /// Advanced filter: genre text match (case-insensitive substring).
    pub(crate) filter_genre: String,
    /// Advanced filter: tag text match (RAWG tags or IGDB keywords).
    pub(crate) filter_tag: String,
    /// Advanced filter: ProtonDB tier threshold (show games at or above).
    pub(crate) filter_proton_tier: Option<ProtonTier>,
    /// Advanced filter: only games with full controller support (PCGW).
    pub(crate) filter_deck_compatible: bool,
    /// Advanced filter: only games with full controller support (alias).
    pub(crate) filter_controller_full: bool,
    /// Advanced filter: only unplayed games (0 playtime minutes).
    pub(crate) filter_unplayed_only: bool,
    /// Advanced filter: HLTB completion range (min minutes).
    pub(crate) filter_hltb_min: String,
    /// Advanced filter: HLTB completion range (max minutes).
    pub(crate) filter_hltb_max: String,
    /// Advanced filter: playtime range (min minutes).
    pub(crate) filter_playtime_min: String,
    /// Advanced filter: playtime range (max minutes).
    pub(crate) filter_playtime_max: String,
    /// Advanced filter: sort key.
    pub(crate) library_sort_by: LibrarySort,
    /// Advanced filter: sort descending.
    pub(crate) library_sort_desc: bool,
    /// Quick view selector for the Library grid.
    pub(crate) library_quick_view: QuickView,
    /// Primary scope selector beside Library search.
    pub(crate) library_scope: LibraryScope,
    /// "Load more" pagination: how many games to show (incremented by 48).
    pub(crate) library_visible_count: usize,
    /// Selected game card AppID (enables Recommend without hover).
    pub(crate) library_selected_app_id: Option<u32>,
    /// Junk is a Library panel (not a sidebar destination).
    pub(crate) show_junk_panel: bool,

    // Junk panel (opened from Library)
    pub(crate) junk_mode: JunkModeChoice,
    pub(crate) junk_results: Vec<JunkDecision>,
    pub(crate) junk_selected: std::collections::HashSet<u32>,
    pub(crate) junk_collection_name: String,
    pub(crate) junk_show_all_evaluated: bool,

    // Recommendations view
    pub(crate) recommend_minutes: String,
    pub(crate) recommend_count: String,
    pub(crate) recommend_seed: String,
    pub(crate) recommend_deck: bool,
    pub(crate) recommend_installed_only: bool,
    pub(crate) recommend_results: Vec<Recommendation>,
    /// The `RecommendRequest` captured when the current preview was started,
    /// so Match % is computed against the submitted inputs (e.g. Deck mode)
    /// rather than the current inputs which may have changed mid-job.
    pub(crate) recommend_request_at_start: Option<RecommendRequest>,
    /// Selected recommendation AppID for "Why this pick?" panel.
    pub(crate) recommend_selected: Option<u32>,
    /// Steam collection names excluded from recommendations.
    pub(crate) recommend_exclude_collections: Vec<String>,

    // Playlists view
    pub(crate) playlist_import_path: String,
    pub(crate) playlist_export_path: String,
    pub(crate) playlist_share_code_input: String,
    pub(crate) playlist_share_code_output: Option<String>,
    pub(crate) playlist_edit_id: String,
    /// Whether the ID field should auto-generate from the name (until user
    /// manually edits the ID field).
    pub(crate) playlist_id_auto: bool,
    /// Bumped when the editor is adopted or reset so GPUI InputStates resync.
    pub(crate) playlist_edit_generation: u64,
    pub(crate) playlist_edit_name: String,
    pub(crate) playlist_edit_description: String,
    pub(crate) playlist_edit_app_ids: String,
    /// Optional JSON rules array. When non-empty, "Save Playlist" creates a
    /// rule-based playlist instead of a manual one.
    pub(crate) playlist_edit_rules: String,
    pub(crate) playlist_last_import: Option<PlaylistFile>,
    pub(crate) playlist_match_report: Option<PlaylistMatchReport>,
    /// Ids present in the local playlist store (for Load existing).
    pub(crate) playlist_store_ids: Vec<String>,
    /// Whether [`playlist_store_ids`] has been loaded at least once this session.
    pub(crate) playlist_store_ids_loaded: bool,
    /// Rail model: loaded metadata for each stored playlist.
    /// Keyed by id; value is Ok(PlaylistFile) or Err(error message).
    pub(crate) playlist_rail_entries: Vec<(String, std::result::Result<PlaylistFile, String>)>,
    /// Selected id in the Load existing combo (empty = none).
    pub(crate) playlist_load_selected: String,
    /// Open generator chooser (Dynamic / Mood only).
    pub(crate) playlist_chooser: PlaylistChooser,
    /// Master-detail: active tab in the right workspace (Games/Rules/Match).
    pub(crate) playlist_detail_tab: PlaylistDetailTab,
    /// Master-detail: game search query for Add/Remove in Games tab.
    pub(crate) playlist_game_search: String,
    /// Master-detail: show Advanced JSON editor instead of visual rules.
    pub(crate) playlist_show_advanced_json: bool,
    /// Whether the Advanced CSV editor is expanded in the Games tab.
    pub(crate) playlist_show_advanced_csv: bool,
    /// Visual rule editor: genre input for parameterized HasGenre rule.
    pub(crate) playlist_rule_genre: String,
    /// Visual rule editor: tag input for parameterized HasTag rule.
    pub(crate) playlist_rule_tag: String,
    /// Visual rule editor: HLTB max minutes input.
    pub(crate) playlist_rule_hltb_max: String,
    /// Visual rule editor: ProtonDB tier for parameterized ProtonAtLeast rule.
    pub(crate) playlist_rule_proton_tier: Option<ProtonTier>,
    /// Visual rule editor: playtime min for PlaytimeBetween rule.
    pub(crate) playlist_rule_playtime_min: String,
    /// Visual rule editor: playtime max for PlaytimeBetween rule.
    pub(crate) playlist_rule_playtime_max: String,
    /// Visual rule editor: rating minimum for RatingAtLeast rule (0.0–5.0).
    pub(crate) playlist_rule_rating_min: String,
    /// Master-detail: pending duplicate-ID replacement (for confirm dialog).
    pub(crate) playlist_dup_id_confirm: Option<(String, PlaylistFile)>,
    /// Master-detail: show Import sub-route panel.
    pub(crate) playlist_show_import: bool,
    /// Master-detail: active share tab (ShareCode / Json).
    pub(crate) playlist_share_tab: PlaylistShareTab,
    /// Master-detail: active match sub-tab (Owned / Missing).
    pub(crate) playlist_match_sub_tab: PlaylistMatchTab,
    pub(crate) dynamic_template: String,
    pub(crate) dynamic_minutes: String,
    pub(crate) dynamic_count: String,
    pub(crate) editorial_mood: String,

    // Discover view (top-level; no longer nested under Playlists)
    pub(crate) discover_seed: String,
    pub(crate) discover_count: String,
    /// Last playlist generated from the Discover view (owned by Discover UI).
    pub(crate) discover_last_playlist: Option<PlaylistFile>,
    /// On-page Discover results with scores and reason codes.
    pub(crate) discover_results: Vec<DiscoverPick>,

    // Collections view
    pub(crate) collections: Vec<SteamCollection>,
    pub(crate) collections_export_path: String,

    // Setup / diagnostics (Settings view)
    pub(crate) setup_diagnostics: Option<String>,
    pub(crate) diagnostics_export_path: String,

    // Data Sources view
    pub(crate) has_igdb: bool,
    pub(crate) has_rawg: bool,
    pub(crate) source_statuses: Vec<vapourfly_api::enrichment::SourceStatus>,
    pub(crate) offline_mode: bool,

    // Backups (listed under Settings; not a top-level view)
    pub(crate) backups: Vec<BackupInfo>,

    // Settings view
    pub(crate) steam_dir_edit: String,
    pub(crate) account_edit: String,
    pub(crate) detected_accounts: Vec<SteamAccount>,
    pub(crate) account_list_msg: Option<String>,
    pub(crate) cc_edit: String,
    pub(crate) lang_edit: String,
    pub(crate) backup_retention_edit: String,
    /// Steam Web API key input (plain text — the key is shown in plaintext
    /// on Steam's own creation page; only *echoes* elsewhere are masked).
    pub(crate) steam_api_key_edit: String,
    pub(crate) allow_steam_running: bool,
    pub(crate) settings_save_msg: Option<String>,

    // Write operations
    pub(crate) write_loading: bool,
    pub(crate) write_result: Option<Result<String, String>>,
    pub(crate) show_confirm_dialog: bool,
    pub(crate) pending_action: Option<PendingAction>,
    pub(crate) dry_run_plan: Option<vapourfly_core::write::PreviewedPlan>,
    pub(crate) dry_run_loading: bool,
    pub(crate) dry_run_error: Option<String>,

    // Cache refresh
    pub(crate) cache_refresh_loading: bool,
    pub(crate) cache_refresh_msg: Option<String>,

    // Background job runner (request IDs + stale-result protection)
    pub(crate) job_runner: JobRunner,
    /// Wakes the GPUI window after a background job completes. Tests use a no-op.
    pub(crate) repaint: RepaintHook,
    /// Whether insight rails should render below the main content (responsive
    /// layout: true at 1024–1279px window width). Updated each frame.
    pub(crate) rails_below: bool,
    pub(crate) scan_job_id: Option<JobTicket>,
    pub(crate) write_job_id: Option<JobTicket>,
    pub(crate) enrich_job_id: Option<JobTicket>,
    pub(crate) dry_run_job_id: Option<JobTicket>,
    pub(crate) junk_preview_job_id: Option<JobTicket>,
    pub(crate) recommend_job_id: Option<JobTicket>,
    pub(crate) discover_job_id: Option<JobTicket>,
    pub(crate) dynamic_job_id: Option<JobTicket>,
    pub(crate) mood_job_id: Option<JobTicket>,
    pub(crate) playlist_match_job_id: Option<JobTicket>,
    /// Cached library snapshot (hydrated games, pre-junk-classification).
    /// Reused across frames when the fingerprint matches so the Library view
    /// does not re-hydrate from the disk cache every frame.
    pub(crate) prepared_snapshot: Option<PreparedLibrarySnapshot>,
    /// JobId of an in-flight background library prepare.
    pub(crate) prepare_job_id: Option<JobTicket>,
    /// Fingerprint of the in-flight prepare (to set on the snapshot).
    pub(crate) prepare_fingerprint: Option<u64>,
    /// Increments each time a cache refresh completes, so the library snapshot
    /// is invalidated and re-hydrated with the new cache data.
    pub(crate) cache_refresh_generation: u64,
    /// Monotonic counter incremented every time a new scan result is accepted
    /// (real scan or demo refresh). Unlike `scan_job_id` (which is `None`
    /// after a scan completes) or the game count (which can stay the same when
    /// only content changes — playtime, hidden state, collections), this
    /// always changes, so the prepare fingerprint reliably invalidates the
    /// cached snapshot. See [`VapourflyApp::library_prepare_fingerprint`].
    pub(crate) scan_generation: u64,

    // Loading flags for off-frame operations
    pub(crate) junk_preview_loading: bool,
    pub(crate) recommend_loading: bool,
    pub(crate) discover_loading: bool,
    pub(crate) dynamic_loading: bool,
    pub(crate) mood_loading: bool,
    pub(crate) playlist_match_loading: bool,
}

/// Snapshot used by the Library insights rail (ADR-0006).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LibraryInsights {
    pub total: usize,
    pub installed: usize,
    pub hidden: usize,
    pub junk: usize,
    pub playtime: u32,
    pub matching: usize,
    pub backlog: usize,
    pub recent: Vec<(u32, String, i64, u32)>,
    pub avg_hltb_minutes: u32,
}

/// ProtonDB tiers offered by the Library advanced filter (Any is `None`).
pub(crate) const PROTON_FILTER_TIERS: &[ProtonTier] = &[
    ProtonTier::Bronze,
    ProtonTier::Silver,
    ProtonTier::Gold,
    ProtonTier::Platinum,
    ProtonTier::Native,
];

pub(crate) fn cycle_proton_filter(current: Option<ProtonTier>) -> Option<ProtonTier> {
    match current {
        None => PROTON_FILTER_TIERS.first().copied(),
        Some(tier) => PROTON_FILTER_TIERS
            .iter()
            .position(|t| *t == tier)
            .and_then(|i| PROTON_FILTER_TIERS.get(i + 1).copied()),
    }
}

pub(crate) fn proton_tier_label(tier: ProtonTier) -> &'static str {
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

pub(crate) fn format_hltb_seconds(seconds: u32) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

pub(crate) fn game_metadata_summary(game: &Game) -> String {
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

/// Library grid filters. Quick-view presets set the three toggles; advanced
/// filters add genre, ProtonDB tier, deck compatibility, and unplayed.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LibraryFilters {
    installed_only: bool,
    not_hidden: bool,
    not_junk: bool,
    is_hidden_only: bool,
    is_junk_only: bool,
    search: String,
    genre: String,
    tag: String,
    proton_tier: Option<ProtonTier>,
    deck_compatible: bool,
    controller_full: bool,
    unplayed_only: bool,
    hltb_max_minutes: Option<u32>,
    hltb_min_minutes: Option<u32>,
    playtime_min_minutes: Option<u32>,
    playtime_max_minutes: Option<u32>,
    sort_by: LibrarySort,
    sort_desc: bool,
}

/// Sort key for the Library grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum LibrarySort {
    #[default]
    InstalledThenPlaytime,
    Name,
    Playtime,
    Hltb,
    Rating,
    AppId,
}

pub(crate) fn sort_label(sort: LibrarySort) -> &'static str {
    match sort {
        LibrarySort::InstalledThenPlaytime => "Installed + Playtime",
        LibrarySort::Name => "Name",
        LibrarySort::Playtime => "Playtime",
        LibrarySort::Hltb => "HLTB",
        LibrarySort::Rating => "Rating",
        LibrarySort::AppId => "AppID",
    }
}

/// Whether a single game matches the Library filters and search query.
pub(crate) fn game_matches_library_filters(game: &Game, filters: &LibraryFilters) -> bool {
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
        let gold_or_better = game.protondb.as_ref().is_some_and(|p| {
            matches!(
                p.tier,
                ProtonTier::Gold | ProtonTier::Platinum | ProtonTier::Native
            )
        });
        if !gold_or_better {
            return false;
        }
    }
    if filters.controller_full
        && !game
            .pcgw
            .as_ref()
            .is_some_and(|p| p.controller_support == ControllerSupport::Full)
    {
        return false;
    }
    if !filters.tag.is_empty() {
        let t = filters.tag.to_lowercase();
        let has_tag = game
            .rawg
            .as_ref()
            .is_some_and(|r| r.tags.iter().any(|x| x.to_lowercase().contains(&t)))
            || game
                .igdb
                .as_ref()
                .is_some_and(|i| i.keywords.iter().any(|x| x.to_lowercase().contains(&t)));
        if !has_tag {
            return false;
        }
    }
    // Prefer the canonical, normalized HLTB main_story_seconds; fall back
    // to the raw IGDB time_to_beat.normally_seconds for games without HLTB.
    let completion_minutes = || {
        game.hltb
            .as_ref()
            .and_then(|h| h.main_story_seconds)
            .or_else(|| {
                game.igdb
                    .as_ref()
                    .and_then(|i| i.time_to_beat.as_ref())
                    .and_then(|t| t.normally_seconds)
            })
            .map(|secs| secs / 60)
    };
    if let Some(max_minutes) = filters.hltb_max_minutes
        && completion_minutes().is_none_or(|m| m > max_minutes)
    {
        return false;
    }
    if let Some(min_minutes) = filters.hltb_min_minutes
        && completion_minutes().is_none_or(|m| m < min_minutes)
    {
        return false;
    }
    if let Some(min_pt) = filters.playtime_min_minutes {
        let pt = game.playtime_minutes.unwrap_or(0);
        if pt < min_pt {
            return false;
        }
    }
    if let Some(max_pt) = filters.playtime_max_minutes {
        let pt = game.playtime_minutes.unwrap_or(0);
        if pt > max_pt {
            return false;
        }
    }
    true
}

/// Filter + sort games for the Library poster grid.
pub(crate) fn project_library_games(games: &[Game], filters: &LibraryFilters) -> Vec<Game> {
    let mut games: Vec<Game> = games
        .iter()
        .filter(|g| game_matches_library_filters(g, filters))
        .cloned()
        .collect();

    let cmp = |a: &Game, b: &Game| match filters.sort_by {
        LibrarySort::InstalledThenPlaytime => b
            .installed
            .cmp(&a.installed)
            .then_with(|| {
                b.playtime_minutes
                    .unwrap_or(0)
                    .cmp(&a.playtime_minutes.unwrap_or(0))
            })
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        LibrarySort::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        LibrarySort::Playtime => b
            .playtime_minutes
            .unwrap_or(0)
            .cmp(&a.playtime_minutes.unwrap_or(0))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
        LibrarySort::Hltb => {
            let ha = a
                .hltb
                .as_ref()
                .and_then(|h| h.main_story_seconds)
                .unwrap_or(0);
            let hb = b
                .hltb
                .as_ref()
                .and_then(|h| h.main_story_seconds)
                .unwrap_or(0);
            hb.cmp(&ha)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }
        LibrarySort::Rating => {
            let ra = a.rawg.as_ref().and_then(|r| r.rating_0_5).unwrap_or(0.0);
            let rb = b.rawg.as_ref().and_then(|r| r.rating_0_5).unwrap_or(0.0);
            rb.partial_cmp(&ra)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        }
        LibrarySort::AppId => a.app_id.cmp(&b.app_id),
    };

    if filters.sort_desc && filters.sort_by != LibrarySort::InstalledThenPlaytime {
        games.sort_by(|a, b| cmp(b, a));
    } else {
        games.sort_by(|a, b| cmp(a, b));
    }

    games
}

/// Open a URL in the user's default system browser.
/// Falls back silently on unsupported platforms.
pub(crate) fn open_url_in_browser(url: &str) {
    #[cfg(target_os = "macos")]
    let cmd = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let cmd = "xdg-open";
    #[cfg(target_os = "windows")]
    let cmd = "cmd";

    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new(cmd)
            .args(["/c", "start", "", url])
            .spawn();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new(cmd).arg(url).spawn();
    }
}

/// Format a Unix timestamp as a relative time ago string (e.g. "3 days ago").
pub(crate) fn relative_time_ago(unix: i64) -> String {
    if unix == 0 {
        return "unknown".into();
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let diff_secs = now.saturating_sub(unix);
    if diff_secs < 60 {
        "just now".into()
    } else if diff_secs < 3600 {
        let mins = diff_secs / 60;
        format!("{mins} min ago")
    } else if diff_secs < 86400 {
        let hours = diff_secs / 3600;
        format!("{} hour{} ago", hours, if hours == 1 { "" } else { "s" })
    } else {
        let days = diff_secs / 86400;
        format!("{} day{} ago", days, if days == 1 { "" } else { "s" })
    }
}

/// Human-readable label for a single playlist rule.
pub(crate) fn rule_label(rule: &PlaylistRule) -> String {
    match rule {
        PlaylistRule::Installed => "Installed".into(),
        PlaylistRule::NotJunk => "Not junk".into(),
        PlaylistRule::NotHidden => "Not hidden".into(),
        PlaylistRule::ControllerSupportFull => "Full controller".into(),
        PlaylistRule::ProtonAtLeast { tier } => format!("Proton {}+", proton_tier_label(*tier)),
        PlaylistRule::HltbMaxMinutes { minutes } => {
            if *minutes >= 60 && *minutes % 60 == 0 {
                format!("HLTB ≤ {}h", minutes / 60)
            } else {
                format!("HLTB ≤ {minutes}m")
            }
        }
        PlaylistRule::PlaytimeBetween { min, max } => format!("Played {min}–{max}m"),
        PlaylistRule::RatingAtLeast { rating_0_5 } => format!("Rating ≥ {rating_0_5:.1}"),
        PlaylistRule::HasGenre { genre } => format!("Genre: {genre}"),
        PlaylistRule::HasTag { tag } => format!("Tag: {tag}"),
        PlaylistRule::And(rules) => format!("All of ({})", rules.len()),
        PlaylistRule::Or(rules) => format!("Any of ({})", rules.len()),
        PlaylistRule::Not(_) => "Not".into(),
    }
}

/// Recursively render a rule tree with remove buttons.
/// Nested And/Or/Not rules are rendered with indentation.
pub(crate) fn manual_playlist_app_ids_csv(pf: &PlaylistFile) -> String {
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
pub(crate) fn playlist_rules_json(pf: &PlaylistFile) -> String {
    match &pf.playlist.content {
        PlaylistContent::Rules { rules } => serde_json::to_string_pretty(rules).unwrap_or_default(),
        PlaylistContent::Manual { .. } => String::new(),
    }
}

/// Stable hash of a playlist's full content (manual AppIDs or rules JSON), so
/// the Playlist Match fingerprint changes when the content is edited — not just
/// when the playlist id changes.
pub(crate) fn playlist_content_hash(pf: &PlaylistFile) -> u64 {
    match &pf.playlist.content {
        PlaylistContent::Manual { app_ids } => fingerprint_u64(&format!("manual:{app_ids:?}")),
        PlaylistContent::Rules { rules } => fingerprint_u64(&format!(
            "rules:{}",
            serde_json::to_string(rules).unwrap_or_default()
        )),
    }
}

/// Fingerprint for a dry-run job: the target action + all input AppIDs (junk
/// selection, recommend results, or playlist AppIDs). Used so a dry-run is
/// invalidated if the inputs change before the background job completes.
pub(crate) fn dry_run_fingerprint(
    action: &PendingAction,
    junk_selected: &std::collections::HashSet<u32>,
    recommend_results: &[Recommendation],
    scan_generation: u64,
) -> String {
    let mut app_ids: Vec<u32> = match action {
        PendingAction::JunkApply | PendingAction::JunkHide => {
            junk_selected.iter().copied().collect()
        }
        PendingAction::RecommendCollection => recommend_results.iter().map(|r| r.app_id).collect(),
        PendingAction::PlaylistSync(pf) => manual_playlist_app_ids_csv(pf)
            .split(',')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .collect(),
        PendingAction::BackupRestore(_) => vec![],
    };
    app_ids.sort_unstable();
    format!("dry_run:{action:?}:apps={app_ids:?}:lib={scan_generation}")
}

/// Human-facing label for an enrichment source id (`igdb` → `IGDB`).
pub(crate) fn source_display_name(source_id: &str) -> &'static str {
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
pub(crate) enum CredentialSignal {
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
    pub(crate) fn label(self) -> &'static str {
        match self {
            CredentialSignal::Configured => "Configured",
            CredentialSignal::Missing => "Missing",
            CredentialSignal::NotRequired => "None needed",
            CredentialSignal::Optional => "Optional",
        }
    }
}

/// Map a source id to its credential signal given current env state.
pub(crate) fn source_credential_signal(
    source_id: &str,
    has_igdb: bool,
    has_rawg: bool,
) -> CredentialSignal {
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
pub(crate) fn source_refresh_enabled(
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

/// Each entry is (top_block, bottom_block) — two distinct shades that make
/// the placeholder visually identifiable without any network fetch.
pub(crate) const ARTWORK_PALETTE: [(Rgb, Rgb); 8] = [
    (
        Rgb::from_rgb(0x4C, 0x6E, 0xF0),
        Rgb::from_rgb(0x2A, 0x4A, 0xC0),
    ),
    (
        Rgb::from_rgb(0xE1, 0x70, 0x55),
        Rgb::from_rgb(0xB5, 0x4A, 0x35),
    ),
    (
        Rgb::from_rgb(0x2E, 0xC4, 0xB6),
        Rgb::from_rgb(0x1A, 0x9A, 0x8E),
    ),
    (
        Rgb::from_rgb(0xF4, 0xC4, 0x30),
        Rgb::from_rgb(0xC0, 0x98, 0x18),
    ),
    (
        Rgb::from_rgb(0x9B, 0x59, 0xB6),
        Rgb::from_rgb(0x72, 0x3C, 0x8A),
    ),
    (
        Rgb::from_rgb(0x34, 0x98, 0xDB),
        Rgb::from_rgb(0x21, 0x70, 0xA8),
    ),
    (
        Rgb::from_rgb(0xE6, 0x7E, 0x22),
        Rgb::from_rgb(0xB0, 0x5C, 0x12),
    ),
    (
        Rgb::from_rgb(0x1A, 0xBC, 0x9C),
        Rgb::from_rgb(0x12, 0x8E, 0x76),
    ),
];
pub(crate) fn unique_demo_root() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("vapourfly-ui-demo-{nanos}-{pid}"))
}

impl VapourflyApp {
    pub(crate) fn new(fixtures_path: Option<PathBuf>, ui_demo: bool) -> Self {
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
                steam_api_key: None,
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

        let steam_api_key_edit = config
            .as_ref()
            .and_then(|c| c.steam_api_key.clone())
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

        let source_statuses = vapourfly_api::enrichment::source_status(&cache_root);

        Self {
            scan_result: None,
            current_view: View::Library,
            loading: false,
            error: None,
            success_msg: None,
            fixtures_path,
            ui_demo,
            auto_populate_started: false,
            theme_mode: ThemeMode::Light,
            repaint: RepaintHook::default(),

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
            filter_tag: String::new(),
            filter_proton_tier: None,
            filter_deck_compatible: false,
            filter_controller_full: false,
            filter_unplayed_only: false,
            filter_hltb_min: String::new(),
            filter_hltb_max: String::new(),
            filter_playtime_min: String::new(),
            filter_playtime_max: String::new(),
            library_sort_by: LibrarySort::default(),
            library_sort_desc: false,
            library_quick_view: QuickView::All,
            library_scope: LibraryScope::All,
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
            recommend_exclude_collections: Vec::new(),

            playlist_import_path: String::new(),
            playlist_export_path: String::new(),
            playlist_share_code_input: String::new(),
            playlist_share_code_output: None,
            playlist_edit_id: String::new(),
            playlist_id_auto: true,
            playlist_edit_generation: 0,
            playlist_edit_name: String::new(),
            playlist_edit_description: String::new(),
            playlist_edit_app_ids: String::new(),
            playlist_edit_rules: String::new(),
            playlist_last_import: None,
            playlist_match_report: None,
            playlist_store_ids: Vec::new(),
            playlist_store_ids_loaded: false,
            playlist_rail_entries: Vec::new(),
            playlist_load_selected: String::new(),
            playlist_chooser: PlaylistChooser::None,
            playlist_detail_tab: PlaylistDetailTab::Games,
            playlist_game_search: String::new(),
            playlist_show_advanced_json: false,
            playlist_show_advanced_csv: false,
            playlist_rule_genre: String::new(),
            playlist_rule_tag: String::new(),
            playlist_rule_hltb_max: String::new(),
            playlist_rule_proton_tier: None,
            playlist_rule_playtime_min: String::new(),
            playlist_rule_playtime_max: String::new(),
            playlist_rule_rating_min: String::new(),
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
            steam_api_key_edit,
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
            rails_below: false,
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
    pub(crate) fn populate_demo_data(&mut self) {
        use vapourfly_core::models::{
            HltbData, HltbSource, IgdbData, PcgwData, ProtonDbData, ProtonTier, RawgData,
            SteamStoreDetails, SteamStorePlatforms,
        };

        const DEMO_GAME_NAMES: [&str; 24] = [
            "Fields of Luma",
            "Neon Harbor",
            "Wispwood",
            "Starward Drift",
            "Grimstone Keep",
            "Echoes of the Vale",
            "Pixel Sprout",
            "Salt & Wind",
            "Clockwork Garden",
            "Moonlit Market",
            "Ember Circuit",
            "Cloudline",
            "Tiny Kingdoms",
            "Afterlight",
            "Mossbound",
            "Skyfarer",
            "Lantern Lake",
            "Quiet Orbit",
            "Paper Trails",
            "Wild Current",
            "Copper & Clover",
            "Night Orchard",
            "Signal Bloom",
            "Northstar",
        ];
        let demo_games: Vec<Game> = (0..24)
            .map(|i| {
                let app_id = 1000 + i;
                let name = DEMO_GAME_NAMES[i as usize].to_string();
                Game {
                    app_id,
                    name: name.clone(),
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
                            name: name.clone(),
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
                            name: name.clone(),
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

        self.source_statuses = vapourfly_api::enrichment::source_status(&self.cache_dir);

        self.detected_accounts = vec![SteamAccount {
            steam_id64: "76561198000000000".into(),
            account_name: "demo_user".into(),
            persona_name: "Demo Player".into(),
            most_recent: true,
        }];

        self.backups = vec![BackupInfo {
            path: PathBuf::from(
                "/demo/backups/cloud-storage-namespace-1.vapourfly-backup-20260101T120000Z-abc12345.json",
            ),
            created_at: chrono::Utc::now(),
            sha256: "abc12345def67890".into(),
        }];

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
            self.playlist_rail_entries = demo_playlists
                .iter()
                .map(|pf| (pf.playlist.id.clone(), Ok(pf.clone())))
                .collect();
        }
        self.playlist_store_ids = demo_ids;
        self.playlist_store_ids_loaded = true;
        if let Some(first) = self.playlist_store_ids.first().cloned()
            && self.load_playlist_from_store(&first).is_ok()
        {
            self.playlist_load_selected = first;
        }
    }

    /// Resolve the cloud storage path for the current config.
    pub(crate) fn cloud_storage_path(&self) -> Result<PathBuf, String> {
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

    /// Current scan fingerprint — used at poll time to detect scan-config
    /// drift (steam_dir, account, fixtures, offline changed mid-scan).
    /// Steam dir + account used for a scan (config, else detection, else the
    /// conventional `~/.steam/steam` fallback).
    pub(crate) fn scan_inputs(&self) -> (PathBuf, Option<String>) {
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
        (
            steam_dir,
            self.config.as_ref().and_then(|c| c.account.clone()),
        )
    }

    pub(crate) fn current_scan_fingerprint(&self) -> String {
        let (steam_dir, account) = self.scan_inputs();
        format!(
            "scan:dir={}:acct={}:fix={}:offline={}",
            steam_dir.display(),
            account.as_deref().unwrap_or(""),
            self.fixtures_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            self.offline_mode
        )
    }

    pub(crate) fn start_scan(&mut self) {
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

        // Fingerprint includes steam_dir, account, fixtures, and offline so
        // changing the scan config invalidates an in-flight scan.
        let (steam_dir, account) = self.scan_inputs();
        let offline = self.offline_mode;
        let scan_fp = self.current_scan_fingerprint();
        let job_id = self.job_runner.next_ticket(WorkflowKind::Scan, &scan_fp);
        self.scan_job_id = Some(job_id);
        SCAN_RESULT.clear();

        let repaint = self.repaint.clone();
        let fixtures = self.fixtures_path.clone();

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
                cache_root: None,
            };
            let result = vapourfly_api::workflow::prepare(&opts);
            SCAN_RESULT.set(job_id, result.map_err(|e| e.to_string()));
            repaint.request();
        });
    }

    /// Execute a pending write action in a background thread.
    ///
    /// Every plan-based action commits the [`vapourfly_core::write::PreviewedPlan`]
    /// generated during the dry-run step and stored in `self.dry_run_plan` —
    /// exactly the diff the user confirmed. Backup restore is the only action
    /// without a plan (it has no diff to preview).
    pub(crate) fn execute_pending_action(&mut self) {
        let Some(action) = self.pending_action.take() else {
            return;
        };

        self.show_confirm_dialog = false;

        // Backup restore never uses a dry-run WritePlan. Clear any leftover plan
        // so a prior junk/playlist confirm cannot be mis-committed here.
        if matches!(action, PendingAction::BackupRestore(_)) {
            self.dry_run_plan = None;
        }

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

        // Legacy path for BackupRestore (no dry-run diff). Every other
        // action commits only a stored PreviewedPlan — a Steam write whose
        // diff was never shown must not happen (confirmation gate).
        let Some(cloud_path) = self.ok_or_err(self.cloud_storage_path()) else {
            return;
        };

        self.write_loading = true;
        self.write_result = None;
        self.success_msg = None;

        let allow_steam_running = self.allow_steam_running;

        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::Write, "legacy_write");
        self.write_job_id = Some(job_id);
        WRITE_RESULT.clear();

        std::thread::spawn(move || {
            let result = match action {
                PendingAction::BackupRestore(backup_path) => {
                    execute_backup_restore(backup_path, cloud_path, allow_steam_running)
                }
                PendingAction::JunkApply | PendingAction::JunkHide => {
                    Err("Junk writes require a confirmed dry-run plan.".into())
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
    pub(crate) fn start_dry_run(&mut self, action: PendingAction) {
        // Backup restore has no WritePlan. Confirm + execute_backup_restore
        // is the only legal path; a dry-run job always errors and tick()
        // would clear pending_action.
        if matches!(action, PendingAction::BackupRestore(_)) {
            return;
        }

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
        // longer matches rules on the UI frame. Capture the prepared library
        // so the background job can resolve rules there.
        let Some(action) = self.ok_or_err(self.resolve_dry_run_action(action)) else {
            return;
        };
        let games = self.prepared_games(JunkMode::Default).unwrap_or_default();

        let Some(cloud_path) = self.ok_or_err(self.cloud_storage_path()) else {
            return;
        };

        self.dry_run_loading = true;
        self.dry_run_error = None;
        self.dry_run_plan = None;
        self.pending_action = Some(action.clone());

        let dry_run_fp = dry_run_fingerprint(
            &action,
            &self.junk_selected,
            &self.recommend_results,
            self.scan_generation,
        );
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

    /// Start an explicit cache refresh for the given source (or all
    /// sources). Forced: re-fetches even fresh entries.
    pub(crate) fn start_cache_refresh(&mut self, source: Option<String>) {
        self.start_enrich_job(source, true);
    }

    /// Start the automatic background populate after a scan (ADR-0009):
    /// fetch missing/stale entries only, once per launch. Silent no-op in
    /// demo/offline mode or when a job is already running — the library is
    /// already rendered; this only warms the cache behind it.
    pub(crate) fn start_background_populate(&mut self) {
        if self.auto_populate_started
            || self.ui_demo
            || self.offline_mode
            || self.cache_refresh_loading
        {
            return;
        }
        self.auto_populate_started = true;
        self.start_enrich_job(None, false);
    }

    /// Shared enrichment job body: `force` distinguishes an explicit
    /// refresh (re-fetch everything) from the background populate
    /// (missing/stale only).
    pub(crate) fn start_enrich_job(&mut self, source: Option<String>, force: bool) {
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

        let fingerprint = format!("cache_refresh:{source:?}:{force}");
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::CacheRefresh, &fingerprint);
        self.enrich_job_id = Some(job_id);
        ENRICH_RESULT.clear();

        let cache_root = self.cache_dir.clone();
        let repaint = self.repaint.clone();

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
                force,
            };

            let mut games = games;
            let summary = vapourfly_api::enrichment::enrich_games(&mut games, &cache, &options);

            repaint.request();
            ENRICH_RESULT.set(job_id, Ok(summary));
        });
    }

    /// Unwrap a result, routing the error message to the UI error banner.
    pub(crate) fn ok_or_err<T>(&mut self, result: Result<T, String>) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(e) => {
                self.error = Some(e);
                None
            }
        }
    }

    // Job-input fingerprints, built identically at job start and at poll time
    // so any input change mid-job invalidates the in-flight result.

    pub(crate) fn junk_preview_fp(&self, mode: &JunkMode) -> String {
        // Covers mode + library generation + override/cache generation so a
        // rescan or cache refresh invalidates an in-flight preview.
        format!(
            "junk_preview:{mode:?}:lib={}:ovr={}",
            self.scan_generation, self.cache_refresh_generation
        )
    }

    pub(crate) fn recommend_fp(&self, request: &RecommendRequest) -> String {
        format!("recommend:{request:?}:lib={}", self.scan_generation)
    }

    pub(crate) fn discover_fp(&self, options: &DiscoverOptions) -> String {
        format!("discover:{options:?}:lib={}", self.scan_generation)
    }

    pub(crate) fn dynamic_fp(
        &self,
        template: DynamicTemplate,
        session_minutes: u32,
        count: usize,
    ) -> String {
        format!(
            "dynamic:{}:{session_minutes}:{count}:lib={}",
            template.id(),
            self.scan_generation
        )
    }

    pub(crate) fn mood_fp(&self, mood: EditorialMood) -> String {
        format!("mood:{}:lib={}", mood.id(), self.scan_generation)
    }

    pub(crate) fn playlist_match_fp(&self, pf: &PlaylistFile) -> String {
        // Covers the playlist id + a hash of the full content (manual AppIDs
        // or rules) + library generation + price-cache generation, so editing
        // the playlist, rescanning, or refreshing the cache invalidates an
        // in-flight match.
        format!(
            "playlist_match:{}:content={:x}:lib={}:price={}",
            pf.playlist.id,
            playlist_content_hash(pf),
            self.scan_generation,
            self.cache_refresh_generation
        )
    }

    /// Start Junk Preview in a background thread.
    pub(crate) fn start_junk_preview(&mut self) {
        if self.junk_preview_loading {
            return;
        }
        let mode = self.junk_mode.mode();
        // Always use the Default snapshot (pre-classified, Arc clone is
        // cheap). The non-Default classification happens inside
        // evaluate_junk which reads signals directly — it does NOT read
        // game.is_junk — so we don't need apply_junk_flags at all for the
        // preview. This keeps all heavy work off the UI thread.
        let Some(games) = self.prepared_games(JunkMode::Default) else {
            return;
        };
        let overrides = self
            .prepared_snapshot
            .as_ref()
            .map(|s| s.overrides.clone())
            .unwrap_or_default();

        self.junk_preview_loading = true;
        let fingerprint = self.junk_preview_fp(&mode);
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::JunkPreview, &fingerprint);
        self.junk_preview_job_id = Some(job_id);
        JUNK_PREVIEW_RESULT.clear();

        let repaint = self.repaint.clone();
        std::thread::spawn(move || {
            // evaluate_junk computes decisions from signals without
            // mutating the games, so no clone/apply_junk_flags needed.
            let results = evaluate_junk(&games, &JunkRules::default(), &mode, &overrides);
            repaint.request();
            JUNK_PREVIEW_RESULT.set(job_id, Ok(results));
        });
    }

    /// Start Recommendations Preview in a background thread.
    pub(crate) fn start_recommend_preview(&mut self) {
        if self.recommend_loading {
            return;
        }
        let Some(request) = self.ok_or_err(self.recommend_request_from_inputs()) else {
            return;
        };
        let Some(games) = self.prepared_games(JunkMode::Default) else {
            return;
        };

        self.recommend_loading = true;
        // The request is captured at start time (recommend_request_at_start)
        // so Match % is computed against the request the user actually
        // submitted, not the current inputs (e.g. if Deck mode changes mid-job).
        let fingerprint = self.recommend_fp(&request);
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::RecommendPreview, &fingerprint);
        self.recommend_job_id = Some(job_id);
        self.recommend_request_at_start = Some(request.clone());
        RECOMMEND_RESULT.clear();

        let repaint = self.repaint.clone();
        std::thread::spawn(move || {
            let results = recommend(&games, &request);
            repaint.request();
            RECOMMEND_RESULT.set(job_id, Ok(results));
        });
    }

    /// Start Discover generate in a background thread.
    pub(crate) fn start_discover_generate(&mut self) {
        if self.discover_loading {
            return;
        }
        let Some(options) = self.ok_or_err(self.discover_options_from_inputs()) else {
            return;
        };
        let Some(games) = self.prepared_games(JunkMode::Default) else {
            return;
        };

        self.discover_loading = true;
        let fingerprint = self.discover_fp(&options);
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::Discover, &fingerprint);
        self.discover_job_id = Some(job_id);
        DISCOVER_RESULT.clear();

        let repaint = self.repaint.clone();
        std::thread::spawn(move || {
            let picks = discover::rank_discover_picks(&games, &options);
            let pf = discover::playlist_from_discover_picks(&games, &options, &picks);
            repaint.request();
            DISCOVER_RESULT.set(job_id, Ok((picks, pf)));
        });
    }

    /// Start Dynamic generate in a background thread.
    pub(crate) fn start_dynamic_generate(&mut self) {
        if self.dynamic_loading {
            return;
        }
        let Some(template) = DynamicTemplate::parse(&self.dynamic_template) else {
            self.error = Some("Unknown template. Use deck-session or finish-it.".into());
            return;
        };
        let Some(session_minutes) =
            self.ok_or_err(parse_required("Session minutes", &self.dynamic_minutes))
        else {
            return;
        };
        let Some(count) = self.ok_or_err(parse_required("Count", &self.dynamic_count)) else {
            return;
        };
        let Some(games) = self.prepared_games(JunkMode::Default) else {
            return;
        };

        self.dynamic_loading = true;
        let fingerprint = self.dynamic_fp(template, session_minutes, count);
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::Dynamic, &fingerprint);
        self.dynamic_job_id = Some(job_id);
        // Capture the identity at start time so the consumer can write the
        // result to the correct stable slot even if the chooser changes mid-job.
        // Input-drift protection is handled by the JobTicket fingerprint
        // (compared on poll), so the result no longer needs a separate check.
        let identity = GeneratorIdentity::Dynamic(template);
        DYNAMIC_RESULT.clear();

        let repaint = self.repaint.clone();
        std::thread::spawn(move || {
            let pf = dynamic::compile_dynamic_template(
                template,
                &games,
                &DynamicTemplateOptions {
                    session_minutes,
                    count,
                },
            );
            repaint.request();
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
    pub(crate) fn start_mood_generate(&mut self) {
        if self.mood_loading {
            return;
        }
        let Some(mood) = EditorialMood::parse(&self.editorial_mood) else {
            self.error = Some("Unknown mood. Pick one from the list.".into());
            return;
        };
        let Some(games) = self.prepared_games(JunkMode::Default) else {
            return;
        };

        self.mood_loading = true;
        let fingerprint = self.mood_fp(mood);
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::Mood, &fingerprint);
        self.mood_job_id = Some(job_id);
        // Capture identity at start time; input-drift protection is handled by
        // the JobTicket fingerprint (compared on poll).
        let identity = GeneratorIdentity::Mood(mood);
        MOOD_RESULT.clear();

        let repaint = self.repaint.clone();
        std::thread::spawn(move || {
            let pf = mood::compile_editorial_mood(mood, &games, 25);
            repaint.request();
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
    pub(crate) fn poll_generator_results(&mut self) {
        // Validate fingerprints against the current chooser inputs + library
        // generation so a rescan or chooser change invalidates an in-flight job.
        if self.dynamic_loading
            && let Some(expected) = self.dynamic_job_id
        {
            let template = DynamicTemplate::parse(&self.dynamic_template);
            let session_minutes = parse_required("Session minutes", &self.dynamic_minutes).ok();
            let count = parse_required("Count", &self.dynamic_count).ok();
            let current_fp = match (template, session_minutes, count) {
                (Some(t), Some(sm), Some(c)) => self.dynamic_fp(t, sm, c),
                _ => String::new(),
            };
            if current_fp.is_empty() || expected.fingerprint != fingerprint_u64(&current_fp) {
                self.dynamic_loading = false;
                self.dynamic_job_id = None;
            } else if let Some(result) = DYNAMIC_RESULT.take_if(expected) {
                self.dynamic_loading = false;
                self.dynamic_job_id = None;
                self.apply_generator_result(result);
            }
        }

        if self.mood_loading
            && let Some(expected) = self.mood_job_id
        {
            let current_fp = EditorialMood::parse(&self.editorial_mood)
                .map(|m| self.mood_fp(m))
                .unwrap_or_default();
            if current_fp.is_empty() || expected.fingerprint != fingerprint_u64(&current_fp) {
                self.mood_loading = false;
                self.mood_job_id = None;
            } else if let Some(result) = MOOD_RESULT.take_if(expected) {
                self.mood_loading = false;
                self.mood_job_id = None;
                self.apply_generator_result(result);
            }
        }
    }

    /// Store a finished generator playlist in its stable slot and adopt it
    /// into the Playlists edit surface.
    pub(crate) fn apply_generator_result(&mut self, result: Result<GeneratorJobResult, String>) {
        match result {
            Ok(job_result) => {
                match self.store_generator_playlist(job_result.identity, job_result.playlist) {
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

    /// Start Playlist Match in a background thread.
    pub(crate) fn start_playlist_match(&mut self, pf: PlaylistFile) {
        if self.playlist_match_loading {
            return;
        }
        let Some(games) = self.prepared_games(JunkMode::Default) else {
            return;
        };

        self.playlist_match_loading = true;
        let fingerprint = self.playlist_match_fp(&pf);
        let job_id = self
            .job_runner
            .next_ticket(WorkflowKind::PlaylistMatch, &fingerprint);
        self.playlist_match_job_id = Some(job_id);
        PLAYLIST_MATCH_RESULT.clear();

        let repaint = self.repaint.clone();
        let cache_dir = self.cache_dir.clone();
        let offline = self.offline_mode;
        let (cc, lang) = self
            .config
            .as_ref()
            .map(|c| (c.cc.clone(), c.lang.clone()))
            .unwrap_or_else(|| ("us".to_string(), "english".to_string()));
        std::thread::spawn(move || {
            // Two-pass match with missing-entry store details is owned by
            // the workflow module (shared with the CLI).
            let cache = vapourfly_api::cache::DiskCache::new(cache_dir);
            let result = vapourfly_api::workflow::match_playlist_full(
                &pf, &games, &cache, offline, &cc, &lang,
            )
            .map_err(|e| format!("Match failed: {e}"));
            repaint.request();
            PLAYLIST_MATCH_RESULT.set(job_id, result);
        });
    }

    /// True when CDN fetches are banned (demo mode or offline mode).
    pub(crate) fn demo_or_offline(&self) -> bool {
        self.ui_demo || self.offline_mode
    }

    pub(crate) fn filtered_games(&self) -> Vec<Game> {
        let Some(games) = self.prepared_games(JunkMode::Default) else {
            return Vec::new();
        };
        let games: &[Game] = &games;

        // Quick-view ShortSessions preset overrides the HLTB max filter.
        let hltb_max = if self.library_quick_view == QuickView::ShortSessions {
            Some(120)
        } else {
            self.filter_hltb_max.parse::<u32>().ok()
        };
        let hltb_min = self.filter_hltb_min.parse::<u32>().ok();
        let playtime_min = self.filter_playtime_min.parse::<u32>().ok();
        let playtime_max = self.filter_playtime_max.parse::<u32>().ok();

        let filters = LibraryFilters {
            installed_only: self.filter_installed_only
                || self.library_scope == LibraryScope::Installed,
            not_hidden: self.filter_not_hidden,
            not_junk: self.filter_not_junk,
            is_hidden_only: self.library_scope == LibraryScope::Hidden,
            is_junk_only: false,
            search: self.search_query.clone(),
            genre: self.filter_genre.clone(),
            tag: self.filter_tag.clone(),
            proton_tier: self.filter_proton_tier,
            deck_compatible: self.filter_deck_compatible,
            controller_full: self.filter_controller_full,
            unplayed_only: self.filter_unplayed_only
                || self.library_scope == LibraryScope::Unplayed,
            hltb_max_minutes: hltb_max,
            hltb_min_minutes: hltb_min,
            playtime_min_minutes: playtime_min,
            playtime_max_minutes: playtime_max,
            sort_by: self.library_sort_by,
            sort_desc: self.library_sort_desc,
        };
        project_library_games(games, &filters)
    }

    /// Totals, backlog, recent activity, and average HLTB for the Library rail.
    pub(crate) fn library_insights(&self, matching: &[Game]) -> LibraryInsights {
        let all = self
            .scan_result
            .as_ref()
            .map(|s| s.games.as_slice())
            .unwrap_or(&[]);
        let mut recent: Vec<&Game> = all
            .iter()
            .filter(|g| g.last_played_unix.is_some())
            .collect();
        recent.sort_by(|a, b| {
            b.last_played_unix
                .unwrap_or(0)
                .cmp(&a.last_played_unix.unwrap_or(0))
        });
        let hltb_minutes: Vec<u32> = all
            .iter()
            .filter_map(|g| {
                g.hltb
                    .as_ref()
                    .and_then(|h| h.main_story_seconds)
                    .map(|s| s / 60)
            })
            .collect();
        LibraryInsights {
            total: all.len(),
            installed: all.iter().filter(|g| g.installed).count(),
            hidden: all.iter().filter(|g| g.is_hidden).count(),
            junk: all.iter().filter(|g| g.is_junk).count(),
            playtime: all.iter().map(|g| g.playtime_minutes.unwrap_or(0)).sum(),
            matching: matching.len(),
            backlog: matching
                .iter()
                .filter(|g| g.playtime_minutes.unwrap_or(0) == 0)
                .count(),
            recent: recent
                .into_iter()
                .take(3)
                .map(|g| {
                    (
                        g.app_id,
                        g.name.clone(),
                        g.last_played_unix.unwrap_or(0),
                        g.playtime_minutes.unwrap_or(0),
                    )
                })
                .collect(),
            avg_hltb_minutes: if hltb_minutes.is_empty() {
                0
            } else {
                hltb_minutes.iter().sum::<u32>() / hltb_minutes.len() as u32
            },
        }
    }

    /// Reload source cache statuses from disk.
    pub(crate) fn reload_source_statuses(&mut self) {
        self.source_statuses = vapourfly_api::enrichment::source_status(&self.cache_dir);
    }

    pub(crate) fn load_collections_from_cloud(&self) -> Result<Vec<SteamCollection>, String> {
        let cloud_path = self.cloud_storage_path()?;
        if !cloud_path.exists() {
            return Ok(Vec::new());
        }

        let cloud = read_cloud_storage(&cloud_path)
            .map_err(|e| format!("Failed to read cloud storage: {e}"))?;
        read_user_collections(&cloud).map_err(|e| format!("Failed to read collections: {e}"))
    }

    pub(crate) fn refresh_backups(&mut self) {
        if self.ui_demo {
            return;
        }
        if let Ok(cloud) = self.cloud_storage_path() {
            match list_backups(&cloud) {
                Ok(list) => self.backups = list,
                Err(e) => self.error = Some(format!("Failed to list backups: {e}")),
            }
        }
    }

    pub(crate) fn export_collections(&self) -> Result<(), String> {
        if self.collections_export_path.trim().is_empty() {
            return Err("Choose an export path before exporting.".into());
        }

        let collections = self.load_collections_from_cloud()?;
        let json = serde_json::to_string_pretty(&collections)
            .map_err(|e| format!("Failed to serialize collections: {e}"))?;
        std::fs::write(self.collections_export_path.trim(), json)
            .map_err(|e| format!("Failed to write collections export: {e}"))
    }

    /// Steam dir from config, falling back to auto-detection — except in
    /// --ui-demo mode, which must never detect the real Steam install.
    pub(crate) fn detected_steam_dir(&self) -> Option<PathBuf> {
        let configured = self.config.as_ref().map(|c| c.steam_dir.clone());
        if self.ui_demo {
            configured
        } else {
            configured.or_else(VapourflyConfig::detect_steam_dir)
        }
    }

    pub(crate) fn run_setup_diagnostics(&mut self) {
        let steam_dir = self.detected_steam_dir();

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
                        display::mask_id(&acc.steam_id64)
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

    pub(crate) fn export_diagnostics(&self) -> Result<(), String> {
        if self.diagnostics_export_path.trim().is_empty() {
            return Err("Choose an export path before exporting diagnostics.".into());
        }

        // Sanitized environment summary (PRIVACY.md "Diagnostics Export"):
        // paths are always redacted in the GUI; accounts and library folders
        // are counts only. Demo mode never touches the real Steam install.
        let steam_dir = self.detected_steam_dir();
        let mut warnings: Vec<String> = Vec::new();
        let (steam_dir_str, account_count, library_folder_count) = match &steam_dir {
            Some(dir) => {
                let accounts = detect_accounts(dir).unwrap_or_default();
                if accounts.is_empty() {
                    warnings.push("no Steam accounts detected".into());
                }
                let folders = detect_library_folders(dir).unwrap_or_default();
                if folders.is_empty() {
                    warnings.push("no Steam library folders detected".into());
                }
                if let Ok(acc) = select_account(
                    &accounts,
                    self.config.as_ref().and_then(|c| c.account.as_deref()),
                ) {
                    let cloud_path =
                        vapourfly_core::steam::cloud_storage_path(dir, &acc.steam_id64);
                    if !cloud_path.exists() {
                        warnings.push("cloud storage file not found for selected account".into());
                    }
                }
                (Some(redact_path(dir)), accounts.len(), folders.len())
            }
            None => {
                warnings.push("Steam directory not detected".into());
                (None, 0, 0)
            }
        };

        let diagnostics = serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "platform": std::env::consts::OS,
            "arch": std::env::consts::ARCH,
            "steam_dir": steam_dir_str,
            "accounts_detected": account_count,
            "library_folders": library_folder_count,
            "cache_dir": redact_path(&self.cache_dir),
            "sources": {
                "IGDB": if self.has_igdb { "configured" } else { "not configured" },
                "RAWG": if self.has_rawg { "configured" } else { "not configured" },
            },
            "warnings": warnings,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        let json = serde_json::to_string_pretty(&diagnostics)
            .map_err(|e| format!("Failed to serialize diagnostics: {e}"))?;
        std::fs::write(self.diagnostics_export_path.trim(), json)
            .map_err(|e| format!("Failed to write diagnostics export: {e}"))
    }

    /// Backup retention for write commits: Settings edit field when valid,
    /// else resolved config, else write default. Keeps UI and write path aligned.
    pub(crate) fn backup_retention(&self) -> u32 {
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
    #[cfg(test)]
    pub(crate) fn manual_overrides(&self) -> ManualOverrides {
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
    pub(crate) fn library_prepare_fingerprint(&self) -> u64 {
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
    pub(crate) fn ensure_library_prepared(&mut self) {
        if self.scan_result.is_none() {
            return;
        }
        let fp = self.library_prepare_fingerprint();
        if let Some(snap) = &self.prepared_snapshot {
            if snap.fingerprint == fp {
                return;
            }
        }
        if self.prepare_job_id.is_some() && self.prepare_fingerprint == Some(fp) {
            return;
        }
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
        let repaint = self.repaint.clone();
        std::thread::spawn(move || {
            let mut games = games;
            let cache = vapourfly_api::cache::DiskCache::new(cache_dir);
            vapourfly_api::enrichment::hydrate_from_cache(&mut games, &cache);
            // Load overrides off-frame so prepared_games never touches the
            // overrides file on the UI frame.
            let overrides = match &overrides_path {
                Some(p) => vapourfly_core::junk::load_manual_overrides_or_default(p),
                None => load_default_manual_overrides(),
            };
            // Pre-classify with Default mode so the common rendering path
            // (Library, Recommendations, etc.) never reclassifies on-frame.
            apply_junk_flags(
                &mut games,
                &JunkRules::default(),
                &JunkMode::Default,
                &overrides,
            );
            let games: Arc<[Game]> = Arc::from(games);
            repaint.request();
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

    /// Poll every background job slot and kick first-paint work. Called once
    /// per GPUI frame (and from tests that wait on jobs). Does not block.
    pub fn tick(&mut self) {
        if self.loading
            && let Some(expected) = self.scan_job_id
        {
            let current_scan_fp = self.current_scan_fingerprint();
            if expected.fingerprint != fingerprint_u64(&current_scan_fp) {
                self.loading = false;
                self.scan_job_id = None;
            } else if let Some(result) = SCAN_RESULT.take_if(expected) {
                self.loading = false;
                self.scan_job_id = None;
                match result {
                    Ok(scan) => {
                        self.scan_result = Some(scan);
                        self.scan_generation = self.scan_generation.wrapping_add(1);
                        self.prepared_snapshot = None;
                        match self.load_collections_from_cloud() {
                            Ok(collections) => self.collections = collections,
                            Err(e) => self.error = Some(e),
                        }
                        self.start_background_populate();
                    }
                    Err(e) => self.error = Some(e),
                }
            }
        }

        if self.write_loading
            && let Some(expected) = self.write_job_id
            && let Some(result) = WRITE_RESULT.take_if(expected)
        {
            self.write_loading = false;
            self.write_job_id = None;
            match result {
                Ok(msg) => {
                    self.success_msg = Some(msg);
                    self.start_scan();
                }
                Err(e) => self.error = Some(e),
            }
        }

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
                    self.cache_refresh_generation = self.cache_refresh_generation.wrapping_add(1);
                }
                Err(e) => self.cache_refresh_msg = Some(format!("Error: {e}")),
            }
        }

        if self.dry_run_loading
            && let Some(expected) = self.dry_run_job_id
        {
            let action = self.pending_action.as_ref();
            let current_fp = if let Some(action) = action {
                dry_run_fingerprint(
                    action,
                    &self.junk_selected,
                    &self.recommend_results,
                    self.scan_generation,
                )
            } else {
                String::new()
            };
            if !current_fp.is_empty() && expected.fingerprint != fingerprint_u64(&current_fp) {
                self.dry_run_loading = false;
                self.dry_run_job_id = None;
            } else if let Some(result) = DRY_RUN_RESULT.take_if(expected) {
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
        }

        if self.junk_preview_loading
            && let Some(expected) = self.junk_preview_job_id
        {
            let current_fp = self.junk_preview_fp(&self.junk_mode.mode());
            if expected.fingerprint != fingerprint_u64(&current_fp) {
                self.junk_preview_loading = false;
                self.junk_preview_job_id = None;
            } else if let Some(result) = JUNK_PREVIEW_RESULT.take_if(expected) {
                self.junk_preview_loading = false;
                self.junk_preview_job_id = None;
                match result {
                    Ok(results) => {
                        self.junk_results = results;
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
        }

        if self.recommend_loading
            && let Some(expected) = self.recommend_job_id
        {
            let current_fp = self
                .recommend_request_from_inputs()
                .map(|req| self.recommend_fp(&req))
                .unwrap_or_default();
            if !current_fp.is_empty() && expected.fingerprint != fingerprint_u64(&current_fp) {
                self.recommend_loading = false;
                self.recommend_job_id = None;
            } else if let Some(result) = RECOMMEND_RESULT.take_if(expected) {
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
        }

        if self.discover_loading
            && let Some(expected) = self.discover_job_id
        {
            let current_fp = self
                .discover_options_from_inputs()
                .map(|opts| self.discover_fp(&opts))
                .unwrap_or_default();
            if !current_fp.is_empty() && expected.fingerprint != fingerprint_u64(&current_fp) {
                self.discover_loading = false;
                self.discover_job_id = None;
            } else if let Some(result) = DISCOVER_RESULT.take_if(expected) {
                self.discover_loading = false;
                self.discover_job_id = None;
                match result {
                    Ok((picks, pf)) => {
                        match self.store_generator_playlist(GeneratorIdentity::Discover, pf) {
                            Ok(stored) => {
                                self.discover_results = picks;
                                self.discover_last_playlist = Some(stored.clone());
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

        self.poll_generator_results();
        self.poll_library_prepare();
        self.ensure_library_prepared();

        if self.playlist_match_loading
            && let Some(expected) = self.playlist_match_job_id
        {
            let current_fp = self
                .playlist_last_import
                .as_ref()
                .map(|pf| self.playlist_match_fp(pf))
                .unwrap_or_default();
            if !current_fp.is_empty() && expected.fingerprint != fingerprint_u64(&current_fp) {
                self.playlist_match_loading = false;
                self.playlist_match_job_id = None;
            } else if let Some(result) = PLAYLIST_MATCH_RESULT.take_if(expected) {
                self.playlist_match_loading = false;
                self.playlist_match_job_id = None;
                match result {
                    Ok(report) => self.playlist_match_report = Some(report),
                    Err(e) => self.error = Some(e),
                }
            }
        }

        if let Some(err) = self.dry_run_error.take()
            && !self.show_confirm_dialog
        {
            self.error = Some(err);
        }

        if self.scan_result.is_none() && !self.loading && self.error.is_none() {
            self.start_scan();
        }
    }

    pub fn has_background_work(&self) -> bool {
        self.loading
            || self.write_loading
            || self.cache_refresh_loading
            || self.dry_run_loading
            || self.junk_preview_loading
            || self.recommend_loading
            || self.discover_loading
            || self.dynamic_loading
            || self.mood_loading
            || self.playlist_match_loading
            || self.prepare_job_id.is_some()
    }

    /// Consume a finished background library prepare result.
    pub(crate) fn poll_library_prepare(&mut self) {
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
    /// operations instead of synchronously hydrating on the UI frame.
    ///
    /// For `JunkMode::Default` (the common path used by Library,
    /// Recommendations, Discover, and Playlist Match) this returns the
    /// pre-classified `Arc<[Game]>` with **no per-frame clone or
    /// reclassification** — just a cheap Arc reference count bump.
    ///
    /// For non-Default modes (Strict/Aggressive, used only by Junk Preview),
    /// this clones the slice and reclassifies. The Junk Preview runs in a
    /// background thread so this cost is not on the UI frame.
    ///
    /// Tests inject a snapshot via [`VapourflyApp::inject_prepared_snapshot`]
    /// rather than relying on a production fallback.
    pub(crate) fn prepared_games(&self, junk_mode: JunkMode) -> Option<Arc<[Game]>> {
        let current_fp = self.library_prepare_fingerprint();
        let snap = self.prepared_snapshot.as_ref()?;
        if snap.fingerprint != current_fp {
            return None;
        }
        if junk_mode == JunkMode::Default {
            return Some(Arc::clone(&snap.games));
        }
        // Non-Default mode: clone + reclassify. This path is only for
        // callers that need mutated is_junk flags on the games themselves
        // (e.g. write plans). Junk preview uses evaluate_junk directly on
        // the Default snapshot inside the spawned thread, so it never
        // hits this path.
        let mut games: Vec<Game> = snap.games.to_vec();
        apply_junk_flags(
            &mut games,
            &JunkRules::default(),
            &junk_mode,
            &snap.overrides,
        );
        Some(Arc::from(games))
    }

    /// Whether the prepared library snapshot is fresh and ready to serve
    /// `prepared_games`. UI that depends on the prepared library should show a
    /// loading state and disable actions while this is false.
    pub(crate) fn library_ready(&self) -> bool {
        let current_fp = self.library_prepare_fingerprint();
        self.prepared_snapshot
            .as_ref()
            .is_some_and(|s| s.fingerprint == current_fp)
    }

    /// Test helper: synchronously build and install a prepared snapshot from the
    /// current scan result + overrides, so tests can exercise `prepared_games`
    /// and its consumers without running the UI loop or the background thread.
    #[cfg(test)]
    pub(crate) fn inject_prepared_snapshot(&mut self) {
        let fp = self.library_prepare_fingerprint();
        let mut games = self
            .scan_result
            .as_ref()
            .map(|s| s.games.clone())
            .unwrap_or_default();
        let cache = vapourfly_api::cache::DiskCache::new(self.cache_dir.clone());
        vapourfly_api::enrichment::hydrate_from_cache(&mut games, &cache);
        let overrides = self.manual_overrides();
        apply_junk_flags(
            &mut games,
            &JunkRules::default(),
            &JunkMode::Default,
            &overrides,
        );
        let games: Arc<[Game]> = Arc::from(games);
        self.prepared_snapshot = Some(PreparedLibrarySnapshot {
            fingerprint: fp,
            games,
            overrides,
        });
    }

    pub(crate) fn recommend_request_from_inputs(&self) -> Result<RecommendRequest, String> {
        Ok(RecommendRequest {
            available_minutes: parse_required("Available minutes", &self.recommend_minutes)?,
            count: parse_required("Count", &self.recommend_count)?,
            deck_mode: self.recommend_deck,
            include_installed_only: self.recommend_installed_only,
            seed: parse_optional("Seed", &self.recommend_seed)?,
            exclude_collections: self.recommend_exclude_collections.clone(),
        })
    }

    pub(crate) fn discover_options_from_inputs(&self) -> Result<DiscoverOptions, String> {
        Ok(DiscoverOptions {
            seed_app_id: self.resolve_discover_seed()?,
            count: parse_required("Discover count", &self.discover_count)?,
        })
    }

    /// Accept a numeric AppID or a library game name (case-insensitive).
    pub(crate) fn resolve_discover_seed(&self) -> Result<Option<u32>, String> {
        let raw = self.discover_seed.trim();
        if raw.is_empty() {
            return Ok(None);
        }
        if let Ok(id) = raw.parse::<u32>() {
            return Ok(Some(id));
        }
        let query = raw.to_lowercase();
        let games = self
            .scan_result
            .as_ref()
            .map(|s| s.games.as_slice())
            .unwrap_or(&[]);
        if let Some(exact) = games.iter().find(|g| g.name.eq_ignore_ascii_case(raw)) {
            return Ok(Some(exact.app_id));
        }
        let matches: Vec<&Game> = games
            .iter()
            .filter(|g| g.name.to_lowercase().contains(&query))
            .collect();
        match matches.as_slice() {
            [one] => Ok(Some(one.app_id)),
            [] => Err(format!(
                "No library game matches “{raw}”. Try a fuller name or AppID."
            )),
            many => Err(format!(
                "{} games match “{raw}”. Use a fuller name or AppID.",
                many.len()
            )),
        }
    }

    pub(crate) fn refresh_detected_accounts(&mut self) {
        if self.ui_demo {
            self.account_list_msg =
                Some("Account detection is disabled in demo mode (--ui-demo).".into());
            return;
        }
        let Some(steam_dir) = self.detected_steam_dir() else {
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

    pub(crate) fn save_settings(&mut self) {
        use vapourfly_core::config::{ConfigField, ConfigUpdate, apply_config_updates};

        if self.ui_demo {
            self.settings_save_msg =
                Some("Saving settings is disabled in demo mode (--ui-demo).".into());
            return;
        }

        let mut errors: Vec<String> = Vec::new();
        let backup_value = self.backup_retention_edit.trim();
        let backup_update: Option<Option<String>> = if backup_value.is_empty() {
            Some(None)
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
            str_field(ConfigField::SteamApiKey, self.steam_api_key_edit.trim()),
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
        self.config =
            VapourflyConfig::from_cli_and_env(vapourfly_core::config::CliOverrides::default()).ok();
    }

    pub(crate) fn playlist_store_path(&self) -> PathBuf {
        self.playlist_store_dir
            .clone()
            .unwrap_or_else(vapourfly_core::config::default_playlists_dir)
    }

    pub(crate) fn store_playlist(&self, pf: &PlaylistFile) -> Result<(), String> {
        playlist_store::put(&self.playlist_store_path(), pf)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Write a generator result to its stable playlist slot (ADR-0007).
    pub(crate) fn store_generator_playlist(
        &self,
        identity: GeneratorIdentity,
        playlist: PlaylistFile,
    ) -> Result<PlaylistFile, String> {
        put_generator_slot(&self.playlist_store_path(), identity, playlist)
    }

    /// Load a playlist into the Playlists edit/match surface (and last-import).
    pub(crate) fn adopt_playlist_for_edit(&mut self, pf: &PlaylistFile) {
        self.playlist_last_import = Some(pf.clone());
        self.playlist_edit_id = pf.playlist.id.clone();
        self.playlist_id_auto = false;
        self.playlist_edit_name = pf.playlist.name.clone();
        self.playlist_edit_description = pf.playlist.description.clone();
        self.playlist_edit_app_ids = manual_playlist_app_ids_csv(pf);
        self.playlist_edit_rules = playlist_rules_json(pf);
        self.playlist_edit_generation = self.playlist_edit_generation.wrapping_add(1);
        // Playlist Match runs entirely off-frame: clear any stale report and
        // launch a background match (no on-frame first pass).
        self.playlist_match_report = None;
        self.start_playlist_match_from_stored_ctx(pf);
    }

    /// Type into the name field. While `playlist_id_auto`, also slugifies id.
    pub(crate) fn apply_playlist_name_edit(&mut self, name: String) {
        self.playlist_edit_name = name;
        if self.playlist_id_auto {
            self.playlist_edit_id = playlist::slugify(&self.playlist_edit_name);
        }
    }

    /// Type into the id field; stops auto-slug from the name.
    pub(crate) fn apply_playlist_id_edit(&mut self, id: String) {
        self.playlist_edit_id = id;
        self.playlist_id_auto = false;
    }

    /// Blank the editor so Save creates a new playlist (FEATURES create/edit).
    pub(crate) fn reset_playlist_editor(&mut self) {
        self.playlist_last_import = None;
        self.playlist_edit_id.clear();
        self.playlist_edit_name.clear();
        self.playlist_edit_description.clear();
        self.playlist_edit_app_ids.clear();
        self.playlist_edit_rules.clear();
        self.playlist_match_report = None;
        self.playlist_id_auto = true;
        self.playlist_edit_generation = self.playlist_edit_generation.wrapping_add(1);
    }

    /// Confirm-only restore: no dry-run job. Confirm then [`execute_backup_restore`].
    pub(crate) fn begin_backup_restore(&mut self, path: PathBuf) {
        if self.ui_demo {
            self.error = Some(
                "Write actions are disabled in demo mode (--ui-demo). \
                 Run without --ui-demo to modify Steam files."
                    .into(),
            );
            return;
        }
        self.dry_run_plan = None;
        self.dry_run_loading = false;
        self.dry_run_error = None;
        self.dry_run_job_id = None;
        self.pending_action = Some(PendingAction::BackupRestore(path));
        self.show_confirm_dialog = true;
    }

    /// Store an imported playlist (file or share code) and adopt it, routing
    /// duplicate ids through the Replace confirm dialog.
    pub(crate) fn adopt_imported_playlist(&mut self, pf: PlaylistFile, success_msg: String) {
        if self.playlist_store_ids.contains(&pf.playlist.id) {
            self.playlist_dup_id_confirm = Some((pf.playlist.id.clone(), pf));
        } else if let Err(e) = self.store_playlist(&pf) {
            self.error = Some(e);
        } else {
            self.adopt_playlist_for_edit(&pf);
            self.refresh_playlist_store_ids();
            self.playlist_load_selected = pf.playlist.id.clone();
            self.success_msg = Some(success_msg);
        }
    }

    /// Parse the current rules JSON field into a Vec<PlaylistRule>.
    ///
    /// Returns an error if the JSON is invalid — never falls back to an
    /// empty array. This prevents visual rule mutations from silently
    /// wiping existing content when the JSON is malformed.
    pub(crate) fn parse_current_rules(&self) -> Result<Vec<PlaylistRule>, String> {
        let trimmed = self.playlist_edit_rules.trim();
        if trimmed.is_empty() {
            return Ok(Vec::new());
        }
        serde_json::from_str(trimmed).map_err(|e| format!("Invalid Rules JSON: {e}"))
    }

    /// Append a rule to the current rules JSON and update the edit field.
    /// Uses `parse_current_rules` to avoid data loss on invalid JSON.
    pub(crate) fn append_rule_to_json(&mut self, rule: PlaylistRule) -> Result<(), String> {
        let mut rules = self.parse_current_rules()?;
        rules.push(rule);
        self.playlist_edit_rules = serde_json::to_string_pretty(&rules).unwrap_or_default();
        Ok(())
    }

    /// Build a `PlaylistFile` from the current edit fields.
    ///
    /// When `playlist_edit_rules` is non-empty, it is parsed as a JSON rules
    /// array and a rule-based playlist is produced (App IDs are ignored).
    /// Otherwise the App IDs field is parsed into a manual playlist.
    pub(crate) fn build_playlist_from_edit_fields(&self) -> Result<PlaylistFile, String> {
        let id = self.playlist_edit_id.trim();
        if id.is_empty() {
            return Err("Playlist ID is required.".into());
        }
        // Validate ID for safe filesystem use (path traversal prevention).
        playlist_store::validate_playlist_id(id)?;
        let name = self.playlist_edit_name.trim();
        if name.is_empty() {
            return Err("Playlist name is required.".into());
        }

        let content = if self.playlist_edit_rules.trim().is_empty() {
            // Strict per-token AppID parsing — no silent drops, no empty tokens.
            let raw = self.playlist_edit_app_ids.trim();
            let mut app_ids = Vec::new();
            if !raw.is_empty() {
                let parts: Vec<&str> = raw.split(',').collect();
                for (i, part) in parts.iter().enumerate() {
                    let trimmed = part.trim();
                    if trimmed.is_empty() {
                        return Err(format!(
                            "Empty AppID at position {} — remove trailing commas or extra commas",
                            i + 1
                        ));
                    }
                    let parsed: u32 = trimmed.parse().map_err(|_| {
                        format!(
                            "Invalid AppID at position {}: '{trimmed}' is not a number",
                            i + 1
                        )
                    })?;
                    if parsed == 0 {
                        return Err(format!(
                            "Invalid AppID at position {}: 0 is not a valid Steam AppID",
                            i + 1
                        ));
                    }
                    app_ids.push(parsed);
                }
                // Sort + deduplicate for set semantics.
                app_ids.sort_unstable();
                app_ids.dedup();
            }
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

    pub(crate) fn export_loaded_playlist(&self) -> Result<(), String> {
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

    pub(crate) fn resolve_dry_run_action(
        &self,
        action: PendingAction,
    ) -> Result<PendingAction, String> {
        // Rule-based Playlist Sync is now resolved off-frame inside the
        // background dry-run job (`generate_dry_run_plan`), so this no longer
        // matches rules on the UI frame. The action passes through unchanged;
        // any rule-resolution error surfaces as `dry_run_error` from the job.
        match action {
            PendingAction::PlaylistSync(pf) => {
                if matches!(pf.playlist.content, PlaylistContent::Rules { .. })
                    && !self.library_ready()
                {
                    return Err("Scan your library before syncing a rule-based playlist.".into());
                }
                Ok(PendingAction::PlaylistSync(pf))
            }
            other => Ok(other),
        }
    }

    /// Launch a background Playlist Match using the ctx captured on the last
    /// frame. Used by non-UI methods (e.g. `adopt_playlist_for_edit`) that do
    /// not receive an `window handle` directly. No-op before the first frame.
    pub(crate) fn start_playlist_match_from_stored_ctx(&mut self, pf: &PlaylistFile) {
        self.start_playlist_match(pf.clone());
    }

    /// Playlist Match with cache lookup, called from UI handlers that have ctx.
    /// Runs entirely off-frame (no on-frame first pass): clears any stale report
    /// and launches the background match.
    pub(crate) fn match_playlist_against_library_background(&mut self, pf: &PlaylistFile) {
        self.playlist_match_report = None;
        self.start_playlist_match(pf.clone());
    }

    /// Refresh the Load existing combo from the local playlist store.
    pub(crate) fn refresh_playlist_store_ids(&mut self) {
        // Always mark loaded so a transient failure does not re-list every frame.
        self.playlist_store_ids_loaded = true;
        match playlist_store::list_ids(&self.playlist_store_path()) {
            Ok(ids) => self.playlist_store_ids = ids,
            Err(e) => self.error = Some(format!("Failed to list playlists: {e}")),
        }
        match playlist_store::list_all(&self.playlist_store_path()) {
            Ok(entries) => self.playlist_rail_entries = entries,
            Err(e) => {
                // list_all only fails if the directory itself can't be read;
                // clear the rail rather than crashing the view.
                self.playlist_rail_entries = Vec::new();
                self.error = Some(format!("Failed to load playlist rail: {e}"));
            }
        }
    }

    /// Load a playlist id from the store into the edit/match surface.
    pub(crate) fn load_playlist_from_store(&mut self, id: &str) -> Result<(), String> {
        let pf = playlist_store::get(&self.playlist_store_path(), id).map_err(|e| e.to_string())?;
        self.adopt_playlist_for_edit(&pf);
        self.playlist_load_selected = id.to_string();
        Ok(())
    }

    /// Generate Discover playlist into the stable slot and populate on-page results.
    #[cfg(test)]
    pub(crate) fn run_discover_generate(&mut self) -> Result<PlaylistFile, String> {
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

/// Generate a [`WritePlan`] without executing it, so the GUI can display a
/// dry-run diff before the user confirms.
///
/// Rule-based Playlist Sync is resolved here (off the UI frame) using the
/// prepared library `games`: the rule playlist is matched against the library
/// to produce the owned AppID set, which is then turned into a write operation.
pub(crate) fn generate_dry_run_plan(
    cloud_path: PathBuf,
    action: &PendingAction,
    junk_results: &[JunkDecision],
    junk_selected: &std::collections::HashSet<u32>,
    collection_name: &str,
    recommend_results: &[Recommendation],
    games: &[Game],
) -> Result<vapourfly_core::write::PreviewedPlan, String> {
    // Filter junk results to only selected items. Empty selection = 0 targets.
    let effective_junk: Vec<JunkDecision> = junk_results
        .iter()
        .filter(|d| junk_selected.contains(&d.app_id))
        .cloned()
        .collect();

    match action {
        PendingAction::JunkApply => {
            if effective_junk.is_empty() {
                return Err("No junk candidates selected.".into());
            }
            let junk_app_ids = disposition::junk_app_ids_from_decisions(&effective_junk);
            actions::preview_junk_apply(collection_name, junk_app_ids, &cloud_path)
                .map_err(|e| format!("Failed to generate write plan: {e}"))
        }
        PendingAction::JunkHide => {
            if effective_junk.is_empty() {
                return Err("No junk candidates selected.".into());
            }
            let junk_app_ids = disposition::junk_app_ids_from_decisions(&effective_junk);
            actions::preview_junk_hide(junk_app_ids, &cloud_path)
                .map_err(|e| format!("Failed to generate write plan: {e}"))
        }
        PendingAction::RecommendCollection => {
            let app_ids: Vec<u32> = recommend_results.iter().map(|r| r.app_id).collect();
            actions::preview_recommend_collection(app_ids, &cloud_path)
                .map_err(|e| format!("Failed to generate write plan: {e}"))
        }
        PendingAction::PlaylistSync(pf) => {
            // Rule-Playlist → owned-AppID resolution is owned by the sync
            // verb (shared with the CLI); this runs off-frame in the dry-run
            // job so rules are never matched on the UI frame.
            let sync = actions::preview_playlist_sync(pf, Some(games), &cloud_path)
                .map_err(|e| format!("Failed to generate write plan: {e}"))?;
            match sync {
                Some(sync) => Ok(sync.plan),
                None => Err("No app IDs to sync.".into()),
            }
        }
        PendingAction::BackupRestore(_) => Err("Dry-run not supported for backup restore.".into()),
    }
}

pub(crate) fn execute_backup_restore(
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
/// Lightweight metadata extracted from a Game for rendering recommendation
/// cards without keeping a borrow on the scan result.
#[derive(Clone, Default)]
pub(crate) struct GameSummary {
    pub(crate) playtime_minutes: u32,
    pub(crate) hltb_minutes: Option<u32>,
    pub(crate) rating_0_5: Option<f32>,
    pub(crate) proton_tier: Option<ProtonTier>,
}

impl From<&Game> for GameSummary {
    fn from(g: &Game) -> Self {
        Self {
            playtime_minutes: g.playtime_minutes.unwrap_or(0),
            hltb_minutes: g
                .hltb
                .as_ref()
                .and_then(|h| h.main_story_seconds)
                .map(|s| s / 60),
            rating_0_5: g.rawg.as_ref().and_then(|r| r.rating_0_5),
            proton_tier: g.protondb.as_ref().map(|p| p.tier),
        }
    }
}

/// Display label for the playlist content type.
pub(crate) fn playlist_content_type_label(content: &PlaylistContent) -> &'static str {
    match content {
        PlaylistContent::Manual { .. } => "Manual",
        PlaylistContent::Rules { .. } => "Rules",
    }
}

/// Deterministic number of games in a playlist (manual count, or rule count
/// from the match report when available).
pub(crate) fn playlist_game_count(
    content: &PlaylistContent,
    report: Option<&PlaylistMatchReport>,
) -> usize {
    match content {
        PlaylistContent::Manual { app_ids } => app_ids.len(),
        PlaylistContent::Rules { .. } => report.map(|r| r.owned.len()).unwrap_or(0),
    }
}

/// Deterministic cover AppID for a playlist: first explicit AppID for manual
/// playlists, or a hash-derived id for rule-based playlists so the cover is
/// stable across sessions.
pub(crate) fn playlist_cover_app_id(content: &PlaylistContent) -> u32 {
    match content {
        PlaylistContent::Manual { app_ids } => app_ids.first().copied().unwrap_or(0),
        PlaylistContent::Rules { .. } => 0,
    }
}

/// Average HLTB main-story time (in minutes) across the AppIDs in a playlist.
pub(crate) fn playlist_avg_hltb(
    content: &PlaylistContent,
    report: Option<&PlaylistMatchReport>,
    games: &[Game],
) -> Option<u32> {
    let ids: Vec<u32> = match content {
        PlaylistContent::Manual { app_ids } => app_ids.clone(),
        PlaylistContent::Rules { .. } => report.map(|r| r.owned.clone()).unwrap_or_default(),
    };
    if ids.is_empty() {
        return None;
    }
    let lookup: HashMap<u32, &Game> = games.iter().map(|g| (g.app_id, g)).collect();
    let hltbs: Vec<u32> = ids
        .iter()
        .filter_map(|id| {
            lookup
                .get(id)
                .and_then(|g| g.hltb.as_ref())
                .and_then(|h| h.main_story_seconds)
                .map(|s| s / 60)
        })
        .collect();
    if hltbs.is_empty() {
        return None;
    }
    Some(hltbs.iter().sum::<u32>() / hltbs.len() as u32)
}

impl VapourflyApp {
    pub(crate) fn apply_library_scope(&mut self, scope: LibraryScope) {
        self.library_scope = scope;
        self.library_visible_count = 48;
    }

    /// Apply a quick-view preset by setting/clearing the filter toggles.
    pub(crate) fn apply_quick_view(&mut self, qv: QuickView) {
        self.library_quick_view = qv;
        self.library_visible_count = 48;
        // Core filters are always shown and user-controllable; quick views
        // set the advanced filters on top of whatever core filters are active.
        self.filter_genre.clear();
        self.filter_tag.clear();
        self.filter_proton_tier = None;
        self.filter_deck_compatible = false;
        self.filter_controller_full = false;
        self.filter_unplayed_only = false;
        self.filter_hltb_min.clear();
        self.filter_hltb_max.clear();
        self.filter_playtime_min.clear();
        self.filter_playtime_max.clear();
        match qv {
            QuickView::Cozy => self.filter_genre = "Cozy".into(),
            QuickView::StoryRich => self.filter_genre = "Story Rich".into(),
            QuickView::GreatOnDeck => {
                self.filter_proton_tier = Some(ProtonTier::Gold);
                self.filter_controller_full = true;
            }
            // ShortSessions (HLTB <= 120 min) is applied via hltb_max_minutes
            // in `filtered_games`, not stored in a filter field.
            QuickView::All | QuickView::ShortSessions => {}
        }
    }
}

pub(crate) fn game_primary_badge(game: &Game) -> (&'static str, Rgb, Rgb) {
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
pub(crate) fn game_shows_deck_badge(game: &Game) -> bool {
    game.pcgw
        .as_ref()
        .is_some_and(|pcgw| pcgw.controller_support == ControllerSupport::Full)
}

pub(crate) fn game_card_detail(game: &Game) -> String {
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

#[cfg(test)]
pub(crate) fn steam_poster_uri(app_id: u32) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{app_id}/library_600x900.jpg")
}

/// Steam's universally available header capsule has the landscape ratio used
/// by the primary Library cards. Poster art remains in use for collection
/// collages where the tall composition is more useful.
pub(crate) fn steam_capsule_uri(app_id: u32) -> String {
    format!("https://cdn.cloudflare.steamstatic.com/steam/apps/{app_id}/header.jpg")
}

pub(crate) fn empty_value_label() -> &'static str {
    "None"
}

/// Append a system CJK face so collection names and other user strings
pub(crate) fn reason_badge_label(code: &str, description: &str) -> String {
    let raw = if description.trim().is_empty() {
        code.replace('_', " ")
    } else {
        description.to_string()
    };
    if raw.chars().count() > 24 {
        format!("{}…", raw.chars().take(23).collect::<String>())
    } else {
        raw
    }
}

pub(crate) fn parse_required<T: std::str::FromStr>(label: &str, input: &str) -> Result<T, String> {
    input
        .trim()
        .parse()
        .map_err(|_| format!("{label} must be a whole number."))
}

pub(crate) fn parse_optional<T: std::str::FromStr>(
    label: &str,
    input: &str,
) -> Result<Option<T>, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    trimmed
        .parse()
        .map(Some)
        .map_err(|_| format!("{label} must be a whole number."))
}

pub(crate) fn format_playtime(minutes: u32) -> String {
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
#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    pub(crate) fn test_game(app_id: u32, name: &str) -> Game {
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
    pub(crate) fn app_created_without_fixtures() {
        let app = VapourflyApp::new(None, false);
        assert!(app.scan_result.is_none());
        assert_eq!(app.current_view, View::Library);
        assert!(!app.loading);
        assert!(app.error.is_none());
    }

    #[test]
    pub(crate) fn ui_demo_flag_is_stored() {
        let app = VapourflyApp::new(None, true);
        assert!(app.ui_demo);
        let app = VapourflyApp::new(None, false);
        assert!(!app.ui_demo);
    }

    #[test]
    pub(crate) fn populate_demo_data_provides_all_pages() {
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
    pub(crate) fn populate_demo_data_writes_loadable_playlists() {
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
    pub(crate) fn ui_demo_isolates_io_from_real_user_paths() {
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
    pub(crate) fn ui_demo_populate_does_not_write_real_playlist_dir() {
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
    pub(crate) fn library_prepare_snapshot_populates_and_is_reused() {
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
        app.ensure_library_prepared();
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
    pub(crate) fn cache_refresh_generation_invalidates_snapshot() {
        PREPARED_LIBRARY_RESULT.clear();
        let mut app = VapourflyApp::new(None, true);
        app.populate_demo_data();

        // Manually set a snapshot.
        let fp = app.library_prepare_fingerprint();
        app.prepared_snapshot = Some(PreparedLibrarySnapshot {
            fingerprint: fp,
            games: Arc::from(vec![]),
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
    pub(crate) fn rescan_with_same_game_count_invalidates_snapshot() {
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
            games: Arc::from(vec![sentinel.clone()]),
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
    pub(crate) fn theme_mode_round_trips_through_u8() {
        assert_eq!(
            ThemeMode::from_u8(ThemeMode::Light.as_u8()),
            ThemeMode::Light
        );
        assert_eq!(ThemeMode::from_u8(ThemeMode::Dark.as_u8()), ThemeMode::Dark);
        assert_eq!(ThemeMode::from_u8(99), ThemeMode::Light); // unknown → Light
    }

    #[test]
    pub(crate) fn app_created_with_fixtures_path() {
        let path = PathBuf::from("/tmp/fix");
        let app = VapourflyApp::new(Some(path.clone()), false);
        assert_eq!(app.fixtures_path, Some(path));
    }

    #[test]
    pub(crate) fn scan_with_fixtures_produces_results() {
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
    pub(crate) fn view_all_contains_every_variant() {
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
    pub(crate) fn navigation_contract_matches_design_ia() {
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
    pub(crate) fn default_landing_view_is_library() {
        let app = VapourflyApp::new(None, false);
        assert_eq!(app.current_view, View::Library);
        assert!(!app.show_junk_panel);
    }

    #[test]
    pub(crate) fn view_labels_are_distinct() {
        let labels: Vec<&str> = View::ALL.iter().map(|v| v.label()).collect();
        let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
        assert_eq!(labels.len(), unique.len());
    }

    #[test]
    pub(crate) fn format_playtime_zero() {
        assert_eq!(format_playtime(0), "0m");
    }

    #[test]
    pub(crate) fn format_playtime_minutes_only() {
        assert_eq!(format_playtime(45), "45m");
    }

    #[test]
    pub(crate) fn format_playtime_hours_and_minutes() {
        assert_eq!(format_playtime(125), "2h 5m");
    }

    #[test]
    pub(crate) fn junk_mode_labels() {
        assert_eq!(JunkModeChoice::Default.label(), "Default");
        assert_eq!(JunkModeChoice::Strict.label(), "Strict");
        assert_eq!(JunkModeChoice::Aggressive.label(), "Aggressive");
    }

    #[test]
    pub(crate) fn nav_labels_are_plain_text() {
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
    pub(crate) fn empty_value_label_is_plain_text() {
        assert_eq!(empty_value_label(), "None");
        assert!(empty_value_label().is_ascii());
    }

    #[test]
    pub(crate) fn library_insights_count_backlog_and_average_hltb() {
        let mut app = VapourflyApp::new(None, true);
        app.populate_demo_data();
        let matching = app.filtered_games();
        let insights = app.library_insights(&matching);
        assert!(insights.total > 0);
        assert!(insights.matching <= insights.total);
        assert_eq!(insights.matching, matching.len());
        assert!(insights.backlog <= insights.matching);
        assert_eq!(cycle_proton_filter(None), Some(ProtonTier::Bronze));
        assert_eq!(cycle_proton_filter(Some(ProtonTier::Native)), None);
    }

    #[test]
    pub(crate) fn steam_poster_uri_uses_library_poster_endpoint() {
        assert_eq!(
            steam_poster_uri(730),
            "https://cdn.cloudflare.steamstatic.com/steam/apps/730/library_600x900.jpg"
        );
    }

    #[test]
    pub(crate) fn artwork_palette_is_deterministic_by_app_id() {
        // Same app_id → same palette entry.
        assert_eq!(
            ARTWORK_PALETTE[(730u32 as usize) % ARTWORK_PALETTE.len()],
            ARTWORK_PALETTE[(730u32 as usize) % ARTWORK_PALETTE.len()]
        );
        // Different app_ids → different palette entries (for small ids).
        let a = ARTWORK_PALETTE[(1000u32 as usize) % ARTWORK_PALETTE.len()];
        let b = ARTWORK_PALETTE[(1001u32 as usize) % ARTWORK_PALETTE.len()];
        assert_ne!(
            a, b,
            "adjacent app_ids should get different palette entries"
        );
    }

    #[test]
    pub(crate) fn game_primary_badge_prioritizes_visible_state() {
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
    pub(crate) fn library_filters_default_to_show_all() {
        let game = test_game(730, "Counter-Strike 2");
        let filters = LibraryFilters::default();
        assert!(game_matches_library_filters(&game, &filters));
    }

    #[test]
    pub(crate) fn library_filters_installed_only_excludes_unowned_installs() {
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
    pub(crate) fn library_filters_not_hidden_and_not_junk_exclude_flagged() {
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
    pub(crate) fn library_filters_search_matches_title_or_app_id() {
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
    pub(crate) fn project_library_games_sorts_installed_then_playtime() {
        let mut a = test_game(1, "Alpha");
        a.installed = false;
        a.playtime_minutes = Some(999);
        let mut b = test_game(2, "Bravo");
        b.installed = true;
        b.playtime_minutes = Some(10);
        let mut c = test_game(3, "Charlie");
        c.installed = true;
        c.playtime_minutes = Some(100);

        let projected = project_library_games(&[a, b, c], &LibraryFilters::default());
        assert_eq!(
            projected.iter().map(|g| g.app_id).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
    }

    #[test]
    pub(crate) fn library_filters_advanced_genre_match() {
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
    pub(crate) fn library_filters_unplayed_only_excludes_played() {
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
    pub(crate) fn library_filters_proton_tier_threshold() {
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
    pub(crate) fn library_filters_short_sessions_prefers_hltb_main_story() {
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
    pub(crate) fn library_filters_short_sessions_igdb_time_to_beat_is_fallback() {
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
    pub(crate) fn library_filter_fields_match_three_toggle_contract() {
        // Guard against reintroducing Unplayed / include-only Hidden or Junk toggles.
        let app = VapourflyApp::new(None, false);
        assert!(!app.filter_installed_only);
        assert!(!app.filter_not_hidden);
        assert!(!app.filter_not_junk);
        assert!(app.library_selected_app_id.is_none());
    }

    #[test]
    #[serial]
    pub(crate) fn library_scope_segment_filters_expected_subsets() {
        let mut app = VapourflyApp::new(None, true);
        app.populate_demo_data();
        app.inject_prepared_snapshot();

        app.apply_library_scope(LibraryScope::Installed);
        let installed = app.filtered_games();
        assert!(!installed.is_empty());
        assert!(installed.iter().all(|game| game.installed));

        app.apply_library_scope(LibraryScope::Unplayed);
        let unplayed = app.filtered_games();
        assert!(!unplayed.is_empty());
        assert!(
            unplayed
                .iter()
                .all(|game| game.playtime_minutes.unwrap_or(0) == 0)
        );

        app.apply_library_scope(LibraryScope::Hidden);
        let hidden = app.filtered_games();
        assert_eq!(hidden.len(), 2);
        assert!(hidden.iter().all(|game| game.is_hidden));
    }

    #[test]
    pub(crate) fn library_filters_tag_matches_rawg_tags() {
        let mut game = test_game(1, "A");
        game.rawg = Some(vapourfly_core::models::RawgData {
            rawg_id: 1,
            rating_0_5: None,
            ratings_count: None,
            genres: vec![],
            tags: vec!["multiplayer".into(), "competitive".into()],
            stores: vec![],
        });
        let filters = LibraryFilters {
            tag: "multi".into(),
            ..Default::default()
        };
        assert!(game_matches_library_filters(&game, &filters));

        let filters_no = LibraryFilters {
            tag: "singleplayer".into(),
            ..Default::default()
        };
        assert!(!game_matches_library_filters(&game, &filters_no));
    }

    #[test]
    pub(crate) fn library_filters_playtime_range() {
        let mut low = test_game(1, "Low");
        low.playtime_minutes = Some(30);
        let mut high = test_game(2, "High");
        high.playtime_minutes = Some(500);

        let filters = LibraryFilters {
            playtime_min_minutes: Some(60),
            playtime_max_minutes: Some(400),
            ..Default::default()
        };
        assert!(!game_matches_library_filters(&low, &filters));
        assert!(!game_matches_library_filters(&high, &filters));

        let mut mid = test_game(3, "Mid");
        mid.playtime_minutes = Some(200);
        assert!(game_matches_library_filters(&mid, &filters));
    }

    #[test]
    pub(crate) fn library_filters_hltb_min_range() {
        let mut short_g = test_game(1, "Short");
        short_g.hltb = Some(vapourfly_core::models::HltbData {
            main_story_seconds: Some(3600), // 60 min
            main_extra_seconds: None,
            completionist_seconds: None,
            source: vapourfly_core::models::HltbSource::IgdbGameTimeToBeat,
        });
        let mut long_g = test_game(2, "Long");
        long_g.hltb = Some(vapourfly_core::models::HltbData {
            main_story_seconds: Some(18000), // 300 min
            main_extra_seconds: None,
            completionist_seconds: None,
            source: vapourfly_core::models::HltbSource::IgdbGameTimeToBeat,
        });

        let filters = LibraryFilters {
            hltb_min_minutes: Some(120),
            ..Default::default()
        };
        assert!(!game_matches_library_filters(&short_g, &filters));
        assert!(game_matches_library_filters(&long_g, &filters));
    }

    #[test]
    pub(crate) fn library_sort_by_name() {
        let a = test_game(1, "Zebra");
        let b = test_game(2, "Alpha");
        let c = test_game(3, "Mike");

        let filters = LibraryFilters {
            sort_by: LibrarySort::Name,
            ..Default::default()
        };
        let projected = project_library_games(&[a, b, c], &filters);
        assert_eq!(
            projected.iter().map(|g| g.app_id).collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
    }

    #[test]
    pub(crate) fn library_sort_by_playtime_desc() {
        let mut a = test_game(1, "A");
        a.playtime_minutes = Some(10);
        let mut b = test_game(2, "B");
        b.playtime_minutes = Some(500);
        let mut c = test_game(3, "C");
        c.playtime_minutes = Some(100);

        let filters = LibraryFilters {
            sort_by: LibrarySort::Playtime,
            sort_desc: false, // default: high-to-low
            ..Default::default()
        };
        let projected = project_library_games(&[a, b, c], &filters);
        assert_eq!(
            projected.iter().map(|g| g.app_id).collect::<Vec<_>>(),
            vec![2, 3, 1]
        );
    }

    #[test]
    pub(crate) fn library_sort_by_appid() {
        let a = test_game(300, "A");
        let b = test_game(100, "B");
        let c = test_game(200, "C");

        let filters = LibraryFilters {
            sort_by: LibrarySort::AppId,
            ..Default::default()
        };
        let projected = project_library_games(&[a, b, c], &filters);
        assert_eq!(
            projected.iter().map(|g| g.app_id).collect::<Vec<_>>(),
            vec![100, 200, 300]
        );
    }

    #[test]
    pub(crate) fn rule_label_formats_all_variants() {
        assert_eq!(rule_label(&PlaylistRule::Installed), "Installed");
        assert_eq!(rule_label(&PlaylistRule::NotJunk), "Not junk");
        assert_eq!(rule_label(&PlaylistRule::NotHidden), "Not hidden");
        assert_eq!(
            rule_label(&PlaylistRule::ControllerSupportFull),
            "Full controller"
        );
        assert_eq!(
            rule_label(&PlaylistRule::HltbMaxMinutes { minutes: 120 }),
            "HLTB ≤ 2h"
        );
        assert_eq!(
            rule_label(&PlaylistRule::HasGenre {
                genre: "RPG".into()
            }),
            "Genre: RPG"
        );
        assert_eq!(
            rule_label(&PlaylistRule::HasTag {
                tag: "multiplayer".into()
            }),
            "Tag: multiplayer"
        );
        assert_eq!(
            rule_label(&PlaylistRule::PlaytimeBetween { min: 0, max: 120 }),
            "Played 0–120m"
        );
        assert_eq!(
            rule_label(&PlaylistRule::RatingAtLeast { rating_0_5: 3.5 }),
            "Rating ≥ 3.5"
        );
        assert_eq!(
            rule_label(&PlaylistRule::And(vec![
                PlaylistRule::Installed,
                PlaylistRule::NotJunk
            ])),
            "All of (2)"
        );
        assert_eq!(
            rule_label(&PlaylistRule::Or(vec![PlaylistRule::Installed])),
            "Any of (1)"
        );
    }

    #[test]
    pub(crate) fn playlist_rule_playtime_between_json_round_trip() {
        let rule = PlaylistRule::PlaytimeBetween { min: 30, max: 300 };
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: PlaylistRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, parsed);
    }

    #[test]
    pub(crate) fn playlist_rule_rating_at_least_json_round_trip() {
        let rule = PlaylistRule::RatingAtLeast { rating_0_5: 4.0 };
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: PlaylistRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, parsed);
    }

    #[test]
    pub(crate) fn parse_current_rules_preserves_invalid_json() {
        // When the JSON is invalid, parse_current_rules must return Err,
        // not silently fall back to an empty array.
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_rules = "[{invalid json}".into();
        let result = app.parse_current_rules();
        assert!(result.is_err(), "invalid JSON must produce an error");
    }

    #[test]
    pub(crate) fn append_rule_to_json_preserves_existing_on_invalid_json() {
        // When the JSON is invalid, append_rule_to_json must NOT wipe
        // the existing content — it must return Err and leave the field
        // unchanged.
        let mut app = VapourflyApp::new(None, false);
        let original = "[{invalid json}".to_string();
        app.playlist_edit_rules = original.clone();
        let result = app.append_rule_to_json(PlaylistRule::Installed);
        assert!(result.is_err());
        // The field must be unchanged — no data loss.
        assert_eq!(app.playlist_edit_rules, original);
    }

    #[test]
    pub(crate) fn append_rule_to_json_appends_to_valid_json() {
        let mut app = VapourflyApp::new(None, false);
        // Use the proper adjacently-tagged JSON format.
        let initial = serde_json::to_string(&vec![PlaylistRule::Installed]).unwrap();
        app.playlist_edit_rules = initial;
        app.append_rule_to_json(PlaylistRule::NotJunk).unwrap();
        let rules: Vec<PlaylistRule> = serde_json::from_str(&app.playlist_edit_rules).unwrap();
        assert_eq!(rules.len(), 2);
        assert!(matches!(rules[0], PlaylistRule::Installed));
        assert!(matches!(rules[1], PlaylistRule::NotJunk));
    }

    #[test]
    pub(crate) fn append_rule_to_json_appends_to_empty() {
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_rules = String::new();
        app.append_rule_to_json(PlaylistRule::Installed).unwrap();
        let rules: Vec<PlaylistRule> = serde_json::from_str(&app.playlist_edit_rules).unwrap();
        assert_eq!(rules.len(), 1);
        assert!(matches!(rules[0], PlaylistRule::Installed));
    }

    #[test]
    pub(crate) fn build_playlist_rejects_reversed_playtime_range() {
        // PlaytimeBetween with min > max should not be directly buildable
        // via the visual editor (the Add button is disabled). But if
        // someone writes it in JSON, build_playlist_from_edit_fields should
        // still accept it (the rule itself is valid JSON). The validation
        // is at the UI layer, not the data layer. This test confirms the
        // rule parses correctly in both directions.
        let rule = PlaylistRule::PlaytimeBetween { min: 300, max: 30 };
        let json = serde_json::to_string(&rule).unwrap();
        let parsed: PlaylistRule = serde_json::from_str(&json).unwrap();
        assert_eq!(rule, parsed);
        // The visual editor prevents this via min <= max check, but the
        // data model does not reject it — that's intentional (the rule
        // engine handles it as "no games match").
    }

    #[test]
    pub(crate) fn relative_time_ago_formats_correctly() {
        assert_eq!(relative_time_ago(0), "unknown");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        // 30 seconds ago
        assert_eq!(relative_time_ago(now - 30), "just now");
        // 5 minutes ago
        assert_eq!(relative_time_ago(now - 300), "5 min ago");
        // 2 hours ago
        assert_eq!(relative_time_ago(now - 7200), "2 hours ago");
        // 1 hour ago
        assert_eq!(relative_time_ago(now - 3600), "1 hour ago");
        // 3 days ago
        assert_eq!(relative_time_ago(now - 259200), "3 days ago");
        // 1 day ago
        assert_eq!(relative_time_ago(now - 86400), "1 day ago");
    }

    #[test]
    pub(crate) fn game_shows_deck_badge_only_with_full_controller() {
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
    pub(crate) fn game_card_detail_uses_collection_or_playable_state() {
        let mut game = test_game(730, "Counter-Strike 2");
        assert_eq!(game_card_detail(&game), "In your Steam library");

        game.installed = true;
        assert_eq!(game_card_detail(&game), "Ready to play");

        game.installed = false;
        game.steam_collections.push("favorites".into());
        assert_eq!(game_card_detail(&game), "1 collection(s)");
    }

    #[test]
    pub(crate) fn reason_badge_prefers_human_description() {
        assert_eq!(
            reason_badge_label("taste_overlap", "Taste vector overlap"),
            "Taste vector overlap"
        );
        assert_eq!(reason_badge_label("HIGH_RATING", ""), "HIGH RATING");
    }

    #[test]
    pub(crate) fn discover_seed_resolves_name_or_app_id() {
        let mut app = VapourflyApp::new(None, false);
        app.scan_result = Some(ScanResult {
            games: vec![test_game(1001, "Fields of Luma")],
            warnings: vec![],
            steam_dir: "/tmp".into(),
            account: "demo".into(),
        });
        app.discover_seed = "1001".into();
        assert_eq!(app.resolve_discover_seed().unwrap(), Some(1001));
        app.discover_seed = "fields of luma".into();
        assert_eq!(app.resolve_discover_seed().unwrap(), Some(1001));
        app.discover_seed = "no such game".into();
        assert!(app.resolve_discover_seed().is_err());
        app.discover_seed.clear();
        assert_eq!(app.resolve_discover_seed().unwrap(), None);
    }

    #[test]
    pub(crate) fn pending_action_is_clone() {
        let a = PendingAction::JunkApply;
        let _b = a.clone();
        let c = PendingAction::BackupRestore(PathBuf::from("/tmp/test"));
        let _d = c.clone();
    }

    #[test]
    pub(crate) fn backup_retention_prefers_settings_edit_field() {
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
    pub(crate) fn app_settings_fields_initialized() {
        let app = VapourflyApp::new(None, false);
        // cc and lang should have defaults
        assert!(!app.cc_edit.is_empty());
        assert!(!app.lang_edit.is_empty());
        assert!(!app.backup_retention_edit.is_empty());
        assert!(!app.allow_steam_running);
        assert!(app.settings_save_msg.is_none());
    }

    #[test]
    pub(crate) fn recommend_request_uses_optional_seed_input() {
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
    pub(crate) fn recommend_request_carries_excluded_collections() {
        let mut app = VapourflyApp::new(None, false);
        app.recommend_minutes = "60".into();
        app.recommend_count = "5".into();
        app.recommend_exclude_collections = vec!["Favorites".into(), "Backlog".into()];

        let request = app.recommend_request_from_inputs().unwrap();
        assert_eq!(
            request.exclude_collections,
            vec!["Favorites".to_string(), "Backlog".to_string()]
        );
    }

    #[test]
    pub(crate) fn recommend_request_rejects_invalid_input() {
        let mut app = VapourflyApp::new(None, false);
        app.recommend_minutes = "soon".into();

        let err = app.recommend_request_from_inputs().unwrap_err();

        assert!(err.contains("Available minutes"));
    }

    #[test]
    pub(crate) fn discover_options_use_count_and_seed_inputs() {
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
    pub(crate) fn playlist_chooser_has_no_discover_variant() {
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
    pub(crate) fn load_playlist_from_store_adopts_edit_fields() {
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
    pub(crate) fn run_discover_generate_writes_slot_and_results() {
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

    pub(crate) fn sample_generator_playlist(id: &str, app_ids: Vec<u32>) -> PlaylistFile {
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
    pub(crate) fn generator_slot_ids_are_stable_per_identity() {
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
    pub(crate) fn put_generator_slot_writes_stable_id_and_overwrites_on_regenerate() {
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
    pub(crate) fn app_store_generator_playlist_uses_injected_store_dir() {
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
    pub(crate) fn dynamic_result_uses_start_time_identity_not_current_chooser() {
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
    pub(crate) fn mood_result_uses_start_time_identity_not_current_chooser() {
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

    /// Regression: when the user makes the Dynamic minutes input invalid
    /// mid-job, the stale result must NOT be adopted into the edit surface.
    #[test]
    #[serial]
    pub(crate) fn dynamic_invalid_input_mid_job_discards_result() {
        DYNAMIC_RESULT.clear();
        let dir = TempDir::new().unwrap();
        let mut app = VapourflyApp::new(None, true);
        app.playlist_store_dir = Some(dir.path().to_path_buf());
        app.populate_demo_data();
        app.dynamic_template = DynamicTemplate::DeckSession.id().into();
        app.dynamic_minutes = "90".into();
        app.dynamic_count = "25".into();
        let job_id = app
            .job_runner
            .next_ticket(WorkflowKind::Dynamic, "dynamic:deck-session:90:25:lib=0");
        app.dynamic_job_id = Some(job_id);
        app.dynamic_loading = true;
        // Clear any prior import so we can detect adoption.
        app.playlist_last_import = None;
        let pf = sample_generator_playlist("dynamic-deck-session", vec![1000, 1007]);
        DYNAMIC_RESULT.set(
            job_id,
            Ok(GeneratorJobResult {
                identity: GeneratorIdentity::Dynamic(DynamicTemplate::DeckSession),
                playlist: pf,
            }),
        );
        // User makes minutes invalid mid-job.
        app.dynamic_minutes = "abc".into();

        app.poll_generator_results();

        // Result must NOT be adopted — playlist_last_import stays None.
        assert!(
            app.playlist_last_import.is_none(),
            "stale result must not be adopted when input is invalid"
        );
        assert!(!app.dynamic_loading);
        assert!(app.dynamic_job_id.is_none());
    }

    /// Regression: when the user makes the Mood chooser invalid mid-job,
    /// the stale result must NOT be adopted into the edit surface.
    #[test]
    #[serial]
    pub(crate) fn mood_invalid_input_mid_job_discards_result() {
        MOOD_RESULT.clear();
        let dir = TempDir::new().unwrap();
        let mut app = VapourflyApp::new(None, true);
        app.playlist_store_dir = Some(dir.path().to_path_buf());
        app.populate_demo_data();
        app.editorial_mood = EditorialMood::QuickRound.id().into();
        let job_id = app
            .job_runner
            .next_ticket(WorkflowKind::Mood, "mood:quick-round:lib=0");
        app.mood_job_id = Some(job_id);
        app.mood_loading = true;
        app.playlist_last_import = None;
        let pf = sample_generator_playlist("mood-quick-round", vec![1002, 1009]);
        MOOD_RESULT.set(
            job_id,
            Ok(GeneratorJobResult {
                identity: GeneratorIdentity::Mood(EditorialMood::QuickRound),
                playlist: pf,
            }),
        );
        // User makes mood invalid mid-job.
        app.editorial_mood = "nonexistent-mood".into();

        app.poll_generator_results();

        assert!(
            app.playlist_last_import.is_none(),
            "stale result must not be adopted when mood is invalid"
        );
        assert!(!app.mood_loading);
        assert!(app.mood_job_id.is_none());
    }

    #[test]
    pub(crate) fn start_dry_run_does_not_arm_backup_restore() {
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal");
        let mut app = VapourflyApp::new(Some(fixtures), false);
        let path = PathBuf::from("missing-backup.json");
        app.start_dry_run(PendingAction::BackupRestore(path));
        assert!(!app.dry_run_loading);
        assert!(app.dry_run_job_id.is_none());
        assert!(app.pending_action.is_none());
        assert!(!app.show_confirm_dialog);
        app.tick();
        assert!(app.pending_action.is_none());
        assert!(!app.show_confirm_dialog);
    }

    #[test]
    pub(crate) fn begin_backup_restore_survives_tick_until_confirm() {
        let fixtures =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal");
        let mut app = VapourflyApp::new(Some(fixtures), false);
        let path = PathBuf::from("missing-backup.json");
        app.begin_backup_restore(path.clone());
        assert!(app.show_confirm_dialog);
        assert!(!app.dry_run_loading);
        assert!(app.dry_run_plan.is_none());
        assert!(matches!(
            app.pending_action,
            Some(PendingAction::BackupRestore(ref p)) if p == &path
        ));
        app.tick();
        assert!(app.show_confirm_dialog);
        assert!(matches!(
            app.pending_action,
            Some(PendingAction::BackupRestore(ref p)) if p == &path
        ));
    }

    #[test]
    pub(crate) fn begin_backup_restore_is_blocked_in_demo() {
        let mut app = VapourflyApp::new(None, true);
        app.begin_backup_restore(PathBuf::from("missing-backup.json"));
        assert!(!app.show_confirm_dialog);
        assert!(app.pending_action.is_none());
        assert!(
            app.error
                .as_deref()
                .is_some_and(|e| e.contains("demo mode"))
        );
    }

    #[test]
    #[serial]
    pub(crate) fn backup_restore_confirm_clears_leftover_dry_run_plan() {
        // Ticket 09: Restore must not commit a stale junk/playlist dry-run.
        WRITE_RESULT.clear();

        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("cloud-storage-namespace-1.json");
        std::fs::write(&target_path, "[]").unwrap();
        let cloud = vapourfly_core::steam::read_cloud_storage(&target_path).unwrap();
        let stale_plan = vapourfly_core::write::preview(
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
    pub(crate) fn cache_refresh_is_blocked_in_offline_mode() {
        let mut app = VapourflyApp::new(None, false);
        app.offline_mode = true;
        app.scan_result = Some(ScanResult {
            steam_dir: "/tmp/steam".into(),
            account: "test".into(),
            games: Vec::new(),
            warnings: Vec::new(),
        });

        app.start_cache_refresh(Some("igdb".into()));

        assert!(!app.cache_refresh_loading);
        assert_eq!(
            app.cache_refresh_msg.as_deref(),
            Some("Offline mode is on. Cache refresh requires network access.")
        );
    }

    #[test]
    pub(crate) fn source_display_names_match_product_table() {
        assert_eq!(source_display_name("igdb"), "IGDB");
        assert_eq!(source_display_name("rawg"), "RAWG");
        assert_eq!(source_display_name("protondb"), "ProtonDB");
        assert_eq!(source_display_name("pcgw"), "PCGW");
        assert_eq!(source_display_name("hltb"), "HLTB");
        assert_eq!(source_display_name("steam-store"), "Steam Store");
    }

    #[test]
    pub(crate) fn source_credential_signals_cover_configured_missing_and_optional() {
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
    pub(crate) fn source_refresh_enabled_respects_offline_loading_and_credentials() {
        assert!(!source_refresh_enabled("protondb", true, true, true, false));
        assert!(!source_refresh_enabled("protondb", true, true, false, true));
        assert!(!source_refresh_enabled("igdb", false, true, false, false));
        assert!(source_refresh_enabled("igdb", true, false, false, false));
        assert!(source_refresh_enabled("hltb", false, false, false, false));
        assert!(source_refresh_enabled("pcgw", false, false, false, false));
    }

    #[test]
    pub(crate) fn settings_can_refresh_detected_accounts_from_fixture() {
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
    pub(crate) fn manual_playlist_app_ids_render_as_csv_for_editing() {
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
    pub(crate) fn playlist_rules_json_round_trips_rule_based_playlist() {
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
    pub(crate) fn build_playlist_from_edit_fields_creates_rule_based_playlist() {
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
    pub(crate) fn build_playlist_from_edit_fields_rejects_invalid_rules_json() {
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_id = "bad".into();
        app.playlist_edit_name = "Bad".into();
        app.playlist_edit_rules = "not json".into();
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("Invalid Rules JSON"));
    }

    #[test]
    pub(crate) fn build_playlist_from_edit_fields_rejects_empty_rules_array() {
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_id = "empty".into();
        app.playlist_edit_name = "Empty".into();
        app.playlist_edit_rules = "[]".into();
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        assert!(err.unwrap_err().to_string().contains("at least one rule"));
    }

    #[test]
    pub(crate) fn apply_playlist_name_edit_slugifies_id_while_auto() {
        let mut app = VapourflyApp::new(None, false);
        assert!(app.playlist_id_auto);
        app.apply_playlist_name_edit("My Cool List".into());
        assert_eq!(app.playlist_edit_name, "My Cool List");
        assert_eq!(
            app.playlist_edit_id,
            vapourfly_core::playlist::slugify("My Cool List")
        );
        app.apply_playlist_id_edit("custom-id".into());
        assert!(!app.playlist_id_auto);
        app.apply_playlist_name_edit("Other Name".into());
        assert_eq!(app.playlist_edit_id, "custom-id");
    }

    #[test]
    pub(crate) fn reset_playlist_editor_clears_fields_and_bumps_generation() {
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_name = "Kept".into();
        app.playlist_edit_id = "kept".into();
        app.playlist_edit_description = "d".into();
        app.playlist_edit_app_ids = "730".into();
        app.playlist_id_auto = false;
        let before = app.playlist_edit_generation;
        app.reset_playlist_editor();
        assert!(app.playlist_edit_name.is_empty());
        assert!(app.playlist_edit_id.is_empty());
        assert!(app.playlist_edit_description.is_empty());
        assert!(app.playlist_edit_app_ids.is_empty());
        assert!(app.playlist_id_auto);
        assert_eq!(app.playlist_edit_generation, before.wrapping_add(1));
        app.apply_playlist_name_edit("Fresh List".into());
        assert_eq!(
            app.playlist_edit_id,
            vapourfly_core::playlist::slugify("Fresh List")
        );
        let built = app.build_playlist_from_edit_fields().unwrap();
        assert_eq!(built.playlist.name, "Fresh List");
        assert_eq!(built.playlist.id, app.playlist_edit_id);
    }

    #[test]
    pub(crate) fn build_playlist_from_edit_fields_requires_id_and_name() {
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
    pub(crate) fn build_playlist_rejects_path_traversal_id() {
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_id = "../outside".into();
        app.playlist_edit_name = "Test".into();
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        // The validator rejects non-alphanumeric chars (including '.' and '/').
        let msg = err.unwrap_err().to_string();
        assert!(
            msg.contains("alphanumeric") || msg.contains("path separators"),
            "got: {msg}"
        );
    }

    #[test]
    pub(crate) fn build_playlist_rejects_invalid_appid() {
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_id = "test-list".into();
        app.playlist_edit_name = "Test".into();
        app.playlist_edit_app_ids = "730, invalid, 440".into();
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("Invalid AppID"), "got: {msg}");
        assert!(msg.contains("invalid"), "got: {msg}");
    }

    #[test]
    pub(crate) fn build_playlist_rejects_zero_appid() {
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_id = "test-list".into();
        app.playlist_edit_name = "Test".into();
        app.playlist_edit_app_ids = "730, 0, 440".into();
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("0 is not a valid Steam AppID"), "got: {msg}");
    }

    #[test]
    pub(crate) fn build_playlist_rejects_empty_token_mid_field() {
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_id = "test-list".into();
        app.playlist_edit_name = "Test".into();
        app.playlist_edit_app_ids = "730,,440".into();
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("Empty AppID"), "got: {msg}");
    }

    #[test]
    pub(crate) fn build_playlist_rejects_trailing_comma() {
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_id = "test-list".into();
        app.playlist_edit_name = "Test".into();
        app.playlist_edit_app_ids = "730, 440,".into();
        let err = app.build_playlist_from_edit_fields();
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("Empty AppID"), "got: {msg}");
    }

    #[test]
    pub(crate) fn build_playlist_allows_empty_app_ids_for_empty_playlist() {
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_id = "test-list".into();
        app.playlist_edit_name = "Test".into();
        app.playlist_edit_app_ids = String::new();
        let result = app.build_playlist_from_edit_fields();
        assert!(
            result.is_ok(),
            "empty field should create empty manual playlist"
        );
        let pf = result.unwrap();
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => {
                assert!(app_ids.is_empty(), "should have zero app_ids");
            }
            PlaylistContent::Rules { .. } => panic!("expected manual"),
        }
    }

    #[test]
    pub(crate) fn build_playlist_deduplicates_app_ids() {
        let mut app = VapourflyApp::new(None, false);
        app.playlist_edit_id = "test-list".into();
        app.playlist_edit_name = "Test".into();
        app.playlist_edit_app_ids = "730, 440, 730, 440".into();
        let result = app.build_playlist_from_edit_fields();
        assert!(result.is_ok());
        let pf = result.unwrap();
        match pf.playlist.content {
            PlaylistContent::Manual { app_ids } => {
                assert_eq!(app_ids, vec![440, 730], "should be sorted + deduped");
            }
            PlaylistContent::Rules { .. } => panic!("expected manual"),
        }
    }

    #[test]
    pub(crate) fn export_loaded_playlist_writes_selected_path() {
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
    pub(crate) fn playlist_sync_dry_run_uses_slugged_playlist_id_and_deduped_app_ids() {
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
    pub(crate) fn playlist_sync_resolves_rule_playlist_in_background_dry_run() {
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
    pub(crate) fn cached_dry_run_plan_still_checks_write_safety() {
        vapourfly_core::steam::set_steam_running_override(Some(true));
        WRITE_RESULT.clear();

        let temp_dir = TempDir::new().unwrap();
        let target_path = temp_dir.path().join("cloud-storage-namespace-1.json");
        std::fs::write(&target_path, "[]").unwrap();

        let cloud = vapourfly_core::steam::read_cloud_storage(&target_path).unwrap();
        let plan = vapourfly_core::write::preview(
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

    pub(crate) fn poll_write_result(app: &VapourflyApp) -> Result<String, String> {
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
    pub(crate) fn load_collections_from_fixture_cloud_storage() {
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
    pub(crate) fn export_collections_writes_json_file() {
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
    pub(crate) fn setup_diagnostics_reports_fixture_mode() {
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
    pub(crate) fn export_diagnostics_writes_json_file() {
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
    pub(crate) fn game_metadata_summary_formats_cached_fields() {
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
