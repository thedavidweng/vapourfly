//! HLTB (HowLongToBeat) client.
//!
//! This is an optional module behind the `hltb_scrape` feature gate.
//! The default build works without HLTB -- `fetch()` returns `Ok(None)` when
//! the feature is not enabled.

use vapourfly_core::error::Result;
use vapourfly_core::models::HltbData;

use crate::http::HttpClient;

/// HLTB client.
///
/// When the `hltb_scrape` feature is enabled, this client can scrape
/// howlongtobeat.com for play-time data. Without the feature it acts as a
/// no-op stub.
pub struct HltbClient {
    #[allow(dead_code)]
    http: HttpClient,
}

impl Default for HltbClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HltbClient {
    /// Create a new HLTB client.
    pub fn new() -> Self {
        Self {
            http: HttpClient::new(),
        }
    }

    /// Create an HLTB client with a custom [`HttpClient`].
    pub fn with_http(http: HttpClient) -> Self {
        Self { http }
    }

    /// Fetch time-to-beat data for a game by name.
    ///
    /// Returns `Ok(None)` when no match is found or when the `hltb_scrape`
    /// feature is not enabled.
    #[cfg(feature = "hltb_scrape")]
    pub fn fetch(&self, name: &str) -> Result<Option<HltbData>> {
        // TODO: Implement HLTB scraping
        // 1. POST https://howlongtobeat.com/api/search with search body
        // 2. Parse the response to find the best match
        // 3. GET https://howlongtobeat.com/game/{id} for details
        // 4. Extract main_story, main_extra, completionist times
        let _ = name;
        let _ = &self.http;
        Ok(None)
    }

    /// Stub for when `hltb_scrape` feature is not enabled.
    #[cfg(not(feature = "hltb_scrape"))]
    pub fn fetch(&self, _name: &str) -> Result<Option<HltbData>> {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::MockBackend;

    #[test]
    fn stub_returns_none() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://howlongtobeat.com/",
            crate::http::HttpResponse {
                status: 200,
                headers: std::collections::HashMap::new(),
                body: Vec::new(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = HltbClient::with_http(http);

        let result = client.fetch("Test Game").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn default_constructor_works() {
        let client = HltbClient::new();
        let result = client.fetch("Any Game").unwrap();
        assert!(result.is_none());
    }
}
