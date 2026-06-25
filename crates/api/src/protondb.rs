//! ProtonDB API client.
//!
//! No credentials required. Uses the community data endpoint at
//! `https://www.protondb.com/api/v1/reports/summaries/{appid}.json`.
//!
//! A 404 or empty response maps to [`ProtonTier::Unknown`].

use serde::Deserialize;

use vapourfly_core::error::{Result, VapourflyError};
use vapourfly_core::models::{ProtonDbData, ProtonTier};

use crate::http::HttpClient;

const PROTONDB_API_BASE: &str = "https://www.protondb.com/api/v1/reports/summaries";

/// ProtonDB API client.
pub struct ProtonDbClient {
    http: HttpClient,
}

impl Default for ProtonDbClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ProtonDbClient {
    /// Create a new ProtonDB client.
    pub fn new() -> Self {
        Self {
            http: HttpClient::new(),
        }
    }

    /// Create a ProtonDB client with a custom [`HttpClient`].
    pub fn with_http(http: HttpClient) -> Self {
        Self { http }
    }

    /// Fetch compatibility summary for a Steam AppID.
    ///
    /// A 404 response or empty body returns a summary with
    /// [`ProtonTier::Unknown`].
    pub fn fetch_summary(&self, app_id: u32) -> Result<ProtonDbData> {
        let url = format!("{PROTONDB_API_BASE}/{app_id}.json");

        let response = self.http.get("protondb", &url)?;

        if response.status == 404 || response.body.is_empty() {
            return Ok(ProtonDbData {
                tier: ProtonTier::Unknown,
                confidence: None,
                score: None,
            });
        }

        let summary: ProtonDbSummary =
            serde_json::from_slice(&response.body).map_err(|e| VapourflyError::ParseError {
                path: vapourfly_core::error::SafePath::new(format!("protondb/{app_id}.json")),
                format: "JSON".into(),
                reason: e.to_string(),
            })?;

        Ok(ProtonDbData {
            tier: summary.tier(),
            confidence: Some(summary.confidence),
            score: summary.score,
        })
    }
}

/// Raw JSON response from the ProtonDB summary endpoint.
#[derive(Debug, Deserialize)]
struct ProtonDbSummary {
    tier: String,
    confidence: String,
    #[serde(default)]
    score: Option<f32>,
    #[allow(dead_code)]
    total: Option<u32>,
}

impl ProtonDbSummary {
    fn tier(&self) -> ProtonTier {
        match self.tier.as_str() {
            "borked" => ProtonTier::Borked,
            "bronze" => ProtonTier::Bronze,
            "silver" => ProtonTier::Silver,
            "gold" => ProtonTier::Gold,
            "platinum" => ProtonTier::Platinum,
            "native" => ProtonTier::Native,
            _ => ProtonTier::Unknown,
        }
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
    fn fetch_summary_returns_unknown_on_404() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://www.protondb.com/",
            HttpResponse {
                status: 404,
                headers: HashMap::new(),
                body: Vec::new(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = ProtonDbClient::with_http(http);

        let data = client.fetch_summary(999999).unwrap();
        assert_eq!(data.tier, ProtonTier::Unknown);
    }

    #[test]
    fn fetch_summary_returns_unknown_on_empty_body() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://www.protondb.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: Vec::new(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = ProtonDbClient::with_http(http);

        let data = client.fetch_summary(292030).unwrap();
        assert_eq!(data.tier, ProtonTier::Unknown);
    }

    #[test]
    fn fetch_summary_parses_valid_json() {
        let body = br#"{"tier":"gold","confidence":"high","score":0.92,"total":1500}"#;
        let mut mock = MockBackend::new();
        mock.register(
            "https://www.protondb.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: body.to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = ProtonDbClient::with_http(http);

        let data = client.fetch_summary(292030).unwrap();
        assert_eq!(data.tier, ProtonTier::Gold);
        assert_eq!(data.confidence, Some("high".to_string()));
        assert_eq!(data.score, Some(0.92));
    }

    #[test]
    fn fetch_summary_handles_unknown_tier() {
        let body = br#"{"tier":"whitelisted","confidence":"low","total":5}"#;
        let mut mock = MockBackend::new();
        mock.register(
            "https://www.protondb.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: body.to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = ProtonDbClient::with_http(http);

        let data = client.fetch_summary(12345).unwrap();
        assert_eq!(data.tier, ProtonTier::Unknown);
    }

    #[test]
    fn fetch_summary_returns_parse_error_on_invalid_json() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://www.protondb.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"not json".to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = ProtonDbClient::with_http(http);

        let err = client.fetch_summary(292030).unwrap_err();
        assert!(err.to_string().contains("JSON"));
    }
}
