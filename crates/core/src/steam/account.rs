//! Steam account discovery and selection.
//!
//! Selects a Steam user account from detected accounts, supporting explicit
//! preference via `--account`, automatic most-recent selection, and single
//! account auto-selection.

use crate::error::{Result, VapourflyError};
use crate::steam::paths::SteamAccount;

/// Select a Steam account from a list of detected accounts.
///
/// Selection priority:
///
/// 1. **Single account** -- auto-selected regardless of `preferred`.
/// 2. **Preferred match** -- `preferred` is matched case-insensitively against
///    `account_name` and `persona_name`. Returns the first match.
/// 3. **Most-recent** -- exactly one account with `most_recent == true`.
/// 4. **Ambiguity** -- returns [`VapourflyError::AmbiguousAccount`].
///
/// Returns `Err` if `accounts` is empty or if the preferred name does not
/// match any account (when multiple accounts exist).
pub fn select_account<'a>(
    accounts: &'a [SteamAccount],
    preferred: Option<&str>,
) -> Result<&'a SteamAccount> {
    if accounts.is_empty() {
        return Err(VapourflyError::InvalidInput(
            "no Steam accounts found".into(),
        ));
    }

    if accounts.len() == 1 {
        return Ok(&accounts[0]);
    }

    if let Some(pref) = preferred {
        let pref_lower = pref.to_lowercase();
        if let Some(acct) = accounts.iter().find(|a| {
            a.account_name.to_lowercase() == pref_lower
                || a.persona_name.to_lowercase() == pref_lower
        }) {
            return Ok(acct);
        }
        // Preferred given but no match -- that is an error when multiple
        // accounts exist (caller explicitly asked for a specific account).
        return Err(VapourflyError::InvalidInput(format!(
            "account '{}' not found among {} Steam accounts",
            pref,
            accounts.len()
        )));
    }

    let most_recent: Vec<&SteamAccount> = accounts.iter().filter(|a| a.most_recent).collect();
    if most_recent.len() == 1 {
        return Ok(most_recent[0]);
    }

    Err(VapourflyError::AmbiguousAccount {
        count: accounts.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Path to the minimal fixture Steam directory.
    fn fixture_steam_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/fixtures/steam_minimal")
    }

    fn make_account(name: &str, persona: &str, most_recent: bool) -> SteamAccount {
        SteamAccount {
            steam_id64: "76561198000000000".into(),
            account_name: name.into(),
            persona_name: persona.into(),
            most_recent,
        }
    }

    // -- Single account auto-select -----------------------------------------

    #[test]
    fn single_account_auto_selects() {
        let accounts = vec![make_account("alice", "Alice", false)];
        let acct = select_account(&accounts, None).unwrap();
        assert_eq!(acct.account_name, "alice");
    }

    #[test]
    fn single_account_ignores_preferred() {
        let accounts = vec![make_account("alice", "Alice", false)];
        let acct = select_account(&accounts, Some("bob")).unwrap();
        assert_eq!(acct.account_name, "alice");
    }

    // -- Preferred matching -------------------------------------------------

    #[test]
    fn preferred_by_account_name() {
        let accounts = vec![
            make_account("alice", "Alice", false),
            make_account("bob", "Bob", true),
        ];
        let acct = select_account(&accounts, Some("alice")).unwrap();
        assert_eq!(acct.account_name, "alice");
    }

    #[test]
    fn preferred_by_persona_name() {
        let accounts = vec![
            make_account("alice", "Alice", false),
            make_account("bob", "Bob", true),
        ];
        let acct = select_account(&accounts, Some("Bob")).unwrap();
        assert_eq!(acct.account_name, "bob");
    }

    #[test]
    fn preferred_case_insensitive() {
        let accounts = vec![
            make_account("alice", "Alice", false),
            make_account("bob", "Bob", true),
        ];
        let acct = select_account(&accounts, Some("ALICE")).unwrap();
        assert_eq!(acct.account_name, "alice");
    }

    #[test]
    fn preferred_no_match_errors() {
        let accounts = vec![
            make_account("alice", "Alice", false),
            make_account("bob", "Bob", true),
        ];
        let result = select_account(&accounts, Some("charlie"));
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("charlie"), "error should mention the name");
    }

    // -- Most-recent fallback -----------------------------------------------

    #[test]
    fn most_recent_selected() {
        let accounts = vec![
            make_account("alice", "Alice", false),
            make_account("bob", "Bob", true),
        ];
        let acct = select_account(&accounts, None).unwrap();
        assert_eq!(acct.account_name, "bob");
    }

    #[test]
    fn ambiguous_multiple_most_recent_errors() {
        let accounts = vec![
            make_account("alice", "Alice", true),
            make_account("bob", "Bob", true),
        ];
        let result = select_account(&accounts, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            VapourflyError::AmbiguousAccount { count } => assert_eq!(count, 2),
            other => panic!("expected AmbiguousAccount, got {other:?}"),
        }
    }

    #[test]
    fn ambiguous_no_most_recent_errors() {
        let accounts = vec![
            make_account("alice", "Alice", false),
            make_account("bob", "Bob", false),
        ];
        let result = select_account(&accounts, None);
        assert!(result.is_err());
    }

    // -- Empty list ---------------------------------------------------------

    #[test]
    fn empty_list_returns_error() {
        let result = select_account(&[], None);
        assert!(result.is_err());
    }

    // -- Fixture integration -----------------------------------------------

    #[test]
    fn fixture_single_account_auto_selects() {
        let accounts = crate::steam::paths::detect_accounts(&fixture_steam_dir()).unwrap();
        let acct = select_account(&accounts, None).unwrap();
        assert_eq!(acct.account_name, "vapourfly_fixture_user");
        assert!(acct.most_recent);
    }

    #[test]
    fn fixture_preferred_by_account_name() {
        let accounts = crate::steam::paths::detect_accounts(&fixture_steam_dir()).unwrap();
        let acct = select_account(&accounts, Some("vapourfly_fixture_user")).unwrap();
        assert_eq!(acct.account_name, "vapourfly_fixture_user");
    }

    #[test]
    fn fixture_preferred_by_persona_name() {
        let accounts = crate::steam::paths::detect_accounts(&fixture_steam_dir()).unwrap();
        let acct = select_account(&accounts, Some("Vapourfly Fixture")).unwrap();
        assert_eq!(acct.persona_name, "Vapourfly Fixture");
    }

    #[test]
    fn fixture_preferred_case_insensitive() {
        let accounts = crate::steam::paths::detect_accounts(&fixture_steam_dir()).unwrap();
        let acct = select_account(&accounts, Some("VAPOURFLY_FIXTURE_USER")).unwrap();
        assert_eq!(acct.account_name, "vapourfly_fixture_user");
    }
}
