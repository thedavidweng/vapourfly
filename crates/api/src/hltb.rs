//! HLTB (HowLongToBeat) client.
//!
//! This is an optional module behind the `hltb_scrape` feature gate.
//! The default build works without HLTB -- `fetch()` returns `Ok(None)` when
//! the feature is not enabled.
//!
//! When enabled, the client scrapes howlongtobeat.com's search API to find
//! play-time data for games. This is a best-effort approach since HLTB has
//! no official public API.

use vapourfly_core::error::Result;
use vapourfly_core::models::HltbData;

use crate::http::HttpClient;

/// HLTB client.
///
/// When the `hltb_scrape` feature is enabled, this client can query
/// howlongtobeat.com for play-time data. Without the feature it acts as a
/// no-op stub.
pub struct HltbClient {
    #[cfg(feature = "hltb_scrape")]
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
            #[cfg(feature = "hltb_scrape")]
            http: HttpClient::new(),
        }
    }

    /// Create an HLTB client with a custom [`HttpClient`].
    ///
    /// The client is unused (and dropped) when the `hltb_scrape` feature is
    /// disabled, so callers can wire HTTP uniformly across all sources.
    pub fn with_http(http: HttpClient) -> Self {
        #[cfg(not(feature = "hltb_scrape"))]
        let _ = http;
        Self {
            #[cfg(feature = "hltb_scrape")]
            http,
        }
    }

    /// Fetch time-to-beat data for a game by name.
    ///
    /// Returns `Ok(None)` when no match is found or when the `hltb_scrape`
    /// feature is not enabled.
    #[cfg(feature = "hltb_scrape")]
    pub fn fetch(&self, name: &str) -> Result<Option<HltbData>> {
        let search_body = HltbSearchRequest::new(name);
        let body_json = serde_json::to_vec(&search_body).map_err(|e| {
            vapourfly_core::error::VapourflyError::Internal(format!(
                "failed to serialize HLTB search request: {e}"
            ))
        })?;

        let response = self.http.post(
            "hltb",
            HLTB_SEARCH_URL,
            vec![("Content-Type".to_string(), "application/json".to_string())]
                .into_iter()
                .collect(),
            body_json,
        )?;

        if response.status != 200 || response.body.is_empty() {
            return Ok(None);
        }

        let search_result: HltbSearchResponse =
            serde_json::from_slice(&response.body).map_err(|e| {
                vapourfly_core::error::VapourflyError::ParseError {
                    path: vapourfly_core::error::SafePath::new("hltb/search.json"),
                    format: "JSON".into(),
                    reason: e.to_string(),
                }
            })?;

        // Find the best match by name similarity.
        let best = search_result
            .data
            .iter()
            .max_by(|a, b| {
                let a_sim = strsim::jaro_winkler(&a.game_name.to_lowercase(), &name.to_lowercase());
                let b_sim = strsim::jaro_winkler(&b.game_name.to_lowercase(), &name.to_lowercase());
                a_sim
                    .partial_cmp(&b_sim)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned();

        let game = match best {
            Some(g) => g,
            None => return Ok(None),
        };

        // Fetch detailed time data.
        let detail_url = format!("{HLTB_GAME_URL}/{}", game.game_id);
        let detail_response = self.http.get("hltb-detail", &detail_url)?;

        if detail_response.status != 200 || detail_response.body.is_empty() {
            // Fall back to search result data if detail fetch fails.
            return Ok(Some(game.to_hltb_data()));
        }

        let detail: HltbGameDetail =
            serde_json::from_slice(&detail_response.body).map_err(|e| {
                vapourfly_core::error::VapourflyError::ParseError {
                    path: vapourfly_core::error::SafePath::new(format!(
                        "hltb/game/{}.json",
                        game.game_id
                    )),
                    format: "JSON".into(),
                    reason: e.to_string(),
                }
            })?;

        Ok(Some(detail.to_hltb_data()))
    }

    /// Stub for when `hltb_scrape` feature is not enabled.
    #[cfg(not(feature = "hltb_scrape"))]
    pub fn fetch(&self, _name: &str) -> Result<Option<HltbData>> {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// HLTB JSON types (only compiled with hltb_scrape feature)
// ---------------------------------------------------------------------------

#[cfg(feature = "hltb_scrape")]
const HLTB_SEARCH_URL: &str = "https://howlongtobeat.com/api/search";

#[cfg(feature = "hltb_scrape")]
const HLTB_GAME_URL: &str = "https://howlongtobeat.com/api/games";

#[cfg(feature = "hltb_scrape")]
mod types {
    use serde::{Deserialize, Serialize};
    use vapourfly_core::models::{HltbData, HltbSource};

    #[derive(Debug, Serialize)]
    pub struct HltbSearchRequest {
        search_type: String,
        search_terms: Vec<String>,
        search_page: u32,
        size: u32,
        #[serde(rename = "searchOptions")]
        search_options: HltbSearchOptions,
    }

    impl HltbSearchRequest {
        pub fn new(name: &str) -> Self {
            Self {
                search_type: "games".into(),
                search_terms: vec![name.to_string()],
                search_page: 1,
                size: 5,
                search_options: HltbSearchOptions {
                    games: HltbGameFilter {
                        user_list: serde_json::Value::Null,
                        user_list_custom: serde_json::Value::Null,
                        platform: String::new(),
                        sort_category: "popular".into(),
                        range_category: "main".into(),
                        range_time: HltbRangeTime {
                            min: serde_json::Value::Null,
                            max: serde_json::Value::Null,
                        },
                        gameplay: HltbGameplay {
                            perspective: String::new(),
                            flow: String::new(),
                            genre: String::new(),
                        },
                        range_year: HltbRangeYear {
                            min: serde_json::Value::Null,
                            max: serde_json::Value::Null,
                        },
                        modifier: String::new(),
                    },
                    users: HltbUsersFilter {
                        sort_category: "postcount".into(),
                        range_time: HltbRangeTime {
                            min: serde_json::Value::Null,
                            max: serde_json::Value::Null,
                        },
                    },
                    filter: String::new(),
                    sort: 0,
                    randomizer: 0,
                },
            }
        }
    }

    #[derive(Debug, Serialize)]
    struct HltbSearchOptions {
        games: HltbGameFilter,
        users: HltbUsersFilter,
        filter: String,
        sort: u32,
        randomizer: u32,
    }

    #[derive(Debug, Serialize)]
    struct HltbGameFilter {
        user_list: serde_json::Value,
        user_list_custom: serde_json::Value,
        platform: String,
        sort_category: String,
        range_category: String,
        range_time: HltbRangeTime,
        gameplay: HltbGameplay,
        range_year: HltbRangeYear,
        modifier: String,
    }

    #[derive(Debug, Serialize)]
    struct HltbUsersFilter {
        sort_category: String,
        range_time: HltbRangeTime,
    }

    #[derive(Debug, Serialize)]
    struct HltbRangeTime {
        min: serde_json::Value,
        max: serde_json::Value,
    }

    #[derive(Debug, Serialize)]
    struct HltbGameplay {
        perspective: String,
        flow: String,
        genre: String,
    }

    #[derive(Debug, Serialize)]
    struct HltbRangeYear {
        min: serde_json::Value,
        max: serde_json::Value,
    }

    #[derive(Debug, Deserialize)]
    pub struct HltbSearchResponse {
        #[serde(default)]
        pub data: Vec<HltbSearchEntry>,
    }

    #[derive(Debug, Clone, Deserialize)]
    pub struct HltbSearchEntry {
        #[serde(rename = "game_id")]
        pub game_id: u64,
        #[serde(rename = "game_name")]
        pub game_name: String,
        #[serde(rename = "comp_main", default)]
        pub comp_main: Option<f64>,
        #[serde(rename = "comp_plus", default)]
        pub comp_plus: Option<f64>,
        #[serde(rename = "comp_100", default)]
        pub comp_100: Option<f64>,
    }

    impl HltbSearchEntry {
        pub fn to_hltb_data(&self) -> HltbData {
            HltbData {
                main_story_seconds: self.comp_main.and_then(seconds_from_hltb),
                main_extra_seconds: self.comp_plus.and_then(seconds_from_hltb),
                completionist_seconds: self.comp_100.and_then(seconds_from_hltb),
                source: HltbSource::HltbScrape,
            }
        }
    }

    #[derive(Debug, Deserialize)]
    pub struct HltbGameDetail {
        #[serde(default)]
        data: Option<HltbGameDetailData>,
    }

    #[derive(Debug, Deserialize)]
    struct HltbGameDetailData {
        #[serde(rename = "comp_main", default)]
        comp_main: Option<f64>,
        #[serde(rename = "comp_plus", default)]
        comp_plus: Option<f64>,
        #[serde(rename = "comp_100", default)]
        comp_100: Option<f64>,
    }

    impl HltbGameDetail {
        pub fn to_hltb_data(&self) -> HltbData {
            let d = self.data.as_ref();
            HltbData {
                main_story_seconds: d.and_then(|d| d.comp_main).and_then(seconds_from_hltb),
                main_extra_seconds: d.and_then(|d| d.comp_plus).and_then(seconds_from_hltb),
                completionist_seconds: d.and_then(|d| d.comp_100).and_then(seconds_from_hltb),
                source: HltbSource::HltbScrape,
            }
        }
    }

    pub fn seconds_from_hltb(val: f64) -> Option<u32> {
        if val > 0.0 { Some(val as u32) } else { None }
    }
}

#[cfg(feature = "hltb_scrape")]
use types::*;

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, MockBackend};
    use std::collections::HashMap;
    #[cfg(feature = "hltb_scrape")]
    use vapourfly_core::models::HltbSource;

    #[test]
    fn stub_returns_none_when_feature_disabled() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://howlongtobeat.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: Vec::new(),
            },
        );
        let client = HltbClient::with_http(HttpClient::with_backend(Box::new(mock)));

        let result = client.fetch("Test Game").unwrap();
        #[cfg(not(feature = "hltb_scrape"))]
        assert!(result.is_none());
        #[cfg(feature = "hltb_scrape")]
        assert!(result.is_none(), "empty body must parse to no match");
    }

    #[test]
    fn default_constructor_works() {
        let client = HltbClient::new();
        let result = client.fetch("Any Game").unwrap();
        assert!(result.is_none());
    }

    #[cfg(feature = "hltb_scrape")]
    #[test]
    fn seconds_from_hltb_converts_correctly() {
        assert_eq!(seconds_from_hltb(3600.0), Some(3600));
        assert_eq!(seconds_from_hltb(0.0), None);
        assert_eq!(seconds_from_hltb(-1.0), None);
    }

    #[cfg(feature = "hltb_scrape")]
    #[test]
    fn search_entry_to_hltb_data() {
        let entry = HltbSearchEntry {
            game_id: 1234,
            game_name: "Test Game".into(),
            comp_main: Some(18000.0),
            comp_plus: Some(36000.0),
            comp_100: Some(72000.0),
        };
        let data = entry.to_hltb_data();
        assert_eq!(data.main_story_seconds, Some(18000));
        assert_eq!(data.main_extra_seconds, Some(36000));
        assert_eq!(data.completionist_seconds, Some(72000));
        assert_eq!(data.source, HltbSource::HltbScrape);
    }

    #[cfg(feature = "hltb_scrape")]
    #[test]
    fn search_request_serializes() {
        let req = HltbSearchRequest::new("Witcher 3");
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("Witcher 3"));
        assert!(json.contains("games"));
    }
}
