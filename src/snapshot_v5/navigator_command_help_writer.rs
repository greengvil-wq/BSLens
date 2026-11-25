//! Политика выборки Навигатора: справка общих команд остаётся в Git.
use super::{navigator_owners_writer, Error, Result};
use rusqlite::Connection;
use serde_json::{json, Value};
use std::collections::BTreeSet;

pub(crate) const REASON: &str = "CommonCommandHelpExcludedNotRead";

pub(super) fn excluded(path: &str) -> bool {
    let mut p = path.split('/');
    matches!(
        (p.next(), p.next(), p.next(), p.next(), p.next(), p.next()),
        (Some("CommonCommands"), Some(name), Some("Ext"), Some("Help.xml"), None, None)
        | (Some("CommonCommands"), Some(name), Some("Ext"), Some("Help"), Some("ru.html"), None)
        if !name.is_empty()
    )
}

pub(crate) fn read_manifest(db: &Connection) -> Result<Value> {
    // Вызывается после общего ограничения metadata.input_manifest (16 MiB).
    let raw: String = db.query_row("SELECT input_manifest FROM metadata", [], |r| r.get(0))?;
    Ok(serde_json::from_str(&raw)?)
}

pub(crate) fn apply_coverage(coverage: &mut Value, manifest: &Value) -> Result<()> {
    let invalid = || Error::Contract("common_command_help_manifest");
    let mut seen = BTreeSet::new();
    let mut count = 0usize;
    for entry in manifest["excluded"].as_array().ok_or_else(invalid)? {
        let path = entry["input"]["path"].as_str().ok_or_else(invalid)?;
        let reason = entry["reason"].as_str().ok_or_else(invalid)?;
        if excluded(path) != (reason == REASON) {
            return Err(invalid());
        }
        if reason == REASON {
            let oid = entry["input"]["blob_oid"].as_str().ok_or_else(invalid)?;
            if path.len() > 1024
                || !seen.insert(path)
                || !matches!(entry["input"]["mode"].as_str(), Some("100644" | "100755"))
                || !matches!(oid.len(), 40 | 64)
                || !oid
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(invalid());
            }
            count += 1;
        }
    }
    for entry in manifest["selected"].as_array().ok_or_else(invalid)? {
        if excluded(entry["path"].as_str().ok_or_else(invalid)?) {
            return Err(invalid());
        }
    }
    coverage["source_exclusions"]["common_command_help"] = json!({
        "status":"excluded_not_read","count":count,"reason":REASON
    });
    Ok(())
}

pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    navigator_owners_writer::validate_metadata_for(db, true)?;
    let mut expected = json!({});
    apply_coverage(&mut expected, &read_manifest(db)?)?;
    let raw: String = db.query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))?;
    let coverage: Value = serde_json::from_str(&raw)?;
    let mut statement =
        db.prepare("SELECT path FROM sources WHERE path GLOB 'CommonCommands/*'")?;
    let rows = statement.query_map([], |r| r.get::<_, String>(0))?;
    for row in rows {
        if excluded(&row?) {
            return Err(Error::Contract("common_command_help_present"));
        }
    }
    if coverage["source_exclusions"]["common_command_help"]
        != expected["source_exclusions"]["common_command_help"]
    {
        return Err(Error::Contract("common_command_help_coverage"));
    }
    Ok(())
}
