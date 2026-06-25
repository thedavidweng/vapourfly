//! Vapourfly CLI — Steam library manager.
//!
//! Implemented commands: doctor, scan, collections, junk, recommend,
//! playlist, sync, cache, sources, backup, diagnostics.

use std::path::PathBuf;
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

use vapourfly_core::junk::{JunkPreviewResult, ManualOverrides, evaluate_junk};
use vapourfly_core::models::{
    JunkMode, JunkRules, JunkSignal, PlaylistContent, PlaylistFile, RecommendRequest,
    VAPOURFLY_JUNK_PREVIEW_SCHEMA, VAPOURFLY_PLAYLIST_SCHEMA, VAPOURFLY_RECOMMENDATIONS_SCHEMA,
    VAPOURFLY_SCAN_SCHEMA, WriteOp,
};
use vapourfly_core::playlist;
use vapourfly_core::recommend;
use vapourfly_core::steam;

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

    /// Path to a custom config file.
    #[arg(long)]
    config: Option<PathBuf>,

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
        Ok(steam_dir
            .join("userdata")
            .join(&selected.steam_id64)
            .join("config/cloudstorage/cloud-storage-namespace-1.json"))
    }
}

/// Check credential status for IGDB and RAWG.
fn credential_status() -> (bool, bool) {
    let igdb = std::env::var("VAPOURFLY_IGDB_CLIENT_ID").is_ok()
        && std::env::var("VAPOURFLY_IGDB_CLIENT_SECRET").is_ok();
    let rawg = std::env::var("VAPOURFLY_RAWG_KEY").is_ok();
    (igdb, rawg)
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
    /// Import a playlist from a JSON file.
    Import {
        /// Path to the playlist file.
        path: PathBuf,
    },

    /// Export a playlist to a JSON file.
    Export {
        /// Playlist ID to export.
        id: String,

        /// Output file path.
        #[arg(long)]
        out: PathBuf,
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
        /// Source to refresh: igdb, rawg, protondb, pcgw, hltb, or all.
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

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum OutputFormat {
    Table,
    Json,
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
            format,
        } => cmd_recommend(
            &cli,
            *minutes,
            *count,
            *deck,
            *installed_only,
            *seed,
            *format,
        ),
        Commands::Playlist { action } => match action {
            PlaylistAction::Import { path } => cmd_playlist_import(&cli, path.clone()),
            PlaylistAction::Export { id, out } => {
                cmd_playlist_export(&cli, id.clone(), out.clone())
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
            BackupAction::Restore { file } => cmd_backup_restore(&cli, file.clone()),
        },
        Commands::Diagnostics { action } => match action {
            DiagnosticsAction::Export { out } => cmd_diagnostics_export(&cli, out.clone()),
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
                        mask_id(&acc.steam_id64)
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
                let cloud_path = dir
                    .join("userdata")
                    .join(&acc.steam_id64)
                    .join("config/cloudstorage/cloud-storage-namespace-1.json");
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
                mask_id(&acc.steam_id64),
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
                .map(|s| s.to_string())
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
                    truncate(&game.name, 38),
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
        .map(|c| c.app_ids.len())
        .unwrap_or(0);

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

    let steam_dir = cli.resolve_steam_dir()?;

    let scan_result = steam::scan_library(&steam::ScanOptions {
        steam_dir,
        account: cli.account.clone(),
        fixtures: cli.fixtures.clone(),
    })?;

    // Print scan warnings to stderr
    for w in &scan_result.warnings {
        eprintln!("warning: [{}] {}", w.code, w.message);
    }

    let rules = JunkRules::default();
    let overrides = ManualOverrides::default();
    let decisions = evaluate_junk(&scan_result.games, &rules, &mode, &overrides);

    match format {
        OutputFormat::Table => {
            println!(
                "{:<10} {:<32} {:>10} {:>10}  Classification",
                "App ID", "Name", "Playtime", "Confidence"
            );
            println!("{}", "-".repeat(86));

            for (game, decision) in scan_result.games.iter().zip(decisions.iter()) {
                let playtime = game
                    .playtime_minutes
                    .map(|m| format!("{m} min"))
                    .unwrap_or_else(|| "N/A".into());

                let confidence = format!("{}%", (decision.confidence * 100.0) as u32);

                let classification = if decision.is_junk {
                    let reasons: Vec<String> =
                        decision.matched.iter().map(format_junk_signal).collect();
                    format!("junk \u{2014} {}", reasons.join(", "))
                } else {
                    "ok".into()
                };

                println!(
                    "{:<10} {:<32} {:>10} {:>10}  {}",
                    game.app_id,
                    truncate(&game.name, 30),
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
                format_junk_mode(&mode)
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
    let steam_dir = cli.resolve_steam_dir()?;

    let scan_result = steam::scan_library(&steam::ScanOptions {
        steam_dir,
        account: cli.account.clone(),
        fixtures: cli.fixtures.clone(),
    })?;

    let rules = JunkRules::default();
    let overrides = ManualOverrides::default();
    let decisions = evaluate_junk(&scan_result.games, &rules, &JunkMode::Default, &overrides);

    let mut junk_app_ids: Vec<u32> = decisions
        .iter()
        .filter(|d| d.is_junk)
        .map(|d| d.app_id)
        .collect();
    junk_app_ids.sort();
    junk_app_ids.dedup();

    if junk_app_ids.is_empty() {
        println!("No junk candidates found.");
        return Ok(());
    }

    let cloud = steam::read_cloud_storage(&cloud_path)?;
    let plan = steam::generate_write_plan(
        &cloud,
        vec![WriteOp::UpsertCollection {
            id: collection.clone(),
            added: junk_app_ids.clone(),
            removed: vec![],
        }],
        cloud_path.clone(),
    )?;

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
        steam::check_write_safety(&cloud_path, cli.allow_steam_running)?;
        steam::execute_write_plan(&plan, 5)?;
        println!();
        println!("Write complete.");
        println!("  Backup: {}", plan.backup_path.display());
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
    let steam_dir = cli.resolve_steam_dir()?;

    let scan_result = steam::scan_library(&steam::ScanOptions {
        steam_dir,
        account: cli.account.clone(),
        fixtures: cli.fixtures.clone(),
    })?;

    let rules = JunkRules::default();
    let overrides = ManualOverrides::default();
    let decisions = evaluate_junk(&scan_result.games, &rules, &JunkMode::Default, &overrides);

    let mut junk_app_ids: Vec<u32> = decisions
        .iter()
        .filter(|d| d.is_junk)
        .map(|d| d.app_id)
        .collect();
    junk_app_ids.sort();
    junk_app_ids.dedup();

    if junk_app_ids.is_empty() {
        println!("No junk candidates found.");
        return Ok(());
    }

    let cloud = steam::read_cloud_storage(&cloud_path)?;
    let plan = steam::generate_write_plan(
        &cloud,
        vec![WriteOp::AddToHidden {
            app_ids: junk_app_ids.clone(),
        }],
        cloud_path.clone(),
    )?;

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
        steam::check_write_safety(&cloud_path, cli.allow_steam_running)?;
        steam::execute_write_plan(&plan, 5)?;
        println!();
        println!("Write complete.");
        println!("  Backup: {}", plan.backup_path.display());
    }

    Ok(())
}

fn cmd_recommend(
    cli: &Cli,
    minutes: u32,
    count: usize,
    deck: bool,
    installed_only: bool,
    seed: Option<u64>,
    format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    let steam_dir = cli.resolve_steam_dir()?;

    let scan_result = steam::scan_library(&steam::ScanOptions {
        steam_dir,
        account: cli.account.clone(),
        fixtures: cli.fixtures.clone(),
    })?;

    let request = RecommendRequest {
        available_minutes: minutes,
        count,
        deck_mode: deck,
        include_installed_only: installed_only,
        seed,
        exclude_collections: vec![],
    };

    let recommendations = recommend::recommend(&scan_result.games, &request);

    match format {
        OutputFormat::Table => {
            println!("{:<10} {:<40} {:>8}  Reasons", "App ID", "Name", "Score");
            println!("{}", "-".repeat(86));
            for rec in &recommendations {
                let reasons: Vec<String> = rec.reasons.iter().map(|r| r.code.clone()).collect();
                println!(
                    "{:<10} {:<40} {:>8.2}  {}",
                    rec.app_id,
                    truncate(&rec.name, 38),
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

fn cmd_playlist_import(cli: &Cli, path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let pf = playlist::import_playlist(&path)?;

    // Store the imported playlist locally for later retrieval by ID.
    let store_dir = playlist_store_dir();
    std::fs::create_dir_all(&store_dir)?;
    let stored_path = store_dir.join(format!("{}.json", pf.playlist.id));
    playlist::export_playlist(&pf, &stored_path)?;

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

    // Match against library for a summary.
    let steam_dir = cli.resolve_steam_dir()?;

    let scan_result = steam::scan_library(&steam::ScanOptions {
        steam_dir,
        account: cli.account.clone(),
        fixtures: cli.fixtures.clone(),
    })?;
    let report = playlist::match_playlist(&pf, &scan_result.games)?;

    println!();
    println!("Match summary:");
    println!("  Owned:    {}", report.owned.len());
    println!("  Missing:  {}", report.missing.len());
    println!("  Played:   {}", report.played.len());
    println!("  Unplayed: {}", report.unplayed.len());
    println!("  Hidden:   {}", report.hidden.len());
    println!("  Junk:     {}", report.junk.len());
    println!();
    println!("Stored to {}", stored_path.display());
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

    let steam_dir = cli.resolve_steam_dir()?;

    let scan_result = steam::scan_library(&steam::ScanOptions {
        steam_dir,
        account: cli.account.clone(),
        fixtures: cli.fixtures.clone(),
    })?;
    let report = playlist::match_playlist(&pf, &scan_result.games)?;

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

    let app_ids = match &pf.playlist.content {
        PlaylistContent::Manual { app_ids } => {
            let mut ids = app_ids.clone();
            ids.sort_unstable();
            ids.dedup();
            ids
        }
        PlaylistContent::Rules { rules: _ } => {
            // Evaluate rules against the library to resolve matching app IDs.
            let steam_dir = cli.resolve_steam_dir()?;

            let scan_result = steam::scan_library(&steam::ScanOptions {
                steam_dir,
                account: cli.account.clone(),
                fixtures: cli.fixtures.clone(),
            })?;

            let report = playlist::match_playlist(&pf, &scan_result.games)?;
            report.owned
        }
    };

    if app_ids.is_empty() {
        println!("No app IDs to sync.");
        return Ok(());
    }

    let steam_dir = cli.resolve_steam_dir()?;

    let accounts = steam::detect_accounts(&steam_dir)?;
    let selected = steam::select_account(&accounts, cli.account.as_deref())?;

    let cloud_path = steam_dir
        .join("userdata")
        .join(&selected.steam_id64)
        .join("config/cloudstorage/cloud-storage-namespace-1.json");

    let cloud = steam::read_cloud_storage(&cloud_path)?;

    // Use the playlist ID as the Steam collection ID (slugified).
    let collection_id = playlist::slugify(&pf.playlist.id);

    let plan = steam::generate_write_plan(
        &cloud,
        vec![WriteOp::UpsertCollection {
            id: collection_id.clone(),
            added: app_ids.clone(),
            removed: vec![],
        }],
        cloud_path.clone(),
    )?;

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
        steam::check_write_safety(&cloud_path, cli.allow_steam_running)?;
        steam::execute_write_plan(&plan, 5)?;
        println!();
        println!("Write complete.");
        println!("  Backup: {}", plan.backup_path.display());
    }

    Ok(())
}

fn cmd_cache_refresh(cli: &Cli, source: String) -> Result<(), Box<dyn std::error::Error>> {
    let valid_sources = ["igdb", "rawg", "protondb", "pcgw", "hltb", "all"];
    if !valid_sources.contains(&source.as_str()) {
        eprintln!(
            "Invalid source '{}'. Must be one of: {}",
            source,
            valid_sources.join(", ")
        );
        process::exit(1);
    }

    if cli.offline {
        eprintln!("Cannot refresh cache in offline mode.");
        process::exit(2);
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
            .map(|s| s.to_string())
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
                let last = s
                    .last_success
                    .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
                    .unwrap_or_else(|| "n/a".into());
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

fn cmd_backup_restore(cli: &Cli, file: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let cloud_path = cli.cloud_storage_path()?;

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
// Helpers
// ---------------------------------------------------------------------------

/// Return the directory where imported playlists are stored.
fn playlist_store_dir() -> PathBuf {
    vapourfly_core::config::default_playlists_dir()
}

/// Load a stored playlist by ID from the local playlist store.
fn load_stored_playlist(id: &str) -> Result<PlaylistFile, Box<dyn std::error::Error>> {
    let path = playlist_store_dir().join(format!("{id}.json"));
    if !path.exists() {
        return Err(format!(
            "playlist '{id}' not found. Import it first with: vapourfly playlist import <file>"
        )
        .into());
    }
    playlist::import_playlist(&path).map_err(|e| e.into())
}

/// Validate that exactly one of `--dry-run` / `--confirm` is set.
fn validate_write_flags(dry_run: bool, confirm: bool) -> Result<(), Box<dyn std::error::Error>> {
    if !dry_run && !confirm {
        eprintln!("Error: must specify either --dry-run or --confirm");
        process::exit(1);
    }
    if dry_run && confirm {
        eprintln!("Error: cannot specify both --dry-run and --confirm");
        process::exit(1);
    }
    Ok(())
}

/// Mask a Steam ID, showing only the last 4 characters.
fn mask_id(id: &str) -> String {
    if id.len() <= 4 {
        "***".to_string()
    } else {
        format!("***{}", &id[id.len() - 4..])
    }
}

/// Truncate a string to `max_len` display characters, adding an ellipsis if truncated.
fn truncate(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len - 1).collect();
        format!("{truncated}\u{2026}")
    }
}

/// Format a [`JunkSignal`] into a human-readable reason string.
fn format_junk_signal(signal: &JunkSignal) -> String {
    match signal {
        JunkSignal::LowPlaytime { minutes } => format!("low playtime ({minutes}m)"),
        JunkSignal::ShortCompletion { seconds, .. } => {
            let h = *seconds as f32 / 3600.0;
            format!("short story ({h:.1}h)")
        }
        JunkSignal::LowRating { rating_0_5, .. } => {
            format!("low rating ({rating_0_5:.1})")
        }
    }
}

/// Format a [`JunkMode`] into a display string.
fn format_junk_mode(mode: &JunkMode) -> &'static str {
    match mode {
        JunkMode::Default => "default",
        JunkMode::Strict => "strict",
        JunkMode::Aggressive => "aggressive",
    }
}
