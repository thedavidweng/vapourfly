//! RAWG API client.
//!
//! Requires the `VAPOURFLY_RAWG_KEY` environment variable.

use vapourfly_core::error::{Result, VapourflyError};
use vapourfly_core::models::RawgData;

use crate::http::HttpClient;

#[allow(dead_code)]
const RAWG_API_BASE: &str = "https://api.rawg.io/api";

/// RAWG API client.
pub struct RawgClient {
    api_key: String,
    http: HttpClient,
}

impl RawgClient {
    /// Create a new RAWG client from environment variables.
    ///
    /// Returns [`CredentialsMissing`](VapourflyError::CredentialsMissing) if
    /// `VAPOURFLY_RAWG_KEY` is not set.
    pub fn from_env() -> Result<Self> {
        Self::from_env_with_http(HttpClient::new())
    }

    /// Create a new RAWG client from environment variables with a custom
    /// [`HttpClient`] (e.g. one backed by a [`MockBackend`](crate::http::MockBackend)).
    pub fn from_env_with_http(http: HttpClient) -> Result<Self> {
        let api_key = std::env::var("VAPOURFLY_RAWG_KEY").map_err(|_| {
            VapourflyError::CredentialsMissing {
                provider: "RAWG".into(),
            }
        })?;
        Ok(Self { api_key, http })
    }

    /// Create a RAWG client with an explicit API key and HTTP client.
    pub fn new(api_key: String, http: HttpClient) -> Self {
        Self { api_key, http }
    }

    /// Search RAWG for a game by name and return the best match.
    ///
    /// Returns `Ok(None)` when no results are found.
    pub fn search_by_name(&self, name: &str) -> Result<Option<RawgData>> {
        let url = format!(
            "{RAWG_API_BASE}/games?key={}&search={}",
            self.api_key,
            encode_query(name)
        );

        let response = self.http.get("rawg", &url)?;

        if response.status == 404 || response.body.is_empty() {
            return Ok(None);
        }

        // TODO: Parse the RAWG JSON response and map to RawgData.
        // Response shape: { "results": [{ "id", "rating", "ratings_count",
        //   "genres": [{ "name" }], "tags": [{ "name" }],
        //   "stores": [{ "store": { "name" } }] }] }
        let _body = response.body_text();

        Err(VapourflyError::Internal(
            "RAWG search_by_name response parsing not yet implemented".into(),
        ))
    }
}

/// Minimal query-string percent encoding for spaces and common special chars.
fn encode_query(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, MockBackend};
    use std::collections::HashMap;

    #[test]
    fn missing_key_returns_credentials_missing() {
        // SAFETY: tests run single-threaded per process, and this env var
        // is only used by this crate.
        unsafe {
            std::env::remove_var("VAPOURFLY_RAWG_KEY");
        }

        let err = match RawgClient::from_env() {
            Ok(_) => panic!("expected error"),
            Err(e) => e,
        };
        match err {
            VapourflyError::CredentialsMissing { provider } => {
                assert_eq!(provider, "RAWG");
            }
            other => panic!("expected CredentialsMissing, got: {other}"),
        }
    }

    #[test]
    fn search_returns_none_on_404() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://api.rawg.io/",
            HttpResponse {
                status: 404,
                headers: HashMap::new(),
                body: Vec::new(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = RawgClient::new("test_key".into(), http);

        let result = client.search_by_name("nonexistent_game").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn search_returns_none_on_empty_body() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://api.rawg.io/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: Vec::new(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = RawgClient::new("test_key".into(), http);

        let result = client.search_by_name("empty").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn search_returns_internal_when_parsing_stubbed() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://api.rawg.io/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"{"results":[{"id":3498,"rating":4.56}]}"#.to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = RawgClient::new("test_key".into(), http);

        // Parsing not yet implemented, should return Internal.
        let err = client.search_by_name("GTA V").unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }

    #[test]
    fn encode_query_handles_spaces() {
        assert_eq!(encode_query("Half Life 2"), "Half+Life+2");
    }
}
