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

// ---------------------------------------------------------------------------
// PCGW JSON response types
// ---------------------------------------------------------------------------

#[derive(Debug, serde::Deserialize)]
struct PcgwAskResponse {
    query: Option<PcgwAskQuery>,
}

#[derive(Debug, serde::Deserialize)]
struct PcgwAskQuery {
    results: Option<std::collections::HashMap<String, serde_json::Value>>,
}

#[derive(Debug, serde::Deserialize)]
struct PcgwCargoResponse {
    cargoquery: Vec<PcgwCargoEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct PcgwCargoEntry {
    title: PcgwCargoTitle,
}

#[derive(Debug, serde::Deserialize)]
struct PcgwCargoTitle {
    #[serde(rename = "Page")]
    #[allow(dead_code)]
    page: Option<String>,
    #[serde(rename = "controller support")]
    controller_support: Option<String>,
    #[serde(rename = "Steam Deck compatibility")]
    steam_deck_compatibility: Option<String>,
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

        let ask_response: PcgwAskResponse =
            serde_json::from_slice(&response.body).map_err(|e| VapourflyError::ParseError {
                path: vapourfly_core::error::SafePath::new(format!("pcgw/{app_id}.json")),
                format: "JSON".into(),
                reason: e.to_string(),
            })?;

        // Extract the first result from the ask response.
        let page_name = ask_response
            .query
            .as_ref()
            .and_then(|q| q.results.as_ref())
            .and_then(|r| r.keys().next().cloned());

        if page_name.is_none() {
            return Ok(PcgwData {
                page_name: None,
                controller_support: ControllerSupport::Unknown,
                steam_deck_notes: None,
                fixes_url: Some(format!("https://www.pcgamingwiki.com/wiki/AppID:{app_id}")),
            });
        }

        // Now query Cargo for structured data about the page.
        let encoded_page = url_encode_value(page_name.as_ref().unwrap());
        let cargo_url = format!(
            "{PCGW_API_BASE}?action=cargoquery&format=json&tables=Infobox_game&fields=Infobox_game._pageName%3DPage,Infobox_game.controller_support,Infobox_game.steam_deck_compatibility&where=Infobox_game._pageName%3D%22{encoded_page}%22",
        );

        let cargo_response = self.http.get("pcgw-cargo", &cargo_url)?;

        let controller_support = if cargo_response.status == 200 && !cargo_response.body.is_empty()
        {
            if let Ok(cargo) = serde_json::from_slice::<PcgwCargoResponse>(&cargo_response.body) {
                cargo
                    .cargoquery
                    .first()
                    .and_then(|entry| entry.title.controller_support.as_deref())
                    .map_or(ControllerSupport::Unknown, parse_controller_support)
            } else {
                ControllerSupport::Unknown
            }
        } else {
            ControllerSupport::Unknown
        };

        let steam_deck_notes = if cargo_response.status == 200 && !cargo_response.body.is_empty() {
            if let Ok(cargo) = serde_json::from_slice::<PcgwCargoResponse>(&cargo_response.body) {
                cargo
                    .cargoquery
                    .first()
                    .and_then(|entry| entry.title.steam_deck_compatibility.clone())
                    .filter(|s| !s.is_empty())
            } else {
                None
            }
        } else {
            None
        };

        let fixes_url = Some(format!(
            "https://www.pcgamingwiki.com/wiki/{}",
            page_name.as_deref().unwrap_or("AppID")
        ));

        Ok(PcgwData {
            page_name,
            controller_support,
            steam_deck_notes,
            fixes_url,
        })
    }
}

/// Percent-encode a value for use in a Cargo query `where` clause.
///
/// Encodes spaces, colons, ampersands, and non-ASCII characters so the
/// resulting URL is valid even for titles like
/// "The Witcher 3: Wild Hunt" or "Tom Clancy's Rainbow Six® Siege".
fn url_encode_value(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            // Unreserved characters (RFC 3986)
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                encoded.push(byte as char);
            }
            // Space -> %20 (not +, since this goes into a URL path component)
            b' ' => encoded.push_str("%20"),
            // Everything else gets percent-encoded
            _ => {
                encoded.push_str(&format!("%{byte:02X}"));
            }
        }
    }
    encoded
}

/// Parse a controller support string from PCGW into our enum.
fn parse_controller_support(s: &str) -> ControllerSupport {
    let lower = s.to_lowercase();
    if lower.contains("full") || lower.contains("native") {
        ControllerSupport::Full
    } else if lower.contains("partial") || lower.contains("limited") {
        ControllerSupport::Partial
    } else if lower == "none" || lower == "no" || lower == "false" || lower == "unsupported" {
        ControllerSupport::None
    } else {
        ControllerSupport::Unknown
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
    fn fetch_parses_valid_ask_response() {
        let body = r#"{"query":{"results":{"The Witcher 3: Wild Hunt":{}}}}"#;
        let mut mock = MockBackend::new();
        mock.register(
            "https://www.pcgamingwiki.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: body.as_bytes().to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = PcgwClient::with_http(http);

        let data = client.fetch_by_appid(292030).unwrap();
        assert_eq!(data.page_name, Some("The Witcher 3: Wild Hunt".to_string()));
        assert!(data.fixes_url.is_some());
    }

    #[test]
    fn fetch_returns_fixes_url_for_known_page() {
        let body = r#"{"query":{"results":{"Cyberpunk 2077":{}}}}"#;
        let mut mock = MockBackend::new();
        mock.register(
            "https://www.pcgamingwiki.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: body.as_bytes().to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = PcgwClient::with_http(http);

        let data = client.fetch_by_appid(1091500).unwrap();
        assert_eq!(data.page_name, Some("Cyberpunk 2077".to_string()));
        assert!(data.fixes_url.unwrap().contains("Cyberpunk"));
    }

    #[test]
    fn parse_controller_support_variants() {
        assert_eq!(parse_controller_support("Full"), ControllerSupport::Full);
        assert_eq!(
            parse_controller_support("Native support"),
            ControllerSupport::Full
        );
        assert_eq!(
            parse_controller_support("Partial"),
            ControllerSupport::Partial
        );
        assert_eq!(parse_controller_support("None"), ControllerSupport::None);
        assert_eq!(
            parse_controller_support("Unknown"),
            ControllerSupport::Unknown
        );
    }

    #[test]
    fn url_encode_simple_name() {
        assert_eq!(url_encode_value("Cyberpunk 2077"), "Cyberpunk%202077");
    }

    #[test]
    fn url_encode_colon_and_apostrophe() {
        assert_eq!(
            url_encode_value("The Witcher 3: Wild Hunt"),
            "The%20Witcher%203%3A%20Wild%20Hunt"
        );
    }

    #[test]
    fn url_encode_registered_mark() {
        assert_eq!(
            url_encode_value("Tom Clancy's Rainbow Six® Siege"),
            "Tom%20Clancy%27s%20Rainbow%20Six%C2%AE%20Siege"
        );
    }

    #[test]
    fn url_encode_plain_ascii_unchanged() {
        assert_eq!(url_encode_value("HalfLife"), "HalfLife");
    }

    #[test]
    fn url_encode_ampersand() {
        assert_eq!(url_encode_value("Rock & Roll"), "Rock%20%26%20Roll");
    }
}
