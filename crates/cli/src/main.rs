//! Vapourfly CLI — Steam library manager.
//!
//! All subcommands exist from Phase 1.  Commands that belong to later phases
//! print a clear "not yet implemented" message and exit with code 2.

mod error;

use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};
use vapourfly_core::models::VdfNode;
use vapourfly_core::steam;

// ---------------------------------------------------------------------------
// CLI definition
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "vapourfly",
    version,
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

    #[command(subcommand)]
    command: Commands,
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
        Commands::Scan { format } => cmd_scan(&cli, *format),
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
// Command implementations (Phase 1: doctor and scan have stub output,
// all others print "not yet implemented" and exit with code 2)
// ---------------------------------------------------------------------------

fn cmd_doctor(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    // When --fixtures is provided, use it as the Steam dir for detection.
    let steam_dir = cli
        .fixtures
        .clone()
        .or_else(|| cli.steam_dir.clone())
        .or_else(vapourfly_core::config::VapourflyConfig::detect_steam_dir);

    println!("Vapourfly Doctor");
    println!("================");

    match &steam_dir {
        Some(dir) => {
            println!("Steam dir: {}", dir.display());

            // Detect accounts from loginusers.vdf
            let loginusers_path = dir.join("config").join("loginusers.vdf");
            if loginusers_path.exists() {
                match std::fs::read_to_string(&loginusers_path) {
                    Ok(contents) => match vapourfly_core::steam::parse_text_vdf(&contents) {
                        Ok(root) => {
                            if let Some(users) = root.child_object(&["users"]) {
                                if let VdfNode::Object(entries) = users {
                                    let count = entries.len();
                                    println!("Accounts:  {count} detected");
                                    for (steam_id, node) in entries {
                                        let persona =
                                            node.first_string("PersonaName").unwrap_or("?");
                                        let account =
                                            node.first_string("AccountName").unwrap_or("?");
                                        let most_recent =
                                            node.first_string("MostRecent").unwrap_or("0");
                                        let marker =
                                            if most_recent == "1" { " (active)" } else { "" };
                                        println!("  - {persona} ({account}) [{steam_id}]{marker}");
                                    }
                                }
                            } else {
                                println!("Accounts:  none found");
                            }
                        }
                        Err(e) => {
                            println!("Accounts:  parse error ({e})");
                        }
                    },
                    Err(e) => {
                        println!("Accounts:  cannot read loginusers.vdf ({e})");
                    }
                }
            } else {
                println!("Accounts:  loginusers.vdf not found");
            }

            // Detect library folders from libraryfolders.vdf
            let libraryfolders_path = dir.join("config").join("libraryfolders.vdf");
            if libraryfolders_path.exists() {
                match std::fs::read_to_string(&libraryfolders_path) {
                    Ok(contents) => match vapourfly_core::steam::parse_text_vdf(&contents) {
                        Ok(root) => {
                            if let Some(folders) = root.child_object(&["LibraryFolders"]) {
                                if let VdfNode::Object(entries) = folders {
                                    let folder_count = entries
                                        .iter()
                                        .filter(|(k, v)| {
                                            k.chars().all(|c| c.is_ascii_digit())
                                                && matches!(v, VdfNode::Object(_))
                                        })
                                        .count();
                                    println!("Library folders: {folder_count}");
                                    for (key, node) in entries {
                                        if !key.chars().all(|c| c.is_ascii_digit()) {
                                            continue;
                                        }
                                        if let Some(path) = node.first_string("path") {
                                            let app_count = node
                                                .child_object(&["apps"])
                                                .and_then(|apps| {
                                                    if let VdfNode::Object(app_entries) = apps {
                                                        Some(app_entries.len())
                                                    } else {
                                                        None
                                                    }
                                                })
                                                .unwrap_or(0);
                                            println!("  - {path} ({app_count} apps)");
                                        }
                                    }
                                }
                            } else {
                                println!("Library folders: none found");
                            }
                        }
                        Err(e) => {
                            println!("Library folders: parse error ({e})");
                        }
                    },
                    Err(e) => {
                        println!("Library folders: cannot read libraryfolders.vdf ({e})");
                    }
                }
            } else {
                println!("Library folders: libraryfolders.vdf not found");
            }
        }
        None => {
            println!("Steam dir: (not detected)");
            println!("Hint: pass --steam-dir or set VAPOURFLY_STEAM_DIR");
        }
    }

    if let Some(fixtures) = &cli.fixtures {
        println!("Fixtures:  {}", fixtures.display());
    }
    println!("Verbose:   {}", cli.verbose);
    println!("Offline:   {}", cli.offline);
    Ok(())
}

fn cmd_accounts_list(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let steam_dir = cli
        .fixtures
        .clone()
        .or_else(|| cli.steam_dir.clone())
        .or_else(|| steam::detect_steam_dirs(None).into_iter().next())
        .ok_or("no Steam directory detected")?;

    let accounts = steam::detect_accounts(&steam_dir)?;
    let selected = steam::select_account(&accounts, cli.account.as_deref()).ok();

    for acc in &accounts {
        let marker = if Some(&acc.steam_id64) == selected.map(|s| &s.steam_id64) {
            " *"
        } else {
            ""
        };
        println!(
            "{} ({}) [{}]{}",
            acc.persona_name, acc.account_name, acc.steam_id64, marker
        );
    }
    Ok(())
}

fn cmd_scan(cli: &Cli, format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    let steam_dir = cli
        .fixtures
        .clone()
        .or_else(|| cli.steam_dir.clone())
        .or_else(vapourfly_core::config::VapourflyConfig::detect_steam_dir);

    let steam_dir_display = steam_dir
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "(not detected)".into());

    match format {
        OutputFormat::Table => {
            println!("Vapourfly Scan");
            println!("==============");
            println!("Steam dir: {steam_dir_display}");
            println!("Status:    stub -- full scan not yet wired up");
            println!();
            println!("  When implemented, this command will:");
            println!("  - Parse app manifests in steamapps/");
            println!("  - Merge playtime from localconfig.vdf");
            println!("  - Resolve collection membership from cloud storage");
            println!("  - Display a table of all detected games");
        }
        OutputFormat::Json => {
            let output = serde_json::json!({
                "schema": "vapourfly.scan.v1",
                "steam_dir": steam_dir_display,
                "games": [],
                "warnings": [],
                "stub": true,
                "message": "full scan not yet wired up"
            });
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
    }
    Ok(())
}

fn cmd_collections_list(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let steam_dir = cli
        .fixtures
        .clone()
        .or_else(|| cli.steam_dir.clone())
        .or_else(|| steam::detect_steam_dirs(None).into_iter().next())
        .ok_or("no Steam directory detected")?;

    let cloud_path = steam_dir
        .join("userdata/76561198000000000/config/cloudstorage/cloud-storage-namespace-1.json");
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
    let steam_dir = cli
        .fixtures
        .clone()
        .or_else(|| cli.steam_dir.clone())
        .or_else(|| steam::detect_steam_dirs(None).into_iter().next())
        .ok_or("no Steam directory detected")?;

    let cloud_path = steam_dir
        .join("userdata/76561198000000000/config/cloudstorage/cloud-storage-namespace-1.json");
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
    _cli: &Cli,
    _format: OutputFormat,
    _strict: bool,
    _aggressive: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("junk preview")
}

fn cmd_junk_apply(
    _cli: &Cli,
    _collection: String,
    dry_run: bool,
    confirm: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_write_flags(dry_run, confirm)?;
    not_implemented("junk apply")
}

fn cmd_junk_hide(
    _cli: &Cli,
    dry_run: bool,
    confirm: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_write_flags(dry_run, confirm)?;
    not_implemented("junk hide")
}

fn cmd_recommend(
    _cli: &Cli,
    _minutes: u32,
    _count: usize,
    _deck: bool,
    _installed_only: bool,
    _seed: Option<u64>,
    _format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("recommend")
}

fn cmd_playlist_import(_cli: &Cli, _path: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("playlist import")
}

fn cmd_playlist_export(
    _cli: &Cli,
    _id: String,
    _out: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("playlist export")
}

fn cmd_playlist_match(
    _cli: &Cli,
    _path: PathBuf,
    _format: OutputFormat,
) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("playlist match")
}

fn cmd_sync_collection(
    _cli: &Cli,
    _id: String,
    dry_run: bool,
    confirm: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    validate_write_flags(dry_run, confirm)?;
    not_implemented("sync collection")
}

fn cmd_cache_refresh(cli: &Cli, _source: String) -> Result<(), Box<dyn std::error::Error>> {
    if cli.offline {
        eprintln!("Cannot refresh cache in offline mode.");
        process::exit(2);
    }
    not_implemented("cache refresh")
}

fn cmd_sources_status(_cli: &Cli, _format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("sources status")
}

fn cmd_backup_list(_cli: &Cli, _format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("backup list")
}

fn cmd_backup_restore(_cli: &Cli, _file: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("backup restore")
}

fn cmd_diagnostics_export(_cli: &Cli, _out: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("diagnostics export")
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

/// Print "not yet implemented" and exit with code 2.
fn not_implemented(command: &str) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Command '{command}' is not yet implemented.");
    process::exit(2);
}
