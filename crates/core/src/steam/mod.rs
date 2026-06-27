//! Steam file parsing and manipulation.
//!
//! This module contains parsers for Steam-specific file formats:
//!
//! * **Text VDF** (`.vdf`) — Valve's KeyValues text format
//! * **Platform paths** — Steam directory detection and account discovery
//! * **Account** — Steam account selection logic
//! * **localconfig** — Playtime and per-app settings
//! * **librarycache** — Legacy aggregated name fallback
//! * **appinfo** — Display name fallback from Steam's appinfo cache
//! * **Collections** — Cloud storage collections and hidden state
//! * **Scan** — Aggregation of all sources into Game records

pub mod account;
pub mod appinfo;
pub mod backup;
pub mod collections;
pub mod librarycache;
pub mod localconfig;
pub mod paths;
pub mod safety;
pub mod scan;
pub mod vdf_text;
pub mod write_plan;

pub use account::select_account;
pub use appinfo::lookup_appinfo_names;
pub use backup::{
    BackupInfo, create_backup, execute_write_plan, list_backups, prune_old_backups, restore_backup,
};
pub use collections::{read_cloud_storage, read_user_collections};
pub use librarycache::parse_librarycache;
pub use localconfig::parse_localconfig;
pub use paths::{
    AppManifest, SteamAccount, detect_accounts, detect_library_folders, detect_steam_dirs,
    parse_appmanifests, redact_path, resolve_userdata_dir, steam_account_id,
};
pub use safety::{check_write_safety, is_steam_running, set_steam_running_override};
pub use scan::{ScanOptions, scan_library};
pub use vdf_text::{parse_text_vdf, write_text_vdf};
pub use write_plan::{compute_sha256, generate_write_plan, merge_hidden, upsert_collection};
