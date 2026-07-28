//! Steam Web API client (`api.steampowered.com`) — optional, key-gated.
//!
//! Valve removed the keyless `ISteamApps/GetAppList` endpoint and gated the
//! community games XML behind login, so the only supported bulk name source
//! left is `IPlayerService/GetOwnedGames` with a Steam Web API key
//! (free, from <https://steamcommunity.com/dev/apikey>).
//!
//! One request returns every owned game's AppID + name, which resolves all
//! placeholder names in seconds. Without a key, names backfill progressively
//! from per-app Steam Store hydration instead.

use std::collections::HashMap;
use std::time::Duration;

use vapourfly_core::error::{Result, VapourflyError};
use vapourfly_core::models::Game;

use crate::cache::DiskCache;
use crate::http::HttpClient;

/// Cache source name for the owned-games name map.
pub const SOURCE_STEAM_WEB: &str = "steam-web";

/// Owned-games map TTL: one bounded request per day at most.
const OWNED_GAMES_TTL: Duration = Duration::from_secs(24 * 3600);

const OWNED_GAMES_URL: &str = "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/";

#[derive(Debug, serde::Deserialize)]
struct OwnedGamesEnvelope {
    response: Option<OwnedGamesResponse>,
}

#[derive(Debug, serde::Deserialize)]
struct OwnedGamesResponse {
    #[serde(default)]
    games: Vec<OwnedGame>,
}

#[derive(Debug, serde::Deserialize)]
struct OwnedGame {
    appid: u32,
    #[serde(default)]
    name: Option<String>,
}

/// Fetch the AppID → name map for an account via `GetOwnedGames`.
fn fetch_owned_names(
    http: &HttpClient,
    api_key: &str,
    steam_id64: &str,
) -> Result<HashMap<u32, String>> {
    let url = format!(
        "{OWNED_GAMES_URL}?key={api_key}&steamid={steam_id64}\
         &include_appinfo=1&include_played_free_games=1&format=json"
    );
    let response = http.get("steam-web", &url)?;
    if response.status != 200 {
        return Err(VapourflyError::NetworkUnavailable {
            source: Box::new(std::io::Error::other(format!(
                "GetOwnedGames returned HTTP {} (is the API key valid and the profile visible?)",
                response.status
            ))),
        });
    }
    let envelope: OwnedGamesEnvelope =
        crate::http::parse_json(&response.body, "steam-web/owned-games.json")?;
    Ok(envelope
        .response
        .map(|r| {
            r.games
                .into_iter()
                .filter_map(|g| g.name.filter(|n| !n.is_empty()).map(|n| (g.appid, n)))
                .collect()
        })
        .unwrap_or_default())
}

/// Resolve placeholder game names from the Steam Web API, cache-first.
///
/// No-op (Ok, zero resolved) when `api_key` is `None`. The owned-games map
/// is cached under `steam-web/owned/<steamid>` with a 1-day TTL, so this
/// costs at most one bounded network request per day. A fetch failure
/// degrades to the stale cached map when one exists.
///
/// Returns how many names were resolved.
pub fn resolve_owned_names(
    games: &mut [Game],
    cache: &DiskCache,
    http: &HttpClient,
    api_key: Option<&str>,
    steam_id64: &str,
    offline: bool,
) -> usize {
    if !games.iter().any(|g| g.has_placeholder_name()) {
        return 0;
    }
    let cache_key = format!("owned/{steam_id64}");

    let cached = cache
        .get::<HashMap<u32, String>>(SOURCE_STEAM_WEB, &cache_key)
        .ok()
        .flatten();
    let fresh_cached = cached.as_ref().is_some_and(|r| !r.stale);

    let map: Option<HashMap<u32, String>> = match api_key {
        Some(key) if !fresh_cached && !offline => match fetch_owned_names(http, key, steam_id64) {
            Ok(map) => {
                let record = crate::http::CacheRecord {
                    source: SOURCE_STEAM_WEB.to_string(),
                    key: cache_key.clone(),
                    fetched_at: chrono::Utc::now(),
                    ttl: OWNED_GAMES_TTL,
                    data: map.clone(),
                    stale: false,
                    etag: None,
                };
                if let Err(e) = cache.put(&record) {
                    tracing::warn!(error = %e, "failed to cache owned-games map");
                }
                Some(map)
            }
            Err(e) => {
                tracing::warn!(error = %e, "GetOwnedGames failed; using stale cache if any");
                cached.map(|r| r.data)
            }
        },
        _ => cached.map(|r| r.data),
    };

    let Some(map) = map else { return 0 };
    let mut resolved = 0;
    for game in games.iter_mut() {
        if game.has_placeholder_name()
            && let Some(name) = map.get(&game.app_id)
        {
            game.name = name.clone();
            resolved += 1;
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, MockBackend};
    use tempfile::TempDir;
    use vapourfly_core::models::SteamAppType;

    fn game(app_id: u32, name: &str) -> Game {
        Game {
            app_id,
            name: name.into(),
            app_type: SteamAppType::Game,
            installed: false,
            install_dir: None,
            library_folder: None,
            playtime_minutes: None,
            playtime_2wks_minutes: None,
            playtime_disconnected_minutes: None,
            last_played_unix: None,
            steam_collections: vec![],
            is_hidden: false,
            is_junk: false,
            hltb: None,
            igdb: None,
            rawg: None,
            protondb: None,
            pcgw: None,
            steam_store: None,
        }
    }

    fn mock_http(body: &[u8], status: u16) -> HttpClient {
        let mut mock = MockBackend::new();
        mock.register(
            "https://api.steampowered.com/",
            HttpResponse {
                status,
                headers: Default::default(),
                body: body.to_vec(),
            },
        );
        HttpClient::with_backend(Box::new(mock))
    }

    const BODY: &[u8] = br#"{"response":{"game_count":2,"games":[
        {"appid":730,"name":"Counter-Strike 2","playtime_forever":14391},
        {"appid":384300,"name":"CPUCores :: Maximize Your FPS","playtime_forever":80740}
    ]}}"#;

    #[test]
    fn resolves_placeholders_and_caches_the_map() {
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        let http = mock_http(BODY, 200);

        let mut games = vec![
            game(730, "App 730"),
            game(384300, "App 384300"),
            game(999, "Resolved Local Name"),
        ];
        let n = resolve_owned_names(&mut games, &cache, &http, Some("k"), "765611", false);
        assert_eq!(n, 2);
        assert_eq!(games[0].name, "Counter-Strike 2");
        assert_eq!(games[1].name, "CPUCores :: Maximize Your FPS");
        assert_eq!(games[2].name, "Resolved Local Name");

        // Second run: served from cache — a failing network must not matter.
        let http_broken = mock_http(b"", 500);
        let mut games2 = vec![game(730, "App 730")];
        let n2 = resolve_owned_names(
            &mut games2,
            &cache,
            &http_broken,
            Some("k"),
            "765611",
            false,
        );
        assert_eq!(n2, 1);
        assert_eq!(games2[0].name, "Counter-Strike 2");
    }

    #[test]
    fn no_key_and_no_cache_is_a_noop() {
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        let http = mock_http(b"", 500); // any network call would fail

        let mut games = vec![game(730, "App 730")];
        assert_eq!(
            resolve_owned_names(&mut games, &cache, &http, None, "765611", false),
            0
        );
        assert_eq!(games[0].name, "App 730");
    }

    #[test]
    fn offline_uses_cache_only() {
        let tmp = TempDir::new().unwrap();
        let cache = DiskCache::new(tmp.path());
        // Warm the cache online first.
        let mut games = vec![game(730, "App 730")];
        resolve_owned_names(
            &mut games,
            &cache,
            &mock_http(BODY, 200),
            Some("k"),
            "765611",
            false,
        );

        // Offline with a broken network: cache still resolves.
        let mut games2 = vec![game(384300, "App 384300")];
        let n = resolve_owned_names(
            &mut games2,
            &cache,
            &mock_http(b"", 500),
            Some("k"),
            "765611",
            true,
        );
        assert_eq!(n, 1);
    }
}
