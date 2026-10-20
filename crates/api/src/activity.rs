//! Optional, account-scoped Steam activity. Reads never fetch; an explicit
//! refresh makes at most two documented user-key requests.

use std::collections::HashSet;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use vapourfly_core::error::{Result, VapourflyError};

use crate::cache::DiskCache;
use crate::http::{CacheRecord, HttpClient};

pub const SOURCE: &str = "steam-activity";
const TTL: Duration = Duration::from_secs(24 * 3600);
const BASE: &str = "https://api.steampowered.com/IPlayerService";

#[derive(Clone, Debug, Default)]
pub struct RefreshControl(Arc<AtomicBool>);

impl RefreshControl {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    pub fn check(&self) -> Result<()> {
        if self.0.load(Ordering::Acquire) {
            Err(VapourflyError::InvalidInput(
                "Account refresh cancelled".into(),
            ))
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Availability {
    Available,
    Empty,
    Incomplete,
    RestrictedOrUnavailable,
    Unauthorized,
    RateLimited,
    TransportFailure,
    Malformed,
    HttpError,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityGame {
    pub app_id: u32,
    pub name: Option<String>,
    pub lifetime_minutes: Option<u32>,
    pub recent_minutes: Option<u32>,
    pub last_played_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivitySection {
    pub availability: Availability,
    pub checked_at: DateTime<Utc>,
    /// Last valid data time, not the time of a failed refresh attempt.
    pub fetched_at: Option<DateTime<Utc>>,
    pub reported_count: Option<u32>,
    pub games: Vec<ActivityGame>,
}

impl ActivitySection {
    pub fn stale(&self) -> bool {
        self.fetched_at.is_none_or(|time| {
            Utc::now().signed_duration_since(time).num_seconds() > TTL.as_secs() as i64
        }) || !matches!(
            self.availability,
            Availability::Available | Availability::Empty
        )
    }

    pub fn summary(&self) -> String {
        format!(
            "{:?}; {}/{} games; {} lifetime, {} recent playtime fields; data {}; {}",
            self.availability,
            self.games.len(),
            self.reported_count
                .map_or_else(|| "?".into(), |n| n.to_string()),
            self.games
                .iter()
                .filter(|g| g.lifetime_minutes.is_some())
                .count(),
            self.games
                .iter()
                .filter(|g| g.recent_minutes.is_some())
                .count(),
            self.fetched_at
                .map_or_else(|| "never".into(), |t| t.to_rfc3339()),
            if self.stale() {
                "stale/limited"
            } else {
                "fresh"
            },
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ActivitySnapshot {
    pub steam_id64: String,
    pub owned: ActivitySection,
    pub recent: ActivitySection,
}

impl ActivitySnapshot {
    pub fn summary(&self) -> String {
        format!(
            "Owned: {} | Recent: {}",
            self.owned.summary(),
            self.recent.summary()
        )
    }
}

pub fn validate_steam_id64(id: &str) -> Result<()> {
    let base = vapourfly_core::steam::STEAM_ID64_BASE;
    if id.len() != 17
        || !id.bytes().all(|b| b.is_ascii_digit())
        || !id
            .parse::<u64>()
            .is_ok_and(|n| n > base && n <= base + u64::from(u32::MAX))
    {
        return Err(VapourflyError::InvalidInput(
            "Invalid individual SteamID64".into(),
        ));
    }
    Ok(())
}

pub(crate) fn encode_query(value: &str) -> String {
    use std::fmt::Write;
    let mut result = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            result.push(char::from(b));
        } else {
            write!(result, "%{b:02X}").expect("writing to String");
        }
    }
    result
}

pub fn load(cache: &DiskCache, id: &str) -> Result<Option<ActivitySnapshot>> {
    validate_steam_id64(id)?;
    let record = match cache.get::<ActivitySnapshot>(SOURCE, id) {
        Ok(record) => record,
        // Unparseable or foreign records cannot establish account evidence.
        Err(VapourflyError::ParseError { .. }) => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(record
        .filter(|record| record.data.steam_id64 == id)
        .map(|record| record.data))
}

#[derive(Deserialize)]
struct Envelope {
    response: Response,
}

#[derive(Deserialize)]
struct Response {
    #[serde(alias = "total_count")]
    game_count: Option<u32>,
    games: Option<Vec<WireGame>>,
}

#[derive(Deserialize)]
struct WireGame {
    appid: u32,
    name: Option<String>,
    playtime_forever: Option<u32>,
    playtime_2weeks: Option<u32>,
    rtime_last_played: Option<i64>,
}

fn parse(body: &[u8]) -> std::result::Result<ActivitySection, Availability> {
    let response = serde_json::from_slice::<Envelope>(body)
        .map_err(|_| Availability::Malformed)?
        .response;
    let count = response.game_count;
    let Some(games) = response.games.or_else(|| (count == Some(0)).then(Vec::new)) else {
        return Err(if count.is_none() {
            Availability::RestrictedOrUnavailable
        } else {
            Availability::Incomplete
        });
    };
    let mut seen = HashSet::new();
    let mut duplicate = false;
    let games: Vec<_> = games
        .into_iter()
        .filter_map(|game| {
            if game.appid == 0 || !seen.insert(game.appid) {
                duplicate = true;
                return None;
            }
            Some(ActivityGame {
                app_id: game.appid,
                name: game.name,
                lifetime_minutes: game.playtime_forever,
                recent_minutes: game.playtime_2weeks,
                last_played_at: game
                    .rtime_last_played
                    .filter(|&t| t > 0 && t <= Utc::now().timestamp()),
            })
        })
        .collect();
    let availability = if duplicate || count.map(u64::from) != Some(games.len() as u64) {
        Availability::Incomplete
    } else if games.is_empty() {
        Availability::Empty
    } else {
        Availability::Available
    };
    Ok(ActivitySection {
        availability,
        checked_at: Utc::now(),
        fetched_at: Some(Utc::now()),
        reported_count: count,
        games,
    })
}

fn fetch(http: &HttpClient, url: &str, previous: Option<ActivitySection>) -> ActivitySection {
    let outcome = match http.get_once("steam-web", url) {
        Ok(response) => match response.status {
            200 => parse(&response.body),
            401 => Err(Availability::Unauthorized),
            403 => Err(Availability::RestrictedOrUnavailable),
            429 => Err(Availability::RateLimited),
            _ => Err(Availability::HttpError),
        },
        Err(_) => Err(Availability::TransportFailure),
    };
    outcome.unwrap_or_else(|availability| ActivitySection {
        availability,
        checked_at: Utc::now(),
        fetched_at: previous.as_ref().and_then(|s| s.fetched_at),
        reported_count: previous.as_ref().and_then(|s| s.reported_count),
        games: previous.map_or_else(Vec::new, |s| s.games),
    })
}

/// Explicit refresh only. Missing credentials/offline returns cached data
/// unchanged; response availability is recorded without persisting keys/URLs.
pub fn refresh(
    cache: &DiskCache,
    http: &HttpClient,
    api_key: Option<&str>,
    id: &str,
    offline: bool,
    control: &RefreshControl,
) -> Result<Option<ActivitySnapshot>> {
    validate_steam_id64(id)?;
    let Some(key) = api_key.filter(|key| !key.is_empty()) else {
        return load(cache, id);
    };
    if offline {
        return load(cache, id);
    }
    let path = cache.path_for(SOURCE, id);
    let _owner = vapourfly_core::personal_store::lock_account(
        path.parent().expect("activity cache directory"),
        id,
    )?;
    let previous = load(cache, id)?;
    control.check()?;
    let owned_input = encode_query(
        &serde_json::json!({
            "steamid": id, "include_appinfo": true, "include_played_free_games": true,
        })
        .to_string(),
    );
    let recent_input = encode_query(&serde_json::json!({"steamid": id, "count": 0}).to_string());
    let key = encode_query(key);
    let owned = fetch(
        http,
        &format!("{BASE}/GetOwnedGames/v1/?key={key}&input_json={owned_input}"),
        previous.as_ref().map(|s| s.owned.clone()),
    );
    control.check()?;
    let recent = fetch(
        http,
        &format!("{BASE}/GetRecentlyPlayedGames/v1/?key={key}&input_json={recent_input}"),
        previous.map(|s| s.recent),
    );
    control.check()?;
    let snapshot = ActivitySnapshot {
        steam_id64: id.into(),
        owned,
        recent,
    };
    cache.put(&CacheRecord {
        source: SOURCE.into(),
        key: id.into(),
        fetched_at: Utc::now(),
        ttl: TTL,
        data: snapshot.clone(),
        stale: false,
        etag: None,
    })?;
    Ok(Some(snapshot))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpBackend, HttpResponse, MockBackend};
    use std::sync::{Arc, atomic::AtomicUsize};
    use tempfile::TempDir;

    const FIRST: &str = "76561198000000001";
    const SECOND: &str = "76561198000000002";

    fn response(body: &str) -> HttpResponse {
        HttpResponse {
            status: 200,
            headers: Default::default(),
            body: body.as_bytes().to_vec(),
        }
    }

    fn cache() -> (DiskCache, TempDir) {
        let dir = TempDir::new().unwrap();
        (DiskCache::new(dir.path()), dir)
    }

    #[test]
    fn parses_activity_and_deduplicates_ids_without_mixing_accounts() {
        let (cache, _dir) = cache();
        let mut backend = MockBackend::new();
        backend.register(
            "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/",
            response(r#"{"response":{"game_count":2,"games":[{"appid":10,"name":"A","playtime_forever":120},{"appid":10,"name":"A duplicate","playtime_forever":999},{"appid":20}]}}"#),
        );
        backend.register(
            "https://api.steampowered.com/IPlayerService/GetRecentlyPlayedGames/v1/",
            response(r#"{"response":{"total_count":0}}"#),
        );
        let client = HttpClient::with_backend(Box::new(backend));
        let snapshot = refresh(
            &cache,
            &client,
            Some("secret-key"),
            FIRST,
            false,
            &RefreshControl::default(),
        )
        .unwrap()
        .unwrap();

        assert_eq!(snapshot.steam_id64, FIRST);
        assert_eq!(snapshot.owned.availability, Availability::Incomplete);
        assert_eq!(snapshot.owned.games.len(), 2);
        assert_eq!(snapshot.recent.availability, Availability::Empty);
        assert!(load(&cache, SECOND).unwrap().is_none());
        assert_eq!(load(&cache, FIRST).unwrap().unwrap().steam_id64, FIRST);
    }

    #[test]
    fn distinguishes_auth_visibility_malformed_and_transport_failures() {
        let (cache, _dir) = cache();
        for (status, expected) in [
            (401, Availability::Unauthorized),
            (403, Availability::RestrictedOrUnavailable),
            (429, Availability::RateLimited),
        ] {
            let mut backend = MockBackend::new();
            backend.register(
                "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/",
                HttpResponse {
                    status,
                    ..response("")
                },
            );
            backend.register(
                "https://api.steampowered.com/IPlayerService/GetRecentlyPlayedGames/v1/",
                response(r#"{"response":{"total_count":0}}"#),
            );
            let snapshot = refresh(
                &cache,
                &HttpClient::with_backend(Box::new(backend)),
                Some("key"),
                FIRST,
                false,
                &RefreshControl::default(),
            )
            .unwrap()
            .unwrap();
            assert_eq!(snapshot.owned.availability, expected);
        }

        let mut backend = MockBackend::new();
        backend.register(
            "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/",
            response("not-json"),
        );
        backend.register_error(
            "https://api.steampowered.com/IPlayerService/GetRecentlyPlayedGames/v1/",
            "offline",
        );
        let snapshot = refresh(
            &cache,
            &HttpClient::with_backend(Box::new(backend)),
            Some("key"),
            SECOND,
            false,
            &RefreshControl::default(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(snapshot.owned.availability, Availability::Malformed);
        assert_eq!(snapshot.recent.availability, Availability::TransportFailure);
    }

    #[test]
    fn offline_and_missing_key_make_zero_requests_and_preserve_cache() {
        let (cache, _dir) = cache();
        let mut backend = MockBackend::new();
        backend.register_error("https://", "must not be called");
        let client = HttpClient::with_backend(Box::new(backend));
        assert!(
            refresh(
                &cache,
                &client,
                None,
                FIRST,
                false,
                &RefreshControl::default()
            )
            .unwrap()
            .is_none()
        );
        assert!(
            refresh(
                &cache,
                &client,
                Some("key"),
                FIRST,
                true,
                &RefreshControl::default()
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn request_debug_and_failures_do_not_expose_key_or_url() {
        let request = crate::http::HttpRequest {
            method: crate::http::HttpMethod::Get,
            url: "https://api.steampowered.com/?key=secret".into(),
            headers: Default::default(),
            body: None,
        };
        let debug = format!("{request:?}");
        assert!(!debug.contains("secret"));
        assert!(!debug.contains("key="));
    }

    struct CountingBackend {
        calls: Arc<AtomicUsize>,
        response: HttpResponse,
    }

    impl HttpBackend for CountingBackend {
        fn execute(&self, _request: &crate::http::HttpRequest) -> Result<HttpResponse> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(self.response.clone())
        }
    }

    #[test]
    fn refresh_makes_one_attempt_per_endpoint_without_retries() {
        let (cache, _dir) = cache();
        let calls = Arc::new(AtomicUsize::new(0));
        let client = HttpClient::with_backend(Box::new(CountingBackend {
            calls: Arc::clone(&calls),
            response: HttpResponse {
                status: 429,
                headers: Default::default(),
                body: Vec::new(),
            },
        }));

        let snapshot = refresh(
            &cache,
            &client,
            Some("secret-key"),
            FIRST,
            false,
            &RefreshControl::default(),
        )
        .unwrap()
        .unwrap();

        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(snapshot.owned.availability, Availability::RateLimited);
        assert_eq!(snapshot.recent.availability, Availability::RateLimited);
    }

    #[test]
    fn invalid_cached_json_does_not_block_an_explicit_refresh() {
        let (cache, _dir) = cache();
        let path = cache.path_for(SOURCE, FIRST);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"not-json").unwrap();
        assert!(load(&cache, FIRST).unwrap().is_none());

        let mut backend = MockBackend::new();
        backend.register(
            "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/",
            response(r#"{"response":{"game_count":0}}"#),
        );
        backend.register(
            "https://api.steampowered.com/IPlayerService/GetRecentlyPlayedGames/v1/",
            response(r#"{"response":{"total_count":0}}"#),
        );
        let snapshot = refresh(
            &cache,
            &HttpClient::with_backend(Box::new(backend)),
            Some("key"),
            FIRST,
            false,
            &RefreshControl::default(),
        )
        .unwrap()
        .unwrap();

        assert_eq!(snapshot.owned.availability, Availability::Empty);
        assert_eq!(
            load(&cache, FIRST).unwrap().unwrap().owned.availability,
            Availability::Empty
        );
        let foreign = ActivitySnapshot {
            steam_id64: SECOND.into(),
            ..snapshot
        };
        cache
            .put(&CacheRecord {
                source: SOURCE.into(),
                key: FIRST.into(),
                data: foreign,
                fetched_at: Utc::now(),
                ttl: TTL,
                stale: false,
                etag: None,
            })
            .unwrap();
        assert!(load(&cache, FIRST).unwrap().is_none());
        assert!(load(&cache, "invalid").is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(matches!(
            load(&cache, FIRST),
            Err(VapourflyError::FileNotFound { .. })
        ));
    }

    #[test]
    fn parses_large_owned_library_without_request_retries_or_duplicate_rows() {
        let (cache, _dir) = cache();
        let mut games = String::with_capacity(300_000);
        games.push_str(r#"{"response":{"game_count":5000,"games":["#);
        for app_id in 1..=5000 {
            if app_id > 1 {
                games.push(',');
            }
            games.push_str(&format!(
                r#"{{"appid":{app_id},"name":"Game {app_id}","playtime_forever":{app_id}}}"#
            ));
        }
        games.push_str("]}}");

        let mut backend = MockBackend::new();
        backend.register(
            "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/",
            response(&games),
        );
        backend.register(
            "https://api.steampowered.com/IPlayerService/GetRecentlyPlayedGames/v1/",
            response(r#"{"response":{"total_count":0}}"#),
        );
        let snapshot = refresh(
            &cache,
            &HttpClient::with_backend(Box::new(backend)),
            Some("key"),
            FIRST,
            false,
            &RefreshControl::default(),
        )
        .unwrap()
        .unwrap();

        assert_eq!(snapshot.owned.availability, Availability::Available);
        assert_eq!(snapshot.owned.reported_count, Some(5000));
        assert_eq!(snapshot.owned.games.len(), 5000);
        assert_eq!(snapshot.owned.games[4999].app_id, 5000);
    }
}
