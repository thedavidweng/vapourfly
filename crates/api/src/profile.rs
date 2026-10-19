//! Read-only account-scoped assembly. No HTTP client or profile persistence.

use std::collections::BTreeMap;

use serde::Serialize;
use vapourfly_core::{Result, models::Game, preferences::Preferences, taste};

use crate::activity::{self, ActivitySnapshot};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AccountProfile {
    pub steam_id64: String,
    pub activity_status: String,
    pub taste: taste::TasteProfile,
    pub traits: Vec<vapourfly_core::traits::Trait>,
}

pub fn derive(
    id: &str,
    games: &[Game],
    prefs: &Preferences,
    snapshot: Option<&ActivitySnapshot>,
    progress: &BTreeMap<u32, vapourfly_core::progress::Summary>,
    now: i64,
) -> Result<AccountProfile> {
    activity::validate_steam_id64(id)?;
    if snapshot.is_some_and(|s| s.steam_id64 != id) {
        return Err(vapourfly_core::VapourflyError::InvalidInput(
            "Activity snapshot does not match selected account".into(),
        ));
    }
    let mut samples: BTreeMap<u32, Vec<taste::ActivityEvidence>> = BTreeMap::new();
    if let Some(snapshot) = snapshot {
        for (source, section) in [
            ("steam.owned", &snapshot.owned),
            ("steam.recent", &snapshot.recent),
        ] {
            // Failed refreshes may retain valid historical evidence, with its
            // original data time. Never turn a failed check into a new observation.
            if let Some(time) = section.fetched_at {
                for game in &section.games {
                    samples
                        .entry(game.app_id)
                        .or_default()
                        .push(taste::ActivityEvidence {
                            source: source.into(),
                            observed_at: Some(time.timestamp()),
                            lifetime_minutes: game.lifetime_minutes,
                            recent_minutes: game.recent_minutes,
                            last_played_at: game.last_played_at,
                        });
                }
            }
        }
    }
    let taste = taste::derive(games, prefs, &samples, now);
    let traits = vapourfly_core::traits::derive(&taste, progress);
    Ok(AccountProfile {
        steam_id64: id.into(),
        activity_status: snapshot.map_or_else(
            || "No cached account activity; local Steam evidence only.".into(),
            |s| {
                format!(
                    "Owned: {:?}, data {:?}; Recent: {:?}, data {:?}",
                    s.owned.availability,
                    s.owned.fetched_at,
                    s.recent.availability,
                    s.recent.fetched_at
                )
            },
        ),
        taste,
        traits,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        activity::{ActivityGame, ActivitySection, Availability},
        cache::DiskCache,
        http::CacheRecord,
    };
    use chrono::{TimeZone, Utc};
    use std::time::Duration;

    const ID: &str = "76561198000000000";
    const OTHER: &str = "76561198000000001";
    const NOW: i64 = 1_800_000_000;

    fn game() -> Game {
        serde_json::from_value(serde_json::json!({
            "app_id": 10, "name":"Local title", "app_type":"Game", "installed":false,
            "steam_collections":[],"is_hidden":false,"is_junk":false,
            "rawg":{"rawg_id":1,"genres":["RPG"],"tags":[],"stores":[]}
        }))
        .unwrap()
    }

    fn snapshot(time: i64) -> ActivitySnapshot {
        let section = ActivitySection {
            availability: Availability::TransportFailure,
            checked_at: Utc.timestamp_opt(NOW, 0).unwrap(),
            fetched_at: Some(Utc.timestamp_opt(time, 0).unwrap()),
            reported_count: Some(1),
            games: vec![ActivityGame {
                app_id: 10,
                name: None,
                lifetime_minutes: Some(600),
                recent_minutes: Some(120),
                last_played_at: Some(NOW),
            }],
        };
        ActivitySnapshot {
            steam_id64: ID.into(),
            owned: section.clone(),
            recent: section,
        }
    }

    #[test]
    fn account_profile_isolated_and_retained_data_recency_is_honest() {
        let temp = tempfile::tempdir().unwrap();
        let cache = DiskCache::new(temp.path());
        cache
            .put(&CacheRecord {
                source: activity::SOURCE.into(),
                key: ID.into(),
                data: snapshot(NOW - taste::RECENT_SECONDS - 1),
                fetched_at: Utc.timestamp_opt(NOW, 0).unwrap(),
                ttl: Duration::from_secs(3600),
                stale: false,
                etag: None,
            })
            .unwrap();
        let activity = activity::load(&cache, ID).unwrap().unwrap();
        let profile = derive(
            ID,
            &[game()],
            &Preferences::new(),
            Some(&activity),
            &BTreeMap::new(),
            NOW,
        )
        .unwrap();
        assert_eq!(profile.taste.long_term.len(), 1);
        assert_eq!(
            profile.taste.recent,
            [] as [vapourfly_core::taste::TagStrength; 0]
        );
        assert!(profile.activity_status.contains("TransportFailure"));
        assert!(
            derive(
                OTHER,
                &[game()],
                &Preferences::new(),
                Some(&activity),
                &BTreeMap::new(),
                NOW
            )
            .is_err()
        );
        assert!(activity::load(&cache, OTHER).unwrap().is_none());
        assert_eq!(
            derive(
                OTHER,
                &[game()],
                &Preferences::new(),
                None,
                &BTreeMap::new(),
                NOW
            )
            .unwrap()
            .taste
            .long_term,
            [] as [vapourfly_core::taste::TagStrength; 0]
        );
        assert_eq!(
            profile,
            derive(
                ID,
                &[game()],
                &Preferences::new(),
                Some(&activity),
                &BTreeMap::new(),
                NOW
            )
            .unwrap()
        );
    }

    #[test]
    fn account_profile_no_key_no_network_fixture_pipeline() {
        struct NoNetwork;
        impl crate::http::HttpBackend for NoNetwork {
            fn execute(&self, _: &crate::http::HttpRequest) -> Result<crate::http::HttpResponse> {
                panic!("cache-only profile must never make a request")
            }
        }
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/fixtures/steam_minimal");
        let temp = tempfile::tempdir().unwrap();
        let http = crate::http::HttpClient::with_backend(Box::new(NoNetwork));
        let scan = crate::workflow::prepare_with(
            &crate::workflow::WorkflowOptions {
                steam_dir: fixtures.clone(),
                account: Some(ID.into()),
                fixtures: Some(fixtures),
                junk_mode: vapourfly_core::models::JunkMode::Default,
                offline: true,
                cache_root: Some(temp.path().to_path_buf()),
            },
            &crate::enrichment::SourceCredentials {
                steam_api_key: None,
                igdb_client_id: None,
                igdb_client_secret: None,
                rawg_key: None,
            },
            &http,
        )
        .unwrap();
        assert_eq!(scan.games.len(), 3);
        let p = derive(
            ID,
            &scan.games,
            &Preferences::new(),
            None,
            &BTreeMap::new(),
            NOW,
        )
        .unwrap();
        assert_eq!(
            p.taste.long_term,
            [] as [vapourfly_core::taste::TagStrength; 0]
        );
        assert!(p.taste.explanation.contains("No eligible titles"));
        assert!(p.traits.iter().all(|t| !t.supported));
        assert_eq!(p.traits[0].unknown.len(), scan.games.len());
        assert!(
            !temp.path().join("profile").exists(),
            "derived data is never persisted"
        );
    }
}
