//! Compact binary share codes for Vapourfly playlists.
//!
//! Share codes encode a [`PlaylistFile`] as `VF1:<compressed-binary-payload>`.
//! The payload carries the playlist's `content` (manual AppID list or rules
//! tree) plus `name` and `description`, encoded as a compact binary format
//! with DEFLATE compression.
//!
//! ## Format (ADR-0003)
//!
//! The binary payload (before compression) is:
//!
//! ```text
//! u8  format version (0x01)
//! u8  content type tag (0x01 = Manual, 0x02 = Rules)
//! --- content ---
//! Manual:  u32 LE count, then count * u32 LE AppIDs
//! Rules:   u32 LE json_len, then json_len bytes of rules JSON
//! --- metadata ---
//! u16 LE name_len, then name_len bytes (UTF-8)
//! u16 LE desc_len, then desc_len bytes (UTF-8)
//! ```
//!
//! The binary payload is DEFLATE-compressed, then base64url-encoded without
//! padding, and prefixed with `VF1:`.
//!
//! ## No backward compatibility
//!
//! The previous `VF1:<base64url(json)>` format is replaced outright (ADR-0003).
//! Existing VF1 codes from older versions will fail to decode under this
//! decoder. The `VF1:` prefix is retained; the `1` is now the format
//! generation, not the JSON-encoding version.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use flate2::{
    Compression,
    read::{ZlibDecoder, ZlibEncoder},
};
use std::io::Read;

use crate::error::{Result, VapourflyError};
use crate::models::{Playlist, PlaylistContent, PlaylistFile, PlaylistRule};
use crate::playlist::validate_playlist_file;

const SHARE_CODE_PREFIX: &str = "VF1:";
const FORMAT_VERSION: u8 = 0x01;
const TAG_MANUAL: u8 = 0x01;
const TAG_RULES: u8 = 0x02;

/// Maximum accepted size of the compressed payload (base64-decoded bytes).
///
/// Legitimate share codes are tiny — a manual playlist with 1000 AppIDs
/// compresses to well under 1 KB. 1 MB is a generous ceiling that rejects
/// absurd inputs early without affecting any real playlist.
const MAX_COMPRESSED_SIZE: usize = 1024 * 1024;

/// Maximum accepted size of the decompressed binary payload.
///
/// This is the primary defense against zlib bombs: a few KB of compressed
/// input can decompress to gigabytes. 10 MB comfortably fits any realistic
/// playlist (a manual playlist with 250K AppIDs is ~1 MB; rules JSON is
/// bounded by the depth-16 validation limit) while bounding memory use on
/// untrusted input.
const MAX_DECOMPRESSED_SIZE: usize = 10 * 1024 * 1024;

/// Encode a playlist as a share code.
pub fn encode_share_code(playlist: &PlaylistFile) -> Result<String> {
    validate_playlist_file(playlist)?;

    // The share code does not carry the playlist id; the decoder derives it
    // from the name via slugify. Reject names that slugify to empty (e.g.
    // "!!!" — no alphanumeric characters) so the round trip is guaranteed
    // to produce a valid playlist id.
    if crate::playlist::slugify(&playlist.playlist.name).is_empty() {
        return Err(VapourflyError::InvalidInput(
            "playlist name must contain at least one alphanumeric character for a share code"
                .into(),
        ));
    }

    let payload = encode_payload(&playlist.playlist)?;
    let compressed = compress(&payload)?;
    let encoded = URL_SAFE_NO_PAD.encode(&compressed);
    Ok(format!("{SHARE_CODE_PREFIX}{encoded}"))
}

/// Decode a share code back into a playlist file.
pub fn decode_share_code(code: &str) -> Result<PlaylistFile> {
    let payload = code.trim().strip_prefix(SHARE_CODE_PREFIX).ok_or_else(|| {
        VapourflyError::InvalidInput(format!("share code must start with '{SHARE_CODE_PREFIX}'"))
    })?;

    let compressed = URL_SAFE_NO_PAD
        .decode(payload)
        .map_err(|e| VapourflyError::InvalidInput(format!("invalid share code encoding: {e}")))?;
    if compressed.len() > MAX_COMPRESSED_SIZE {
        return Err(VapourflyError::InvalidInput(format!(
            "share code too large ({} bytes, max {MAX_COMPRESSED_SIZE})",
            compressed.len()
        )));
    }
    let binary = decompress(&compressed)?;
    let playlist = decode_payload(&binary)?;
    let pf = PlaylistFile {
        vapourfly_schema: crate::models::VAPOURFLY_PLAYLIST_SCHEMA.into(),
        created_by: "share-code".into(),
        playlist,
    };
    validate_playlist_file(&pf)?;
    Ok(pf)
}

// ---------------------------------------------------------------------------
// Binary payload encoding
// ---------------------------------------------------------------------------

fn encode_payload(playlist: &Playlist) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    buf.push(FORMAT_VERSION);

    match &playlist.content {
        PlaylistContent::Manual { app_ids } => {
            buf.push(TAG_MANUAL);
            write_u32_le(&mut buf, app_ids.len() as u32);
            for &id in app_ids {
                write_u32_le(&mut buf, id);
            }
        }
        PlaylistContent::Rules { rules } => {
            buf.push(TAG_RULES);
            let json = serde_json::to_vec(rules)
                .map_err(|e| VapourflyError::Internal(format!("failed to serialize rules: {e}")))?;
            write_u32_le(&mut buf, json.len() as u32);
            buf.extend_from_slice(&json);
        }
    }

    write_string_u16(&mut buf, &playlist.name)?;
    write_string_u16(&mut buf, &playlist.description)?;

    Ok(buf)
}

fn decode_payload(binary: &[u8]) -> Result<Playlist> {
    if binary.is_empty() {
        return Err(VapourflyError::InvalidInput(
            "empty share code payload".into(),
        ));
    }
    if binary[0] != FORMAT_VERSION {
        return Err(VapourflyError::InvalidInput(format!(
            "unsupported share code format version: {} (expected {})",
            binary[0], FORMAT_VERSION
        )));
    }
    let mut cursor = 1usize;

    if cursor >= binary.len() {
        return Err(VapourflyError::InvalidInput(
            "truncated share code: missing content tag".into(),
        ));
    }
    let tag = binary[cursor];
    cursor += 1;

    let content = match tag {
        TAG_MANUAL => {
            let count = read_u32_le(binary, &mut cursor)? as usize;
            // Each AppID is a u32 LE (4 bytes). Verify the declared count fits
            // within the remaining payload before allocating — a crafted
            // share code could declare count = u32::MAX to trigger an OOM
            // via Vec::with_capacity.
            let remaining = binary.len().saturating_sub(cursor);
            if count > remaining / 4 {
                return Err(VapourflyError::InvalidInput(
                    "truncated share code: app_id count exceeds payload".into(),
                ));
            }
            let mut app_ids = Vec::with_capacity(count);
            for _ in 0..count {
                app_ids.push(read_u32_le(binary, &mut cursor)?);
            }
            PlaylistContent::Manual { app_ids }
        }
        TAG_RULES => {
            let json_len = read_u32_le(binary, &mut cursor)? as usize;
            if cursor + json_len > binary.len() {
                return Err(VapourflyError::InvalidInput(
                    "truncated share code: rules JSON extends past end".into(),
                ));
            }
            let json_bytes = &binary[cursor..cursor + json_len];
            cursor += json_len;
            let rules: Vec<PlaylistRule> = serde_json::from_slice(json_bytes).map_err(|e| {
                VapourflyError::InvalidInput(format!("invalid share code rules json: {e}"))
            })?;
            PlaylistContent::Rules { rules }
        }
        other => {
            return Err(VapourflyError::InvalidInput(format!(
                "unknown content type tag: {other:#x}"
            )));
        }
    };

    let name = read_string_u16(binary, &mut cursor)?;
    let description = read_string_u16(binary, &mut cursor)?;

    // The share code does not carry the playlist id (ADR-0003: payload is
    // content + name + description). Derive a stable id from the name so the
    // playlist can be stored and referenced. The caller is free to rename it.
    let id = crate::playlist::slugify(&name);

    Ok(Playlist {
        id,
        name,
        description,
        content,
    })
}

// ---------------------------------------------------------------------------
// Compression
// ---------------------------------------------------------------------------

fn compress(data: &[u8]) -> Result<Vec<u8>> {
    let mut encoder = ZlibEncoder::new(data, Compression::default());
    let mut out = Vec::new();
    encoder
        .read_to_end(&mut out)
        .map_err(|e| VapourflyError::Internal(format!("compression failed: {e}")))?;
    Ok(out)
}

/// Decompress a zlib payload with a hard limit on output size.
///
/// Reads in chunks and aborts as soon as the decompressed output exceeds
/// [`MAX_DECOMPRESSED_SIZE`]. This is the primary defense against zlib
/// bombs: a small compressed input can otherwise expand to gigabytes and
/// exhaust memory. The caller has already enforced [`MAX_COMPRESSED_SIZE`]
/// on the input.
fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = ZlibDecoder::new(data);
    let mut out = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = decoder
            .read(&mut buf)
            .map_err(|e| VapourflyError::InvalidInput(format!("decompression failed: {e}")))?;
        if n == 0 {
            break;
        }
        if out.len() + n > MAX_DECOMPRESSED_SIZE {
            return Err(VapourflyError::InvalidInput(format!(
                "decompressed share code too large (max {MAX_DECOMPRESSED_SIZE} bytes)"
            )));
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Little-endian primitive helpers
// ---------------------------------------------------------------------------

fn write_u32_le(buf: &mut Vec<u8>, value: u32) {
    buf.extend_from_slice(&value.to_le_bytes());
}

fn write_string_u16(buf: &mut Vec<u8>, s: &str) -> Result<()> {
    let bytes = s.as_bytes();
    let len = u16::try_from(bytes.len()).map_err(|_| {
        VapourflyError::InvalidInput(format!(
            "string too long for share code ({} bytes, max {})",
            bytes.len(),
            u16::MAX
        ))
    })?;
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(bytes);
    Ok(())
}

fn read_u32_le(binary: &[u8], cursor: &mut usize) -> Result<u32> {
    if *cursor + 4 > binary.len() {
        return Err(VapourflyError::InvalidInput(
            "truncated share code: expected u32".into(),
        ));
    }
    let bytes: [u8; 4] = binary[*cursor..*cursor + 4]
        .try_into()
        .map_err(|_| VapourflyError::InvalidInput("invalid u32 slice".into()))?;
    *cursor += 4;
    Ok(u32::from_le_bytes(bytes))
}

fn read_string_u16(binary: &[u8], cursor: &mut usize) -> Result<String> {
    if *cursor + 2 > binary.len() {
        return Err(VapourflyError::InvalidInput(
            "truncated share code: expected string length".into(),
        ));
    }
    let len_bytes: [u8; 2] = binary[*cursor..*cursor + 2]
        .try_into()
        .map_err(|_| VapourflyError::InvalidInput("invalid u16 slice".into()))?;
    *cursor += 2;
    let len = u16::from_le_bytes(len_bytes) as usize;
    if *cursor + len > binary.len() {
        return Err(VapourflyError::InvalidInput(
            "truncated share code: string extends past end".into(),
        ));
    }
    let bytes = &binary[*cursor..*cursor + len];
    *cursor += len;
    String::from_utf8(bytes.to_vec())
        .map_err(|e| VapourflyError::InvalidInput(format!("invalid share code utf-8: {e}")))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Playlist, PlaylistContent, VAPOURFLY_PLAYLIST_SCHEMA};

    fn sample_manual_playlist() -> PlaylistFile {
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

    fn sample_rules_playlist() -> PlaylistFile {
        PlaylistFile {
            vapourfly_schema: VAPOURFLY_PLAYLIST_SCHEMA.into(),
            created_by: "user".into(),
            playlist: Playlist {
                id: "installed-unplayed".into(),
                name: "Installed Unplayed".into(),
                description: "Installed games with no recorded playtime".into(),
                content: PlaylistContent::Rules {
                    rules: vec![
                        PlaylistRule::Installed,
                        PlaylistRule::NotHidden,
                        PlaylistRule::PlaytimeBetween { min: 0, max: 0 },
                    ],
                },
            },
        }
    }

    #[test]
    fn share_code_round_trip_manual() {
        let pf = sample_manual_playlist();
        let code = encode_share_code(&pf).unwrap();
        assert!(code.starts_with(SHARE_CODE_PREFIX));
        let decoded = decode_share_code(&code).unwrap();
        // id is not carried by the share code; it's assigned by the caller.
        assert_eq!(decoded.playlist.name, pf.playlist.name);
        assert_eq!(decoded.playlist.description, pf.playlist.description);
        match &decoded.playlist.content {
            PlaylistContent::Manual { app_ids } => {
                assert_eq!(*app_ids, vec![292030, 367520]);
            }
            _ => panic!("expected manual"),
        }
    }

    #[test]
    fn share_code_round_trip_rules() {
        let pf = sample_rules_playlist();
        let code = encode_share_code(&pf).unwrap();
        assert!(code.starts_with(SHARE_CODE_PREFIX));
        let decoded = decode_share_code(&code).unwrap();
        assert_eq!(decoded.playlist.name, pf.playlist.name);
        assert_eq!(decoded.playlist.description, pf.playlist.description);
        match &decoded.playlist.content {
            PlaylistContent::Rules { rules } => {
                assert_eq!(rules.len(), 3);
                assert_eq!(rules[0], PlaylistRule::Installed);
                assert_eq!(rules[1], PlaylistRule::NotHidden);
            }
            _ => panic!("expected rules"),
        }
    }

    #[test]
    fn decode_rejects_bad_prefix() {
        let err = decode_share_code("BAD:abc").unwrap_err();
        assert!(err.to_string().contains("share code must start with"));
    }

    #[test]
    fn decode_rejects_old_base64url_json_format() {
        // The old format was VF1:<base64url(json)>. A JSON object starts with
        // '{' (0x7B), which after base64url decode becomes the first byte.
        // Our format expects 0x01 as the first byte, so old codes must fail
        // — either at decompression (the JSON bytes are not a valid zlib
        // stream) or at the version check.
        let old_json = r#"{"vapourfly_schema":"vapourfly.playlist.v1","created_by":"user","playlist":{"id":"x","name":"X","description":"","content":{"type":"Manual","value":{"app_ids":[1]}}}}"#;
        let old_encoded = URL_SAFE_NO_PAD.encode(old_json.as_bytes());
        let old_code = format!("{SHARE_CODE_PREFIX}{old_encoded}");
        let err = decode_share_code(&old_code).unwrap_err();
        // The key property: old codes do not decode successfully. The error
        // is either a decompression failure or a format-version mismatch.
        let msg = err.to_string();
        assert!(
            msg.contains("decompression") || msg.contains("format version"),
            "expected a decompression or format-version error, got: {msg}"
        );
    }

    #[test]
    fn encode_rejects_invalid_playlist() {
        let mut pf = sample_manual_playlist();
        pf.playlist.id.clear();
        let err = encode_share_code(&pf).unwrap_err();
        assert!(err.to_string().contains("playlist id must not be empty"));
    }

    #[test]
    fn decode_rejects_truncated_payload() {
        // Manually craft a compressed payload that decodes to a too-short binary.
        let truncated = compress(&[FORMAT_VERSION]).unwrap();
        let encoded = URL_SAFE_NO_PAD.encode(&truncated);
        let code = format!("{SHARE_CODE_PREFIX}{encoded}");
        let err = decode_share_code(&code).unwrap_err();
        assert!(err.to_string().contains("truncated"));
    }

    #[test]
    fn decode_rejects_unknown_version() {
        let bad = compress(&[0x99, TAG_MANUAL, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
        let encoded = URL_SAFE_NO_PAD.encode(&bad);
        let code = format!("{SHARE_CODE_PREFIX}{encoded}");
        let err = decode_share_code(&code).unwrap_err();
        assert!(
            err.to_string()
                .contains("unsupported share code format version")
        );
    }

    #[test]
    fn compact_code_is_shorter_than_json_for_manual_playlists() {
        // The ADR's rationale is that compact codes are shorter. Verify this
        // holds for a typical manual playlist.
        let pf = sample_manual_playlist();
        let code = encode_share_code(&pf).unwrap();
        let json = serde_json::to_string(&pf).unwrap();
        let json_code = format!(
            "{SHARE_CODE_PREFIX}{}",
            URL_SAFE_NO_PAD.encode(json.as_bytes())
        );
        assert!(
            code.len() < json_code.len(),
            "compact code ({}) should be shorter than json code ({})",
            code.len(),
            json_code.len()
        );
    }

    #[test]
    fn decode_rejects_inflated_app_id_count_without_oom() {
        // Craft a manual payload whose declared count is u32::MAX but whose
        // body has no app_ids. Without the bounds check this would call
        // Vec::with_capacity(u32::MAX) and panic on allocation. With the
        // check it must return a clean "exceeds payload" error.
        let mut payload = Vec::new();
        payload.push(FORMAT_VERSION);
        payload.push(TAG_MANUAL);
        payload.extend_from_slice(&u32::MAX.to_le_bytes()); // count
        // No app_id bytes follow — name + description are also absent, but
        // the count check fires first.
        let compressed = compress(&payload).unwrap();
        let encoded = URL_SAFE_NO_PAD.encode(&compressed);
        let code = format!("{SHARE_CODE_PREFIX}{encoded}");
        let err = decode_share_code(&code).unwrap_err();
        assert!(
            err.to_string().contains("exceeds payload"),
            "expected an 'exceeds payload' error, got: {err}"
        );
    }

    #[test]
    fn decode_rejects_app_id_count_exceeding_remaining_bytes() {
        // count = 10 but only 1 app_id present (4 bytes). 10 > 1, so reject.
        let mut payload = Vec::new();
        payload.push(FORMAT_VERSION);
        payload.push(TAG_MANUAL);
        payload.extend_from_slice(&10u32.to_le_bytes()); // count
        payload.extend_from_slice(&1u32.to_le_bytes()); // only one app_id
        let compressed = compress(&payload).unwrap();
        let encoded = URL_SAFE_NO_PAD.encode(&compressed);
        let code = format!("{SHARE_CODE_PREFIX}{encoded}");
        let err = decode_share_code(&code).unwrap_err();
        assert!(err.to_string().contains("exceeds payload"));
    }

    #[test]
    fn encode_rejects_name_that_slugifies_to_empty() {
        // A name with no alphanumeric characters slugifies to "", which
        // would make the decoded playlist fail validation (empty id).
        // Encode must reject this so the round trip is guaranteed valid.
        let mut pf = sample_manual_playlist();
        pf.playlist.name = "!!!".into();
        let err = encode_share_code(&pf).unwrap_err();
        assert!(
            err.to_string()
                .contains("at least one alphanumeric character"),
            "expected an alphanumeric-character error, got: {err}"
        );
    }

    #[test]
    fn encode_accepts_name_with_mixed_punctuation_and_alphanumerics() {
        // A name like "Deck - Shortlist!" slugifies to "deck-shortlist",
        // which is non-empty. This must encode and round-trip cleanly.
        let mut pf = sample_manual_playlist();
        pf.playlist.name = "Deck - Shortlist!".into();
        let code = encode_share_code(&pf).unwrap();
        let decoded = decode_share_code(&code).unwrap();
        assert_eq!(decoded.playlist.name, "Deck - Shortlist!");
        assert_eq!(decoded.playlist.id, "deck-shortlist");
    }

    #[test]
    fn decode_rejects_zlib_bomb() {
        // Build a zlib bomb: a small compressed stream that decompresses to
        // a payload larger than MAX_DECOMPRESSED_SIZE. We craft a valid
        // binary payload (version + tag + a huge run of zero app_ids) and
        // compress it. The compressed form is tiny; the decompressed form
        // is huge. Without the decompressed-size limit this would allocate
        // ~hundreds of MB and likely OOM the test process.
        let app_id_count: u32 = (MAX_DECOMPRESSED_SIZE as u32 / 4) + 1024;
        let mut payload = Vec::with_capacity(8 + app_id_count as usize * 4);
        payload.push(FORMAT_VERSION);
        payload.push(TAG_MANUAL);
        payload.extend_from_slice(&app_id_count.to_le_bytes());
        // All-zero app_ids (0 is invalid, but decode_payload's count check
        // happens after decompression — the decompression limit must fire
        // first, before we ever look at the payload).
        payload.resize(payload.len() + app_id_count as usize * 4, 0);
        let compressed = compress(&payload).unwrap();
        // Sanity: the compressed form is much smaller than the decompressed
        // form (this is what makes it a bomb).
        assert!(
            compressed.len() < payload.len() / 100,
            "test setup: expected a high compression ratio, got {} -> {}",
            payload.len(),
            compressed.len()
        );
        let encoded = URL_SAFE_NO_PAD.encode(&compressed);
        let code = format!("{SHARE_CODE_PREFIX}{encoded}");
        let err = decode_share_code(&code).unwrap_err();
        assert!(
            err.to_string().contains("too large"),
            "expected a 'too large' error, got: {err}"
        );
    }

    #[test]
    fn decode_rejects_oversized_compressed_input() {
        // A compressed payload larger than MAX_COMPRESSED_SIZE is rejected
        // before decompression even begins. Build a payload just over the
        // limit by padding a valid compressed stream with trailing garbage
        // (the size check fires before the zlib decoder sees the bytes).
        let pf = sample_manual_playlist();
        let code = encode_share_code(&pf).unwrap();
        let payload = code.strip_prefix(SHARE_CODE_PREFIX).unwrap();
        let mut compressed = URL_SAFE_NO_PAD.decode(payload).unwrap();
        // Pad to just over the limit.
        compressed.resize(MAX_COMPRESSED_SIZE + 1, 0);
        let encoded = URL_SAFE_NO_PAD.encode(&compressed);
        let oversized_code = format!("{SHARE_CODE_PREFIX}{encoded}");
        let err = decode_share_code(&oversized_code).unwrap_err();
        assert!(
            err.to_string().contains("share code too large"),
            "expected a 'share code too large' error, got: {err}"
        );
    }

    #[test]
    fn decode_accepts_payload_at_decompression_limit() {
        // A payload whose decompressed size is exactly at the limit must
        // decode without hitting the "too large" error. Build a manual
        // payload whose total size is exactly MAX_DECOMPRESSED_SIZE:
        //   1 (version) + 1 (tag) + 4 (count) + count*4 (app_ids)
        //   + 2 (name len) + 2 (name "ab") + 2 (desc len) + 0 (desc)
        // = 12 + count*4. Solving for count with MAX_DECOMPRESSED_SIZE even
        // and divisible by 4 after subtracting 12.
        let overhead = 1 + 1 + 4 + 2 + 2 + 2; // version + tag + count + name_len + name + desc_len
        let count = (MAX_DECOMPRESSED_SIZE - overhead) / 4;
        let mut payload = Vec::with_capacity(MAX_DECOMPRESSED_SIZE);
        payload.push(FORMAT_VERSION);
        payload.push(TAG_MANUAL);
        payload.extend_from_slice(&(count as u32).to_le_bytes());
        // Use app_id = 1 (valid, non-zero) so the payload passes
        // validate_app_ids if it reaches that check.
        for _ in 0..count {
            payload.extend_from_slice(&1u32.to_le_bytes());
        }
        // 2-byte name "ab" (slugifies to "ab", non-empty) and empty desc.
        payload.extend_from_slice(&2u16.to_le_bytes());
        payload.extend_from_slice(b"ab");
        payload.extend_from_slice(&0u16.to_le_bytes());
        assert_eq!(
            payload.len(),
            MAX_DECOMPRESSED_SIZE,
            "test setup: payload should be exactly at the limit"
        );
        let compressed = compress(&payload).unwrap();
        let encoded = URL_SAFE_NO_PAD.encode(&compressed);
        let code = format!("{SHARE_CODE_PREFIX}{encoded}");
        let result = decode_share_code(&code);
        // The decode must not fail with a "too large" error. It may succeed
        // (the payload is valid) or fail for an unrelated reason, but the
        // size check itself must not fire at exactly the limit.
        match result {
            Ok(pf) => {
                assert_eq!(pf.playlist.name, "ab");
                assert_eq!(pf.playlist.id, "ab");
            }
            Err(e) => {
                let msg = e.to_string();
                assert!(
                    !msg.contains("too large"),
                    "decode at the exact limit should not hit the size check, got: {msg}"
                );
            }
        }
    }
}
