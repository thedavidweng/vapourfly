//! Vapourfly CLI — Steam library manager.
//!
//! All subcommands exist from Phase 1.  Commands that belong to later phases
//! print a clear "not yet implemented" message and exit with code 2.

mod error;

use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

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
    #[arg(long, hide = true)]
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
// Command implementations (Phase 1: doctor is live, rest are stubs)
// ---------------------------------------------------------------------------

fn cmd_doctor(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let steam_dir = cli
        .steam_dir
        .clone()
        .or_else(vapourfly_core::config::VapourflyConfig::detect_steam_dir)
        .unwrap_or_else(|| PathBuf::from("(not detected)"));

    println!("Vapourfly Doctor");
    println!("================");
    println!("Steam dir: {}", steam_dir.display());
    if let Some(fixtures) = &cli.fixtures {
        println!("Fixtures:  {}", fixtures.display());
    }
    println!("Verbose:   {}", cli.verbose);
    println!("Offline:   {}", cli.offline);
    Ok(())
}

fn cmd_accounts_list(_cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("accounts list")
}

fn cmd_scan(_cli: &Cli, _format: OutputFormat) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("scan")
}

fn cmd_collections_list(_cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("collections list")
}

fn cmd_collections_export(_cli: &Cli, _out: PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    not_implemented("collections export")
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
