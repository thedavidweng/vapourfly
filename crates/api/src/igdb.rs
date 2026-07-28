//! IGDB API client.
//!
//! Uses Twitch OAuth for authentication with credentials supplied by the
//! caller (resolved from `VAPOURFLY_IGDB_CLIENT_ID` /
//! `VAPOURFLY_IGDB_CLIENT_SECRET` at the enrichment seam).
//!
//! The client acquires an OAuth token on first request and caches it in memory
//! for subsequent calls.

use std::collections::HashMap;
use std::sync::Mutex;

use serde::Deserialize;

use vapourfly_core::error::{Result, VapourflyError};
use vapourfly_core::models::{IgdbData, IgdbTimeToBeat};

use crate::http::{HttpClient, parse_json};

const IGDB_API_BASE: &str = "https://api.igdb.com/v4";
const TWITCH_TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";

/// IGDB API client.
pub struct IgdbClient {
    client_id: String,
    client_secret: String,
    http: HttpClient,
    token: Mutex<Option<CachedToken>>,
}

struct CachedToken {
    access_token: String,
    expires_at_unix: i64,
}

impl IgdbClient {
    /// Create an IGDB client with explicit credentials and HTTP client.
    pub fn new(client_id: String, client_secret: String, http: HttpClient) -> Self {
        Self {
            client_id,
            client_secret,
            http,
            token: Mutex::new(None),
        }
    }

    /// Get a valid access token, refreshing when less than an hour remains.
    fn get_token(&self) -> Result<String> {
        {
            let guard = self.token.lock().unwrap();
            if let Some(ref cached) = *guard {
                let now = chrono::Utc::now().timestamp();
                if cached.expires_at_unix - now > 3600 {
                    return Ok(cached.access_token.clone());
                }
            }
        }

        let url = format!(
            "{TWITCH_TOKEN_URL}?client_id={}&client_secret={}&grant_type=client_credentials",
            self.client_id, self.client_secret
        );

        let response = self
            .http
            .post("igdb-token", &url, HashMap::new(), Vec::new())?;

        if response.status != 200 {
            return Err(VapourflyError::Internal(format!(
                "IGDB token fetch failed with status {}",
                response.status
            )));
        }

        let token_resp: TwitchTokenResponse = parse_json(&response.body, "igdb/token.json")?;

        let expires_at_unix = chrono::Utc::now().timestamp() + token_resp.expires_in;

        {
            let mut guard = self.token.lock().unwrap();
            *guard = Some(CachedToken {
                access_token: token_resp.access_token.clone(),
                expires_at_unix,
            });
        }

        Ok(token_resp.access_token)
    }

    /// Build and execute a POST request to an IGDB endpoint.
    fn igdb_post(&self, endpoint: &str, body: &str) -> Result<crate::http::HttpResponse> {
        let token = self.get_token()?;
        let url = format!("{IGDB_API_BASE}/{endpoint}");
        let mut headers = HashMap::new();
        headers.insert("Client-ID".to_string(), self.client_id.clone());
        headers.insert("Authorization".to_string(), format!("Bearer {token}"));
        headers.insert("Content-Type".to_string(), "text/plain".to_string());
        self.http
            .post("igdb", &url, headers, body.as_bytes().to_vec())
    }

    /// Resolve an IGDB game by Steam AppID.
    ///
    /// Uses the `external_games` endpoint to find the IGDB game ID, then
    /// fetches full game details. Returns `Ok(None)` when no match is found.
    pub fn resolve_by_steam_appid(&self, app_id: u32) -> Result<Option<IgdbData>> {
        // external_game_source 1 is IGDB's ID for the Steam store.
        let query = format!(
            "fields game,name,uid,external_game_source,url; where uid = \"{app_id}\" & external_game_source = 1; limit 10;",
        );

        let response = self.igdb_post("external_games", &query)?;

        if response.status == 404 || response.body.is_empty() || response.body == b"[]" {
            return Ok(None);
        }

        let entries: Vec<ExternalGameEntry> =
            parse_json(&response.body, &format!("igdb/external_games/{app_id}.json"))?;

        let Some(entry) = entries.into_iter().next() else {
            return Ok(None);
        };
        let Some(igdb_id) = entry.game else {
            return Ok(None);
        };

        let mut data = self.fetch_game_details(igdb_id)?;

        // Time-to-beat is non-critical; log and continue on error.
        match self.fetch_time_to_beat(igdb_id) {
            Ok(Some(ttb)) => data.time_to_beat = Some(ttb),
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, igdb_id, "failed to fetch time-to-beat"),
        }

        if entry.external_game_source == Some(1) {
            data.steam_app_id_confirmed = true;
        }

        Ok(Some(data))
    }

    /// Fetch game details by IGDB ID.
    pub fn fetch_game_details(&self, igdb_id: u64) -> Result<IgdbData> {
        let query = format!(
            "fields id,name,slug,rating,total_rating,genres.name,themes.name,keywords.name,\
             similar_games,external_games,first_release_date,url; where id = {igdb_id}; limit 1;",
        );

        let response = self.igdb_post("games", &query)?;

        if response.status != 200 {
            return Err(VapourflyError::Internal(format!(
                "IGDB games query failed with status {}",
                response.status
            )));
        }

        let entries: Vec<IgdbGameEntry> =
            parse_json(&response.body, &format!("igdb/games/{igdb_id}.json"))?;

        let game = entries
            .into_iter()
            .next()
            .ok_or_else(|| VapourflyError::Internal(format!("IGDB game {igdb_id} not found")))?;

        Ok(IgdbData {
            igdb_id: game.id,
            name: game.name.unwrap_or_default(),
            slug: game.slug,
            rating_0_100: game.rating.map(|r| r as f32),
            total_rating_0_100: game.total_rating.map(|r| r as f32),
            genres: game.genres.into_iter().flatten().map(|g| g.name).collect(),
            themes: game.themes.into_iter().flatten().map(|t| t.name).collect(),
            keywords: game
                .keywords
                .into_iter()
                .flatten()
                .map(|k| k.name)
                .collect(),
            similar_game_ids: game.similar_games.unwrap_or_default(),
            steam_app_id_confirmed: false,
            time_to_beat: None,
        })
    }

    /// Fetch time-to-beat data by IGDB ID.
    ///
    /// Returns `Ok(None)` when no time-to-beat data exists for the game.
    pub fn fetch_time_to_beat(&self, igdb_id: u64) -> Result<Option<IgdbTimeToBeat>> {
        let query = format!(
            "fields game_id,hastily,normally,completely,comp_count,updated_at; \
             where game_id = {igdb_id}; limit 1;",
        );

        let response = self.igdb_post("game_time_to_beats", &query)?;

        if response.status != 200 || response.body.is_empty() || response.body == b"[]" {
            return Ok(None);
        }

        let entries: Vec<GameTimeToBeatEntry> =
            parse_json(&response.body, &format!("igdb/time_to_beat/{igdb_id}.json"))?;

        let Some(entry) = entries.into_iter().next() else {
            return Ok(None);
        };

        Ok(Some(IgdbTimeToBeat {
            hastily_seconds: entry.hastily,
            normally_seconds: entry.normally,
            completely_seconds: entry.completely,
            submission_count: entry.comp_count,
        }))
    }
}

// IGDB JSON response types. Unconsumed response fields are omitted — serde
// ignores unknown keys, so the queries can keep requesting them unchanged.

#[derive(Debug, Deserialize)]
struct TwitchTokenResponse {
    access_token: String,
    expires_in: i64,
}

#[derive(Debug, Deserialize)]
struct ExternalGameEntry {
    game: Option<u64>,
    external_game_source: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct IgdbGameEntry {
    id: u64,
    name: Option<String>,
    slug: Option<String>,
    rating: Option<f64>,
    total_rating: Option<f64>,
    genres: Option<Vec<IgdbNameEntry>>,
    themes: Option<Vec<IgdbNameEntry>>,
    keywords: Option<Vec<IgdbNameEntry>>,
    similar_games: Option<Vec<u64>>,
}

#[derive(Debug, Deserialize)]
struct IgdbNameEntry {
    name: String,
}

#[derive(Debug, Deserialize)]
struct GameTimeToBeatEntry {
    hastily: Option<u32>,
    normally: Option<u32>,
    completely: Option<u32>,
    comp_count: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, MockBackend};

    #[test]
    fn resolve_returns_none_on_empty_response() {
        let mut mock = MockBackend::new();
        // Token endpoint
        mock.register(
            "https://id.twitch.tv/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"{"access_token":"test_token","token_type":"bearer","expires_in":36000}"#
                    .to_vec(),
            },
        );
        // External games endpoint - empty result
        mock.register(
            "https://api.igdb.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"[]".to_vec(),
            },
        );

        let http = HttpClient::with_backend(Box::new(mock));
        let client = IgdbClient::new("id".into(), "secret".into(), http);

        let result = client.resolve_by_steam_appid(999999).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn fetch_game_details_parses_valid_response() {
        let mut mock = MockBackend::new();
        // Token endpoint
        mock.register(
            "https://id.twitch.tv/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"{"access_token":"test_token","token_type":"bearer","expires_in":36000}"#
                    .to_vec(),
            },
        );
        // Games endpoint
        mock.register(
            "https://api.igdb.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"[{"id":1942,"name":"The Witcher 3: Wild Hunt","slug":"the-witcher-3-wild-hunt","rating":93.0,"total_rating":92.0,"genres":[{"name":"Role-playing (RPG)"}],"themes":[{"name":"Fantasy"}],"keywords":[{"name":"open world"}],"similar_games":[1234,5678]}]"#.to_vec(),
            },
        );

        let http = HttpClient::with_backend(Box::new(mock));
        let client = IgdbClient::new("id".into(), "secret".into(), http);

        let data = client.fetch_game_details(1942).unwrap();
        assert_eq!(data.igdb_id, 1942);
        assert_eq!(data.name, "The Witcher 3: Wild Hunt");
        assert_eq!(data.rating_0_100, Some(93.0));
        assert_eq!(data.genres, vec!["Role-playing (RPG)"]);
        assert_eq!(data.themes, vec!["Fantasy"]);
        assert_eq!(data.keywords, vec!["open world"]);
        assert_eq!(data.similar_game_ids, vec![1234, 5678]);
    }

    #[test]
    fn fetch_time_to_beat_parses_valid_response() {
        let mut mock = MockBackend::new();
        // Token endpoint
        mock.register(
            "https://id.twitch.tv/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"{"access_token":"test_token","token_type":"bearer","expires_in":36000}"#
                    .to_vec(),
            },
        );
        // Time to beat endpoint
        mock.register(
            "https://api.igdb.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"[{"game_id":1942,"hastily":12000,"normally":36000,"completely":108000,"comp_count":500}]"#.to_vec(),
            },
        );

        let http = HttpClient::with_backend(Box::new(mock));
        let client = IgdbClient::new("id".into(), "secret".into(), http);

        let ttb = client.fetch_time_to_beat(1942).unwrap().unwrap();
        assert_eq!(ttb.hastily_seconds, Some(12000));
        assert_eq!(ttb.normally_seconds, Some(36000));
        assert_eq!(ttb.completely_seconds, Some(108000));
        assert_eq!(ttb.submission_count, Some(500));
    }

    #[test]
    fn fetch_time_to_beat_returns_none_on_empty() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://id.twitch.tv/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"{"access_token":"test_token","token_type":"bearer","expires_in":36000}"#
                    .to_vec(),
            },
        );
        mock.register(
            "https://api.igdb.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"[]".to_vec(),
            },
        );

        let http = HttpClient::with_backend(Box::new(mock));
        let client = IgdbClient::new("id".into(), "secret".into(), http);

        let result = client.fetch_time_to_beat(1234).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn resolve_full_chain_external_games_to_details_to_ttb() {
        // Covers the complete enrichment path:
        //   external_games (Steam AppID -> IGDB ID)
        //   -> games (IGDB ID -> full details)
        //   -> game_time_to_beats (IGDB ID -> time-to-beat)
        //
        // MockBackend matches by first prefix. Register most-specific
        // URL prefixes first so they aren't shadowed by the general one.
        let mut mock = MockBackend::new();

        // Token endpoint
        mock.register(
            "https://id.twitch.tv/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"{"access_token":"test_token","token_type":"bearer","expires_in":36000}"#
                    .to_vec(),
            },
        );
        // 1. external_games: Steam AppID 292030 -> IGDB game 1942
        mock.register(
            "https://api.igdb.com/v4/external_games",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"[{"game":1942,"uid":"292030","external_game_source":1}]"#.to_vec(),
            },
        );
        // 2. games: IGDB ID 1942 -> full game details
        mock.register(
            "https://api.igdb.com/v4/games",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"[{"id":1942,"name":"The Witcher 3: Wild Hunt","slug":"the-witcher-3-wild-hunt","rating":93.0,"total_rating":92.0,"genres":[{"name":"Role-playing (RPG)"}],"themes":[{"name":"Fantasy"}],"keywords":[{"name":"open world"}],"similar_games":[1234,5678]}]"#.to_vec(),
            },
        );
        // 3. game_time_to_beats: IGDB ID 1942 -> time-to-beat data
        mock.register(
            "https://api.igdb.com/v4/game_time_to_beats",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"[{"game_id":1942,"hastily":12000,"normally":36000,"completely":108000,"comp_count":500}]"#.to_vec(),
            },
        );

        let http = HttpClient::with_backend(Box::new(mock));
        let client = IgdbClient::new("id".into(), "secret".into(), http);

        // Steam AppID 292030 enters external_games, IGDB ID 1942 enters games.
        let result = client.resolve_by_steam_appid(292030).unwrap().unwrap();

        // Verify external_games mapping produced correct IGDB ID.
        assert_eq!(result.igdb_id, 1942);
        assert_eq!(result.name, "The Witcher 3: Wild Hunt");
        assert_eq!(result.rating_0_100, Some(93.0));
        assert_eq!(result.genres, vec!["Role-playing (RPG)"]);
        assert_eq!(result.themes, vec!["Fantasy"]);
        assert_eq!(result.keywords, vec!["open world"]);
        assert_eq!(result.similar_game_ids, vec![1234, 5678]);
        // Verify external_games source was Steam (source = 1).
        assert!(result.steam_app_id_confirmed);
        // Verify game_time_to_beats was fetched.
        let ttb = result.time_to_beat.unwrap();
        assert_eq!(ttb.hastily_seconds, Some(12000));
        assert_eq!(ttb.normally_seconds, Some(36000));
        assert_eq!(ttb.completely_seconds, Some(108000));
        assert_eq!(ttb.submission_count, Some(500));
    }
}
