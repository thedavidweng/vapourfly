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

impl Progress {
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
}
