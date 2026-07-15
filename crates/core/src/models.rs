//! Domain models for Vapourfly.
//!
//! These types are the shared vocabulary between core, api, and cli crates.
//! All public JSON-facing structs implement `Serialize`/`Deserialize` with
//! deterministic key ordering where practical.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Schema version constants
// ---------------------------------------------------------------------------

pub const VAPOURFLY_PLAYLIST_SCHEMA: &str = "vapourfly.playlist.v1";
pub const VAPOURFLY_SCAN_SCHEMA: &str = "vapourfly.scan.v1";
pub const VAPOURFLY_DIFF_SCHEMA: &str = "vapourfly.write_plan.v1";
pub const VAPOURFLY_JUNK_PREVIEW_SCHEMA: &str = "vapourfly.junk_preview.v1";
pub const VAPOURFLY_RECOMMENDATIONS_SCHEMA: &str = "vapourfly.recommendations.v1";

// ---------------------------------------------------------------------------
// VDF
// ---------------------------------------------------------------------------

/// A node in a Text VDF tree.  Objects preserve insertion order so that a
/// round-trip through parse → emit does not silently re-order keys.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum VdfNode {
    Object(Vec<(String, VdfNode)>),
    String(String),
}

impl VdfNode {
    /// Navigate to a nested object by following a chain of keys.
    ///
    /// Returns `None` if any key along the path is missing or is not an
    /// [`Object`](VdfNode::Object).
    ///
    /// ```
    /// use vapourfly_core::models::VdfNode;
    ///
    /// let root = VdfNode::Object(vec![
    ///     ("a".into(), VdfNode::Object(vec![
    ///         ("b".into(), VdfNode::Object(vec![
    ///             ("c".into(), VdfNode::String("val".into())),
    ///         ])),
    ///     ])),
    /// ]);
    /// assert_eq!(root.child_object(&["a", "b"]).unwrap().first_string("c"), Some("val"));
    /// assert!(root.child_object(&["a", "missing"]).is_none());
    /// ```
    pub fn child_object(&self, path: &[&str]) -> Option<&VdfNode> {
        let mut current = self;
        for key in path {
            match current {
                VdfNode::Object(entries) => {
                    current = &entries.iter().find(|(k, _)| k == key)?.1;
                }
                _ => return None,
            }
        }
        Some(current)
    }

    /// Return every value associated with `key`, preserving insertion order.
    ///
    /// Useful when a VDF file contains duplicate keys (common in Steam
    /// configuration files). Returns an empty `Vec` if the node is not an
    /// object or the key is absent.
    pub fn child_values(&self, key: &str) -> Vec<&VdfNode> {
        match self {
            VdfNode::Object(entries) => entries
                .iter()
                .filter(|(k, _)| k == key)
                .map(|(_, v)| v)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// Return the first string value for `key`, or `None`.
    ///
    /// Convenience wrapper around [`child_values`](Self::child_values) for the
    /// common case where you only care about the first match and it is a
    /// string.
    pub fn first_string(&self, key: &str) -> Option<&str> {
        match self {
            VdfNode::Object(entries) => {
                entries
                    .iter()
                    .find(|(k, _)| k == key)
                    .and_then(|(_, v)| match v {
                        VdfNode::String(s) => Some(s.as_str()),
                        _ => None,
                    })
            }
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Steam application types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SteamAppType {
    Game,
    Application,
    Tool,
    Demo,
    Dlc,
    Unknown(String),
}

impl Default for SteamAppType {
    fn default() -> Self {
        Self::Unknown("unknown".into())
    }
}

// ---------------------------------------------------------------------------
// Core Game record
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Game {
    pub app_id: u32,
    pub name: String,
    pub app_type: SteamAppType,
    pub installed: bool,
    pub install_dir: Option<PathBuf>,
    pub library_folder: Option<PathBuf>,

    // localconfig.vdf / appmanifest / librarycache
    pub playtime_minutes: Option<u32>,
    pub playtime_2wks_minutes: Option<u32>,
    pub playtime_disconnected_minutes: Option<u32>,
    pub last_played_unix: Option<i64>,

    // Steam collection state
    pub steam_collections: Vec<String>,
    pub is_hidden: bool,
    pub is_junk: bool,

    // External data (populated later)
    pub hltb: Option<HltbData>,
    pub igdb: Option<IgdbData>,
    pub rawg: Option<RawgData>,
    pub protondb: Option<ProtonDbData>,
    pub pcgw: Option<PcgwData>,
    pub steam_store: Option<SteamStoreDetails>,
}

// ---------------------------------------------------------------------------
// External data models
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HltbData {
    pub main_story_seconds: Option<u32>,
    pub main_extra_seconds: Option<u32>,
    pub completionist_seconds: Option<u32>,
    pub source: HltbSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HltbSource {
    IgdbGameTimeToBeat,
    HltbScrape,
    ManualOverride,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IgdbData {
    pub igdb_id: u64,
    pub name: String,
    pub slug: Option<String>,
    pub rating_0_100: Option<f32>,
    pub total_rating_0_100: Option<f32>,
    pub genres: Vec<String>,
    pub themes: Vec<String>,
    pub keywords: Vec<String>,
    pub similar_game_ids: Vec<u64>,
    pub steam_app_id_confirmed: bool,
    pub time_to_beat: Option<IgdbTimeToBeat>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IgdbTimeToBeat {
    pub hastily_seconds: Option<u32>,
    pub normally_seconds: Option<u32>,
    pub completely_seconds: Option<u32>,
    pub submission_count: Option<u32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawgData {
    pub rawg_id: u64,
    pub rating_0_5: Option<f32>,
    pub ratings_count: Option<u32>,
    pub genres: Vec<String>,
    pub tags: Vec<String>,
    pub stores: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProtonDbData {
    pub tier: ProtonTier,
    pub confidence: Option<String>,
    pub score: Option<f32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ProtonTier {
    Borked,
    Bronze,
    Silver,
    Gold,
    Platinum,
    Native,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PcgwData {
    pub page_name: Option<String>,
    pub controller_support: ControllerSupport,
    pub steam_deck_notes: Option<String>,
    pub fixes_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ControllerSupport {
    Full,
    Partial,
    None,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SteamStoreDetails {
    pub app_id: u32,
    pub name: String,
    pub steam_store_type: String,
    pub is_free: bool,
    pub short_description: Option<String>,
    pub header_image: Option<String>,
    pub developers: Vec<String>,
    pub publishers: Vec<String>,
    pub genres: Vec<String>,
    pub categories: Vec<String>,
    pub release_date: Option<String>,
    pub metacritic_score: Option<u32>,
    pub platforms: SteamStorePlatforms,
    pub coming_soon: bool,
    pub price_overview: Option<PriceOverview>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SteamStorePlatforms {
    pub windows: bool,
    pub mac: bool,
    pub linux: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceOverview {
    pub currency: String,
    pub initial_price_cents: u32,
    pub final_price_cents: u32,
    pub discount_percent: u32,
}

// ---------------------------------------------------------------------------
// Local app state (from localconfig.vdf)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LocalAppState {
    pub app_id: u32,
    pub last_played_unix: Option<i64>,
    pub playtime_minutes: Option<u32>,
    pub playtime_2wks_minutes: Option<u32>,
    pub playtime_disconnected_minutes: Option<u32>,
    pub raw_fields: BTreeMap<String, String>,
}

// ---------------------------------------------------------------------------
// Cloud storage / collections
// ---------------------------------------------------------------------------

pub type CloudStorageFile = Vec<(String, CloudEntry)>;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CloudEntry {
    pub key: String,
    pub timestamp: Option<i64>,
    pub value: Option<String>,
    pub version: Option<String>,

    #[serde(default)]
    pub is_deleted: Option<bool>,

    #[serde(default, rename = "conflictResolutionMethod")]
    pub conflict_resolution_method: Option<String>,

    #[serde(default, rename = "strMethodId")]
    pub str_method_id: Option<String>,

    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollectionValue {
    pub id: String,
    pub name: String,
    pub added: Vec<u32>,
    pub removed: Vec<u32>,

    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SteamCollection {
    pub id: String,
    pub name: String,
    pub app_ids: Vec<u32>,
    pub removed_app_ids: Vec<u32>,
    pub is_hidden_collection: bool,
}

// ---------------------------------------------------------------------------
// Write plan
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WritePlan {
    pub target_path: PathBuf,
    pub backup_path: PathBuf,
    pub tmp_path: PathBuf,
    pub before_sha256: String,
    pub after_sha256: String,
    pub after_content: Vec<u8>,
    pub operations: Vec<WriteOp>,
    pub diff: WritePlanDiff,
}

/// Human-readable diff summary produced alongside a [`WritePlan`].
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct WritePlanDiff {
    /// Collections that were created or updated (id -> "created" | "updated").
    pub collections_changed: Vec<CollectionChange>,
    /// AppIDs added across all collections (sorted ascending).
    pub app_ids_added: Vec<u32>,
    /// AppIDs removed across all collections (sorted ascending).
    pub app_ids_removed: Vec<u32>,
    /// AppIDs added to the hidden collection.
    pub hidden_app_ids_added: Vec<u32>,
    /// Number of entries in the file that were not touched.
    pub unchanged_count: usize,
    /// Number of deleted entries that were skipped.
    pub skipped_deleted_count: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollectionChange {
    pub id: String,
    pub action: String, // "created" or "updated"
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum WriteOp {
    UpsertCollection {
        id: String,
        added: Vec<u32>,
        removed: Vec<u32>,
    },
    AddToHidden {
        app_ids: Vec<u32>,
    },
}

// ---------------------------------------------------------------------------
// Playlist models
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaylistFile {
    pub vapourfly_schema: String,
    pub created_by: String,
    pub playlist: Playlist,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub description: String,
    pub content: PlaylistContent,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum PlaylistContent {
    Manual { app_ids: Vec<u32> },
    Rules { rules: Vec<PlaylistRule> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", content = "args")]
pub enum PlaylistRule {
    ProtonAtLeast { tier: ProtonTier },
    HltbMaxMinutes { minutes: u32 },
    ControllerSupportFull,
    PlaytimeBetween { min: u32, max: u32 },
    RatingAtLeast { rating_0_5: f32 },
    HasGenre { genre: String },
    HasTag { tag: String },
    Installed,
    NotJunk,
    NotHidden,
    And(Vec<PlaylistRule>),
    Or(Vec<PlaylistRule>),
    Not(Box<PlaylistRule>),
}

// ---------------------------------------------------------------------------
// Recommendation models
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecommendRequest {
    pub available_minutes: u32,
    pub count: usize,
    pub deck_mode: bool,
    pub include_installed_only: bool,
    pub seed: Option<u64>,
    pub exclude_collections: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recommendation {
    pub app_id: u32,
    pub name: String,
    pub score: f32,
    pub reasons: Vec<RecommendReason>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RecommendReason {
    pub code: String,
    pub description: String,
    pub weight: f32,
}

// ---------------------------------------------------------------------------
// Junk models
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JunkRules {
    pub max_playtime_minutes: u32,
    pub max_main_story_seconds: u32,
    pub max_rating_0_5: f32,
    pub min_available_signals: usize,
}

impl Default for JunkRules {
    fn default() -> Self {
        Self {
            max_playtime_minutes: 30,
            max_main_story_seconds: 7200,
            max_rating_0_5: 2.5,
            min_available_signals: 2,
        }
    }
}

/// Manual overrides that let the user force specific games in or out of the
/// junk set, or supply their own HLTB / rating data.
///
/// Lives with the data model so [`crate::signal`] can apply rating/HLTB
/// precedence without depending on the junk evaluation module.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ManualOverrides {
    /// App IDs that should **never** be marked junk, regardless of signals.
    pub force_include: HashSet<u32>,
    /// App IDs that should **always** be marked junk, regardless of signals.
    pub force_exclude: HashSet<u32>,
    /// User-supplied completion time in seconds (replaces HLTB lookup).
    pub manual_hltb: HashMap<u32, u32>,
    /// User-supplied rating on a 0-5 scale (replaces RAWG/IGDB lookup).
    pub manual_rating: HashMap<u32, f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct JunkDecision {
    pub app_id: u32,
    pub name: String,
    pub is_junk: bool,
    pub confidence: f32,
    pub matched: Vec<JunkSignal>,
    pub missing: Vec<JunkSignalKind>,
    pub mode: JunkMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum JunkSignal {
    LowPlaytime {
        minutes: u32,
    },
    ShortCompletion {
        seconds: u32,
        source: HltbSource,
    },
    LowRating {
        rating_0_5: f32,
        source: RatingSource,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum JunkSignalKind {
    Playtime,
    CompletionTime,
    Rating,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RatingSource {
    Rawg,
    Igdb,
    ManualOverride,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JunkMode {
    Default,
    Strict,
    Aggressive,
}

// ---------------------------------------------------------------------------
// Playlist match report
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlaylistMatchReport {
    pub owned: Vec<u32>,
    pub missing: Vec<u32>,
    pub played: Vec<u32>,
    pub unplayed: Vec<u32>,
    pub hidden: Vec<u32>,
    pub junk: Vec<u32>,
    /// Sum of Steam Store final prices for **missing, non-free** Playlist
    /// entries with available price data. Owned/unplayed games are never
    /// included. Rule Playlists evaluate only the owned library, so they have
    /// no missing entries and this is `None`.
    ///
    /// See [`CompletionPrice`] for single vs mixed-currency handling.
    pub completion_price: Option<CompletionPrice>,
    /// Fraction of missing non-free entries that have price data, so the GUI
    /// can label partial estimates. `None` when there are no missing non-free
    /// entries (e.g. rule Playlists or fully-owned manual Playlists).
    pub price_coverage: Option<PriceCoverage>,
}

/// Completion price for a Playlist Match, distinguishing single-currency
/// totals from mixed-currency situations where summing would be meaningless.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CompletionPrice {
    /// All priced missing entries share one currency.
    Single(Money),
    /// Multiple currencies encountered; each entry is a per-currency total.
    Mixed(Vec<Money>),
}

impl CompletionPrice {
    /// Format the completion price for display.
    ///
    /// Single-currency: `"USD 49.98"`.
    /// Mixed-currency: `"USD 29.99 + EUR 19.99"` (sorted by currency code).
    pub fn format(&self) -> String {
        match self {
            Self::Single(money) => money.format(),
            Self::Mixed(totals) => {
                let mut sorted: Vec<&Money> = totals.iter().collect();
                sorted.sort_by(|a, b| a.currency.cmp(&b.currency));
                sorted
                    .iter()
                    .map(|m| m.format())
                    .collect::<Vec<_>>()
                    .join(" + ")
            }
        }
    }
}

/// How many missing non-free entries have price data vs. how many are missing
/// and non-free overall.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PriceCoverage {
    /// Missing entries that are non-free and have price data.
    pub priced: usize,
    /// Missing entries that are non-free (regardless of whether price data
    /// is available).
    pub non_free: usize,
}

impl PriceCoverage {
    /// Ratio of priced to non-free missing entries, or `None` when
    /// `non_free == 0`.
    pub fn ratio(&self) -> Option<f64> {
        if self.non_free == 0 {
            None
        } else {
            Some(self.priced as f64 / self.non_free as f64)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Money {
    pub amount_cents: u64,
    pub currency: String,
}

impl Money {
    /// Format this amount as a human-readable price string.
    ///
    /// The major unit is derived from cents (divided by 100 with two decimal
    /// places). The currency code is prefixed, e.g. `"USD 7.99"`. This is a
    /// deliberately neutral format — the GUI and CLI both use it for display.
    pub fn format(&self) -> String {
        let major = self.amount_cents / 100;
        let minor = self.amount_cents % 100;
        format!("{} {}.{:02}", self.currency, major, minor)
    }
}

// ---------------------------------------------------------------------------
// Scan result
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanResult {
    pub games: Vec<Game>,
    pub warnings: Vec<ScanWarning>,
    pub steam_dir: String,
    pub account: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanWarning {
    pub code: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_format_renders_cents_as_major_unit() {
        let free = Money {
            amount_cents: 0,
            currency: "USD".into(),
        };
        assert_eq!(free.format(), "USD 0.00");

        let priced = Money {
            amount_cents: 799,
            currency: "USD".into(),
        };
        assert_eq!(priced.format(), "USD 7.99");

        let large = Money {
            amount_cents: 59999,
            currency: "EUR".into(),
        };
        assert_eq!(large.format(), "EUR 599.99");
    }

    #[test]
    fn game_round_trip() {
        let game = Game {
            app_id: 730,
            name: "Counter-Strike 2".into(),
            app_type: SteamAppType::Game,
            installed: true,
            install_dir: Some("/fake/cs2".into()),
            library_folder: Some("/fake".into()),
            playtime_minutes: Some(418),
            playtime_2wks_minutes: Some(213),
            playtime_disconnected_minutes: Some(3),
            last_played_unix: Some(1628871494),
            steam_collections: vec!["favorite".into()],
            is_hidden: false,
            is_junk: false,
            hltb: None,
            igdb: None,
            rawg: None,
            protondb: None,
            pcgw: None,
            steam_store: None,
        };
        let json = serde_json::to_string(&game).unwrap();
        let _: Game = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn cloud_entry_round_trip() {
        let entry = CloudEntry {
            key: "user-collections.test".into(),
            timestamp: Some(1234567890),
            value: Some(r#"{"id":"test","name":"Test","added":[730],"removed":[]}"#.into()),
            version: Some("1".into()),
            is_deleted: None,
            conflict_resolution_method: None,
            str_method_id: None,
            extra: BTreeMap::new(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let _: CloudEntry = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn playlist_file_round_trip() {
        let pf = PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "Vapourfly 0.1".into(),
            playlist: Playlist {
                id: "test-playlist".into(),
                name: "Test".into(),
                description: "A test playlist".into(),
                content: PlaylistContent::Manual {
                    app_ids: vec![730, 427520],
                },
            },
        };
        let json = serde_json::to_string_pretty(&pf).unwrap();
        let back: PlaylistFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back.playlist.id, "test-playlist");
    }

    #[test]
    fn junk_rules_default() {
        let rules = JunkRules::default();
        assert_eq!(rules.max_playtime_minutes, 30);
        assert_eq!(rules.max_main_story_seconds, 7200);
        assert_eq!(rules.max_rating_0_5, 2.5);
        assert_eq!(rules.min_available_signals, 2);
    }

    #[test]
    fn proton_tier_ordering() {
        assert!(ProtonTier::Borked < ProtonTier::Native);
        assert!(ProtonTier::Gold < ProtonTier::Platinum);
    }

    // -- Snapshot-style tests: assert exact JSON output, then round-trip back --

    #[test]
    fn steam_app_type_snapshot() {
        let variants = [
            (SteamAppType::Game, r#""Game""#),
            (SteamAppType::Application, r#""Application""#),
            (SteamAppType::Tool, r#""Tool""#),
            (SteamAppType::Demo, r#""Demo""#),
            (SteamAppType::Dlc, r#""Dlc""#),
            (
                SteamAppType::Unknown("CustomType".into()),
                r#"{"Unknown":"CustomType"}"#,
            ),
        ];
        for (val, expected_json) in &variants {
            let json = serde_json::to_string(val).unwrap();
            assert_eq!(&json, expected_json);
            let back: SteamAppType = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, val);
        }
    }

    #[test]
    fn hltb_data_snapshot() {
        let data = HltbData {
            main_story_seconds: Some(18000),
            main_extra_seconds: Some(36000),
            completionist_seconds: Some(72000),
            source: HltbSource::IgdbGameTimeToBeat,
        };
        let json = serde_json::to_string(&data).unwrap();
        let expected = r#"{"main_story_seconds":18000,"main_extra_seconds":36000,"completionist_seconds":72000,"source":"IgdbGameTimeToBeat"}"#;
        assert_eq!(json, expected);
        let back: HltbData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.main_story_seconds, Some(18000));
        assert_eq!(back.source, HltbSource::IgdbGameTimeToBeat);
    }

    #[test]
    fn proton_db_data_snapshot() {
        let data = ProtonDbData {
            tier: ProtonTier::Platinum,
            confidence: Some("high".into()),
            score: Some(0.95),
        };
        let json = serde_json::to_string(&data).unwrap();
        let expected = r#"{"tier":"Platinum","confidence":"high","score":0.95}"#;
        assert_eq!(json, expected);
        let back: ProtonDbData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tier, ProtonTier::Platinum);
    }

    #[test]
    fn controller_support_snapshot() {
        let variants = [
            (ControllerSupport::Full, r#""Full""#),
            (ControllerSupport::Partial, r#""Partial""#),
            (ControllerSupport::None, r#""None""#),
            (ControllerSupport::Unknown, r#""Unknown""#),
        ];
        for (val, expected_json) in &variants {
            let json = serde_json::to_string(val).unwrap();
            assert_eq!(&json, expected_json);
            let back: ControllerSupport = serde_json::from_str(&json).unwrap();
            assert_eq!(&back, val);
        }
    }

    #[test]
    fn collection_value_snapshot() {
        let cv = CollectionValue {
            id: "favorites".into(),
            name: "Favorites".into(),
            added: vec![730, 427520],
            removed: vec![],
            extra: BTreeMap::new(),
        };
        let json = serde_json::to_string(&cv).unwrap();
        let expected = r#"{"id":"favorites","name":"Favorites","added":[730,427520],"removed":[]}"#;
        assert_eq!(json, expected);
        let back: CollectionValue = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "favorites");
        assert_eq!(back.added, vec![730, 427520]);
    }

    #[test]
    fn write_plan_snapshot() {
        let plan = WritePlan {
            target_path: "/tmp/cloud.json".into(),
            backup_path: "/tmp/cloud.json.vapourfly-backup-1.json".into(),
            tmp_path: "/tmp/.cloud.json.vapourfly.tmp".into(),
            before_sha256: "aa".repeat(32),
            after_sha256: "bb".repeat(32),
            after_content: b"[]".to_vec(),
            operations: vec![
                WriteOp::UpsertCollection {
                    id: "test".into(),
                    added: vec![730],
                    removed: vec![],
                },
                WriteOp::AddToHidden {
                    app_ids: vec![427520],
                },
            ],
            diff: WritePlanDiff::default(),
        };
        let json = serde_json::to_string(&plan).unwrap();
        let back: WritePlan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.operations.len(), 2);
        assert_eq!(back.target_path, PathBuf::from("/tmp/cloud.json"));
    }

    #[test]
    fn playlist_rule_snapshot() {
        let rule = PlaylistRule::And(vec![
            PlaylistRule::NotHidden,
            PlaylistRule::NotJunk,
            PlaylistRule::ProtonAtLeast {
                tier: ProtonTier::Gold,
            },
        ]);
        let json = serde_json::to_string(&rule).unwrap();
        let back: PlaylistRule = serde_json::from_str(&json).unwrap();
        match back {
            PlaylistRule::And(rules) => {
                assert_eq!(rules.len(), 3);
                assert_eq!(rules[0], PlaylistRule::NotHidden);
                assert_eq!(rules[1], PlaylistRule::NotJunk);
            }
            other => panic!("expected And, got {other:?}"),
        }
    }

    #[test]
    fn playlist_content_tagged_snapshot() {
        let manual = PlaylistContent::Manual {
            app_ids: vec![730, 440],
        };
        let json = serde_json::to_string(&manual).unwrap();
        assert_eq!(json, r#"{"type":"Manual","value":{"app_ids":[730,440]}}"#);
        let back: PlaylistContent = serde_json::from_str(&json).unwrap();
        match back {
            PlaylistContent::Manual { app_ids } => assert_eq!(app_ids, vec![730, 440]),
            _ => panic!("expected Manual"),
        }
    }

    #[test]
    fn recommend_request_snapshot() {
        let req = RecommendRequest {
            available_minutes: 120,
            count: 5,
            deck_mode: true,
            include_installed_only: false,
            seed: Some(42),
            exclude_collections: vec!["hidden".into()],
        };
        let json = serde_json::to_string(&req).unwrap();
        let back: RecommendRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.available_minutes, 120);
        assert_eq!(back.count, 5);
        assert!(back.deck_mode);
        assert_eq!(back.seed, Some(42));
    }

    #[test]
    fn vdf_node_round_trip() {
        let node = VdfNode::Object(vec![
            ("key1".into(), VdfNode::String("value1".into())),
            (
                "nested".into(),
                VdfNode::Object(vec![("inner".into(), VdfNode::String("inner_val".into()))]),
            ),
        ]);
        let json = serde_json::to_string(&node).unwrap();
        let back: VdfNode = serde_json::from_str(&json).unwrap();
        assert_eq!(node, back);
    }

    #[test]
    fn local_app_state_snapshot() {
        let mut raw = BTreeMap::new();
        raw.insert("unmapped_field".into(), "some_value".into());
        let state = LocalAppState {
            app_id: 70,
            last_played_unix: Some(1628871494),
            playtime_minutes: Some(418),
            playtime_2wks_minutes: Some(213),
            playtime_disconnected_minutes: Some(3),
            raw_fields: raw,
        };
        let json = serde_json::to_string(&state).unwrap();
        let back: LocalAppState = serde_json::from_str(&json).unwrap();
        assert_eq!(back.app_id, 70);
        assert_eq!(back.playtime_minutes, Some(418));
        assert_eq!(back.raw_fields.get("unmapped_field").unwrap(), "some_value");
    }

    #[test]
    fn recommendation_snapshot() {
        let rec = Recommendation {
            app_id: 730,
            name: "Counter-Strike 2".into(),
            score: 4.75,
            reasons: vec![RecommendReason {
                code: "low_playtime".into(),
                description: "Never played".into(),
                weight: 2.0,
            }],
        };
        let json = serde_json::to_string(&rec).unwrap();
        let back: Recommendation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.app_id, 730);
        assert_eq!(back.reasons.len(), 1);
        assert_eq!(back.reasons[0].weight, 2.0);
    }

    #[test]
    fn junk_decision_snapshot() {
        let decision = JunkDecision {
            app_id: 220,
            name: "Half-Life 2".into(),
            is_junk: false,
            confidence: 0.1,
            matched: vec![],
            missing: vec![JunkSignalKind::Playtime, JunkSignalKind::Rating],
            mode: JunkMode::Default,
        };
        let json = serde_json::to_string(&decision).unwrap();
        let back: JunkDecision = serde_json::from_str(&json).unwrap();
        assert!(!back.is_junk);
        assert_eq!(back.missing.len(), 2);
    }
}
