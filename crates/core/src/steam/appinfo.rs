//! Parse Steam's `appinfo.vdf` cache for application display names.
//!
//! Path: `{steam}/appcache/appinfo.vdf`
//!
//! Supports V1/V2/V3 magic numbers. V3 uses a string table and 32-bit property
//! name indices; V1/V2 use null-terminated UTF-8 property names.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::error::{Result, VapourflyError};

const MAGIC_V1: u32 = 0x0756_4427;
const MAGIC_V2: u32 = 0x0756_4428;
const MAGIC_V3: u32 = 0x0756_4429;

const TYPE_TABLE: u8 = 0;
const TYPE_STRING: u8 = 1;
const TYPE_INT32: u8 = 2;
const TYPE_FLOAT: u8 = 3;
const TYPE_WSTRING: u8 = 5;
const TYPE_COLOR: u8 = 6;
const TYPE_UINT64: u8 = 7;
const TYPE_END: u8 = 8;

/// Look up display names for the given AppIDs from `appcache/appinfo.vdf`.
///
/// Missing AppIDs are omitted from the result. Returns an empty map when the
/// file does not exist.
pub fn lookup_appinfo_names(
    steam_dir: &Path,
    wanted: &HashSet<u32>,
) -> Result<HashMap<u32, String>> {
    if wanted.is_empty() {
        return Ok(HashMap::new());
    }

    let path = steam_dir.join("appcache/appinfo.vdf");
    if !path.is_file() {
        return Ok(HashMap::new());
    }

    let mut file = File::open(&path).map_err(|e| {
        VapourflyError::InvalidInput(format!("failed to open {}: {e}", path.display()))
    })?;

    parse_appinfo_file(&mut file, wanted)
}

fn parse_appinfo_file<R: Read + Seek>(
    file: &mut R,
    wanted: &HashSet<u32>,
) -> Result<HashMap<u32, String>> {
    let magic = read_u32(file)?;
    let _universe = read_u32(file)?;

    let (string_pool, has_v2_extra) = match magic {
        MAGIC_V1 => (Vec::new(), false),
        MAGIC_V2 => (Vec::new(), true),
        MAGIC_V3 => {
            let string_table_offset = read_i64(file)? as u64;
            let entries_offset = file.stream_position().map_err(map_io)?;
            let pool = read_string_pool(file, string_table_offset)?;
            file.seek(SeekFrom::Start(entries_offset)).map_err(map_io)?;
            (pool, true)
        }
        other => {
            return Err(VapourflyError::ParseError {
                path: crate::SafePath::new("appcache/appinfo.vdf"),
                format: "appinfo".into(),
                reason: format!("unsupported magic 0x{other:08X}"),
            });
        }
    };

    let mut names = HashMap::new();

    loop {
        let app_id = read_u32(file)?;
        if app_id == 0 {
            break;
        }

        let data_len = read_u32(file)? as usize;
        let mut data = vec![0u8; data_len];
        file.read_exact(&mut data).map_err(map_io)?;

        if !wanted.contains(&app_id) {
            continue;
        }

        if let Some(name) = extract_common_name(&data, &string_pool, has_v2_extra)
            && !name.is_empty()
        {
            names.insert(app_id, name);
        }
    }

    Ok(names)
}

fn read_string_pool<R: Read + Seek>(file: &mut R, offset: u64) -> Result<Vec<String>> {
    file.seek(SeekFrom::Start(offset)).map_err(map_io)?;
    let count = read_u32(file)? as usize;
    let mut pool = Vec::with_capacity(count);
    for _ in 0..count {
        pool.push(read_cstring(file)?);
    }
    Ok(pool)
}

fn extract_common_name(data: &[u8], string_pool: &[String], has_v2_extra: bool) -> Option<String> {
    let mut cursor = 0usize;
    if data.len() < 16 + 20 + 4 {
        return None;
    }
    cursor += 16; // pre-hash header
    cursor += 20; // sha1
    cursor += 4; // change number
    if has_v2_extra {
        if cursor + 20 > data.len() {
            return None;
        }
        cursor += 20;
    }

    let mut found = None;
    walk_properties(
        data,
        &mut cursor,
        string_pool,
        &mut Vec::new(),
        &mut |path, value| {
            if path.len() == 3 && path[0] == "appinfo" && path[1] == "common" && path[2] == "name" {
                found = Some(value.to_string());
            }
        },
    );
    found
}

fn walk_properties(
    data: &[u8],
    cursor: &mut usize,
    string_pool: &[String],
    path: &mut Vec<String>,
    on_string: &mut dyn FnMut(&[String], &str),
) -> bool {
    loop {
        let Some(type_byte) = read_byte(data, cursor) else {
            return false;
        };
        if type_byte == TYPE_END {
            return true;
        }

        let Some(name) = read_property_name(data, cursor, string_pool) else {
            return false;
        };

        match type_byte {
            TYPE_TABLE => {
                path.push(name);
                if !walk_properties(data, cursor, string_pool, path, on_string) {
                    path.pop();
                    return false;
                }
                path.pop();
            }
            TYPE_STRING => {
                path.push(name);
                if let Some(value) = read_cstring_from(data, cursor) {
                    on_string(path, &value);
                } else {
                    path.pop();
                    return false;
                }
                path.pop();
            }
            TYPE_WSTRING => {
                path.push(name);
                if let Some(value) = read_wstring_from(data, cursor) {
                    on_string(path, &value);
                } else {
                    path.pop();
                    return false;
                }
                path.pop();
            }
            TYPE_INT32 | TYPE_FLOAT => {
                *cursor += 4;
            }
            TYPE_COLOR => {
                *cursor += 3;
            }
            TYPE_UINT64 => {
                *cursor += 8;
            }
            _ => return false,
        }
    }
}

fn read_property_name(data: &[u8], cursor: &mut usize, string_pool: &[String]) -> Option<String> {
    if string_pool.is_empty() {
        read_cstring_from(data, cursor)
    } else {
        let idx = read_i32_from(data, cursor)? as usize;
        string_pool.get(idx).cloned()
    }
}

fn read_byte(data: &[u8], cursor: &mut usize) -> Option<u8> {
    if *cursor >= data.len() {
        return None;
    }
    let b = data[*cursor];
    *cursor += 1;
    Some(b)
}

fn read_cstring_from(data: &[u8], cursor: &mut usize) -> Option<String> {
    let start = *cursor;
    while *cursor < data.len() && data[*cursor] != 0 {
        *cursor += 1;
    }
    if *cursor >= data.len() {
        return None;
    }
    let s = std::str::from_utf8(&data[start..*cursor]).ok()?.to_string();
    *cursor += 1;
    Some(s)
}

fn read_wstring_from(data: &[u8], cursor: &mut usize) -> Option<String> {
    let mut chars = Vec::new();
    while *cursor + 1 < data.len() {
        let code = u16::from_le_bytes([data[*cursor], data[*cursor + 1]]);
        *cursor += 2;
        if code == 0 {
            break;
        }
        chars.push(code);
    }
    String::from_utf16(&chars).ok()
}

fn read_i32_from(data: &[u8], cursor: &mut usize) -> Option<i32> {
    if *cursor + 4 > data.len() {
        return None;
    }
    let value = i32::from_le_bytes([
        data[*cursor],
        data[*cursor + 1],
        data[*cursor + 2],
        data[*cursor + 3],
    ]);
    *cursor += 4;
    Some(value)
}

fn read_cstring<R: Read>(file: &mut R) -> Result<String> {
    let mut bytes = Vec::new();
    loop {
        let mut buf = [0u8; 1];
        file.read_exact(&mut buf).map_err(map_io)?;
        if buf[0] == 0 {
            break;
        }
        bytes.push(buf[0]);
    }
    Ok(String::from_utf8(bytes).unwrap_or_default())
}

fn read_u32<R: Read>(file: &mut R) -> Result<u32> {
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf).map_err(map_io)?;
    Ok(u32::from_le_bytes(buf))
}

fn read_i64<R: Read>(file: &mut R) -> Result<i64> {
    let mut buf = [0u8; 8];
    file.read_exact(&mut buf).map_err(map_io)?;
    Ok(i64::from_le_bytes(buf))
}

fn map_io(err: std::io::Error) -> VapourflyError {
    VapourflyError::InvalidInput(format!("appinfo.vdf read error: {err}"))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn write_cstring(buf: &mut Vec<u8>, s: &str) {
        buf.extend_from_slice(s.as_bytes());
        buf.push(0);
    }

    fn build_v1_fixture(app_id: u32, name: &str) -> Vec<u8> {
        let mut props = Vec::new();
        buf_push_table(&mut props, "appinfo", |appinfo| {
            buf_push_table(appinfo, "common", |common| {
                buf_push_string(common, "name", name);
            });
        });

        let mut entry = Vec::new();
        entry.extend_from_slice(&[0u8; 16]);
        entry.extend_from_slice(&[0u8; 20]);
        entry.extend_from_slice(&1u32.to_le_bytes());
        entry.extend_from_slice(&props);

        let mut file = Vec::new();
        file.extend_from_slice(&MAGIC_V1.to_le_bytes());
        file.extend_from_slice(&1u32.to_le_bytes());
        file.extend_from_slice(&app_id.to_le_bytes());
        file.extend_from_slice(&(entry.len() as u32).to_le_bytes());
        file.extend_from_slice(&entry);
        file.extend_from_slice(&0u32.to_le_bytes());
        file
    }

    fn buf_push_table(buf: &mut Vec<u8>, name: &str, fill: impl FnOnce(&mut Vec<u8>)) {
        buf.push(TYPE_TABLE);
        write_cstring(buf, name);
        fill(buf);
        buf.push(TYPE_END);
    }

    fn buf_push_string(buf: &mut Vec<u8>, name: &str, value: &str) {
        buf.push(TYPE_STRING);
        write_cstring(buf, name);
        write_cstring(buf, value);
    }

    #[test]
    fn extracts_name_from_v1_fixture() {
        let data = build_v1_fixture(42, "Fixture Game");
        let mut cursor = Cursor::new(data);
        let wanted = HashSet::from([42]);
        let names = parse_appinfo_file(&mut cursor, &wanted).unwrap();
        assert_eq!(names.get(&42).map(String::as_str), Some("Fixture Game"));
    }

    #[test]
    fn skips_unwanted_apps() {
        let data = build_v1_fixture(42, "Fixture Game");
        let mut cursor = Cursor::new(data);
        let wanted = HashSet::from([99]);
        let names = parse_appinfo_file(&mut cursor, &wanted).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    fn lookup_missing_file_returns_empty() {
        let names =
            lookup_appinfo_names(Path::new("/nonexistent/steam"), &HashSet::from([1])).unwrap();
        assert!(names.is_empty());
    }

    #[test]
    #[ignore = "requires local Steam installation"]
    fn lookup_real_steam_favorites_if_present() {
        let steam = dirs::home_dir()
            .unwrap()
            .join("Library/Application Support/Steam");
        if !steam.join("appcache/appinfo.vdf").is_file() {
            return;
        }
        let wanted = HashSet::from([274190, 204360, 730]);
        let names = lookup_appinfo_names(&steam, &wanted).unwrap();
        assert_eq!(names.get(&274190).map(String::as_str), Some("Broforce"));
        assert_eq!(
            names.get(&204360).map(String::as_str),
            Some("Castle Crashers")
        );
        assert_eq!(
            names.get(&730).map(String::as_str),
            Some("Counter-Strike 2")
        );
    }
}
