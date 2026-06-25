//! PCGamingWiki API client.
//!
//! Uses the MediaWiki/Cargo API. No credentials required.
//!
//! Missing data maps to [`ControllerSupport::Unknown`].

use vapourfly_core::error::{Result, VapourflyError};
use vapourfly_core::models::{ControllerSupport, PcgwData};

use crate::http::HttpClient;

/// PCGamingWiki API base URL (MediaWiki Cargo query endpoint).
const PCGW_API_BASE: &str = "https://www.pcgamingwiki.com/w/api.php";

/// PCGamingWiki API client.
pub struct PcgwClient {
    http: HttpClient,
}

impl Default for PcgwClient {
    fn default() -> Self {
        Self::new()
    }
}

impl PcgwClient {
    /// Create a new PCGW client.
    pub fn new() -> Self {
        Self {
            http: HttpClient::new(),
        }
    }

    /// Create a PCGW client with a custom [`HttpClient`].
    pub fn with_http(http: HttpClient) -> Self {
        Self { http }
    }

    /// Fetch game data by Steam AppID.
    ///
    /// When the page is not found or data fields are missing, sensible defaults
    /// are returned (Unknown controller support, no notes/fixes URL).
    pub fn fetch_by_appid(&self, app_id: u32) -> Result<PcgwData> {
        let url = format!("{PCGW_API_BASE}?action=ask&query=[[Steam_AppID::{app_id}]]&format=json");

        let response = self.http.get("pcgw", &url)?;

        if response.status == 404 || response.body.is_empty() {
            return Ok(PcgwData {
                page_name: None,
                controller_support: ControllerSupport::Unknown,
                steam_deck_notes: None,
                fixes_url: None,
            });
        }

        // TODO: Parse the MediaWiki Cargo response and extract:
        // - Page name
        // - Controller support (full/partial/none)
        // - Steam Deck compatibility notes
        // - Fixes URL
        let _body = response.body_text();

        Err(VapourflyError::Internal(
            "PCGW fetch_by_appid response parsing not yet implemented".into(),
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
    use std::collections::HashMap;

    #[test]
    fn fetch_returns_unknown_on_404() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://www.pcgamingwiki.com/",
            HttpResponse {
                status: 404,
                headers: HashMap::new(),
                body: Vec::new(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = PcgwClient::with_http(http);

        let data = client.fetch_by_appid(999999).unwrap();
        assert_eq!(data.controller_support, ControllerSupport::Unknown);
        assert!(data.page_name.is_none());
    }

    #[test]
    fn fetch_returns_unknown_on_empty_body() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://www.pcgamingwiki.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: Vec::new(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = PcgwClient::with_http(http);

        let data = client.fetch_by_appid(292030).unwrap();
        assert_eq!(data.controller_support, ControllerSupport::Unknown);
    }

    #[test]
    fn fetch_returns_internal_when_parsing_stubbed() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://www.pcgamingwiki.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"{"query":{"results":{}}}"#.to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = PcgwClient::with_http(http);

        let err = client.fetch_by_appid(292030).unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }
}
