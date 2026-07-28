//! RAWG API client.
//!
//! Requires an API key, resolved from `VAPOURFLY_RAWG_KEY` at the
//! enrichment seam.

use serde::Deserialize;
use vapourfly_core::error::Result;
use vapourfly_core::models::RawgData;

use crate::http::{HttpClient, parse_json};

const RAWG_API_BASE: &str = "https://api.rawg.io/api";

/// RAWG API client.
pub struct RawgClient {
    api_key: String,
    http: HttpClient,
}

impl RawgClient {
    /// Create a RAWG client with an explicit API key and HTTP client.
    pub fn new(api_key: String, http: HttpClient) -> Self {
        Self { api_key, http }
    }

    /// Search RAWG for a game by name and return the best match.
    ///
    /// Returns `Ok(None)` when no results are found. Prefers results that
    /// have a Steam store listing.
    pub fn search_by_name(&self, name: &str) -> Result<Option<RawgData>> {
        let url = format!(
            "{RAWG_API_BASE}/games?key={}&search={}&stores=1&page_size=10",
            self.api_key,
            encode_query(name)
        );

        let response = self.http.get("rawg", &url)?;

        if response.status == 404 || response.body.is_empty() {
            return Ok(None);
        }

        let search_result: RawgSearchResponse = parse_json(
            &response.body,
            &format!("rawg/search/{}.json", encode_query(name)),
        )?;

        // Prefer Steam store presence, then rating, then ratings count.
        let Some(best) = search_result.results.into_iter().max_by(|a, b| {
            let a_has_steam = a.stores.iter().any(|s| s.store.name == "Steam");
            let b_has_steam = b.stores.iter().any(|s| s.store.name == "Steam");
            a_has_steam
                .cmp(&b_has_steam)
                .then(
                    a.rating
                        .partial_cmp(&b.rating)
                        .unwrap_or(std::cmp::Ordering::Equal),
                )
                .then(a.ratings_count.cmp(&b.ratings_count))
        }) else {
            return Ok(None);
        };

        Ok(Some(RawgData {
            rawg_id: best.id as u64,
            rating_0_5: (best.rating > 0.0).then_some(best.rating as f32),
            ratings_count: Some(best.ratings_count),
            genres: best.genres.into_iter().map(|g| g.name).collect(),
            tags: best.tags.into_iter().map(|t| t.name).collect(),
            stores: best.stores.into_iter().map(|s| s.store.name).collect(),
        }))
    }
}

#[derive(Debug, Deserialize)]
struct RawgSearchResponse {
    results: Vec<RawgGameResult>,
}

#[derive(Debug, Deserialize)]
struct RawgGameResult {
    id: i64,
    #[serde(default)]
    rating: f64,
    #[serde(default)]
    ratings_count: u32,
    #[serde(default)]
    genres: Vec<RawgNameEntry>,
    #[serde(default)]
    tags: Vec<RawgNameEntry>,
    #[serde(default)]
    stores: Vec<RawgStoreEntry>,
}

#[derive(Debug, Deserialize)]
struct RawgNameEntry {
    name: String,
}

#[derive(Debug, Deserialize)]
struct RawgStoreEntry {
    store: RawgStoreName,
}

#[derive(Debug, Deserialize)]
struct RawgStoreName {
    name: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{HttpResponse, MockBackend};
    use std::collections::HashMap;

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
    fn search_parses_valid_response() {
        let body = r#"{
            "results": [
                {
                    "id": 3498,
                    "rating": 4.56,
                    "ratings_count": 5000,
                    "genres": [{"name": "Action"}, {"name": "RPG"}],
                    "tags": [{"name": "open world"}, {"name": "story rich"}],
                    "stores": [{"store": {"name": "Steam"}}, {"store": {"name": "Epic Games"}}]
                }
            ]
        }"#;
        let mut mock = MockBackend::new();
        mock.register(
            "https://api.rawg.io/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: body.as_bytes().to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = RawgClient::new("test_key".into(), http);

        let data = client.search_by_name("GTA V").unwrap().unwrap();
        assert_eq!(data.rawg_id, 3498);
        assert_eq!(data.rating_0_5, Some(4.56));
        assert_eq!(data.ratings_count, Some(5000));
        assert_eq!(data.genres, vec!["Action", "RPG"]);
        assert_eq!(data.tags, vec!["open world", "story rich"]);
        assert!(data.stores.contains(&"Steam".to_string()));
    }

    #[test]
    fn search_prefers_steam_store_result() {
        let body = r#"{
            "results": [
                {
                    "id": 111,
                    "rating": 3.0,
                    "ratings_count": 100,
                    "genres": [],
                    "tags": [],
                    "stores": [{"store": {"name": "GOG"}}]
                },
                {
                    "id": 222,
                    "rating": 4.0,
                    "ratings_count": 200,
                    "genres": [],
                    "tags": [],
                    "stores": [{"store": {"name": "Steam"}}]
                }
            ]
        }"#;
        let mut mock = MockBackend::new();
        mock.register(
            "https://api.rawg.io/",
            HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: body.as_bytes().to_vec(),
            },
        );
        let http = HttpClient::with_backend(Box::new(mock));
        let client = RawgClient::new("test_key".into(), http);

        let data = client.search_by_name("test").unwrap().unwrap();
        assert_eq!(data.rawg_id, 222, "should prefer the Steam store result");
    }

    #[test]
    fn encode_query_handles_spaces() {
        assert_eq!(encode_query("Half Life 2"), "Half+Life+2");
    }
}
