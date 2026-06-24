//! Steam file parsing and manipulation.
//!
//! This module contains parsers for Steam-specific file formats:
//!
//! * **Text VDF** (`.vdf`) — Valve's KeyValues text format
//! * **Platform paths** — Steam directory detection and account discovery
//! * **Account** — Steam account selection logic
//! * **localconfig** — Playtime and per-app settings
//! * **librarycache** — Name fallback from Steam's library cache JSON
//! * **Collections** — Cloud storage collections and hidden state
//! * **Scan** — Aggregation of all sources into Game records

pub mod account;
pub mod collections;
pub mod librarycache;
pub mod localconfig;
pub mod paths;
pub mod scan;
pub mod vdf_text;

pub use account::select_account;
pub use collections::{read_cloud_storage, read_user_collections};
pub use librarycache::parse_librarycache;
pub use localconfig::parse_localconfig;
pub use paths::{
    AppManifest, SteamAccount, detect_accounts, detect_library_folders, detect_steam_dirs,
    parse_appmanifests, redact_path,
};
pub use scan::{ScanOptions, scan_library};
pub use vdf_text::{parse_text_vdf, write_text_vdf};
