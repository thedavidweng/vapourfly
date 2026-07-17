//! Vapourfly CLI — Steam library manager.
//!
//! Implemented commands: doctor, scan, collections, junk, recommend,
//! playlist, sync, cache, sources, backup, diagnostics.

use std::path::{Path, PathBuf};
use std::process;

use clap::{Parser, Subcommand};

/// Semver from Cargo.toml.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git commit hash, set by build.rs.
const GIT_HASH: &str = env!("VF_GIT_HASH");

/// Build date (UTC), set by build.rs.
const BUILD_DATE: &str = env!("VF_BUILD_DATE");

/// Full version string with git and build metadata.
fn long_version() -> &'static str {
    // Leak a String so we can return &'static from a computed value.
    // This runs at most once.
    static INIT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    INIT.get_or_init(|| format!("{VERSION} ({GIT_HASH}, {BUILD_DATE})"))
        .as_str()
}

mod format;

use vapourfly_core::config::{self, ConfigField, VapourflyConfig};
use vapourfly_core::discover::{self, DiscoverOptions};
use vapourfly_core::disposition;
use vapourfly_core::dynamic::{self, DynamicTemplate, DynamicTemplateOptions};
use vapourfly_core::junk::{JunkPreviewResult, evaluate_junk, load_default_manual_overrides};
use vapourfly_core::models::{
    JunkMode, JunkRules, PlaylistContent, PlaylistFile, PlaylistRule, RecommendRequest, ScanResult,
    VAPOURFLY_JUNK_PREVIEW_SCHEMA, VAPOURFLY_PLAYLIST_SCHEMA, VAPOURFLY_RECOMMENDATIONS_SCHEMA,
    VAPOURFLY_SCAN_SCHEMA, WriteOp,
};
use vapourfly_core::mood::{self, EditorialMood};
use vapourfly_core::playlist;
use vapourfly_core::playlist_store;
use vapourfly_core::recommend;
use vapourfly_core::share_code;
use vapourfly_core::steam;
use vapourfly_core::write;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "vapourfly",
    version,
    long_version = long_version(),
    about = "Manage your Steam library like Spotify playlists"
)]
struct Cli {
    /// Path to a Steam installation fixture (for testing).
    #[arg(long, hide = true, global = true)]
    fixtures: Option<PathBuf>,

    /// Override the Steam installation directory.
    #[arg(long)]
    steam_dir: Option<PathBuf>,

    /// Override the Steam account identifier.
    #[arg(long)]
    account: Option<String>,

    /// Enable verbose output (shows full paths instead of redacted names).
    #[arg(long, global = true)]
    verbose: bool,

    /// Prohibit live network calls; use cache only.
    #[arg(long, global = true)]
    offline: bool,

    /// Allow writing to Steam files even when Steam is detected as running.
    #[arg(long, global = true)]
    allow_steam_running: bool,

    #[command(subcommand)]
    command: Commands,
}

impl Cli {
    /// Resolve the Steam directory from fixtures, CLI flag, or platform detection.
    fn resolve_steam_dir(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        self.fixtures
            .clone()
            .or_else(|| self.steam_dir.clone())
            .or_else(|| steam::detect_steam_dirs(None).into_iter().next())
            .ok_or_else(|| "no Steam directory detected".into())
    }

    /// Resolve the cloud storage path for the selected account.
    fn cloud_storage_path(&self) -> Result<PathBuf, Box<dyn std::error::Error>> {
        let steam_dir = self.resolve_steam_dir()?;
        let accounts = steam::detect_accounts(&steam_dir)?;
        let selected = steam::select_account(&accounts, self.account.as_deref())?;
        Ok(steam::cloud_storage_path(&steam_dir, &selected.steam_id64))
    }
}

/// Check credential status for IGDB and RAWG.
fn credential_status() -> (bool, bool) {
    let igdb = std::env::var("VAPOURFLY_IGDB_CLIENT_ID").is_ok()
        && std::env::var("VAPOURFLY_IGDB_CLIENT_SECRET").is_ok();
    let rawg = std::env::var("VAPOURFLY_RAWG_KEY").is_ok();
    (igdb, rawg)
}

fn cache_refresh_valid_sources() -> Vec<&'static str> {
    vapourfly_api::enrichment::ALL_SOURCES
        .iter()
        .copied()
        .chain(std::iter::once("all"))
        .collect()
}

#[derive(Subcommand)]
enum Commands {
    /// Diagnose Steam installation and configuration.
    Doctor,

    /// List detected Steam accounts.
    Accounts {
        #[command(subcommand)]
        action: AccountsAction,
    },

    /// Scan the Steam library.
    Scan {
        /// Output format.
        #[arg(long, value_enum, default_value = "table")]
        format: OutputFormat,

        /// Enrich scan results with external API data (uses cache, fetches if stale).
        #[arg(long)]
        enrich: bool,
    },

    /// Manage Steam collections.
    Collections {
        #[command(subcommand)]
        action: CollectionsAction,
    },

    /// Junk detection and management.
    Junk {
        #[command(subcommand)]
        action: JunkAction,
    },

    /// Get game recommendations.
    Recommend {
        /// Available play time in minutes.
        #[arg(long)]
        minutes: u32,

        /// Number of recommendations to return.
        #[arg(long, default_value = "5")]
        count: usize,

        /// Optimize for Steam Deck compatibility.
        #[arg(long)]
        deck: bool,

        /// Only recommend installed games.
        #[arg(long)]
        installed_only: bool,

        /// Deterministic seed for reproducible results.
        #[arg(long)]
        seed: Option<u64>,

        /// Write recommendations to a temporary Steam collection.
        #[arg(long)]
        to_collection: bool,

        /// Preview collection write without modifying Steam files.
        #[arg(long)]
        dry_run: bool,

        /// Confirm and write the temporary recommendation collection.
        #[arg(long)]
        confirm: bool,

        /// Output format.
        #[arg(long, value_enum, default_value = "table")]
        format: OutputFormat,
    },

    /// Playlist import/export/match.
    Playlist {
        #[command(subcommand)]
        action: PlaylistAction,
    },

    /// Sync collections to Steam.
    Sync {
        #[command(subcommand)]
        action: SyncAction,
    },

    /// Manage API data cache.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },

    /// Show external data source status.
    Sources {
        #[command(subcommand)]
        action: SourcesAction,
    },

    /// Manage backups.
    Backup {
        #[command(subcommand)]
        action: BackupAction,
    },

    /// Export sanitized diagnostics.
    Diagnostics {
        #[command(subcommand)]
        action: DiagnosticsAction,
    },

    /// Show or edit Vapourfly settings stored in config.toml.
    Settings {
        #[command(subcommand)]
        action: SettingsAction,
    },
}

#[derive(Subcommand)]
enum AccountsAction {
    /// List detected accounts.
    List,
}

#[derive(Subcommand)]
enum CollectionsAction {
    /// List active collections.
    List,

    /// Export collections to a Vapourfly JSON file.
    Export {
        /// Output file path.
        #[arg(long)]
        out: PathBuf,
    },

    /// Compile a dynamic collection template into a playlist.
    Dynamic {
        /// Template name: deck-session, finish-it.
        template: String,

        /// Session length in minutes (deck-session).
        #[arg(long, default_value = "90")]
        minutes: u32,

        /// Output playlist JSON path.
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Compile an Editorial Mood into a playlist (ADR-0004).
    ///
    /// With no mood name, lists the available moods. With a mood name,
    /// compiles it against the current library and stores the resulting
    /// playlist.
    Mood {
        /// Mood id (e.g. "todays-biggest-hits"). Omit to list available moods.
        mood: Option<String>,

        /// Maximum number of games to include.
        #[arg(long, default_value = "25")]
        count: usize,

        /// Output playlist JSON path.
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum JunkAction {
    /// Preview junk candidates with explanations.
    Preview {
        /// Output format.
        #[arg(long, value_enum, default_value = "table")]
        format: OutputFormat,

        /// Require all three signals (playtime + short completion + low rating).
        #[arg(long)]
        strict: bool,

        /// Lower threshold: playtime + any one negative signal.
        #[arg(long)]
        aggressive: bool,
    },

    /// Apply junk classification to a Steam collection.
    Apply {
        /// Collection ID to write.
        #[arg(long)]
        collection: String,

        /// Preview changes without writing.
        #[arg(long)]
        dry_run: bool,

        /// Confirm and execute the write.
        #[arg(long)]
        confirm: bool,
    },

    /// Add junk games to the hidden collection.
    Hide {
        /// Preview changes without writing.
        #[arg(long)]
        dry_run: bool,

        /// Confirm and execute the write.
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
enum PlaylistAction {
    /// Create and store a manual playlist.
    Create {
        /// Playlist ID.
        #[arg(long)]
        id: String,

        /// Playlist display name.
        #[arg(long)]
        name: String,

        /// Optional description.
        #[arg(long, default_value = "")]
        description: String,

        /// Comma-separated Steam AppIDs.
        #[arg(long, value_delimiter = ',')]
        app_ids: Vec<u32>,
    },

    /// Create and store a rule-based playlist from a JSON rules file.
    CreateRules {
        /// Playlist ID.
        #[arg(long)]
        id: String,

        /// Playlist display name.
        #[arg(long)]
        name: String,

        /// Optional description.
        #[arg(long, default_value = "")]
        description: String,

        /// Path to a JSON file containing the rules array (the value of
        /// `content.value.rules` in a Vapourfly playlist file).
        #[arg(long)]
        rules: PathBuf,
    },

    /// Import a playlist from a JSON file or share code.
    Import {
        /// Path to the playlist file.
        path: Option<PathBuf>,

        /// Vapourfly share code (VF1:...).
        #[arg(long)]
        code: Option<String>,
    },

    /// Export a playlist to a JSON file.
    Export {
        /// Playlist ID to export.
        id: String,

        /// Output file path.
        #[arg(long)]
        out: PathBuf,
    },

    /// Emit a share code for a stored playlist.
    Share {
        /// Playlist ID to share.
        id: String,
    },

    /// Generate a Discover playlist from taste similarity.
    Discover {
        /// Optional seed AppID.
        #[arg(long)]
        seed: Option<u32>,

        /// Number of games to include.
        #[arg(long, default_value = "20")]
        count: usize,

        /// Output playlist JSON path.
        #[arg(long)]
        out: Option<PathBuf>,
    },

    /// Match a playlist against the local library.
    Match {
        /// Path to the playlist file.
        path: PathBuf,

        /// Output format.
        #[arg(long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
}

#[derive(Subcommand)]
enum SyncAction {
    /// Sync a playlist or collection to Steam cloud storage.
    Collection {
        /// Playlist or collection ID to sync.
        id: String,

        /// Preview changes without writing.
        #[arg(long)]
        dry_run: bool,

        /// Confirm and execute the write.
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
enum CacheAction {
    /// Refresh cached data from external sources.
    Refresh {
        /// Source to refresh: igdb, rawg, protondb, pcgw, hltb, steam-store, or all.
        #[arg(long)]
        source: String,
    },
}

#[derive(Subcommand)]
enum SourcesAction {
    /// Show status of external data sources.
    Status {
        /// Output format.
        #[arg(long, value_enum, default_value = "table")]
        format: OutputFormat,
    },
}

#[derive(Subcommand)]
enum BackupAction {
    /// List available backups.
    List {
        /// Output format.
        #[arg(long, value_enum, default_value = "table")]
        format: OutputFormat,
    },

    /// Restore a backup.
    Restore {
        /// Path to the backup file.
        file: PathBuf,

        /// Preview the restore without writing.
        #[arg(long)]
        dry_run: bool,

        /// Confirm and execute the restore.
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
enum DiagnosticsAction {
    /// Export sanitized diagnostics.
    Export {
        /// Output file path.
        #[arg(long)]
        out: PathBuf,
    },
}

#[derive(Subcommand)]
enum SettingsAction {
    /// Show the resolved Vapourfly configuration and the config file path.
    Show {
        /// Output format.
        #[arg(long, value_enum, default_value = "table")]
        format: OutputFormat,
    },

    /// Set a config field in config.toml.
    Set {
        /// Field key: steam_dir, account, cc, lang, backup_retention_count.
        key: String,

        /// Value to store.
        value: String,
    },

    /// Remove a config field from config.toml.
    Unset {
        /// Field key: steam_dir, account, cc, lang, backup_retention_count.
        key: String,
    },
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum OutputFormat {
    Table,
    Json,
}

/// Bundled arguments for `cmd_recommend` so the function stays under clippy's
/// argument-count threshold.
#[derive(Clone, Copy, Debug)]
struct RecommendArgs {
    minutes: u32,
    count: usize,
    deck: bool,
    installed_only: bool,
    seed: Option<u64>,
    to_collection: bool,
    dry_run: bool,
    confirm: bool,
    format: OutputFormat,
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let cli = Cli::parse();

    // Set up logging
    let level = if cli.verbose { "debug" } else { "warn" };
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level)),
        )
        .init();

    let result = match &cli.command {
        Commands::Doctor => cmd_doctor(&cli),
        Commands::Accounts { action } => match action {
            AccountsAction::List => cmd_accounts_list(&cli),
        },
        Commands::Scan { format, enrich } => cmd_scan(&cli, *format, *enrich),
        Commands::Collections { action } => match action {
            CollectionsAction::List => cmd_collections_list(&cli),
            CollectionsAction::Export { out } => cmd_collections_export(&cli, out.clone()),
            CollectionsAction::Dynamic {
                template,
                minutes,
                out,
            } => cmd_collections_dynamic(&cli, template.clone(), *minutes, out.clone()),
            CollectionsAction::Mood { mood, count, out } => {
                cmd_collections_mood(&cli, mood.clone(), *count, out.clone())
            }
        },
        Commands::Junk { action } => match action {
            JunkAction::Preview {
                format,
                strict,
                aggressive,
            } => cmd_junk_preview(&cli, *format, *strict, *aggressive),
            JunkAction::Apply {
                collection,
                dry_run,
                confirm,
            } => cmd_junk_apply(&cli, collection.clone(), *dry_run, *confirm),
            JunkAction::Hide { dry_run, confirm } => cmd_junk_hide(&cli, *dry_run, *confirm),
        },
        Commands::Recommend {
            minutes,
            count,
            deck,
            installed_only,
            seed,
            to_collection,
            dry_run,
            confirm,
            format,
        } => cmd_recommend(
            &cli,
            RecommendArgs {
                minutes: *minutes,
                count: *count,
                deck: *deck,
                installed_only: *installed_only,
                seed: *seed,
                to_collection: *to_collection,
                dry_run: *dry_run,
                confirm: *confirm,
                format: *format,
            },
        ),
        Commands::Playlist { action } => match action {
            PlaylistAction::Create {
                id,
                name,
                description,
                app_ids,
            } => cmd_playlist_create(
                id.clone(),
                name.clone(),
                description.clone(),
                app_ids.clone(),
            ),
            PlaylistAction::CreateRules {
                id,
                name,
                description,
                rules,
            } => cmd_playlist_create_rules(
                id.clone(),
                name.clone(),
                description.clone(),
                rules.clone(),
            ),
            PlaylistAction::Import { path, code } => {
                cmd_playlist_import(&cli, path.clone(), code.clone())
            }
            PlaylistAction::Export { id, out } => {
                cmd_playlist_export(&cli, id.clone(), out.clone())
            }
            PlaylistAction::Share { id } => cmd_playlist_share(&cli, id.clone()),
            PlaylistAction::Discover { seed, count, out } => {
                cmd_playlist_discover(&cli, *seed, *count, out.clone())
            }
            PlaylistAction::Match { path, format } => {
                cmd_playlist_match(&cli, path.clone(), *format)
            }
        },
        Commands::Sync { action } => match action {
            SyncAction::Collection {
                id,
                dry_run,
                confirm,
            } => cmd_sync_collection(&cli, id.clone(), *dry_run, *confirm),
        },
        Commands::Cache { action } => match action {
            CacheAction::Refresh { source } => cmd_cache_refresh(&cli, source.clone()),
        },
        Commands::Sources { action } => match action {
            SourcesAction::Status { format } => cmd_sources_status(&cli, *format),
        },
        Commands::Backup { action } => match action {
            BackupAction::List { format } => cmd_backup_list(&cli, *format),
            BackupAction::Restore {
                file,
                dry_run,
                confirm,
            } => cmd_backup_restore(&cli, file.clone(), *dry_run, *confirm),
        },
        Commands::Diagnostics { action } => match action {
            DiagnosticsAction::Export { out } => cmd_diagnostics_export(&cli, out.clone()),
        },
        Commands::Settings { action } => match action {
            SettingsAction::Show { format } => cmd_settings_show(&cli, *format),
            SettingsAction::Set { key, value } => cmd_settings_set(key.clone(), value.clone()),
            SettingsAction::Unset { key } => cmd_settings_unset(key.clone()),
        },
    };

    if let Err(e) = result {
        eprintln!("Error: {e}");
        process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Command implementations
// ---------------------------------------------------------------------------

fn cmd_doctor(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let steam_dir = cli.resolve_steam_dir().ok();

    println!("Vapourfly Doctor");
    println!("================");

    if cli.verbose {
        println!("Version:       {VERSION}");
        println!("Git commit:    {GIT_HASH}");
        println!("Build date:    {BUILD_DATE}");
        println!();
    }

    match &steam_dir {
        Some(dir) => {
            // Steam dir
            if cli.verbose {
                println!("Steam dir:     {}", dir.display());
            } else {
                println!("Steam dir:     {}", steam::redact_path(dir));
            }

            // Accounts
            let accounts = steam::detect_accounts(dir).unwrap_or_default();
            let selected = steam::select_account(&accounts, cli.account.as_deref()).ok();
            println!("Accounts:      {} detected", accounts.len());
            if let Some(acc) = selected {
                if cli.verbose {
                    println!(
                        "Selected:      {} ({}) [{}]",
                        acc.persona_name, acc.account_name, acc.steam_id64
                    );
                } else {
                    println!(
                        "Selected:      {} (***) [{}]",
                        acc.persona_name,
                        format::mask_id(&acc.steam_id64)
                    );
                }
            }

            // Libraries
            let folders = steam::detect_library_folders(dir).unwrap_or_default();
            println!("Libraries:     {}", folders.len());
            if cli.verbose {
                for f in &folders {
                    println!("  - {}", f.display());
                }
            }

            // Cloud storage
            if let Some(acc) = selected {
                let cloud_path = steam::cloud_storage_path(dir, &acc.steam_id64);
                if cloud_path.exists() {
                    println!("Cloud storage: available");
                } else {
                    println!("Cloud storage: not found");
                }
            } else {
                println!("Cloud storage: (no account selected)");
            }

            // Cache root
            let cache_dir = vapourfly_core::config::default_cache_dir();
            if cli.verbose {
                println!("Cache root:    {}", cache_dir.display());
            } else {
                println!("Cache root:    {}", steam::redact_path(&cache_dir));
            }
        }
        None => {
            println!("Steam dir:     (not detected)");
            println!("Hint: pass --steam-dir or set VAPOURFLY_STEAM_DIR");
        }
    }

    // Credential status
    println!();
    println!("Credentials");
    println!("-----------");
    let igdb_ok = std::env::var("VAPOURFLY_IGDB_CLIENT_ID").is_ok()
        && std::env::var("VAPOURFLY_IGDB_CLIENT_SECRET").is_ok();
    let rawg_ok = std::env::var("VAPOURFLY_RAWG_KEY").is_ok();
    println!(
        "IGDB:          {}",
        if igdb_ok {
            "configured"
        } else {
            "not configured"
        }
    );
    println!(
        "RAWG:          {}",
        if rawg_ok {
            "configured"
        } else {
            "not configured"
        }
    );

    if let Some(fixtures) = &cli.fixtures {
        if cli.verbose {
            println!("Fixtures:      {}", fixtures.display());
        } else {
            println!("Fixtures:      enabled");
        }
    }

    Ok(())
}

fn cmd_accounts_list(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let steam_dir = cli.resolve_steam_dir()?;

    let accounts = steam::detect_accounts(&steam_dir)?;
    let selected = steam::select_account(&accounts, cli.account.as_deref()).ok();

    for acc in &accounts {
        let marker = if Some(&acc.steam_id64) == selected.map(|s| &s.steam_id64) {
            " *"
        } else {
            ""
        };
        if cli.verbose {
            println!(
                "{} ({}) [{}]{}",
                acc.persona_name, acc.account_name, acc.steam_id64, marker
            );
        } else {
            println!(
                "{} (***) [{}]{}",
                acc.persona_name,
                format::mask_id(&acc.steam_id64),
                marker
            );
        }
    }
    Ok(())
}

fn cmd_scan(
    cli: &Cli,
    format: OutputFormat,
    enrich: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let steam_dir = cli.resolve_steam_dir()?;

    let opts = steam::ScanOptions {
        steam_dir,
        account: cli.account.clone(),
        fixtures: cli.fixtures.clone(),
    };

    let mut result = steam::scan_library(&opts)?;

    // Enrich with external API data if requested.
    if enrich {
        let cache =
            vapourfly_api::cache::DiskCache::new(vapourfly_core::config::default_cache_dir());
        let options = vapourfly_api::enrichment::EnrichmentOptions {
            sources: vapourfly_api::enrichment::ALL_SOURCES
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            offline: cli.offline,
            force: false,
        };
        let summary = vapourfly_api::enrichment::enrich_games(&mut result.games, &cache, &options);
        tracing::info!(
            processed = summary.games_processed,
            cache_hits = summary.cache_hits,
            network = summary.network_fetches,
            errors = summary.errors.len(),
            "enrichment complete"
        );
    }

    match format {
        OutputFormat::Table => {
            println!(
                "{:<10} {:<40} {:<10} {:<12} {:<12}",
                "AppID", "Name", "Installed", "Playtime", "Collections"
            );
            println!("{}", "-".repeat(86));
            for game in &result.games {
                let installed = if game.installed { "yes" } else { "no" };
                let playtime = game.playtime_minutes.unwrap_or(0);
                let coll_count = game.steam_collections.len();
                println!(
                    "{:<10} {:<40} {:<10} {:<12} {:<12}",
                    game.app_id,
                    format::truncate(&game.name, 38),
                    installed,
                    playtime,
                    coll_count
                );
            }
            println!();
            println!("{} games found", result.games.len());
            if !result.warnings.is_empty() {
                println!("Warnings:");
                for w in &result.warnings {
                    println!("  [{}] {}", w.code, w.message);
                }
            }
        }
        OutputFormat::Json => {
            let mut sorted: Vec<&_> = result.games.iter().collect();
            sorted.sort_by_key(|g| g.app_id);

            let games_json: Vec<serde_json::Value> = sorted
                .iter()
                .map(|g| {
                    let mut entry = serde_json::json!({
                        "app_id": g.app_id,
                        "name": g.name,
                        "installed": g.installed,
                        "playtime_minutes": g.playtime_minutes,
                        "playtime_2wks_minutes": g.playtime_2wks_minutes,
                        "collections": g.steam_collections,
                        "is_hidden": g.is_hidden,
                    });
                    // Include enriched data when present
                    if let Some(protondb) = &g.protondb {
                        entry["protondb"] = serde_json::to_value(protondb).unwrap_or_default();
                    }
                    if let Some(pcgw) = &g.pcgw {
                        entry["pcgw"] = serde_json::to_value(pcgw).unwrap_or_default();
                    }
                    if let Some(hltb) = &g.hltb {
                        entry["hltb"] = serde_json::to_value(hltb).unwrap_or_default();
                    }
                    if let Some(rawg) = &g.rawg {
                        entry["rawg"] = serde_json::to_value(rawg).unwrap_or_default();
                    }
                    if let Some(igdb) = &g.igdb {
                        entry["igdb"] = serde_json::to_value(igdb).unwrap_or_default();
                    }
                    entry
                })
                .collect();

            let output = serde_json::json!({
                "schema": VAPOURFLY_SCAN_SCHEMA,
                "steam_dir": result.steam_dir,
                "account": result.account,
                "games": games_json,
                "warnings": result.warnings,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }
    Ok(())
}

fn cmd_collections_list(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let cloud_path = cli.cloud_storage_path()?;

    if !cloud_path.exists() {
        println!("No cloud storage file found.");
        return Ok(());
    }

    let cloud = steam::read_cloud_storage(&cloud_path)?;
    let collections = steam::read_user_collections(&cloud)?;

    let hidden_count = collections
        .iter()
        .find(|c| c.is_hidden_collection)
        .map_or(0, |c| c.app_ids.len());

    println!("{:<30} {:<10} {:<10}", "Name", "ID", "Apps");
    println!("{}", "-".repeat(55));
    for col in &collections {
        if col.is_hidden_collection {
            continue;
        }
        println!("{:<30} {:<10} {:<10}", col.name, col.id, col.app_ids.len());
    }
    println!();
    println!("Hidden: {hidden_count} apps");
    Ok(())
}

fn cmd_collections_dynamic(
    cli: &Cli,
    template: String,
    minutes: u32,
    out: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let template = DynamicTemplate::parse(&template).ok_or_else(|| {
        format!("unknown dynamic template '{template}'. Expected one of: deck-session, finish-it")
    })?;

    let scan_result = scan_library_hydrated(cli, JunkMode::Default)?;
    let pf = dynamic::compile_dynamic_template(
        template,
        &scan_result.games,
        &DynamicTemplateOptions {
            session_minutes: minutes,
            count: 25,
        },
    );

    store_playlist(&pf)?;
    println!("Compiled dynamic template: {}", template.label());
    println!("  Playlist ID: {}", pf.playlist.id);
    println!("  Name:        {}", pf.playlist.name);
    match &pf.playlist.content {
        PlaylistContent::Manual { app_ids } => println!("  Games:       {}", app_ids.len()),
        PlaylistContent::Rules { rules } => println!("  Rules:       {}", rules.len()),
    }

    if let Some(out) = out {
        playlist::export_playlist(&pf, &out)?;
        println!("  Exported to {}", out.display());
    }

    Ok(())
}

fn cmd_collections_mood(
    cli: &Cli,
    mood: Option<String>,
    count: usize,
    out: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    // No mood name: list available moods.
    let Some(mood_name) = mood else {
        println!("Editorial Moods:");
        for m in EditorialMood::all() {
            println!("  {:<22} {}", m.id(), m.name());
            println!("  {:<22} {}", "", m.description());
        }
        println!();
        println!("Compile one with: vapourfly collections mood <id>");
        return Ok(());
    };

    let mood = EditorialMood::parse(&mood_name).ok_or_else(|| {
        format!(
            "unknown editorial mood '{mood_name}'. Expected one of: {}",
            EditorialMood::all()
                .iter()
                .map(|m| m.id())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;

    let scan_result = scan_library_hydrated(cli, JunkMode::Default)?;
    let pf = mood::compile_editorial_mood(mood, &scan_result.games, count);

    store_playlist(&pf)?;
    println!("Compiled editorial mood: {}", mood.name());
    println!("  Playlist ID: {}", pf.playlist.id);
    println!("  Name:        {}", pf.playlist.name);
    println!("  Description: {}", pf.playlist.description);
    match &pf.playlist.content {
        PlaylistContent::Manual { app_ids } => println!("  Games:       {}", app_ids.len()),
        PlaylistContent::Rules { rules } => println!("  Rules:       {}", rules.len()),
    }

    if let Some(out) = out {
        playlist::export_playlist(&pf, &out)?;
        println!("  Exported to {}", out.display());
    }

    Ok(())
}

fn cmd_collections_export(cli: &Cli, out: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let cloud_path = cli.cloud_storage_path()?;
    let cloud = steam::read_cloud_storage(&cloud_path)?;
    let collections = steam::read_user_collections(&cloud)?;

    let json = serde_json::to_string_pretty(&collections)?;
    std::fs::write(&out, json)?;
    println!(
        "Exported {} collections to {}",
        collections.len(),
        out.display()
    );
    Ok(())
}

fn scan_library_hydrated(
    cli: &Cli,
    junk_mode: JunkMode,
) -> Result<ScanResult, Box<dyn std::error::Error>> {
    let steam_dir = cli.resolve_steam_dir()?;
    let result = vapourfly_api::workflow::prepare(&vapourfly_api::workflow::WorkflowOptions {
        steam_dir,
        account: cli.account.clone(),
        fixtures: cli.fixtures.clone(),
        junk_mode,
        offline: cli.offline,
    })?;
    Ok(result)
}

/// Run a playlist match with Steam Store details for missing entries,
/// so `completion_price` reflects the corrected semantics (missing
/// non-free entries only).
///
/// Uses the shared [`vapourfly_api::enrichment::resolve_missing_store_details`]
/// API: in online mode it fetches uncached missing AppIDs via SteamStoreClient
/// and writes them to cache; in offline mode it uses cache-only lookups.
fn match_playlist_with_missing(
    pf: &PlaylistFile,
    games: &[vapourfly_core::models::Game],
    offline: bool,
) -> Result<vapourfly_core::models::PlaylistMatchReport, Box<dyn std::error::Error>> {
    use std::collections::HashMap;
    // First pass: find missing AppIDs with no store details.
    let empty: HashMap<u32, vapourfly_core::models::SteamStoreDetails> = HashMap::new();
    let preliminary = playlist::match_playlist(pf, games, &empty)?;
    // Resolve store details for missing entries (online fetch + cache).
    let cache = vapourfly_api::cache::DiskCache::new(vapourfly_core::config::default_cache_dir());
    let cfg = vapourfly_core::config::VapourflyConfig::from_cli_and_env(Default::default())?;
    let missing_details = vapourfly_api::enrichment::resolve_missing_store_details(
        &preliminary.missing,
        &cache,
        offline,
        &cfg.cc,
        &cfg.lang,
    );
    // Second pass with store details for completion price.
    let report = playlist::match_playlist(pf, games, &missing_details)?;
    Ok(report)
}

fn cmd_junk_preview(
    cli: &Cli,
    format: OutputFormat,
    strict: bool,
    aggressive: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if strict && aggressive {
        return Err("cannot specify both --strict and --aggressive".into());
    }

    let mode = if strict {
        JunkMode::Strict
    } else if aggressive {
        JunkMode::Aggressive
    } else {
        JunkMode::Default
    };

    let scan_result = scan_library_hydrated(cli, mode.clone())?;

    // Print scan warnings to stderr
    for w in &scan_result.warnings {
        eprintln!("warning: [{}] {}", w.code, w.message);
    }

    let rules = JunkRules::default();
    // Same overrides as workflow::prepare — never wipe with empty defaults.
    let overrides = load_default_manual_overrides();
    let decisions = evaluate_junk(&scan_result.games, &rules, &mode, &overrides);

    match format {
        OutputFormat::Table => {
            println!(
                "{:<10} {:<32} {:>10} {:>10}  Classification",
                "AppID", "Name", "Playtime", "Confidence"
            );
            println!("{}", "-".repeat(86));

            for (game, decision) in scan_result.games.iter().zip(decisions.iter()) {
                let playtime = game
                    .playtime_minutes
                    .map_or_else(|| "N/A".into(), |m| format!("{m} min"));

                let confidence = format!("{}%", (decision.confidence * 100.0) as u32);

                let classification = if decision.is_junk {
                    let reasons: Vec<String> = decision
                        .matched
                        .iter()
                        .map(format::format_junk_signal)
                        .collect();
                    format!("junk \u{2014} {}", reasons.join(", "))
                } else {
                    "ok".into()
                };

                println!(
                    "{:<10} {:<32} {:>10} {:>10}  {}",
                    game.app_id,
                    format::truncate(&game.name, 30),
                    playtime,
                    confidence,
                    classification
                );
            }

            let junk_count = decisions.iter().filter(|d| d.is_junk).count();
            println!();
            println!(
                "{} games scanned, {} junk candidates (mode: {})",
                decisions.len(),
                junk_count,
                format::format_junk_mode(&mode)
            );
        }
        OutputFormat::Json => {
            let result = JunkPreviewResult {
                schema: VAPOURFLY_JUNK_PREVIEW_SCHEMA.to_string(),
                decisions,
                rules,
                mode,
            };
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }

    Ok(())
}

fn cmd_junk_apply(
    cli: &Cli,
    collection: String,
    dry_run: bool,
    confirm: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_write_flags(dry_run, confirm)?;

    let cloud_path = cli.cloud_storage_path()?;
    let scan_result = scan_library_hydrated(cli, JunkMode::Default)?;

    let junk_app_ids = disposition::junk_app_ids_from_games(&scan_result.games);
    if junk_app_ids.is_empty() {
        println!("No junk candidates found.");
        return Ok(());
    }

    let cloud = steam::read_cloud_storage(&cloud_path)?;
    let op = disposition::junk_apply(&collection, junk_app_ids.clone())?;
    let plan = write::preview(&cloud, vec![op], cloud_path.clone())?;

    println!("Junk Apply");
    println!("==========");
    println!("Collection: {collection}");
    println!("Junk games: {}", junk_app_ids.len());
    println!();

    println!("Diff:");
    for change in &plan.diff.collections_changed {
        println!("  Collection '{}': {}", change.id, change.action);
    }
    if !plan.diff.app_ids_added.is_empty() {
        println!("  App IDs to add: {}", plan.diff.app_ids_added.len());
    }
    if !plan.diff.app_ids_removed.is_empty() {
        println!("  App IDs to remove: {}", plan.diff.app_ids_removed.len());
    }
    println!("  Unchanged entries: {}", plan.diff.unchanged_count);

    if dry_run {
        println!();
        println!("Dry run complete. No changes made.");
    } else {
        let backup =
            write::commit_with_retention(&plan, cli.allow_steam_running, backup_retention())?;
        println!();
        println!("Write complete.");
        println!("  Backup: {}", backup.display());
    }

    Ok(())
}

fn cmd_junk_hide(
    cli: &Cli,
    dry_run: bool,
    confirm: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_write_flags(dry_run, confirm)?;

    let cloud_path = cli.cloud_storage_path()?;
    let scan_result = scan_library_hydrated(cli, JunkMode::Default)?;

    let junk_app_ids = disposition::junk_app_ids_from_games(&scan_result.games);
    if junk_app_ids.is_empty() {
        println!("No junk candidates found.");
        return Ok(());
    }

    let cloud = steam::read_cloud_storage(&cloud_path)?;
    let op = disposition::junk_hide(junk_app_ids.clone())?;
    let plan = write::preview(&cloud, vec![op], cloud_path.clone())?;

    println!("Junk Hide");
    println!("=========");
    println!("Junk games: {}", junk_app_ids.len());
    println!();

    println!("Diff:");
    if !plan.diff.hidden_app_ids_added.is_empty() {
        println!(
            "  Hidden app IDs to add: {}",
            plan.diff.hidden_app_ids_added.len()
        );
    }
    println!("  Unchanged entries: {}", plan.diff.unchanged_count);

    if dry_run {
        println!();
        println!("Dry run complete. No changes made.");
    } else {
        let backup =
            write::commit_with_retention(&plan, cli.allow_steam_running, backup_retention())?;
        println!();
        println!("Write complete.");
        println!("  Backup: {}", backup.display());
    }

    Ok(())
}

fn cmd_recommend(cli: &Cli, args: RecommendArgs) -> Result<(), Box<dyn std::error::Error>> {
    let RecommendArgs {
        minutes,
        count,
        deck,
        installed_only,
        seed,
        to_collection,
        dry_run,
        confirm,
        format,
    } = args;

    if to_collection {
        validate_write_flags(dry_run, confirm)?;
    }

    let scan_result = scan_library_hydrated(cli, JunkMode::Default)?;

    let request = RecommendRequest {
        available_minutes: minutes,
        count,
        deck_mode: deck,
        include_installed_only: installed_only,
        seed,
        exclude_collections: vec![],
    };

    let recommendations = recommend::recommend(&scan_result.games, &request);

    if to_collection {
        let app_ids: Vec<u32> = recommendations.iter().map(|r| r.app_id).collect();
        if app_ids.is_empty() {
            println!("No recommendations to write.");
            return Ok(());
        }

        let cloud_path = cli.cloud_storage_path()?;
        let cloud = steam::read_cloud_storage(&cloud_path)?;
        let op = disposition::recommend_to_collection(app_ids.clone())?;
        let plan = write::preview(&cloud, vec![op], cloud_path.clone())?;

        println!("Temporary recommendation collection");
        println!("===============================");
        println!("Collection ID: {}", disposition::RECOMMEND_COLLECTION_ID);
        println!("Games:         {}", app_ids.len());
        println!();
        println!("Diff:");
        for change in &plan.diff.collections_changed {
            println!("  Collection '{}': {}", change.id, change.action);
        }
        if !plan.diff.app_ids_added.is_empty() {
            println!("  App IDs to add: {}", plan.diff.app_ids_added.len());
        }
        println!("  Unchanged entries: {}", plan.diff.unchanged_count);

        if dry_run {
            println!();
            println!("Dry run complete. No changes made.");
            return Ok(());
        }

        let backup =
            write::commit_with_retention(&plan, cli.allow_steam_running, backup_retention())?;
        println!();
        println!("Write complete.");
        println!("  Backup: {}", backup.display());
        return Ok(());
    }

    match format {
        OutputFormat::Table => {
            println!("{:<10} {:<40} {:>8}  Reasons", "AppID", "Name", "Score");
            println!("{}", "-".repeat(86));
            for rec in &recommendations {
                let reasons: Vec<String> = rec.reasons.iter().map(|r| r.code.clone()).collect();
                println!(
                    "{:<10} {:<40} {:>8.2}  {}",
                    rec.app_id,
                    format::truncate(&rec.name, 38),
                    rec.score,
                    reasons.join(", ")
                );
            }
            println!();
            println!(
                "{} recommendations ({} games scanned)",
                recommendations.len(),
                scan_result.games.len()
            );
        }
        OutputFormat::Json => {
            let result = serde_json::json!({
                "schema": VAPOURFLY_RECOMMENDATIONS_SCHEMA,
                "request": {
                    "available_minutes": minutes,
                    "count": count,
                    "deck_mode": deck,
                    "installed_only": installed_only,
                    "seed": seed,
                },
                "recommendations": recommendations,
            });
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
    }
    Ok(())
}

fn cmd_playlist_create(
    id: String,
    name: String,
    description: String,
    app_ids: Vec<u32>,
) -> Result<(), Box<dyn std::error::Error>> {
    let pf = PlaylistFile {
        vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
        created_by: "user".into(),
        playlist: vapourfly_core::models::Playlist {
            id,
            name,
            description,
            content: PlaylistContent::Manual { app_ids },
        },
    };

    store_playlist(&pf)?;
    println!("Created playlist: {}", pf.playlist.name);
    println!("  ID: {}", pf.playlist.id);
    Ok(())
}

fn cmd_playlist_create_rules(
    id: String,
    name: String,
    description: String,
    rules_path: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let pf = build_rule_playlist_from_file(&id, &name, &description, &rules_path)?;
    store_playlist(&pf)?;
    println!("Created rule-based playlist: {}", pf.playlist.name);
    println!("  ID: {}", pf.playlist.id);
    match &pf.playlist.content {
        PlaylistContent::Rules { rules } => println!("  Rules: {}", rules.len()),
        PlaylistContent::Manual { .. } => {}
    }
    Ok(())
}

/// Parse a rules file and build a `PlaylistFile` for a rule-based playlist.
///
/// `rules_path` may point at either a bare JSON rules array (`[...]`) or a
/// full Vapourfly playlist JSON file (rules are extracted from
/// `content.value.rules`). The playlist is built with the given `id`, `name`,
/// and `description`; it is not persisted.
fn build_rule_playlist_from_file(
    id: &str,
    name: &str,
    description: &str,
    rules_path: &Path,
) -> Result<PlaylistFile, Box<dyn std::error::Error>> {
    let contents = std::fs::read_to_string(rules_path)
        .map_err(|_| format!("could not read rules file: {}", rules_path.display()))?;

    let rules = parse_rules_file_contents(&contents)?;
    let pf = PlaylistFile {
        vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
        created_by: "user".into(),
        playlist: vapourfly_core::models::Playlist {
            id: id.to_string(),
            name: name.to_string(),
            description: description.to_string(),
            content: PlaylistContent::Rules { rules },
        },
    };
    Ok(pf)
}

/// Parse rules from either a bare JSON rules array or a full playlist file.
fn parse_rules_file_contents(
    contents: &str,
) -> Result<Vec<PlaylistRule>, Box<dyn std::error::Error>> {
    // Accept either a bare rules array `[...]` or a full playlist file
    // (which contains `content.value.rules`).
    let rules: Vec<PlaylistRule> = if contents.trim_start().starts_with('[') {
        serde_json::from_str(contents)
            .map_err(|e| format!("rules file is not a valid JSON array of rules: {e}"))?
    } else {
        let pf: PlaylistFile = serde_json::from_str(contents)
            .map_err(|e| format!("rules file is neither a rules array nor a playlist file: {e}"))?;
        match pf.playlist.content {
            PlaylistContent::Rules { rules } => rules,
            PlaylistContent::Manual { .. } => {
                return Err("rules file is a manual playlist, not a rule-based playlist".into());
            }
        }
    };

    if rules.is_empty() {
        return Err("rules file contains no rules".into());
    }
    Ok(rules)
}

/// Persist a playlist to an explicit store directory.
///
/// This is the storage primitive used by [`store_playlist`] (which writes to
/// the platform playlist store) and by tests that need to target a temporary
/// directory.
fn cmd_playlist_import(
    cli: &Cli,
    path: Option<PathBuf>,
    code: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let pf = if let Some(code) = code {
        share_code::decode_share_code(&code)?
    } else if let Some(path) = path {
        playlist::import_playlist(&path)?
    } else {
        return Err("must specify a playlist file path or --code".into());
    };

    store_playlist(&pf)?;
    let stored_path = playlist_store_dir().join(format!("{}.json", pf.playlist.id));

    println!("Imported playlist: {}", pf.playlist.name);
    println!("  ID: {}", pf.playlist.id);
    if !pf.playlist.description.is_empty() {
        println!("  Description: {}", pf.playlist.description);
    }
    match &pf.playlist.content {
        PlaylistContent::Manual { app_ids } => {
            println!("  Type: manual ({} apps)", app_ids.len());
        }
        PlaylistContent::Rules { rules } => {
            println!("  Type: rules ({} rules)", rules.len());
        }
    }

    let scan_result = scan_library_hydrated(cli, JunkMode::Default)?;
    let report = match_playlist_with_missing(&pf, &scan_result.games, cli.offline)?;

    println!();
    println!("Match summary:");
    println!("  Owned:    {}", report.owned.len());
    println!("  Missing:  {}", report.missing.len());
    println!("  Played:   {}", report.played.len());
    println!("  Unplayed: {}", report.unplayed.len());
    println!("  Hidden:   {}", report.hidden.len());
    println!("  Junk:     {}", report.junk.len());
    match &report.completion_price {
        Some(price) => println!("  Completion price: {}", price.format()),
        None => println!(
            "  Completion price: (unavailable — missing entries may be free, unpriced, or not cached)"
        ),
    }
    println!();
    println!("Stored to {}", stored_path.display());
    Ok(())
}

fn cmd_playlist_share(_cli: &Cli, id: String) -> Result<(), Box<dyn std::error::Error>> {
    let pf = load_stored_playlist(&id)?;
    let code = share_code::encode_share_code(&pf)?;
    println!("Share code for '{}':", pf.playlist.name);
    println!("{code}");
    Ok(())
}

fn cmd_playlist_discover(
    cli: &Cli,
    seed: Option<u32>,
    count: usize,
    out: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let scan_result = scan_library_hydrated(cli, JunkMode::Default)?;
    let pf = discover::generate_discover_playlist(
        &scan_result.games,
        &DiscoverOptions {
            seed_app_id: seed,
            count,
        },
    );

    store_playlist(&pf)?;
    println!("Generated Discover playlist: {}", pf.playlist.name);
    println!("  ID: {}", pf.playlist.id);
    match &pf.playlist.content {
        PlaylistContent::Manual { app_ids } => println!("  Games: {}", app_ids.len()),
        PlaylistContent::Rules { .. } => {}
    }

    if let Some(out) = out {
        playlist::export_playlist(&pf, &out)?;
        println!("  Exported to {}", out.display());
    }

    Ok(())
}

fn cmd_playlist_export(
    _cli: &Cli,
    id: String,
    out: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let pf = load_stored_playlist(&id)?;
    playlist::export_playlist(&pf, &out)?;
    println!(
        "Exported playlist '{}' to {}",
        pf.playlist.name,
        out.display()
    );
    Ok(())
}

fn cmd_playlist_match(
    cli: &Cli,
    path: PathBuf,
    format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let pf = playlist::import_playlist(&path)?;
    let scan_result = scan_library_hydrated(cli, JunkMode::Default)?;
    let report = match_playlist_with_missing(&pf, &scan_result.games, cli.offline)?;

    match format {
        OutputFormat::Table => {
            println!("Playlist: {}", pf.playlist.name);
            println!("  ID:       {}", pf.playlist.id);
            println!();
            println!("Match report:");
            println!("  Owned:    {}", report.owned.len());
            println!("  Missing:  {}", report.missing.len());
            println!("  Played:   {}", report.played.len());
            println!("  Unplayed: {}", report.unplayed.len());
            println!("  Hidden:   {}", report.hidden.len());
            println!("  Junk:     {}", report.junk.len());
            match &report.completion_price {
                Some(price) => println!("  Completion price: {}", price.format()),
                None => println!(
                    "  Completion price: (unavailable — missing entries may be free, unpriced, or not cached)"
                ),
            }
        }
        OutputFormat::Json => {
            let output = serde_json::json!({
                "schema": VAPOURFLY_PLAYLIST_SCHEMA,
                "playlist_id": pf.playlist.id,
                "playlist_name": pf.playlist.name,
                "report": report,
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }
    Ok(())
}

fn cmd_sync_collection(
    cli: &Cli,
    id: String,
    dry_run: bool,
    confirm: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_write_flags(dry_run, confirm)?;

    // Load the stored playlist to get app IDs to sync.
    let pf = load_stored_playlist(&id)?;

    let resolved_owned = match &pf.playlist.content {
        PlaylistContent::Manual { .. } => None,
        PlaylistContent::Rules { .. } => {
            let scan_result = scan_library_hydrated(cli, JunkMode::Default)?;
            let report = playlist::match_playlist(
                &pf,
                &scan_result.games,
                &std::collections::HashMap::new(),
            )?;
            Some(report.owned)
        }
    };
    let app_ids = disposition::playlist_sync_app_ids(&pf, resolved_owned)?;
    if app_ids.is_empty() {
        println!("No app IDs to sync.");
        return Ok(());
    }

    let cloud_path = cli.cloud_storage_path()?;
    let cloud = steam::read_cloud_storage(&cloud_path)?;
    let op = disposition::playlist_sync(&pf, app_ids.clone())?;
    let collection_id = match &op {
        WriteOp::UpsertCollection { id, .. } => id.clone(),
        _ => playlist::slugify(&pf.playlist.id),
    };
    let plan = write::preview(&cloud, vec![op], cloud_path.clone())?;

    println!("Sync playlist '{}' to Steam collection", pf.playlist.name);
    println!("  Playlist ID:   {}", pf.playlist.id);
    println!("  Collection ID: {collection_id}");
    println!("  App IDs:       {}", app_ids.len());
    println!("  Target:        {}", cloud_path.display());
    println!();
    println!("Diff:");
    for change in &plan.diff.collections_changed {
        println!("  Collection '{}': {}", change.id, change.action);
    }
    if !plan.diff.app_ids_added.is_empty() {
        println!("  App IDs to add: {}", plan.diff.app_ids_added.len());
    }
    if !plan.diff.app_ids_removed.is_empty() {
        println!("  App IDs to remove: {}", plan.diff.app_ids_removed.len());
    }
    println!("  Unchanged entries: {}", plan.diff.unchanged_count);

    if dry_run {
        println!();
        println!("Dry run complete. No changes made.");
    } else {
        let backup =
            write::commit_with_retention(&plan, cli.allow_steam_running, backup_retention())?;
        println!();
        println!("Write complete.");
        println!("  Backup: {}", backup.display());
    }

    Ok(())
}

fn cmd_cache_refresh(cli: &Cli, source: String) -> Result<(), Box<dyn std::error::Error>> {
    let valid_sources = cache_refresh_valid_sources();
    if !valid_sources.contains(&source.as_str()) {
        return Err(format!(
            "Invalid source '{}'. Must be one of: {}",
            source,
            valid_sources.join(", ")
        )
        .into());
    }

    if cli.offline {
        return Err("Cannot refresh cache in offline mode.".into());
    }

    let steam_dir = cli.resolve_steam_dir()?;
    let scan_result = steam::scan_library(&steam::ScanOptions {
        steam_dir,
        account: cli.account.clone(),
        fixtures: cli.fixtures.clone(),
    })?;

    let cache = vapourfly_api::cache::DiskCache::new(vapourfly_core::config::default_cache_dir());

    let sources = if source == "all" {
        vapourfly_api::enrichment::ALL_SOURCES
            .iter()
            .map(|s| (*s).to_string())
            .collect()
    } else {
        vec![source.clone()]
    };

    let options = vapourfly_api::enrichment::EnrichmentOptions {
        sources,
        offline: false,
        force: true,
    };

    let mut games = scan_result.games;
    let summary = vapourfly_api::enrichment::enrich_games(&mut games, &cache, &options);

    println!("Cache refresh complete (source: {source})");
    println!("  Games processed: {}", summary.games_processed);
    println!("  Network fetches: {}", summary.network_fetches);
    println!("  Cache hits:      {}", summary.cache_hits);
    println!("  Errors:          {}", summary.errors.len());

    for stat in &summary.source_stats {
        println!(
            "  {}: {} refreshed, {} skipped, {} errors",
            stat.source, stat.entries_refreshed, stat.entries_skipped, stat.errors
        );
    }

    if !summary.errors.is_empty() {
        println!();
        println!("Errors (first 10):");
        for err in summary.errors.iter().take(10) {
            println!("  [{}:{}] {}", err.source, err.app_id, err.message);
        }
    }

    Ok(())
}

fn cmd_sources_status(_cli: &Cli, format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let (igdb_configured, rawg_configured) = credential_status();
    let cache_root = vapourfly_core::config::default_cache_dir();
    let statuses = vapourfly_api::enrichment::source_status(&cache_root);

    match format {
        OutputFormat::Table => {
            println!(
                "{:<15} {:<15} {:<15} {:<8} {:<8} {:<10}",
                "Source", "Credentials", "Last Success", "Entries", "Stale", "Cached"
            );
            println!("{}", "-".repeat(75));
            for s in &statuses {
                let cred = match s.name.as_str() {
                    "igdb" => {
                        if igdb_configured {
                            "configured"
                        } else {
                            "missing"
                        }
                    }
                    "rawg" => {
                        if rawg_configured {
                            "configured"
                        } else {
                            "missing"
                        }
                    }
                    _ => "not required",
                };
                let last = s.last_success.map_or_else(
                    || "n/a".into(),
                    |dt| dt.format("%Y-%m-%d %H:%M").to_string(),
                );
                let cached = if s.cache_dir_exists { "yes" } else { "no" };
                println!(
                    "{:<15} {:<15} {:<15} {:<8} {:<8} {:<10}",
                    s.name, cred, last, s.cache_entries, s.stale_entries, cached
                );
            }
        }
        OutputFormat::Json => {
            let json_sources: Vec<serde_json::Value> = statuses
                .iter()
                .map(|s| {
                    let cred = match s.name.as_str() {
                        "igdb" => {
                            if igdb_configured {
                                "configured"
                            } else {
                                "missing"
                            }
                        }
                        "rawg" => {
                            if rawg_configured {
                                "configured"
                            } else {
                                "missing"
                            }
                        }
                        _ => "not required",
                    };
                    serde_json::json!({
                        "name": s.name,
                        "credentials": cred,
                        "last_success": s.last_success,
                        "cache_entries": s.cache_entries,
                        "stale_entries": s.stale_entries,
                        "cache_dir_exists": s.cache_dir_exists,
                    })
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string_pretty(&serde_json::json!({ "sources": json_sources }))?
            );
        }
    }
    Ok(())
}

fn cmd_backup_list(cli: &Cli, format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let cloud_path = cli.cloud_storage_path()?;
    let backups = steam::list_backups(&cloud_path)?;

    match format {
        OutputFormat::Table => {
            if backups.is_empty() {
                println!("No backups found.");
            } else {
                println!("{:<50} {:<20} {:<12}", "Path", "Created", "SHA256");
                println!("{}", "-".repeat(85));
                for backup in &backups {
                    let name = backup
                        .path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    println!(
                        "{:<50} {:<20} {:<12}",
                        name,
                        backup.created_at.format("%Y%m%dT%H%M%SZ"),
                        &backup.sha256[..8]
                    );
                }
            }
        }
        OutputFormat::Json => {
            let json_backups: Vec<serde_json::Value> = backups
                .iter()
                .map(|b| {
                    serde_json::json!({
                        "path": b.path.display().to_string(),
                        "created_at": b.created_at,
                        "sha256": b.sha256,
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_backups)?);
        }
    }
    Ok(())
}

fn cmd_backup_restore(
    cli: &Cli,
    file: PathBuf,
    dry_run: bool,
    confirm: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_write_flags(dry_run, confirm)?;
    let cloud_path = cli.cloud_storage_path()?;

    if dry_run {
        // Preview: show what would be restored without writing.
        let backup_bytes = std::fs::read(&file)
            .map_err(|e| format!("failed to read backup file {}: {e}", file.display()))?;
        let current_bytes = std::fs::read(&cloud_path).unwrap_or_default();
        let backup_hash = steam::compute_sha256(&backup_bytes);
        let current_hash = steam::compute_sha256(&current_bytes);
        println!("Dry run: restore backup");
        println!("  Backup:  {}", file.display());
        println!("  Target:  {}", cloud_path.display());
        println!("  Backup SHA-256:   {backup_hash}");
        println!("  Current SHA-256:  {current_hash}");
        if backup_hash == current_hash {
            println!("  (backup and current target are identical — no change)");
        }
        println!();
        println!("Dry run complete. No changes made.");
        return Ok(());
    }

    steam::check_write_safety(&cloud_path, cli.allow_steam_running)?;
    steam::restore_backup(&file, &cloud_path)?;
    println!(
        "Restored backup {} to {}",
        file.display(),
        cloud_path.display()
    );
    Ok(())
}

fn cmd_diagnostics_export(_cli: &Cli, out: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let (igdb_configured, rawg_configured) = credential_status();

    let diagnostics = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "platform": std::env::consts::OS,
        "arch": std::env::consts::ARCH,
        "sources": {
            "IGDB": if igdb_configured { "configured" } else { "not configured" },
            "RAWG": if rawg_configured { "configured" } else { "not configured" },
        },
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });

    let json = serde_json::to_string_pretty(&diagnostics)?;
    std::fs::write(&out, json)?;
    println!("Diagnostics exported to {}", out.display());
    Ok(())
}

// ---------------------------------------------------------------------------
// Settings command
// ---------------------------------------------------------------------------

fn cmd_settings_show(cli: &Cli, format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let overrides = config::CliOverrides {
        steam_dir: cli.steam_dir.clone(),
        account: cli.account.clone(),
    };
    let cfg = VapourflyConfig::from_cli_and_env(overrides)?;
    let config_path = config::config_file_path();

    match format {
        OutputFormat::Table => {
            println!("Vapourfly Settings");
            println!("==================");
            if let Some(path) = &config_path {
                println!("Config file:    {}", path.display());
            } else {
                println!("Config file:    (platform config dir not available)");
            }
            println!();
            println!("{:<22} {}", "Steam dir:", cfg.steam_dir.display());
            println!(
                "{:<22} {}",
                "Account:",
                cfg.account.as_deref().unwrap_or("(auto)"),
            );
            println!("{:<22} {}", "Store country code:", cfg.cc);
            println!("{:<22} {}", "Store language:", cfg.lang);
            println!("{:<22} {}", "Backup retention:", cfg.backup_retention_count,);
            println!();
            println!("Credentials");
            println!("-----------");
            println!(
                "{:<22} {}",
                "IGDB:",
                if cfg.has_igdb_credentials {
                    "configured"
                } else {
                    "not configured"
                },
            );
            println!(
                "{:<22} {}",
                "RAWG:",
                if cfg.has_rawg_credentials {
                    "configured"
                } else {
                    "not configured"
                },
            );
            println!();
            println!("Settable fields: steam_dir, account, cc, lang, backup_retention_count");
            println!("Use: vapourfly settings set <field> <value>");
            println!("     vapourfly settings unset <field>");
        }
        OutputFormat::Json => {
            let json = serde_json::json!({
                "config_file": config_path.map(|p| p.display().to_string()),
                "steam_dir": cfg.steam_dir.display().to_string(),
                "account": cfg.account,
                "cc": cfg.cc,
                "lang": cfg.lang,
                "backup_retention_count": cfg.backup_retention_count,
                "has_igdb_credentials": cfg.has_igdb_credentials,
                "has_rawg_credentials": cfg.has_rawg_credentials,
            });
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
    }
    Ok(())
}

fn cmd_settings_set(key: String, value: String) -> Result<(), Box<dyn std::error::Error>> {
    let field = ConfigField::from_key(&key).ok_or_else(|| {
        format!(
            "unknown field '{key}'. Valid fields: {}",
            ConfigField::all()
                .iter()
                .map(|f| f.as_key())
                .collect::<Vec<_>>()
                .join(", "),
        )
    })?;

    let normalised = field
        .normalise(&value)
        .map_err(|e| format!("invalid value for {key}: {e}"))?;
    config::set_config_field(field, &normalised)?;

    let path = config::config_file_path();
    println!("Set {key} = {normalised}");
    if let Some(path) = path {
        println!("  Written to {}", path.display());
    }
    Ok(())
}

fn cmd_settings_unset(key: String) -> Result<(), Box<dyn std::error::Error>> {
    let field = ConfigField::from_key(&key).ok_or_else(|| {
        format!(
            "unknown field '{key}'. Valid fields: {}",
            ConfigField::all()
                .iter()
                .map(|f| f.as_key())
                .collect::<Vec<_>>()
                .join(", "),
        )
    })?;

    config::unset_config_field(field)?;

    let path = config::config_file_path();
    println!("Unset {key}");
    if let Some(path) = path {
        println!("  Written to {}", path.display());
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return the directory where imported playlists are stored.
fn playlist_store_dir() -> PathBuf {
    vapourfly_core::config::default_playlists_dir()
}

/// Persist a playlist to the local playlist store.
fn store_playlist(pf: &PlaylistFile) -> Result<(), Box<dyn std::error::Error>> {
    playlist_store::put(&playlist_store_dir(), pf)?;
    Ok(())
}

/// Persist a playlist under an explicit store root (tests).
#[cfg(test)]
fn store_playlist_at(
    pf: &PlaylistFile,
    store_dir: &std::path::Path,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(playlist_store::put(store_dir, pf)?)
}

/// Load a stored playlist by ID from the local playlist store.
fn load_stored_playlist(id: &str) -> Result<PlaylistFile, Box<dyn std::error::Error>> {
    match playlist_store::get(&playlist_store_dir(), id) {
        Ok(pf) => Ok(pf),
        Err(_) => Err(format!(
            "playlist '{id}' not found. Import it first with: vapourfly playlist import <file>"
        )
        .into()),
    }
}

/// Backup retention from config when available, else write default.
fn backup_retention() -> u32 {
    vapourfly_core::config::VapourflyConfig::from_cli_and_env(Default::default())
        .map(|c| c.backup_retention_count)
        .unwrap_or(vapourfly_core::write::DEFAULT_BACKUP_RETENTION)
}

/// Validate that exactly one of `--dry-run` / `--confirm` is set.
fn validate_write_flags(dry_run: bool, confirm: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !dry_run && !confirm {
        return Err("must specify either --dry-run or --confirm".into());
    }
    if dry_run && confirm {
        return Err("cannot specify both --dry-run and --confirm".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_refresh_accepts_every_enrichment_source() {
        let valid_sources = cache_refresh_valid_sources();

        for source in vapourfly_api::enrichment::ALL_SOURCES {
            assert!(valid_sources.contains(source), "missing source: {source}");
        }
        assert!(valid_sources.contains(&"all"));
    }

    #[test]
    fn settings_set_rejects_unknown_field() {
        let result = cmd_settings_set("unknown_field".into(), "value".into());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("unknown field"), "got: {msg}");
    }

    #[test]
    fn settings_unset_rejects_unknown_field() {
        let result = cmd_settings_unset("unknown_field".into());
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("unknown field"), "got: {msg}");
    }

    #[test]
    fn settings_set_rejects_invalid_backup_retention() {
        let result = cmd_settings_set("backup_retention_count".into(), "not-a-number".into());
        assert!(result.is_err());
    }

    #[test]
    fn playlist_create_rules_reads_bare_rules_array() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rules_path = tmp.path().join("rules.json");
        std::fs::write(&rules_path, r#"[{"op":"Installed"},{"op":"NotHidden"}]"#).unwrap();

        let pf = build_rule_playlist_from_file("test-id", "Test", "", &rules_path).unwrap();
        assert_eq!(pf.playlist.id, "test-id");
        assert_eq!(pf.playlist.name, "Test");
        match &pf.playlist.content {
            PlaylistContent::Rules { rules } => {
                assert_eq!(rules.len(), 2);
                assert_eq!(rules[0], PlaylistRule::Installed);
                assert_eq!(rules[1], PlaylistRule::NotHidden);
            }
            PlaylistContent::Manual { .. } => panic!("expected rule-based playlist"),
        }

        // Verify the playlist can be persisted and re-read.
        let store_dir = tmp.path().join("store");
        let stored_path = store_playlist_at(&pf, &store_dir).unwrap();
        assert!(stored_path.exists());
        let reloaded = playlist::import_playlist(&stored_path).unwrap();
        assert_eq!(reloaded.playlist.id, "test-id");
    }

    #[test]
    fn playlist_create_rules_reads_full_playlist_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rules_path = tmp.path().join("playlist.json");
        std::fs::write(
            &rules_path,
            r#"{
                "vapourfly_schema": "vapourfly.playlist.v1",
                "created_by": "user",
                "playlist": {
                    "id": "test",
                    "name": "Test",
                    "description": "",
                    "content": {
                        "type": "Rules",
                        "value": {
                            "rules": [{"op": "Installed"}]
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        let pf = build_rule_playlist_from_file("new-id", "New Name", "desc", &rules_path).unwrap();
        // The id/name/description come from the arguments, not the file.
        assert_eq!(pf.playlist.id, "new-id");
        assert_eq!(pf.playlist.name, "New Name");
        assert_eq!(pf.playlist.description, "desc");
        match &pf.playlist.content {
            PlaylistContent::Rules { rules } => assert_eq!(rules.len(), 1),
            PlaylistContent::Manual { .. } => panic!("expected rule-based playlist"),
        }
    }

    #[test]
    fn playlist_create_rules_rejects_empty_rules_array() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rules_path = tmp.path().join("rules.json");
        std::fs::write(&rules_path, "[]").unwrap();

        let result = build_rule_playlist_from_file("id", "Name", "", &rules_path);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("no rules"), "got: {msg}");
    }

    #[test]
    fn playlist_create_rules_rejects_manual_playlist_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rules_path = tmp.path().join("playlist.json");
        std::fs::write(
            &rules_path,
            r#"{
                "vapourfly_schema": "vapourfly.playlist.v1",
                "created_by": "user",
                "playlist": {
                    "id": "test",
                    "name": "Test",
                    "description": "",
                    "content": {
                        "type": "Manual",
                        "value": {
                            "app_ids": [10, 20]
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        let result = build_rule_playlist_from_file("id", "Name", "", &rules_path);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("manual playlist"), "got: {msg}");
    }

    #[test]
    fn playlist_create_rules_rejects_unparseable_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rules_path = tmp.path().join("rules.json");
        std::fs::write(&rules_path, "not json at all").unwrap();

        let result = build_rule_playlist_from_file("id", "Name", "", &rules_path);
        assert!(result.is_err());
    }

    #[test]
    fn playlist_create_rules_rejects_missing_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let rules_path = tmp.path().join("nonexistent.json");

        let result = build_rule_playlist_from_file("id", "Name", "", &rules_path);
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("could not read"), "got: {msg}");
    }
}
