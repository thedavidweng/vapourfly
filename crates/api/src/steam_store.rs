//! Steam Store API client.
//!
//! No credentials required. Uses the public `appdetails` endpoint at
//! `https://store.steampowered.com/api/appdetails`.

use serde::Deserialize;
use vapourfly_core::error::{Result, VapourflyError};
use vapourfly_core::models::{PriceOverview, SteamStoreDetails, SteamStorePlatforms};

use crate::http::HttpClient;

const STEAM_STORE_API_BASE: &str = "https://store.steampowered.com/api";

/// Steam Store API client.
pub struct SteamStoreClient {
    http: HttpClient,
}

impl Default for SteamStoreClient {
    fn default() -> Self {
        Self::new()
    }
}

impl SteamStoreClient {
    /// Create a new Steam Store client.
    pub fn new() -> Self {
        Self {
            http: HttpClient::new(),
        }
    }

    /// Create a Steam Store client with a custom [`HttpClient`].
    pub fn with_http(http: HttpClient) -> Self {
        Self { http }
    }

    /// Fetch app details from Steam Store.
    ///
    /// `cc` is the country code for pricing (e.g. `"us"`), and `lang` is the
    /// language for localization (e.g. `"english"`).
    pub fn fetch_appdetails(&self, app_id: u32, cc: &str, lang: &str) -> Result<SteamStoreDetails> {
        let url = format!("{STEAM_STORE_API_BASE}/appdetails?appids={app_id}&cc={cc}&l={lang}");

        let response = self.http.get("steam-store", &url)?;

        if response.status == 404 {
            return Err(VapourflyError::InvalidInput(format!(
                "Steam app {app_id} not found"
            )));
        }

        let wrapper: AppDetailsResponse =
            serde_json::from_slice(&response.body).map_err(|e| VapourflyError::ParseError {
                path: vapourfly_core::error::SafePath::new(format!(
                    "steam-store/appdetails/{app_id}.json"
                )),
                format: "JSON".into(),
                reason: e.to_string(),
            })?;

        let entry = wrapper
            .get(&app_id.to_string())
            .ok_or_else(|| VapourflyError::ParseError {
                path: vapourfly_core::error::SafePath::new(format!(
                    "steam-store/appdetails/{app_id}.json"
                )),
                format: "JSON".into(),
                reason: format!("missing key for app {app_id}"),
            })?;

        if !entry.success {
            return Err(VapourflyError::InvalidInput(format!(
                "Steam reports app {app_id} is not available"
            )));
        }

        let data = entry
            .data
            .as_ref()
            .ok_or_else(|| VapourflyError::ParseError {
                path: vapourfly_core::error::SafePath::new(format!(
                    "steam-store/appdetails/{app_id}.json"
                )),
                format: "JSON".into(),
                reason: "success=true but data is null".into(),
            })?;

        Ok(SteamStoreDetails {
            app_id,
            name: data.name.clone().unwrap_or_default(),
            steam_store_type: data.steam_store_type.clone().unwrap_or_default(),
            is_free: data.is_free.unwrap_or(false),
            short_description: data.short_description.clone(),
            header_image: data.header_image.clone(),
            developers: data.developers.clone().unwrap_or_default(),
            publishers: data.publishers.clone().unwrap_or_default(),
            genres: data
                .genres
                .as_ref()
                .map(|gs| gs.iter().map(|g| g.description.clone()).collect())
                .unwrap_or_default(),
            categories: data
                .categories
                .as_ref()
                .map(|cs| cs.iter().map(|c| c.description.clone()).collect())
                .unwrap_or_default(),
            release_date: data.release_date.as_ref().and_then(|r| r.date.clone()),
            metacritic_score: data.metacritic.as_ref().map(|m| m.score),
            platforms: data
                .platforms
                .as_ref()
                .map(|p| SteamStorePlatforms {
                    windows: p.windows,
                    mac: p.mac,
                    linux: p.linux,
                })
                .unwrap_or(SteamStorePlatforms {
                    windows: false,
                    mac: false,
                    linux: false,
                }),
            coming_soon: data
                .release_date
                .as_ref()
                .map(|r| r.coming_soon)
                .unwrap_or(false),
            price_overview: data.price_overview.as_ref().map(|p| PriceOverview {
                currency: p.currency.clone(),
                initial_price_cents: p.initial,
                final_price_cents: p.final_price,
                discount_percent: p.discount_percent,
            }),
        })
    }
}

// ---------------------------------------------------------------------------
// Deserialization helpers (Steam Store JSON shape)
// ---------------------------------------------------------------------------

type AppDetailsResponse = std::collections::HashMap<String, AppDetailsEntry>;

#[derive(Debug, Deserialize)]
struct AppDetailsEntry {
    success: bool,
    data: Option<AppDetailsData>,
}

#[derive(Debug, Deserialize)]
struct AppDetailsData {
    #[serde(rename = "type")]
    steam_store_type: Option<String>,
    name: Option<String>,
    #[serde(default)]
    is_free: Option<bool>,
    short_description: Option<String>,
    header_image: Option<String>,
    developers: Option<Vec<String>>,
    publishers: Option<Vec<String>>,
    genres: Option<Vec<NameDescription>>,
    categories: Option<Vec<NameDescription>>,
    release_date: Option<ReleaseDate>,
    metacritic: Option<Metacritic>,
    platforms: Option<Platforms>,
    price_overview: Option<RawPriceOverview>,
}

#[derive(Debug, Deserialize)]
struct NameDescription {
    #[allow(dead_code)]
    id: u32,
    description: String,
}

#[derive(Debug, Deserialize)]
struct ReleaseDate {
    coming_soon: bool,
    date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Metacritic {
    score: u32,
}

#[derive(Debug, Deserialize)]
struct Platforms {
    windows: bool,
    mac: bool,
    linux: bool,
}

#[derive(Debug, Deserialize)]
struct RawPriceOverview {
    currency: String,
    initial: u32,
    #[serde(rename = "final")]
    final_price: u32,
    discount_percent: u32,
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
    fn client_creation() {
        let client = SteamStoreClient::new();
        let _ = client;
    }

    #[test]
    fn fetch_appdetails_parses_valid_response() {
        let body = r#"{
            "292030": {
                "success": true,
                "data": {
                    "type": "game",
                    "name": "The Witcher 3: Wild Hunt",
                    "is_free": false,
                    "short_description": "A story-driven RPG.",
                    "header_image": "https://cdn.akamai.steamstatic.com/steam/apps/292030/header.jpg",
                    "developers": ["CD PROJEKT RED"],
                    "publishers": ["CD PROJEKT RED"],
                    "genres": [{"id": 3, "description": "RPG"}],
                    "categories": [{"id": 2, "description": "Single-player"}],
                    "release_date": {"coming_soon": false, "date": "May 18, 2015"},
                    "metacritic": {"score": 92},
                    "platforms": {"windows": true, "mac": true, "linux": true},
                    "price_overview": {
                        "currency": "USD",
                        "initial": 3999,
                        "final": 799,
                        "discount_percent": 80
                    }
                }
            }
        }"#;

        let mut mock = MockBackend::new();
        mock.register(
            "https://store.steampowered.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: body.as_bytes().to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = SteamStoreClient::with_http(http);

        let details = client.fetch_appdetails(292030, "us", "english").unwrap();
        assert_eq!(details.app_id, 292030);
        assert_eq!(details.name, "The Witcher 3: Wild Hunt");
        assert_eq!(details.steam_store_type, "game");
        assert!(!details.is_free);
        assert!(details.short_description.is_some());
        assert_eq!(details.developers, vec!["CD PROJEKT RED"]);
        assert_eq!(details.publishers, vec!["CD PROJEKT RED"]);
        assert_eq!(details.genres, vec!["RPG"]);
        assert_eq!(details.categories, vec!["Single-player"]);
        assert_eq!(details.release_date, Some("May 18, 2015".into()));
        assert_eq!(details.metacritic_score, Some(92));
        assert!(details.platforms.windows);
        assert!(details.platforms.mac);
        assert!(details.platforms.linux);
        assert!(!details.coming_soon);
        let price = details.price_overview.unwrap();
        assert_eq!(price.currency, "USD");
        assert_eq!(price.final_price_cents, 799);
        assert_eq!(price.discount_percent, 80);
    }

    #[test]
    fn fetch_appdetails_handles_free_game() {
        let body = r#"{
            "440": {
                "success": true,
                "data": {
                    "type": "game",
                    "name": "Team Fortress 2",
                    "is_free": true,
                    "developers": ["Valve"],
                    "publishers": ["Valve"],
                    "platforms": {"windows": true, "mac": true, "linux": true}
                }
            }
        }"#;

        let mut mock = MockBackend::new();
        mock.register(
            "https://store.steampowered.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: body.as_bytes().to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = SteamStoreClient::with_http(http);

        let details = client.fetch_appdetails(440, "us", "english").unwrap();
        assert!(details.is_free);
        assert!(details.price_overview.is_none());
        assert!(details.genres.is_empty());
        assert_eq!(details.developers, vec!["Valve"]);
    }

    #[test]
    fn fetch_appdetails_returns_error_for_404() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://store.steampowered.com/",
            HttpResponse {
                status: 404,
                headers: HashMap::new(),
                body: Vec::new(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = SteamStoreClient::with_http(http);

        let err = client
            .fetch_appdetails(999999, "us", "english")
            .unwrap_err();
        assert!(err.to_string().contains("not found"));
    }

    #[test]
    fn fetch_appdetails_returns_error_when_success_false() {
        let body = r#"{"292030": {"success": false}}"#;

        let mut mock = MockBackend::new();
        mock.register(
            "https://store.steampowered.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: body.as_bytes().to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = SteamStoreClient::with_http(http);

        let err = client
            .fetch_appdetails(292030, "us", "english")
            .unwrap_err();
        assert!(err.to_string().contains("not available"));
    }

    #[test]
    fn fetch_appdetails_returns_parse_error_on_invalid_json() {
        let mut mock = MockBackend::new();
        mock.register(
            "https://store.steampowered.com/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: b"not json at all".to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = SteamStoreClient::with_http(http);

        let err = client
            .fetch_appdetails(292030, "us", "english")
            .unwrap_err();
        assert!(err.to_string().contains("JSON"));
    }
}
