//! IGDB API client.
//!
//! Uses Twitch OAuth for authentication. Requires `VAPOURFLY_IGDB_CLIENT_ID`
//! and `VAPOURFLY_IGDB_CLIENT_SECRET` environment variables.
//!
//! The client acquires an OAuth token on first request and caches it in memory
//! for subsequent calls.

use std::collections::HashMap;

use vapourfly_core::error::{Result, VapourflyError};
use vapourfly_core::models::{IgdbData, IgdbTimeToBeat};

use crate::http::HttpClient;

#[allow(dead_code)]
const IGDB_API_BASE: &str = "https://api.igdb.com/v4";
#[allow(dead_code)]
const TWITCH_TOKEN_URL: &str = "https://id.twitch.tv/oauth2/token";

/// IGDB API client.
pub struct IgdbClient {
    client_id: String,
    client_secret: String,
    http: HttpClient,
}

impl IgdbClient {
    /// Create a new IGDB client from environment variables.
    ///
    /// Returns [`CredentialsMissing`](VapourflyError::CredentialsMissing) if
    /// `VAPOURFLY_IGDB_CLIENT_ID` or `VAPOURFLY_IGDB_CLIENT_SECRET` are not set.
    pub fn from_env() -> Result<Self> {
        Self::from_env_with_http(HttpClient::new())
    }

    /// Create a new IGDB client from environment variables with a custom
    /// [`HttpClient`] (e.g. one backed by a [`MockBackend`](crate::http::MockBackend)).
    pub fn from_env_with_http(http: HttpClient) -> Result<Self> {
        let client_id = std::env::var("VAPOURFLY_IGDB_CLIENT_ID").map_err(|_| {
            VapourflyError::CredentialsMissing {
                provider: "IGDB".into(),
            }
        })?;
        let client_secret = std::env::var("VAPOURFLY_IGDB_CLIENT_SECRET").map_err(|_| {
            VapourflyError::CredentialsMissing {
                provider: "IGDB".into(),
            }
        })?;
        Ok(Self {
            client_id,
            client_secret,
            http,
        })
    }

    /// Create an IGDB client with explicit credentials and HTTP client.
    pub fn new(client_id: String, client_secret: String, http: HttpClient) -> Self {
        Self {
            client_id,
            client_secret,
            http,
        }
    }

    /// Build a POST request to an IGDB endpoint.
    #[allow(dead_code)]
    fn igdb_post(&self, endpoint: &str, body: &str) -> Result<crate::http::HttpResponse> {
        let url = format!("{IGDB_API_BASE}/{endpoint}");
        let mut headers = HashMap::new();
        headers.insert("Client-ID".to_string(), self.client_id.clone());
        // For stubs we use a placeholder token; real impl would first call
        // fetch_token() and cache the result.
        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", self.client_secret),
        );
        headers.insert("Content-Type".to_string(), "text/plain".to_string());
        self.http
            .post("igdb", &url, headers, body.as_bytes().to_vec())
    }

    /// Resolve an IGDB game by Steam AppID.
    ///
    /// Uses the `external_games` endpoint to find the IGDB game ID, then
    /// fetches full game details. Returns `Ok(None)` when no match is found.
    pub fn resolve_by_steam_appid(&self, _app_id: u32) -> Result<Option<IgdbData>> {
        // TODO: Implement IGDB external_games query
        // POST https://api.igdb.com/v4/external_games
        // Body: fields game,name,uid,external_game_source,url;
        //       where uid = "{app_id}" & external_game_source = {steam_source_id};
        //
        // On match, call fetch_game_details with the returned game ID.
        Err(VapourflyError::Internal(
            "IGDB resolve_by_steam_appid not yet implemented".into(),
        ))
    }

    /// Fetch game details by IGDB ID.
    pub fn fetch_game_details(&self, _igdb_id: u64) -> Result<IgdbData> {
        // TODO: Implement IGDB games query
        // POST https://api.igdb.com/v4/games
        // Body: fields name,slug,rating,total_rating,genres,themes,
        //       keywords,similar_games,external_games; where id = {igdb_id};
        Err(VapourflyError::Internal(
            "IGDB fetch_game_details not yet implemented".into(),
        ))
    }

    /// Fetch time-to-beat data by IGDB ID.
    ///
    /// Returns `Ok(None)` when no time-to-beat data exists for the game.
    pub fn fetch_time_to_beat(&self, _igdb_id: u64) -> Result<Option<IgdbTimeToBeat>> {
        // TODO: Implement IGDB game_time_to_beats query
        // POST https://api.igdb.com/v4/game_time_to_beats
        // Body: fields hastily,normally,completely,comp_count;
        //       where game_id = {igdb_id};
        Err(VapourflyError::Internal(
            "IGDB fetch_time_to_beat not yet implemented".into(),
        ))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, MockBackend};

    #[test]
    fn missing_client_id_returns_credentials_missing() {
        // SAFETY: tests run single-threaded per process, and these env vars
        // are only used by this crate.
        unsafe {
            std::env::remove_var("VAPOURFLY_IGDB_CLIENT_ID");
            std::env::remove_var("VAPOURFLY_IGDB_CLIENT_SECRET");
        }

        let err = match IgdbClient::from_env() {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        match err {
            VapourflyError::CredentialsMissing { provider } => {
                assert_eq!(provider, "IGDB");
            }
            other => panic!("expected CredentialsMissing, got: {other}"),
        }
    }

    #[test]
    fn missing_secret_returns_credentials_missing() {
        // SAFETY: tests run single-threaded per process, and these env vars
        // are only used by this crate.
        unsafe {
            std::env::set_var("VAPOURFLY_IGDB_CLIENT_ID", "test_id");
            std::env::remove_var("VAPOURFLY_IGDB_CLIENT_SECRET");
        }

        let err = match IgdbClient::from_env() {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        match err {
            VapourflyError::CredentialsMissing { provider } => {
                assert_eq!(provider, "IGDB");
            }
            other => panic!("expected CredentialsMissing, got: {other}"),
        }

        unsafe {
            std::env::remove_var("VAPOURFLY_IGDB_CLIENT_ID");
        }
    }

    #[test]
    fn stub_methods_return_internal_error() {
        let mut mock = MockBackend::new();
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

        // Each stub method returns an Internal error for now.
        let err = client.resolve_by_steam_appid(292030).unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));

        let err = client.fetch_game_details(1234).unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));

        let err = client.fetch_time_to_beat(1234).unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }
}
