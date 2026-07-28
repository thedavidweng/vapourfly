//! HLTB (HowLongToBeat) client.
//!
//! This is an optional module behind the `hltb_scrape` feature gate
//! (enabled by default). Without the feature, `fetch()` returns `Ok(None)`.
//!
//! When enabled, the client uses howlongtobeat.com's current (2026) search
//! flow, discovered from their app bundle since HLTB has no official API:
//!
//! 1. `GET /api/bleed/init?t=<millis>` → `{token, hpKey, hpVal}`. The token
//!    is bound to the caller's IP and User-Agent, so both requests must go
//!    through the same [`HttpClient`].
//! 2. `POST /api/bleed` with headers `x-auth-token` / `x-hp-key` /
//!    `x-hp-val` **and** the dynamic `{[hpKey]: hpVal}` pair injected into
//!    the JSON body. The response carries `comp_main` / `comp_plus` /
//!    `comp_100` in seconds.
//!
//! The previous `/api/search` + `/api/games` endpoints were removed by HLTB.
//! The session trio is cached per client and refreshed once on rejection.

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
    /// Cached bleed session (token, hpKey, hpVal); refreshed on rejection.
    #[cfg(feature = "hltb_scrape")]
    session: std::sync::Mutex<Option<types::BleedSession>>,
}

impl Default for HltbClient {
    fn default() -> Self {
        Self::new()
    }
}

impl HltbClient {
    /// Create a new HLTB client.
    pub fn new() -> Self {
        Self::with_http(HttpClient::new())
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
            #[cfg(feature = "hltb_scrape")]
            session: std::sync::Mutex::new(None),
        }
    }

    /// Fetch time-to-beat data for a game by name.
    ///
    /// Returns `Ok(None)` when no match is found or when the `hltb_scrape`
    /// feature is not enabled.
    #[cfg(feature = "hltb_scrape")]
    pub fn fetch(&self, name: &str) -> Result<Option<HltbData>> {
        // First attempt with the cached session; on a rejected/expired
        // session (non-200), refresh once and retry.
        let session = match self.current_session()? {
            Some(s) => s,
            None => return Ok(None), // init unavailable — degrade
        };
        match self.search(&session, name)? {
            SearchOutcome::Done(result) => Ok(result),
            SearchOutcome::SessionRejected => {
                let Some(fresh) = self.refresh_session()? else {
                    return Ok(None);
                };
                match self.search(&fresh, name)? {
                    SearchOutcome::Done(result) => Ok(result),
                    SearchOutcome::SessionRejected => Ok(None),
                }
            }
        }
    }

    /// Stub for when `hltb_scrape` feature is not enabled.
    #[cfg(not(feature = "hltb_scrape"))]
    pub fn fetch(&self, _name: &str) -> Result<Option<HltbData>> {
        Ok(None)
    }

    #[cfg(feature = "hltb_scrape")]
    fn current_session(&self) -> Result<Option<types::BleedSession>> {
        {
            let guard = self.session.lock().expect("hltb session lock poisoned");
            if let Some(s) = guard.as_ref() {
                return Ok(Some(s.clone()));
            }
        }
        self.refresh_session()
    }

    #[cfg(feature = "hltb_scrape")]
    fn refresh_session(&self) -> Result<Option<types::BleedSession>> {
        use crate::http::{HttpMethod, HttpRequest};
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let url = format!("{HLTB_INIT_URL}?t={millis}");
        let mut headers = std::collections::HashMap::new();
        headers.insert("user-agent".into(), HLTB_USER_AGENT.into());
        headers.insert("referer".into(), "https://howlongtobeat.com/".into());
        let response = self.http.request(
            "hltb",
            HttpRequest {
                url,
                method: HttpMethod::Get,
                headers,
                body: None,
            },
        )?;
        if response.status != 200 || response.body.is_empty() {
            tracing::warn!(status = response.status, "HLTB bleed/init rejected");
            return Ok(None);
        }
        let session: types::BleedSession =
            crate::http::parse_json(&response.body, "hltb/bleed-init.json")?;
        *self.session.lock().expect("hltb session lock poisoned") = Some(session.clone());
        Ok(Some(session))
    }

    #[cfg(feature = "hltb_scrape")]
    fn search(&self, session: &types::BleedSession, name: &str) -> Result<SearchOutcome> {
        // Search terms are whitespace-split words, matching the site's own
        // request shape.
        let terms: Vec<&str> = name.split_whitespace().collect();
        let mut body = serde_json::json!({
            "searchType": "games",
            "searchTerms": terms,
            "searchPage": 1,
            "size": 5,
            "searchOptions": {
                "games": {
                    "userId": 0,
                    "platform": "",
                    "sortCategory": "popular",
                    "rangeCategory": "main",
                    "rangeTime": {"min": null, "max": null},
                    "gameplay": {"perspective": "", "flow": "", "genre": "", "difficulty": ""},
                    "rangeYear": {"min": "", "max": ""},
                    "modifier": ""
                },
                "users": {"sortCategory": "postcount"},
                "lists": {"sortCategory": "follows"},
                "filter": "",
                "sort": 0,
                "randomizer": 0
            },
            "useCache": true
        });
        // The dynamic hpKey/hpVal pair must appear in the body as well as
        // the headers.
        body.as_object_mut()
            .expect("bleed body is an object")
            .insert(session.hp_key.clone(), serde_json::json!(session.hp_val));

        let mut headers = std::collections::HashMap::new();
        headers.insert("Content-Type".into(), "application/json".into());
        // The bleed token is bound to the User-Agent used at init; both
        // requests must present the same (browser-like) UA — HLTB rejects
        // non-browser agents with 403.
        headers.insert("user-agent".into(), HLTB_USER_AGENT.into());
        headers.insert("referer".into(), "https://howlongtobeat.com/".into());
        headers.insert("x-auth-token".into(), session.token.clone());
        headers.insert("x-hp-key".into(), session.hp_key.clone());
        headers.insert("x-hp-val".into(), session.hp_val.clone());

        let body_bytes = serde_json::to_vec(&body).map_err(|e| {
            vapourfly_core::error::VapourflyError::Internal(format!(
                "failed to serialize HLTB search request: {e}"
            ))
        })?;
        let response = self
            .http
            .post("hltb", HLTB_SEARCH_URL, headers, body_bytes)?;

        if response.status != 200 || response.body.is_empty() {
            return Ok(SearchOutcome::SessionRejected);
        }

        let search_result: types::HltbSearchResponse = match serde_json::from_slice(&response.body)
        {
            Ok(r) => r,
            // A 200 with a non-JSON body (e.g. an HTML error page) means the
            // session token was not accepted; retry with a fresh one.
            Err(_) => return Ok(SearchOutcome::SessionRejected),
        };

        // Find the best match by name similarity.
        let target = name.to_lowercase();
        let best = search_result
            .data
            .iter()
            .max_by(|a, b| {
                let a_sim = strsim::jaro_winkler(&a.game_name.to_lowercase(), &target);
                let b_sim = strsim::jaro_winkler(&b.game_name.to_lowercase(), &target);
                a_sim
                    .partial_cmp(&b_sim)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned();

        Ok(SearchOutcome::Done(best.map(|g| g.to_hltb_data())))
    }
}

#[cfg(feature = "hltb_scrape")]
enum SearchOutcome {
    Done(Option<HltbData>),
    SessionRejected,
}

/// Browser-like User-Agent for HLTB requests. howlongtobeat.com returns
/// 403 "Access Denied" for non-browser agents, and the bleed token is bound
/// to whatever UA performed the init — so one shared constant.
#[cfg(feature = "hltb_scrape")]
const HLTB_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
     AppleWebKit/537.36 (KHTML, like Gecko) Chrome/126.0.0.0 Safari/537.36";

#[cfg(feature = "hltb_scrape")]
const HLTB_INIT_URL: &str = "https://howlongtobeat.com/api/bleed/init";

#[cfg(feature = "hltb_scrape")]
const HLTB_SEARCH_URL: &str = "https://howlongtobeat.com/api/bleed";

#[cfg(feature = "hltb_scrape")]
mod types {
    use serde::Deserialize;
    use vapourfly_core::models::{HltbData, HltbSource};

    /// Session trio from `/api/bleed/init`. The token is bound to the
    /// caller's IP and User-Agent.
    #[derive(Debug, Clone, Deserialize)]
    pub struct BleedSession {
        pub token: String,
        #[serde(rename = "hpKey")]
        pub hp_key: String,
        #[serde(rename = "hpVal")]
        pub hp_val: String,
    }

    #[derive(Debug, Deserialize)]
    pub struct HltbSearchResponse {
        #[serde(default)]
        pub data: Vec<HltbSearchEntry>,
    }

    #[derive(Debug, Clone, Deserialize)]
    pub struct HltbSearchEntry {
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

    pub fn seconds_from_hltb(val: f64) -> Option<u32> {
        if val > 0.0 { Some(val as u32) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, MockBackend};
    use std::collections::HashMap;
    #[cfg(feature = "hltb_scrape")]
    use types::{HltbSearchEntry, seconds_from_hltb};
    #[cfg(feature = "hltb_scrape")]
    use vapourfly_core::models::HltbSource;

    #[cfg(feature = "hltb_scrape")]
    const INIT_BODY: &[u8] = br#"{"token":"tok123","hpKey":"ign_abc","hpVal":"deadbeef"}"#;

    #[test]
    fn stub_or_no_match_returns_none() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://howlongtobeat.com/api/bleed/init",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: INIT_BODY_OR_EMPTY.to_vec(),
            },
        );
        mock.register(
            "https://howlongtobeat.com/api/bleed",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: br#"{"data":[]}"#.to_vec(),
            },
        );
        let client = HltbClient::with_http(HttpClient::with_backend(Box::new(mock)));

        let result = client.fetch("Test Game").unwrap();
        assert!(result.is_none());
    }

    #[cfg(feature = "hltb_scrape")]
    const INIT_BODY_OR_EMPTY: &[u8] = INIT_BODY;
    #[cfg(not(feature = "hltb_scrape"))]
    const INIT_BODY_OR_EMPTY: &[u8] = b"";

    #[test]
    fn default_constructor_works() {
        let client = HltbClient::new();
        // Without the feature this is a pure stub; with it, the missing mock
        // network makes init fail, which must degrade to Ok(None).
        #[cfg(not(feature = "hltb_scrape"))]
        assert!(client.fetch("Any Game").unwrap().is_none());
        let _ = client;
    }

    #[cfg(feature = "hltb_scrape")]
    #[test]
    fn bleed_flow_parses_search_response() {
        let search_body = br#"{"color":"blue","count":1,"data":[{"game_id":42818,"game_name":"Celeste","comp_main":29936.0,"comp_plus":52710.0,"comp_100":140494.0}]}"#;
        let mut mock = MockBackend::new();
        mock.register(
            "https://howlongtobeat.com/api/bleed/init",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: INIT_BODY.to_vec(),
            },
        );
        mock.register(
            "https://howlongtobeat.com/api/bleed",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: search_body.to_vec(),
            },
        );
        let client = HltbClient::with_http(HttpClient::with_backend(Box::new(mock)));

        let data = client.fetch("Celeste").unwrap().expect("match");
        assert_eq!(data.main_story_seconds, Some(29936));
        assert_eq!(data.main_extra_seconds, Some(52710));
        assert_eq!(data.completionist_seconds, Some(140494));
        assert_eq!(data.source, HltbSource::HltbScrape);
    }

    #[cfg(feature = "hltb_scrape")]
    #[test]
    fn init_failure_degrades_to_none() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://howlongtobeat.com/",
            HttpResponse {
                status: 403,
                headers: HashMap::new(),
                body: Vec::new(),
            },
        );
        let client = HltbClient::with_http(HttpClient::with_backend(Box::new(mock)));
        assert!(client.fetch("Celeste").unwrap().is_none());
    }

    #[cfg(feature = "hltb_scrape")]
    #[test]
    fn html_error_page_treated_as_session_rejection_not_parse_error() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://howlongtobeat.com/api/bleed/init",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: INIT_BODY.to_vec(),
            },
        );
        mock.register(
            "https://howlongtobeat.com/api/bleed",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"<!DOCTYPE html><html>404</html>".to_vec(),
            },
        );
        let client = HltbClient::with_http(HttpClient::with_backend(Box::new(mock)));
        // Session rejected twice (same mock response) -> graceful None.
        assert!(client.fetch("Celeste").unwrap().is_none());
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
}
