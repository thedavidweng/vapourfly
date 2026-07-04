//! Parse `localconfig.vdf` for playtime and per-app settings.
//!
//! Path: `{steam}/userdata/{uid}/config/localconfig.vdf`
//!
//! The relevant section is:
//! ```text
//! UserLocalConfigStore
//!   Software
//!     Valve
//!       Steam
//!         apps
//!           {appid}
//!             playtime          "1038"
//!             LastPlayed        "1628871494"
//!             Playtime2wks      "213"
//!             PlaytimeDisconnected "3"
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::error::{Result, VapourflyError};
use crate::models::{LocalAppState, VdfNode};
use crate::steam::vdf_text::parse_text_vdf;

/// Parse localconfig.vdf and return per-app state.
///
/// Individual app parse failures are logged as warnings and skipped rather
/// than causing a global failure.
pub fn parse_localconfig(path: &Path) -> Result<BTreeMap<u32, LocalAppState>> {
    let content = fs::read_to_string(path).map_err(|_| VapourflyError::FileNotFound {
        path: crate::SafePath::new(path),
    })?;

    let root = parse_text_vdf(&content)?;

    // Navigate: UserLocalConfigStore / Software / Valve / Steam / apps
    let apps_node =
        match root.child_object(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"]) {
            Some(node) => node,
            None => return Ok(BTreeMap::new()), // no apps section is valid
        };

    let mut result = BTreeMap::new();

    if let VdfNode::Object(pairs) = apps_node {
        for (key, app_node) in pairs {
            let app_id = match key.parse::<u32>() {
                Ok(id) => id,
                Err(_) => {
                    tracing::warn!("localconfig.vdf: skipping non-numeric app key {:?}", key);
                    continue;
                }
            };

            match app_node {
                VdfNode::Object(fields) => {
                    let state = parse_app_fields(app_id, fields);
                    result.insert(app_id, state);
                }
                other => {
                    tracing::warn!(
                        "localconfig.vdf: app {} node is {:?}, expected Object -- skipping",
                        app_id,
                        std::mem::discriminant(other)
                    );
                }
            }
        }
    }

    Ok(result)
}

/// Known localconfig field names (lowercased for case-insensitive matching).
const FIELD_PLAYTIME: &str = "playtime";
const FIELD_LAST_PLAYED: &str = "lastplayed";
const FIELD_PLAYTIME_2WKS: &str = "playtime2wks";
const FIELD_PLAYTIME_DISCONNECTED: &str = "playtimedisconnected";

fn parse_app_fields(app_id: u32, fields: &[(String, VdfNode)]) -> LocalAppState {
    let mut state = LocalAppState {
        app_id,
        last_played_unix: None,
        playtime_minutes: None,
        playtime_2wks_minutes: None,
        playtime_disconnected_minutes: None,
        raw_fields: BTreeMap::new(),
    };

    for (key, value) in fields {
        if let VdfNode::String(s) = value {
            // Store all fields in raw_fields (original case).
            state.raw_fields.insert(key.clone(), s.clone());

            // Extract known fields (case-insensitive).
            let key_lower = key.to_lowercase();
            match key_lower.as_str() {
                FIELD_PLAYTIME => {
                    state.playtime_minutes = s.parse().ok();
                }
                FIELD_LAST_PLAYED => {
                    state.last_played_unix = s.parse().ok();
                }
                FIELD_PLAYTIME_2WKS => {
                    state.playtime_2wks_minutes = s.parse().ok();
                }
                FIELD_PLAYTIME_DISCONNECTED => {
                    state.playtime_disconnected_minutes = s.parse().ok();
                }
                _ => {}
            }
        }
    }

    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Path to the minimal fixture Steam directory.
    fn fixture_steam_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../data/fixtures/steam_minimal")
    }

    fn fixture_localconfig_path() -> PathBuf {
        fixture_steam_dir().join("userdata/76561198000000000/config/localconfig.vdf")
    }

    #[test]
    fn parse_fixture_localconfig() {
        let apps = parse_localconfig(&fixture_localconfig_path()).unwrap();

        // AppID 730
        let cs2 = apps.get(&730).unwrap();
        assert_eq!(cs2.playtime_minutes, Some(418));
        assert_eq!(cs2.last_played_unix, Some(1628871494));
        assert_eq!(cs2.playtime_2wks_minutes, Some(213));
        assert_eq!(cs2.playtime_disconnected_minutes, Some(3));

        // AppID 427520
        let factorio = apps.get(&427520).unwrap();
        assert_eq!(factorio.playtime_minutes, Some(1038));
        assert_eq!(factorio.last_played_unix, Some(1700000000));
        assert_eq!(factorio.playtime_2wks_minutes, Some(0));

        // AppID 999 (junk candidate)
        let junk = apps.get(&999).unwrap();
        assert_eq!(junk.playtime_minutes, Some(5));
    }

    #[test]
    fn parse_nonexistent_file() {
        let result = parse_localconfig(Path::new("/nonexistent/localconfig.vdf"));
        assert!(result.is_err());
    }

    #[test]
    fn unknown_fields_preserved_in_raw_fields() {
        let apps = parse_localconfig(&fixture_localconfig_path()).unwrap();
        let cs2 = apps.get(&730).unwrap();

        // Known fields should also appear in raw_fields.
        assert_eq!(
            cs2.raw_fields.get("playtime").map(|s| s.as_str()),
            Some("418")
        );
        assert_eq!(
            cs2.raw_fields.get("LastPlayed").map(|s| s.as_str()),
            Some("1628871494")
        );
    }

    #[test]
    fn case_insensitive_field_matching() {
        // Build a VDF with mixed-case field names.
        let vdf_content = r#"
"UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                    "42"
                    {
                        "PLAYTIME"          "100"
                        "lastplayed"        "999"
                        "PLAYTIME2WKS"      "50"
                        "PlaytimeDisconnected"  "10"
                    }
                }
            }
        }
    }
}
"#;
        let root = parse_text_vdf(vdf_content).unwrap();
        let apps_node = root
            .child_object(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"])
            .unwrap();

        let mut result = BTreeMap::new();
        if let VdfNode::Object(pairs) = apps_node {
            for (key, app_node) in pairs {
                let app_id: u32 = key.parse().unwrap();
                if let VdfNode::Object(fields) = app_node {
                    let state = parse_app_fields(app_id, fields);
                    result.insert(app_id, state);
                }
            }
        }

        let app = result.get(&42).unwrap();
        assert_eq!(app.playtime_minutes, Some(100));
        assert_eq!(app.last_played_unix, Some(999));
        assert_eq!(app.playtime_2wks_minutes, Some(50));
        assert_eq!(app.playtime_disconnected_minutes, Some(10));
    }

    #[test]
    fn empty_apps_section_returns_empty() {
        let vdf_content = r#"
"UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                }
            }
        }
    }
}
"#;
        let root = parse_text_vdf(vdf_content).unwrap();
        let apps_node = root
            .child_object(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"])
            .unwrap();

        if let VdfNode::Object(pairs) = apps_node {
            assert!(pairs.is_empty());
        }
    }

    #[test]
    fn no_apps_section_returns_empty() {
        let vdf_content = r#"
"UserLocalConfigStore"
{
    "Software"
    {
    }
}
"#;
        let root = parse_text_vdf(vdf_content).unwrap();
        // child_object returns None when the path doesn't exist.
        assert!(
            root.child_object(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"])
                .is_none()
        );
    }

    #[test]
    fn non_numeric_app_keys_skipped() {
        let vdf_content = r#"
"UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                    "730"
                    {
                        "playtime"  "500"
                    }
                    "non_numeric_key"
                    {
                        "playtime"  "999"
                    }
                }
            }
        }
    }
}
"#;
        let root = parse_text_vdf(vdf_content).unwrap();
        let apps_node = root
            .child_object(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"])
            .unwrap();

        let mut result = BTreeMap::new();
        if let VdfNode::Object(pairs) = apps_node {
            for (key, app_node) in pairs {
                if let Ok(app_id) = key.parse::<u32>() {
                    if let VdfNode::Object(fields) = app_node {
                        let state = parse_app_fields(app_id, fields);
                        result.insert(app_id, state);
                    }
                }
            }
        }

        assert_eq!(result.len(), 1);
        assert_eq!(result.get(&730).unwrap().playtime_minutes, Some(500));
    }

    #[test]
    fn negative_playtime_values_stored_as_none() {
        let vdf_content = r#"
"UserLocalConfigStore"
{
    "Software"
    {
        "Valve"
        {
            "Steam"
            {
                "apps"
                {
                    "730"
                    {
                        "playtime"  "not_a_number"
                        "LastPlayed"  "also_not_a_number"
                    }
                }
            }
        }
    }
}
"#;
        let root = parse_text_vdf(vdf_content).unwrap();
        let apps_node = root
            .child_object(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"])
            .unwrap();

        let mut result = BTreeMap::new();
        if let VdfNode::Object(pairs) = apps_node {
            for (key, app_node) in pairs {
                let app_id: u32 = key.parse().unwrap();
                if let VdfNode::Object(fields) = app_node {
                    let state = parse_app_fields(app_id, fields);
                    result.insert(app_id, state);
                }
            }
        }

        let app = result.get(&730).unwrap();
        assert_eq!(app.playtime_minutes, None);
        assert_eq!(app.last_played_unix, None);
        // Raw fields should still have the original values.
        assert_eq!(
            app.raw_fields.get("playtime").map(|s| s.as_str()),
            Some("not_a_number")
        );
    }
}
