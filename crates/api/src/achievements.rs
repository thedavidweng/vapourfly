//! Documented user-key achievement reads. Cached reads never use the network.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use vapourfly_core::error::{Result, VapourflyError};

use crate::activity::{RefreshControl, encode_query, validate_steam_id64};
use crate::cache::DiskCache;
use crate::http::{CacheRecord, HttpClient};

pub const SOURCE: &str = "steam-achievements";
const SCHEMA_SOURCE: &str = "steam-achievement-schema";
const TTL: Duration = Duration::from_secs(24 * 3600);
const BASE: &str = "https://api.steampowered.com/ISteamUserStats";
pub const DEFAULT_BATCH_LIMIT: usize = 12;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Available,
    NoAchievements,
    Incomplete,
    MissingSchema,
    RestrictedOrUnavailable,
    Unauthorized,
    RateLimited,
    TransportFailure,
    Malformed,
    HttpError,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Progress {
    pub steam_id64: String,
    pub app_id: u32,
    pub status: Status,
    pub total: Option<u32>,
    pub unlocked: Option<u32>,
    pub unlocks: BTreeMap<String, Option<i64>>,
    pub checked_at: DateTime<Utc>,
    pub fetched_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchSummary {
    pub selected: usize,
    pub available: usize,
    pub stale: usize,
    pub unavailable: usize,
    pub missing: usize,
    pub attempted: usize,
    pub pending: usize,
    pub stop_reason: Option<Status>,
}

pub struct BatchOptions<'a> {
    pub app_ids: &'a [u32],
    pub limit: usize,
    pub force: bool,
    pub offline: bool,
}

fn validate_batch_limit(limit: usize) -> Result<()> {
    if !(1..=DEFAULT_BATCH_LIMIT).contains(&limit) {
        return Err(VapourflyError::InvalidInput(format!(
            "Achievement batch limit must be 1..={DEFAULT_BATCH_LIMIT}"
        )));
    }
    Ok(())
}

/// Cache-only priority hints: selected owned game, then most recently played
/// owned games. Activity hints may be stale; they never establish progress.
pub fn active_app_ids(
    owned: &[vapourfly_core::models::Game],
    recent: &[crate::activity::ActivityGame],
    selected: Option<u32>,
    limit: usize,
) -> Result<Vec<u32>> {
    validate_batch_limit(limit)?;
    let owned_ids: BTreeSet<_> = owned.iter().map(|game| game.app_id).collect();
    if selected.is_some_and(|id| id == 0 || !owned_ids.contains(&id)) {
        return Err(VapourflyError::InvalidInput(
            "Selected AppID is not in the local Library".into(),
        ));
    }
    let recent: BTreeMap<_, _> = recent.iter().map(|game| (game.app_id, game)).collect();
    let mut ranked: Vec<_> = owned
        .iter()
        .filter(|game| {
            game.app_id != 0
                && (game.last_played_unix.is_some_and(|time| time > 0)
                    || recent.contains_key(&game.app_id))
        })
        .map(|game| {
            let hint = recent.get(&game.app_id);
            (
                game.app_id,
                game.last_played_unix
                    .max(hint.and_then(|hint| hint.last_played_at)),
                hint.and_then(|hint| hint.recent_minutes),
            )
        })
        .collect();
    ranked.sort_by_key(|&(id, time, minutes)| {
        (std::cmp::Reverse(time), std::cmp::Reverse(minutes), id)
    });
    let mut seen = BTreeSet::new();
    Ok(selected
        .into_iter()
        .chain(ranked.into_iter().map(|(id, _, _)| id))
        .filter(|id| seen.insert(*id))
        .take(limit)
        .collect())
}

fn batch_ids(app_ids: &[u32], limit: usize) -> Result<Vec<u32>> {
    validate_batch_limit(limit)?;
    if app_ids.contains(&0) {
        return Err(VapourflyError::InvalidInput(
            "AppID must be positive".into(),
        ));
    }
    let mut seen = BTreeSet::new();
    Ok(app_ids
        .iter()
        .copied()
        .filter(|id| seen.insert(*id))
        .take(limit)
        .collect())
}

fn provider_stop(status: Status) -> bool {
    matches!(
        status,
        Status::Unauthorized | Status::RateLimited | Status::TransportFailure | Status::HttpError
    )
}

pub(crate) fn batch_lock(cache: &DiskCache, id: &str) -> Result<std::fs::File> {
    let path = cache.path_for(SOURCE, id);
    vapourfly_core::personal_store::lock_account(path.parent().expect("cache source directory"), id)
}

/// Inspect a bounded active set without any network requests.
pub fn batch_status(cache: &DiskCache, id: &str, app_ids: &[u32]) -> Result<BatchSummary> {
    validate_steam_id64(id)?;
    let selected = batch_ids(app_ids, DEFAULT_BATCH_LIMIT)?;
    let mut summary = BatchSummary {
        selected: selected.len(),
        ..Default::default()
    };
    for app_id in selected {
        match load(cache, id, app_id)? {
            None => summary.missing += 1,
            Some(progress) if !progress.stale() => summary.available += 1,
            Some(progress) if progress.fetched_at.is_some() => summary.stale += 1,
            Some(_) => summary.unavailable += 1,
        }
    }
    Ok(summary)
}

impl Progress {
    /// Only complete authoritative counts establish completion evidence.
    pub fn completion_evidence(&self) -> Option<vapourfly_core::progress::AchievementEvidence> {
        match (self.status, self.unlocked, self.total) {
            (Status::Available, Some(unlocked), Some(total)) if total > 0 && unlocked <= total => {
                Some(vapourfly_core::progress::AchievementEvidence {
                    unlocked,
                    total,
                    stale: self.stale(),
                })
            }
            _ => None,
        }
    }

    pub fn automatic_refresh_due(&self) -> bool {
        self.stale()
            && (matches!(self.status, Status::Available | Status::NoAchievements)
                || Utc::now()
                    .signed_duration_since(self.checked_at)
                    .num_seconds()
                    >= 3600)
    }

    pub fn stale(&self) -> bool {
        !matches!(self.status, Status::Available | Status::NoAchievements)
            || self.fetched_at.is_none_or(|time| {
                Utc::now().signed_duration_since(time).num_seconds() > TTL.as_secs() as i64
            })
    }

    pub fn percentage(&self) -> Option<f64> {
        match (self.status, self.unlocked, self.total) {
            (Status::Available, Some(unlocked), Some(total)) if total > 0 => {
                Some(100.0 * f64::from(unlocked) / f64::from(total))
            }
            _ => None,
        }
    }

    pub fn perfected(&self) -> bool {
        self.status == Status::Available
            && self.total.is_some_and(|n| n > 0)
            && self.unlocked == self.total
    }

    pub fn summary(&self) -> String {
        format!(
            "Steam achievements: {:?}; {}/{}; {}; {} (user-key API)",
            self.status,
            self.unlocked.map_or_else(|| "?".into(), |n| n.to_string()),
            self.total.map_or_else(|| "?".into(), |n| n.to_string()),
            self.percentage()
                .map_or_else(|| "percentage unknown".into(), |n| format!("{n:.1}%")),
            if self.stale() {
                "stale/limited"
            } else {
                "fresh"
            },
        )
    }
}

/// Cache-only Library progress shared by CLI and the GUI background prepare.
pub fn library_progress(
    cache: &DiskCache,
    story_root: &std::path::Path,
    id: &str,
    games: &[vapourfly_core::models::Game],
) -> Result<BTreeMap<u32, vapourfly_core::progress::Summary>> {
    validate_steam_id64(id)?;
    games
        .iter()
        .map(|game| {
            let story = vapourfly_core::progress::story_completed(story_root, id, game.app_id)?;
            let evidence = load(cache, id, game.app_id)?.and_then(|p| p.completion_evidence());
            Ok((
                game.app_id,
                vapourfly_core::progress::Summary::new(game.playtime_minutes, story, evidence),
            ))
        })
        .collect()
}

fn validate_request(id: &str, app_id: u32) -> Result<()> {
    validate_steam_id64(id)?;
    if app_id == 0 {
        return Err(VapourflyError::InvalidInput(
            "AppID must be positive".into(),
        ));
    }
    Ok(())
}

pub fn load(cache: &DiskCache, id: &str, app_id: u32) -> Result<Option<Progress>> {
    validate_request(id, app_id)?;
    let record = match cache.get::<Progress>(SOURCE, &format!("{id}-{app_id}")) {
        Ok(record) => record,
        // A corrupt cache cannot establish progress; an explicit refresh can replace it.
        Err(VapourflyError::ParseError { .. }) => return Ok(None),
        Err(error) => return Err(error),
    };
    match record {
        Some(record) if record.data.steam_id64 != id || record.data.app_id != app_id => Ok(None),
        other => Ok(other.map(|r| r.data)),
    }
}

#[derive(Deserialize)]
struct SchemaEnvelope {
    game: Option<SchemaGame>,
}

#[derive(Deserialize)]
struct SchemaGame {
    #[serde(rename = "availableGameStats")]
    stats: Option<SchemaStats>,
}

#[derive(Deserialize)]
struct SchemaStats {
    achievements: Option<Vec<SchemaAchievement>>,
}

#[derive(Deserialize)]
struct SchemaAchievement {
    name: String,
}

#[derive(Deserialize)]
struct PlayerEnvelope {
    playerstats: PlayerStats,
}

#[derive(Deserialize)]
struct PlayerStats {
    success: bool,
    #[serde(rename = "steamID")]
    steam_id: Option<String>,
    achievements: Option<Vec<PlayerAchievement>>,
}

#[derive(Deserialize)]
struct PlayerAchievement {
    apiname: String,
    achieved: u8,
    unlocktime: Option<i64>,
}

fn schema(body: &[u8]) -> std::result::Result<BTreeSet<String>, Status> {
    let wire: SchemaEnvelope = serde_json::from_slice(body).map_err(|_| Status::Malformed)?;
    let game = wire.game.ok_or(Status::MissingSchema)?;
    let rows = game
        .stats
        .and_then(|stats| stats.achievements)
        .unwrap_or_default();
    let mut names = BTreeSet::new();
    for row in rows {
        if row.name.is_empty() {
            return Err(Status::Incomplete);
        }
        names.insert(row.name);
    }
    Ok(names)
}

fn player(
    body: &[u8],
    id: &str,
    names: &BTreeSet<String>,
) -> std::result::Result<BTreeMap<String, Option<i64>>, Status> {
    let wire: PlayerEnvelope = serde_json::from_slice(body).map_err(|_| Status::Malformed)?;
    if !wire.playerstats.success {
        return Err(Status::RestrictedOrUnavailable);
    }
    if wire.playerstats.steam_id.as_deref() != Some(id) {
        return Err(Status::Incomplete);
    }
    let rows = wire.playerstats.achievements.ok_or(Status::Incomplete)?;
    let mut seen = BTreeMap::new();
    let mut unlocks = BTreeMap::new();
    for row in rows {
        if !names.contains(&row.apiname) || row.achieved > 1 {
            return Err(Status::Incomplete);
        }
        if let Some(previous) = seen.insert(row.apiname.clone(), row.achieved)
            && previous != row.achieved
        {
            return Err(Status::Incomplete);
        }
        if row.achieved == 1 {
            unlocks.insert(
                row.apiname,
                row.unlocktime
                    .filter(|&t| t > 0 && t <= Utc::now().timestamp()),
            );
        }
    }
    if seen.len() != names.len() {
        return Err(Status::Incomplete);
    }
    Ok(unlocks)
}

fn request(http: &HttpClient, url: &str) -> std::result::Result<Vec<u8>, Status> {
    let response = http
        .get_once("steam-web", url)
        .map_err(|_| Status::TransportFailure)?;
    match response.status {
        200 => Ok(response.body),
        401 => Err(Status::Unauthorized),
        403 => Err(Status::RestrictedOrUnavailable),
        429 => Err(Status::RateLimited),
        _ => Err(Status::HttpError),
    }
}

enum RefreshFailure {
    Status(Status),
    Error(VapourflyError),
}

impl From<VapourflyError> for RefreshFailure {
    fn from(error: VapourflyError) -> Self {
        Self::Error(error)
    }
}

/// One schema request (unless fresh shared schema exists), one player request,
/// no retries. No key/offline/cancelled paths never issue further requests.
pub fn refresh(
    cache: &DiskCache,
    http: &HttpClient,
    api_key: Option<&str>,
    id: &str,
    app_id: u32,
    offline: bool,
    control: &RefreshControl,
) -> Result<Option<Progress>> {
    validate_request(id, app_id)?;
    let _owner = if !offline && api_key.is_some_and(|key| !key.is_empty()) {
        Some(batch_lock(cache, id)?)
    } else {
        None
    };
    refresh_owned(cache, http, api_key, id, app_id, offline, control)
}

// The batch already owns the same account lock for its full request sequence.
fn refresh_owned(
    cache: &DiskCache,
    http: &HttpClient,
    api_key: Option<&str>,
    id: &str,
    app_id: u32,
    offline: bool,
    control: &RefreshControl,
) -> Result<Option<Progress>> {
    let cache_key = format!("{id}-{app_id}");
    let previous = load(cache, id, app_id)?;
    let Some(api_key) = api_key.filter(|k| !k.is_empty() && !offline) else {
        return Ok(previous);
    };
    control.check()?;
    let api_key = encode_query(api_key);
    let result: std::result::Result<_, RefreshFailure> = (|| {
        let cached = cache.get::<BTreeSet<String>>(SCHEMA_SOURCE, &app_id.to_string())?;
        let names = if let Some(cached) = cached.filter(|r| !r.stale) {
            cached.data
        } else {
            let body = request(
                http,
                &format!("{BASE}/GetSchemaForGame/v2/?key={api_key}&appid={app_id}"),
            )
            .map_err(RefreshFailure::Status)?;
            let names = schema(&body).map_err(RefreshFailure::Status)?;
            cache.put(&CacheRecord {
                source: SCHEMA_SOURCE.into(),
                key: app_id.to_string(),
                data: names.clone(),
                fetched_at: Utc::now(),
                ttl: Duration::from_secs(7 * 24 * 3600),
                stale: false,
                etag: None,
            })?;
            names
        };
        if names.is_empty() {
            return Ok((Status::NoAchievements, names, BTreeMap::new()));
        }
        control.check()?;
        let body = request(
            http,
            &format!("{BASE}/GetPlayerAchievements/v1/?key={api_key}&steamid={id}&appid={app_id}"),
        )
        .map_err(RefreshFailure::Status)?;
        let unlocks = player(&body, id, &names).map_err(RefreshFailure::Status)?;
        Ok((Status::Available, names, unlocks))
    })();
    control.check()?;
    let progress = match result {
        Ok((status, names, unlocks)) => Progress {
            steam_id64: id.into(),
            app_id,
            status,
            total: Some(names.len() as u32),
            unlocked: Some(unlocks.len() as u32),
            unlocks,
            checked_at: Utc::now(),
            fetched_at: Some(Utc::now()),
        },
        Err(RefreshFailure::Error(error)) => return Err(error),
        Err(RefreshFailure::Status(status)) => Progress {
            status,
            checked_at: Utc::now(),
            ..previous.unwrap_or(Progress {
                steam_id64: id.into(),
                app_id,
                status,
                total: None,
                unlocked: None,
                unlocks: BTreeMap::new(),
                checked_at: Utc::now(),
                fetched_at: None,
            })
        },
    };
    cache.put(&CacheRecord {
        source: SOURCE.into(),
        key: cache_key,
        data: progress.clone(),
        fetched_at: Utc::now(),
        ttl: TTL,
        stale: false,
        etag: None,
    })?;
    Ok(Some(progress))
}

/// Refresh a bounded, ordered set of games for one account.
///
/// Work is deliberately sequential: one caller owns the control token, and
/// each game makes at most one schema and one player request. Background
/// callers skip fresh cache entries; explicit callers can force them.
pub fn refresh_batch(
    cache: &DiskCache,
    http: &HttpClient,
    api_key: Option<&str>,
    id: &str,
    options: BatchOptions<'_>,
    control: &RefreshControl,
) -> Result<(BatchSummary, Vec<Progress>)> {
    validate_steam_id64(id)?;
    let selected = batch_ids(options.app_ids, options.limit)?;
    control.check()?;
    let live = !options.offline && api_key.is_some_and(|key| !key.is_empty());
    if !live {
        let mut progress = Vec::new();
        for &app_id in &selected {
            control.check()?;
            if let Some(cached) = load(cache, id, app_id)? {
                progress.push(cached);
            }
        }
        return Ok((batch_status(cache, id, &selected)?, progress));
    }
    // OS lock ownership ends on drop, including cancellation or process exit.
    let _owner = if selected.is_empty() {
        None
    } else {
        Some(batch_lock(cache, id)?)
    };
    let mut attempted = 0;
    let mut pending = 0;
    let mut stop_reason = None;
    let mut progress = Vec::new();
    for (index, &app_id) in selected.iter().enumerate() {
        control.check()?;
        if !options.force
            && let Some(cached) = load(cache, id, app_id)?
            && !cached.automatic_refresh_due()
        {
            let stop = provider_stop(cached.status);
            if stop {
                stop_reason = Some(cached.status);
                pending = selected.len() - index - 1;
            }
            progress.push(cached);
            if stop {
                break;
            }
            continue;
        }
        if let Some(current) =
            refresh_owned(cache, http, api_key, id, app_id, options.offline, control)?
        {
            attempted += 1;
            let stop = provider_stop(current.status);
            if stop {
                stop_reason = Some(current.status);
            }
            progress.push(current);
            if stop {
                pending = selected.len() - index - 1;
                break;
            }
        }
    }
    control.check()?;
    let mut summary = batch_status(cache, id, &selected)?;
    summary.attempted = attempted;
    summary.pending = pending;
    summary.stop_reason = stop_reason;
    Ok((summary, progress))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpBackend, HttpRequest, HttpResponse};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    const ID: &str = "76561198000000001";
    const OTHER: &str = "76561198000000002";

    struct Backend {
        calls: Arc<AtomicUsize>,
        body: String,
        status: u16,
    }
    impl HttpBackend for Backend {
        fn execute(&self, request: &HttpRequest) -> Result<HttpResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(HttpResponse {
                status: self.status,
                headers: Default::default(),
                body: if request.url.contains("GetSchema") {
                    br#"{"game":{"availableGameStats":{"achievements":[{"name":"A"},{"name":"B"},{"name":"A"}]}}}"#.to_vec()
                } else {
                    self.body.as_bytes().to_vec()
                },
            })
        }
    }

    struct FailingBackend;

    impl HttpBackend for FailingBackend {
        fn execute(&self, _request: &HttpRequest) -> Result<HttpResponse> {
            Err(VapourflyError::Internal("transport test failure".into()))
        }
    }

    fn progress() -> Progress {
        Progress {
            steam_id64: ID.into(),
            app_id: 10,
            status: Status::Available,
            total: Some(2),
            unlocked: Some(1),
            unlocks: BTreeMap::from([("private-unlock-history".into(), Some(100))]),
            checked_at: Utc::now(),
            fetched_at: Some(Utc::now()),
        }
    }

    #[test]
    fn story_progress_uses_cache_only_and_never_infers_from_unsupported_or_failed_counts() {
        let temp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(temp.path().join("cache"));
        let story_root = temp.path().join("personal/story");
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/fixtures/steam_minimal");
        let scan = vapourfly_core::steam::scan_library(&vapourfly_core::steam::ScanOptions {
            steam_dir: fixtures,
            account: Some("76561198000000000".into()),
            fixtures: None,
        })
        .unwrap();
        let game = scan.games.first().unwrap();
        let mut p = progress();
        p.app_id = game.app_id;
        for status in [
            Status::NoAchievements,
            Status::RestrictedOrUnavailable,
            Status::MissingSchema,
            Status::Incomplete,
            Status::RateLimited,
        ] {
            p.status = status;
            assert!(p.completion_evidence().is_none(), "{status:?}");
        }
        p.status = Status::Available;
        cache
            .put(&CacheRecord {
                source: SOURCE.into(),
                key: format!("{ID}-{}", game.app_id),
                data: p,
                fetched_at: Utc::now(),
                ttl: TTL,
                stale: false,
                etag: None,
            })
            .unwrap();
        vapourfly_core::progress::set_story_completed(&story_root, ID, game.app_id, true).unwrap();
        let summary = library_progress(&cache, &story_root, ID, &scan.games).unwrap();
        let row = &summary[&game.app_id];
        assert!(row.story_completed);
        assert_eq!(row.achievements.unwrap().unlocked, 1);
        assert!(!row.perfected());
        let json = serde_json::to_string(&summary).unwrap();
        assert!(!json.contains(ID) && !json.contains("private-unlock-history"));
        let other = library_progress(&cache, &story_root, OTHER, &scan.games).unwrap();
        assert!(!other[&game.app_id].story_completed);
        assert!(other[&game.app_id].achievements.is_none());
        assert!(
            !story_root.join(OTHER).exists(),
            "reads do not create another account's state"
        );
    }

    #[test]
    fn summary_exposes_progress_not_identity_or_unlock_history() {
        let summary = progress().summary();
        assert!(summary.contains("1/2") && summary.contains("50.0%"));
        assert!(!summary.contains(ID));
        assert!(!summary.contains("private-unlock-history"));
    }

    #[test]
    fn automatic_refresh_cools_down_failures_only() {
        let mut p = progress();
        assert!(!p.automatic_refresh_due());
        p.fetched_at = Some(Utc::now() - chrono::Duration::hours(25));
        assert!(p.automatic_refresh_due());
        p.status = Status::RateLimited;
        assert!(p.stale());
        assert!(!p.automatic_refresh_due());
        p.checked_at = Utc::now() - chrono::Duration::seconds(3601);
        assert!(p.automatic_refresh_due());
        p.status = Status::NoAchievements;
        p.checked_at = Utc::now();
        assert!(p.automatic_refresh_due());
    }

    #[test]
    fn invalid_cache_is_absent_evidence_and_refresh_replaces_it() {
        let temp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(temp.path());
        let path = cache.path_for(SOURCE, &format!("{ID}-10"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"invalid json").unwrap();
        assert!(load(&cache, ID, 10).unwrap().is_none());
        let mut foreign = progress();
        foreign.steam_id64 = OTHER.into();
        for data in [
            foreign,
            Progress {
                app_id: 20,
                ..progress()
            },
        ] {
            cache
                .put(&CacheRecord {
                    source: SOURCE.into(),
                    key: format!("{ID}-10"),
                    data,
                    fetched_at: Utc::now(),
                    ttl: TTL,
                    stale: false,
                    etag: None,
                })
                .unwrap();
            assert!(load(&cache, ID, 10).unwrap().is_none());
        }
        let http = HttpClient::with_backend(Box::new(Backend {
            calls: Arc::new(AtomicUsize::new(0)),
            status: 200,
            body: format!(
                r#"{{"playerstats":{{"success":true,"steamID":"{ID}","achievements":[{{"apiname":"A","achieved":0}},{{"apiname":"B","achieved":0}}]}}}}"#
            ),
        }));
        assert_eq!(
            refresh(
                &cache,
                &http,
                Some("secret"),
                ID,
                10,
                false,
                &RefreshControl::default()
            )
            .unwrap()
            .unwrap()
            .unlocked,
            Some(0)
        );
        assert_eq!(
            load(&cache, ID, 10).unwrap().unwrap().status,
            Status::Available
        );
        assert!(load(&cache, "invalid", 10).is_err());
        assert!(load(&cache, ID, 0).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(matches!(
            load(&cache, ID, 10),
            Err(VapourflyError::FileNotFound { .. })
        ));
    }

    #[test]
    fn complete_sets_zero_partial_all_and_account_isolation() {
        let temp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(temp.path());
        let calls = Arc::new(AtomicUsize::new(0));
        for (a, b, expected) in [(0, 0, 0), (1, 0, 1), (1, 1, 2)] {
            let http = HttpClient::with_backend(Box::new(Backend {
                calls: calls.clone(),
                status: 200,
                body: format!(
                    r#"{{"playerstats":{{"success":true,"steamID":"{ID}","achievements":[{{"apiname":"A","achieved":{a},"unlocktime":100}},{{"apiname":"B","achieved":{b}}}]}}}}"#
                ),
            }));
            let p = refresh(
                &cache,
                &http,
                Some("secret"),
                ID,
                10,
                false,
                &RefreshControl::default(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(p.total, Some(2));
            assert_eq!(p.unlocked, Some(expected));
            assert_eq!(p.percentage(), Some(50.0 * f64::from(expected)));
            assert_eq!(p.perfected(), expected == 2);
            assert!(load(&cache, OTHER, 10).unwrap().is_none());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 4, "schema shared and reused");
        let http = HttpClient::with_backend(Box::new(Backend {
            calls: calls.clone(),
            status: 429,
            body: String::new(),
        }));
        let p = refresh(
            &cache,
            &http,
            Some("secret"),
            ID,
            10,
            false,
            &RefreshControl::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(p.status, Status::RateLimited);
        assert_eq!(p.unlocked, Some(2), "retain prior evidence");
        assert!(!p.perfected());
        let count = calls.load(Ordering::SeqCst);
        for (key, offline) in [(None, false), (Some("secret"), true)] {
            refresh(
                &cache,
                &http,
                key,
                ID,
                10,
                offline,
                &RefreshControl::default(),
            )
            .unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), count);
        let bytes = std::fs::read_to_string(cache.path_for(SOURCE, &format!("{ID}-10"))).unwrap();
        assert!(!bytes.contains("secret") && !bytes.contains("key="));
    }

    #[test]
    fn missing_restricted_incomplete_and_no_achievements_are_distinct() {
        assert_eq!(schema(b"{}"), Err(Status::MissingSchema));
        assert!(schema(br#"{"game":{}}"#).unwrap().is_empty());
        assert!(
            schema(br#"{"game":{"availableGameStats":{}}}"#)
                .unwrap()
                .is_empty()
        );
        let names = BTreeSet::from(["A".into()]);
        assert_eq!(
            player(br#"{"playerstats":{"success":false}}"#, ID, &names),
            Err(Status::RestrictedOrUnavailable)
        );
        assert_eq!(
            player(
                format!(
                    r#"{{"playerstats":{{"success":true,"steamID":"{OTHER}","achievements":[]}}}}"#
                )
                .as_bytes(),
                ID,
                &names
            ),
            Err(Status::Incomplete)
        );
        assert_eq!(
            player(
                format!(
                    r#"{{"playerstats":{{"success":true,"steamID":"{ID}","achievements":[]}}}}"#
                )
                .as_bytes(),
                ID,
                &names
            ),
            Err(Status::Incomplete)
        );
        assert_eq!(player(b"invalid", ID, &names), Err(Status::Malformed));
    }

    #[test]
    fn provider_and_transport_failures_preserve_distinct_statuses() {
        for (http_status, expected) in [
            (401, Status::Unauthorized),
            (403, Status::RestrictedOrUnavailable),
            (429, Status::RateLimited),
            (500, Status::HttpError),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let cache = DiskCache::new(temp.path());
            let http = HttpClient::with_backend(Box::new(Backend {
                calls: Arc::new(AtomicUsize::new(0)),
                status: http_status,
                body: String::new(),
            }));
            let progress = refresh(
                &cache,
                &http,
                Some("secret"),
                ID,
                10,
                false,
                &RefreshControl::default(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(progress.status, expected);
            assert_eq!(progress.percentage(), None);
            assert!(!progress.perfected());
        }

        let temp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(temp.path());
        let http = HttpClient::with_backend(Box::new(FailingBackend));
        let progress = refresh(
            &cache,
            &http,
            Some("secret"),
            ID,
            10,
            false,
            &RefreshControl::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(progress.status, Status::TransportFailure);
    }

    #[test]
    fn cancellation_stops_before_network_and_cache_errors_are_not_masked() {
        let temp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(temp.path());
        let calls = Arc::new(AtomicUsize::new(0));
        let http = HttpClient::with_backend(Box::new(Backend {
            calls: calls.clone(),
            status: 200,
            body: format!(
                r#"{{"playerstats":{{"success":true,"steamID":"{ID}","achievements":[{{"apiname":"A","achieved":0}},{{"apiname":"B","achieved":0}}]}}}}"#
            ),
        }));
        let control = RefreshControl::default();
        control.cancel();
        assert!(refresh(&cache, &http, Some("secret"), ID, 10, false, &control).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let root = temp.path().join("not-a-directory");
        std::fs::write(&root, b"file").unwrap();
        let broken_cache = DiskCache::new(root);
        let error = refresh(
            &broken_cache,
            &http,
            Some("secret"),
            ID,
            10,
            false,
            &RefreshControl::default(),
        )
        .unwrap_err();
        assert!(matches!(error, VapourflyError::Internal(_)));
    }

    #[test]
    fn batch_is_ordered_bounded_and_skips_fresh_cache() {
        let temp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(temp.path());
        let calls = Arc::new(AtomicUsize::new(0));
        let http = HttpClient::with_backend(Box::new(Backend {
            calls: calls.clone(),
            status: 200,
            body: format!(
                r#"{{"playerstats":{{"success":true,"steamID":"{ID}","achievements":[{{"apiname":"A","achieved":1}},{{"apiname":"B","achieved":0}}]}}}}"#
            ),
        }));
        refresh(
            &cache,
            &http,
            Some("secret"),
            ID,
            10,
            false,
            &RefreshControl::default(),
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 2);

        let (summary, progress) = refresh_batch(
            &cache,
            &http,
            Some("secret"),
            ID,
            BatchOptions {
                app_ids: &[10, 11, 11, 12],
                limit: 2,
                force: false,
                offline: false,
            },
            &RefreshControl::default(),
        )
        .unwrap();
        assert_eq!(summary.selected, 2);
        assert_eq!(summary.available, 2);
        assert_eq!(summary.attempted, 1);
        assert_eq!(summary.missing, 0);
        assert_eq!(
            progress.iter().map(|p| p.app_id).collect::<Vec<_>>(),
            [10, 11]
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            4,
            "one cached and one refreshed"
        );
    }

    fn owned_game(app_id: u32, last_played: Option<i64>) -> vapourfly_core::models::Game {
        serde_json::from_value(serde_json::json!({
            "app_id": app_id, "name": format!("Game {app_id}"), "app_type": "Game",
            "installed": false, "last_played_unix": last_played,
            "steam_collections": [], "is_hidden": false, "is_junk": false,
        }))
        .unwrap()
    }

    #[test]
    fn batch_large_library_selection_and_status_are_cache_only() {
        let owned: Vec<_> = (1..=1500)
            .map(|id| owned_game(id, Some(i64::from(id))))
            .collect();
        let recent = vec![
            crate::activity::ActivityGame {
                app_id: 2,
                name: None,
                lifetime_minutes: None,
                recent_minutes: Some(10),
                last_played_at: Some(2000),
            },
            crate::activity::ActivityGame {
                app_id: 9000,
                name: None,
                lifetime_minutes: None,
                recent_minutes: Some(20),
                last_played_at: Some(3000),
            },
        ];
        let ids = active_app_ids(&owned, &recent, Some(10), 12).unwrap();
        assert_eq!(&ids[..3], &[10, 2, 1500]);
        assert_eq!(ids.len(), 12);
        assert!(!ids.contains(&9000));
        assert!(active_app_ids(&owned, &recent, Some(9000), 12).is_err());
        assert!(active_app_ids(&owned, &recent, None, 13).is_err());
        assert!(active_app_ids(&owned, &recent, None, 0).is_err());
        assert_eq!(
            active_app_ids(&[owned_game(1, None)], &[], None, 12).unwrap(),
            [] as [u32; 0]
        );
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("never-created");
        let cache = DiskCache::new(&root);
        let coverage = batch_status(&cache, ID, &ids).unwrap();
        assert_eq!(coverage.selected, 12);
        assert_eq!(coverage.missing, 12);
        assert!(
            !root.exists(),
            "cache inspection does not write or start a batch"
        );
    }

    #[test]
    fn batch_offline_no_key_and_other_account_never_request_or_create_cache() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("never-created");
        let cache = DiskCache::new(&root);
        let calls = Arc::new(AtomicUsize::new(0));
        let http = HttpClient::with_backend(Box::new(Backend {
            calls: calls.clone(),
            body: String::new(),
            status: 200,
        }));
        for (api_key, offline) in [(None, false), (Some("secret"), true)] {
            let (summary, result) = refresh_batch(
                &cache,
                &http,
                api_key,
                ID,
                BatchOptions {
                    app_ids: &[10, 10, 11],
                    limit: 12,
                    force: true,
                    offline,
                },
                &RefreshControl::default(),
            )
            .unwrap();
            assert_eq!(summary.selected, 2);
            assert_eq!(summary.missing, 2);
            assert_eq!(summary.attempted, 0);
            assert!(result.is_empty());
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(!root.exists());
        assert_eq!(batch_status(&cache, OTHER, &[10]).unwrap().missing, 1);
        assert!(
            refresh_batch(
                &cache,
                &http,
                Some("secret"),
                ID,
                BatchOptions {
                    app_ids: &[0],
                    limit: 1,
                    force: true,
                    offline: false
                },
                &RefreshControl::default()
            )
            .is_err()
        );
        assert!(!root.exists(), "validate before locking or making requests");
    }

    #[test]
    fn batch_provider_failures_stop_without_retry_and_preserve_counts() {
        for (status, expected) in [
            (401, Status::Unauthorized),
            (429, Status::RateLimited),
            (503, Status::HttpError),
        ] {
            let temp = tempfile::tempdir().unwrap();
            let cache = DiskCache::new(temp.path());
            let before = progress();
            cache
                .put(&CacheRecord {
                    source: SOURCE.into(),
                    key: format!("{ID}-10"),
                    data: before.clone(),
                    fetched_at: Utc::now(),
                    ttl: TTL,
                    stale: false,
                    etag: None,
                })
                .unwrap();
            let calls = Arc::new(AtomicUsize::new(0));
            let http = HttpClient::with_backend(Box::new(Backend {
                calls: calls.clone(),
                body: String::new(),
                status,
            }));
            let (summary, result) = refresh_batch(
                &cache,
                &http,
                Some("secret"),
                ID,
                BatchOptions {
                    app_ids: &[10, 11, 12],
                    limit: 3,
                    force: true,
                    offline: false,
                },
                &RefreshControl::default(),
            )
            .unwrap();
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert_eq!(summary.stop_reason, Some(expected));
            assert_eq!(summary.pending, 2);
            assert_eq!(summary.stale, 1);
            assert_eq!(summary.missing, 2);
            assert_eq!(result[0].unlocked, before.unlocked);
            assert_eq!(result[0].fetched_at, before.fetched_at);
            assert!(result[0].stale());
            assert_eq!(batch_status(&cache, OTHER, &[10]).unwrap().missing, 1);
            let calls_before = calls.load(Ordering::SeqCst);
            let (summary, _) = refresh_batch(
                &cache,
                &http,
                Some("secret"),
                ID,
                BatchOptions {
                    app_ids: &[10, 11, 12],
                    limit: 3,
                    force: false,
                    offline: false,
                },
                &RefreshControl::default(),
            )
            .unwrap();
            assert_eq!(summary.attempted, 0, "automatic failure cooldown");
            assert_eq!(
                summary.pending, 2,
                "provider cooldown also holds queued games"
            );
            assert_eq!(summary.stop_reason, Some(expected));
            assert_eq!(calls.load(Ordering::SeqCst), calls_before);
        }
        let temp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(temp.path());
        let http = HttpClient::with_backend(Box::new(FailingBackend));
        let (summary, _) = refresh_batch(
            &cache,
            &http,
            Some("secret"),
            ID,
            BatchOptions {
                app_ids: &[10, 11],
                limit: 2,
                force: true,
                offline: false,
            },
            &RefreshControl::default(),
        )
        .unwrap();
        assert_eq!(summary.stop_reason, Some(Status::TransportFailure));
        assert_eq!(summary.unavailable, 1);
        assert_eq!(summary.pending, 1);
        let (cached, _) = refresh_batch(
            &cache,
            &http,
            None,
            ID,
            BatchOptions {
                app_ids: &[10, 11],
                limit: 2,
                force: true,
                offline: false,
            },
            &RefreshControl::default(),
        )
        .unwrap();
        assert_eq!(cached.unavailable, 1);
        assert_eq!(cached.attempted, 0);
        assert_eq!(cached.pending, 0, "cache inspection does not queue work");
        assert_eq!(
            cached.stop_reason, None,
            "a cached failure is not a new provider stop"
        );
    }

    struct CancelBackend {
        control: RefreshControl,
        calls: Arc<AtomicUsize>,
    }
    impl HttpBackend for CancelBackend {
        fn execute(&self, _: &HttpRequest) -> Result<HttpResponse> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.control.cancel();
            Ok(HttpResponse {
                status: 200,
                headers: Default::default(),
                body: br#"{"game":{"availableGameStats":{"achievements":[{"name":"A"}]}}}"#
                    .to_vec(),
            })
        }
    }

    #[test]
    fn batch_cancellation_prevents_player_request_and_releases_account_owner() {
        let temp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(temp.path());
        let calls = Arc::new(AtomicUsize::new(0));
        let control = RefreshControl::default();
        let http = HttpClient::with_backend(Box::new(CancelBackend {
            control: control.clone(),
            calls: calls.clone(),
        }));
        assert!(
            refresh_batch(
                &cache,
                &http,
                Some("secret"),
                ID,
                BatchOptions {
                    app_ids: &[10, 11],
                    limit: 2,
                    force: true,
                    offline: false
                },
                &control
            )
            .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(load(&cache, ID, 10).unwrap().is_none());
        let owner = batch_lock(&cache, ID).unwrap();
        assert!(batch_lock(&cache, ID).is_err());
        assert!(batch_lock(&cache, OTHER).is_ok());
        drop(owner);
        assert!(batch_lock(&cache, ID).is_ok());
        assert!(
            refresh_batch(
                &cache,
                &http,
                Some("secret"),
                ID,
                BatchOptions {
                    app_ids: &[10],
                    limit: 1,
                    force: true,
                    offline: false
                },
                &control
            )
            .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn batch_hard_cap_means_at_most_twenty_four_attempts() {
        let temp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(temp.path());
        let calls = Arc::new(AtomicUsize::new(0));
        let http = HttpClient::with_backend(Box::new(Backend {
            calls: calls.clone(),
            status: 200,
            body: format!(
                r#"{{"playerstats":{{"success":true,"steamID":"{ID}","achievements":[{{"apiname":"A","achieved":1}},{{"apiname":"B","achieved":0}}]}}}}"#
            ),
        }));
        let ids: Vec<_> = (1..=1500).collect();
        let (summary, result) = refresh_batch(
            &cache,
            &http,
            Some("secret"),
            ID,
            BatchOptions {
                app_ids: &ids,
                limit: 12,
                force: true,
                offline: false,
            },
            &RefreshControl::default(),
        )
        .unwrap();
        assert_eq!(summary.selected, 12);
        assert_eq!(summary.available, 12);
        assert_eq!(summary.attempted, 12);
        assert_eq!(calls.load(Ordering::SeqCst), 24);
        assert_eq!(
            result.iter().map(|p| p.app_id).collect::<Vec<_>>(),
            (1..=12).collect::<Vec<_>>()
        );
        assert!(
            refresh_batch(
                &cache,
                &http,
                Some("secret"),
                ID,
                BatchOptions {
                    app_ids: &ids,
                    limit: 13,
                    force: true,
                    offline: false
                },
                &RefreshControl::default()
            )
            .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 24);
    }
}
