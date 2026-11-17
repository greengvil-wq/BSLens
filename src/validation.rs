//! Explicit development-only reader. Not a substitute for ValidatedSnapshot.
use crate::{Error, Result};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
};

fn fail<T>(message: &str) -> Result<T> {
    Err(Error(message.into()))
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn sha(raw: &[u8]) -> String {
    hex(&Sha256::digest(raw))
}
fn oid(raw: &[u8], size: usize) -> String {
    let mut data = format!("blob {}\0", raw.len()).into_bytes();
    data.extend(raw);
    if size == 40 {
        hex(&Sha1::digest(&data))
    } else {
        sha(&data)
    }
}
fn is_oid(s: &str, size: usize) -> bool {
    s.len() == size
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
pub(super) fn address(s: &str) -> Result<()> {
    if s.is_empty()
        || s.contains(['\\', ':'])
        || s.chars().any(|c| c < ' ')
        || s.split('/').any(|c| c.is_empty() || c == "." || c == "..")
    {
        return fail("invalid Git address");
    }
    Ok(())
}
fn positions(raw: &[u8]) -> Result<Vec<[u64; 3]>> {
    std::str::from_utf8(raw).map_err(|_| Error("invalid UTF-8".into()))?;
    let mut start = 0;
    let mut result = vec![];
    for chunk in raw.split_inclusive(|&c| c == b'\n') {
        let end = start + chunk.len() as u64;
        result.push([result.len() as u64 + 1, start, end]);
        start = end;
    }
    Ok(result)
}
// Python ensure_ascii=True, sorted-key JSON canonicalization for v4 identity.
fn canonical(v: &Value) -> Result<String> {
    let encoded = serde_json::to_string(v)?;
    let mut ascii = String::new();
    for c in encoded.chars() {
        if c.is_ascii() {
            ascii.push(c);
        } else {
            for u in c.encode_utf16(&mut [0; 2]) {
                ascii.push_str(&format!("\\u{u:04x}"));
            }
        }
    }
    Ok(ascii)
}

pub(super) struct Source {
    pub raw: Vec<u8>,
    pub oid: String,
    pub sha256: String,
    pub positions: Vec<[u64; 3]>,
}
pub(super) fn read_fixture(
    path: &Path,
    expected_id: &str,
) -> Result<(Value, BTreeMap<String, Source>)> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| Error(e.to_string()))?
            .join(path)
    };
    let mut current = std::path::PathBuf::new();
    for part in absolute.components() {
        if matches!(part, Component::ParentDir) {
            return fail("parent traversal unsupported");
        }
        current.push(part);
        let info = fs::symlink_metadata(&current).map_err(|e| Error(e.to_string()))?;
        if info.file_type().is_symlink() {
            return fail("filesystem links unsupported");
        }
    }
    let info = fs::metadata(&absolute).map_err(|e| Error(e.to_string()))?;
    if !info.is_file() || info.len() > 64 * 1024 * 1024 {
        return fail("synthetic fixture file limit");
    }
    let db = Connection::open_with_flags(
        absolute,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA query_only=ON; BEGIN;")?;
    let integrity: Vec<String> = db
        .prepare("PRAGMA integrity_check")?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    if integrity != ["ok"] {
        return fail("SQLite integrity failed");
    }
    if db.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? != 4 {
        return fail("unsupported schema");
    }
    let mut meta = serde_json::Map::new();
    let mut stmt = db.prepare("SELECT key,value FROM metadata")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (key, text) = row?;
        if meta.insert(key, serde_json::from_str(&text)?).is_some() {
            return fail("duplicate metadata");
        }
    }
    let mut meta = Value::Object(meta);
    if meta["schema_version"] != 4 || meta["profile"] != "synthetic-bsl-utf8-lines-v2" {
        return fail("R01 only supports authored synthetic BSL fixtures");
    }
    if meta["snapshot_id"].as_str() != Some(expected_id) {
        return fail("snapshot mismatch");
    }
    let commit = meta["commit"]
        .as_str()
        .ok_or_else(|| Error("missing commit".into()))?;
    if ![40, 64].contains(&commit.len()) || !is_oid(commit, commit.len()) {
        return fail("invalid exact Git SHA");
    }
    let size = commit.len();
    let mut sources = BTreeMap::new();
    let mut records = vec![];
    let mut stmt = db.prepare("SELECT path,oid,sha256,content FROM sources ORDER BY path")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Vec<u8>>(3)?,
        ))
    })? {
        let (path, stored_oid, stored_sha, raw) = row?;
        address(&path)?;
        if !path.to_ascii_lowercase().ends_with(".bsl") {
            return fail("non-BSL synthetic source");
        }
        let actual_positions = positions(&raw)?;
        let stored: Vec<[u64; 3]> = db
            .prepare("SELECT line,start_byte,end_byte FROM positions WHERE path=? ORDER BY line")?
            .query_map([&path], |r| Ok([r.get(0)?, r.get(1)?, r.get(2)?]))?
            .collect::<std::result::Result<_, _>>()?;
        if sha(&raw) != stored_sha || oid(&raw, size) != stored_oid || stored != actual_positions {
            return fail("source content or positions mismatch");
        }
        records.push(json!([path, stored_oid, stored_sha, stored]));
        if sources
            .insert(
                path,
                Source {
                    raw,
                    oid: stored_oid,
                    sha256: stored_sha,
                    positions: stored,
                },
            )
            .is_some()
        {
            return fail("duplicate source");
        }
    }
    if sources.is_empty() {
        return fail("empty sources");
    }
    let inventory = meta["inventory"]
        .as_array()
        .ok_or_else(|| Error("missing inventory".into()))?;
    let mut selected = vec![];
    let mut last: Option<&str> = None;
    for item in inventory {
        let pair = item
            .as_array()
            .filter(|p| p.len() == 2)
            .ok_or_else(|| Error("invalid inventory".into()))?;
        let p = pair[0]
            .as_str()
            .ok_or_else(|| Error("invalid inventory path".into()))?;
        address(p)?;
        let o = pair[1]
            .as_str()
            .ok_or_else(|| Error("invalid inventory OID".into()))?;
        if last.is_some_and(|l| l >= p) || !is_oid(o, size) {
            return fail("invalid inventory order or OID");
        }
        last = Some(p);
        if p.to_ascii_lowercase().ends_with(".bsl") || p.to_ascii_lowercase().ends_with(".xml") {
            selected.push(json!([p, o]));
        }
    }
    let actual: Vec<Value> = sources.iter().map(|(p, s)| json!([p, s.oid])).collect();
    if selected != actual {
        return fail("inventory mismatch");
    }
    if db.prepare("PRAGMA foreign_key_check")?.exists([])? {
        return fail("foreign key violation");
    }
    let id = meta.as_object_mut().unwrap().remove("snapshot_id").unwrap();
    let computed = format!(
        "bslens-v4-{}",
        sha(canonical(&json!({"metadata":meta,"files":records}))?.as_bytes())
    );
    if id != computed {
        return fail("snapshot identity mismatch");
    }
    meta.as_object_mut()
        .unwrap()
        .insert("snapshot_id".into(), id);
    // Connection closes here. All reads below use the checked in-memory fixture.
    Ok((meta, sources))
}
