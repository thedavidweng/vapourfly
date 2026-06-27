//! Compact share codes for Vapourfly playlists.
//!
//! Share codes encode a [`PlaylistFile`] as `VF1:<base64url(json)>` so users
//! can copy a single string instead of exchanging JSON files.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;

use crate::error::{Result, VapourflyError};
use crate::models::PlaylistFile;
use crate::playlist::validate_playlist_file;

const SHARE_CODE_PREFIX: &str = "VF1:";

/// Encode a playlist as a share code.
pub fn encode_share_code(playlist: &PlaylistFile) -> Result<String> {
    validate_playlist_file(playlist)?;

    let json = serde_json::to_string(playlist)
        .map_err(|e| VapourflyError::Internal(format!("failed to serialize playlist: {e}")))?;
    let encoded = URL_SAFE_NO_PAD.encode(json.as_bytes());
    Ok(format!("{SHARE_CODE_PREFIX}{encoded}"))
}

/// Decode a share code back into a playlist file.
pub fn decode_share_code(code: &str) -> Result<PlaylistFile> {
    let payload = code.trim().strip_prefix(SHARE_CODE_PREFIX).ok_or_else(|| {
        VapourflyError::InvalidInput(format!("share code must start with '{SHARE_CODE_PREFIX}'"))
    })?;

    let bytes = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|e| VapourflyError::InvalidInput(format!("invalid share code encoding: {e}")))?;

    let json = String::from_utf8(bytes)
        .map_err(|e| VapourflyError::InvalidInput(format!("invalid share code utf-8: {e}")))?;

    let pf: PlaylistFile = serde_json::from_str(&json).map_err(|e| {
        VapourflyError::InvalidInput(format!("invalid share code playlist json: {e}"))
    })?;

    validate_playlist_file(&pf)?;

    Ok(pf)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Playlist, PlaylistContent, VAPOURFLY_PLAYLIST_SCHEMA};

    fn sample_playlist() -> PlaylistFile {
        PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "user".into(),
            playlist: Playlist {
                id: "deck-shortlist".into(),
                name: "Deck Shortlist".into(),
                description: "Games to try on Steam Deck".into(),
                content: PlaylistContent::Manual {
                    app_ids: vec![292030, 367520],
                },
            },
        }
    }

    #[test]
    fn share_code_round_trip() {
        let pf = sample_playlist();
        let code = encode_share_code(&pf).unwrap();
        assert!(code.starts_with(SHARE_CODE_PREFIX));
        let decoded = decode_share_code(&code).unwrap();
        assert_eq!(decoded.playlist.id, pf.playlist.id);
        assert_eq!(decoded.playlist.name, pf.playlist.name);
    }

    #[test]
    fn decode_rejects_bad_prefix() {
        let err = decode_share_code("BAD:abc").unwrap_err();
        assert!(err.to_string().contains("share code must start with"));
    }

    #[test]
    fn encode_rejects_invalid_playlist() {
        let mut pf = sample_playlist();
        pf.playlist.id.clear();

        let err = encode_share_code(&pf).unwrap_err();
        assert!(err.to_string().contains("playlist id must not be empty"));
    }
}
