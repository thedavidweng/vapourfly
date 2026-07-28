//! PCGamingWiki API client.
//!
//! Uses the MediaWiki/Cargo API. No credentials required.
//!
//! Missing data maps to [`ControllerSupport::Unknown`].

use vapourfly_core::error::Result;
use vapourfly_core::models::{ControllerSupport, PcgwData};

use crate::http::{HttpClient, parse_json};

/// PCGamingWiki API base URL (MediaWiki Cargo query endpoint).
const PCGW_API_BASE: &str = "https://www.pcgamingwiki.com/w/api.php";

/// PCGamingWiki API client.
pub struct PcgwClient {
    http: HttpClient,
}

#[derive(Debug, serde::Deserialize)]
struct PcgwCargoResponse {
    #[serde(default)]
    cargoquery: Vec<PcgwCargoEntry>,
}

#[derive(Debug, serde::Deserialize)]
struct PcgwCargoEntry {
    title: PcgwCargoTitle,
}

/// One row of the joined `Infobox_game` + `Input` Cargo query. Cargo
/// replaces underscores with spaces in response keys.
#[derive(Debug, serde::Deserialize)]
struct PcgwCargoTitle {
    #[serde(rename = "Page")]
    page: Option<String>,
    #[serde(rename = "Controller support")]
    controller_support: Option<String>,
    #[serde(rename = "Full controller support")]
    full_controller_support: Option<String>,
}

impl PcgwClient {
    /// Create a PCGW client with a custom [`HttpClient`].
    pub fn with_http(http: HttpClient) -> Self {
        Self { http }
    }

    /// Fetch game data by Steam AppID.
    ///
    /// One Cargo query joins `Infobox_game` (AppID → page) with `Input`
    /// (controller support). The Semantic MediaWiki `action=ask` API this
    /// client previously used was removed by PCGW in 2022.
    ///
    /// When the page is not found or data fields are missing, sensible defaults
    /// are returned (Unknown controller support, AppID-search fixes URL).
    /// `steam_deck_notes` is always `None`: PCGW's Cargo tables expose no
    /// Steam Deck field (Deck data comes from ProtonDB).
    pub fn fetch_by_appid(&self, app_id: u32) -> Result<PcgwData> {
        let url = format!(
            "{PCGW_API_BASE}?action=cargoquery&format=json\
             &tables=Infobox_game%2CInput\
             &join_on=Infobox_game._pageID%3DInput._pageID\
             &fields=Infobox_game._pageName%3DPage%2CInput.Controller_support%2CInput.Full_controller_support\
             &where=Infobox_game.Steam_AppID%20HOLDS%20%22{app_id}%22"
        );

        let response = self.http.get("pcgw", &url)?;

        if response.status == 404 || response.body.is_empty() {
            return Ok(PcgwData {
                page_name: None,
                controller_support: ControllerSupport::Unknown,
                steam_deck_notes: None,
                fixes_url: None,
            });
        }

        let cargo: PcgwCargoResponse = parse_json(&response.body, &format!("pcgw/{app_id}.json"))?;

        // An AppID can map to several pages (e.g. a game and its legacy
        // edition both HOLD the id); take the first row.
        let Some(row) = cargo.cargoquery.first() else {
            return Ok(PcgwData {
                page_name: None,
                controller_support: ControllerSupport::Unknown,
                steam_deck_notes: None,
                fixes_url: Some(format!("https://www.pcgamingwiki.com/wiki/AppID:{app_id}")),
            });
        };

        let page_name = row.title.page.clone();
        let controller_support = parse_controller_flags(
            row.title.controller_support.as_deref(),
            row.title.full_controller_support.as_deref(),
        );
        let fixes_url = Some(match page_name.as_deref() {
            Some(page) => format!(
                "https://www.pcgamingwiki.com/wiki/{}",
                url_encode_value(page)
            ),
            None => format!("https://www.pcgamingwiki.com/wiki/AppID:{app_id}"),
        });

        Ok(PcgwData {
            page_name,
            controller_support,
            steam_deck_notes: None,
            fixes_url,
        })
    }
}

/// Map the Cargo `Input` table's boolean-ish flags onto [`ControllerSupport`].
///
/// `Full_controller_support: true` → Full; otherwise `Controller_support:
/// true` → Partial, `false` → None, missing/other → Unknown.
fn parse_controller_flags(
    controller_support: Option<&str>,
    full_controller_support: Option<&str>,
) -> ControllerSupport {
    let is_true = |v: Option<&str>| v.is_some_and(|s| s.eq_ignore_ascii_case("true"));
    let is_false = |v: Option<&str>| v.is_some_and(|s| s.eq_ignore_ascii_case("false"));

    if is_true(full_controller_support) {
        ControllerSupport::Full
    } else if is_true(controller_support) {
        ControllerSupport::Partial
    } else if is_false(controller_support) {
        ControllerSupport::None
    } else {
        ControllerSupport::Unknown
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
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

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
    fn fetch_parses_joined_cargo_response() {
        // Shape verified against the live Cargo API (2026-07): one row from
        // Infobox_game joined with Input, keys with spaces.
        let body = r#"{"cargoquery":[{"title":{"Page":"The Witcher 3: Wild Hunt","Controller support":"true","Full controller support":"true"}}]}"#;
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
        assert_eq!(data.controller_support, ControllerSupport::Full);
        assert!(data.fixes_url.unwrap().contains("Witcher"));
    }

    #[test]
    fn fetch_maps_empty_cargoquery_to_appid_fixes_url() {
        let body = r#"{"cargoquery":[]}"#;
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

        let data = client.fetch_by_appid(999999).unwrap();
        assert!(data.page_name.is_none());
        assert_eq!(data.controller_support, ControllerSupport::Unknown);
        assert!(data.fixes_url.unwrap().contains("AppID:999999"));
    }

    #[test]
    fn parse_controller_flags_variants() {
        assert_eq!(
            parse_controller_flags(Some("true"), Some("true")),
            ControllerSupport::Full
        );
        assert_eq!(
            parse_controller_flags(Some("true"), Some("false")),
            ControllerSupport::Partial
        );
        assert_eq!(
            parse_controller_flags(Some("true"), None),
            ControllerSupport::Partial
        );
        assert_eq!(
            parse_controller_flags(Some("false"), Some("false")),
            ControllerSupport::None
        );
        assert_eq!(
            parse_controller_flags(None, None),
            ControllerSupport::Unknown
        );
        assert_eq!(
            parse_controller_flags(Some("garbage"), None),
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
