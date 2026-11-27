//! Полные ограниченные XML diagnostics вне metadata.
use super::{companion_writer, corpus_config_writer, streaming_facts, BuiltSnapshot, Result};
use crate::git_manifest::ingest;
use rusqlite::Connection;
use std::path::Path;
pub const PROFILE: &str = "linux-v5-diagnostics1-xml221-r09";
pub(crate) fn schema() -> String {
    format!(
        "{}\n{}",
        corpus_config_writer::schema().replace(corpus_config_writer::PROFILE, PROFILE),
        DDL
    )
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(db, &super::profile_spec::DIAGNOSTICS)
}
pub fn build(
    repo: &Path,
    commit: &str,
    tree: &str,
    source_id: &str,
    output: &Path,
    limits: streaming_facts::Limits,
    git: ingest::Limits,
    globals: &[String],
    bin_bytes: u64,
    diagnostics_bytes: u64,
) -> Result<BuiltSnapshot> {
    companion_writer::build_mode(
        repo,
        commit,
        tree,
        source_id,
        output,
        limits,
        git,
        &super::profile_spec::DIAGNOSTICS,
        super::companion_writer::Runtime {
            global_opaque: globals,
            bin_bytes: Some(bin_bytes),
            diagnostics: Some(diagnostics_bytes),
            processor_help: None,
            omit_command_help: super::profile_spec::Feature::Off,
        },
    )
}

use super::{Error, Limits};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
pub const ROW_BYTES: u64 = 16384;
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const DOMAIN: &str = "BSLens-v5/xml-diagnostics/json1";
pub(crate) const DDL:&str="CREATE TABLE xml_diagnostics (diagnostic_id INTEGER PRIMARY KEY, path TEXT NOT NULL REFERENCES sources(path), selector TEXT NOT NULL) STRICT;
CREATE INDEX xml_diagnostics_order ON xml_diagnostics(path COLLATE BINARY,selector COLLATE BINARY);";
pub(crate) fn check_limit(bytes: u64) -> Result<()> {
    if bytes == 0 {
        return Err(Error::Contract("diagnostics_limit"));
    }
    Limits::check("diagnostics_bytes", MAX_BYTES, bytes)
}
pub(crate) fn row_json(path: &str, selector: &str) -> Result<String> {
    // Bound input before serialization, then account exact escaped UTF-8 JSON.
    Limits::check(
        "diagnostic_row_bytes",
        ROW_BYTES,
        (path.len() + selector.len()) as u64,
    )?;
    let row = json!({"path":path,"selector":selector}).to_string();
    Limits::check("diagnostic_row_bytes", ROW_BYTES, row.len() as u64)?;
    Ok(row)
}
pub(crate) fn summarize(db: &Connection, cap: u64, facts: u64) -> Result<Value> {
    check_limit(cap)?;
    let count: u64 = db.query_row(
        "SELECT count(*) FROM (SELECT 1 FROM xml_diagnostics LIMIT ?1)",
        [facts.saturating_add(1)],
        |r| r.get(0),
    )?;
    Limits::check("facts", facts, count)?;
    let oversized:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM xml_diagnostics WHERE length(CAST(path AS BLOB))+length(CAST(selector AS BLOB))>?1)",[ROW_BYTES],|r|r.get(0))?;
    if oversized {
        return Err(Error::Limit {
            name: "diagnostic_row_bytes",
            maximum: ROW_BYTES,
            actual: ROW_BYTES + 1,
        });
    }
    let mut h = Sha256::new();
    h.update(DOMAIN.as_bytes());
    h.update([0]);
    let mut count = 0u64;
    let mut bytes = 0u64;
    // Indexed order, no sorting/all-row text allocation. Preserve duplicate diagnostics.
    let mut q=db.prepare("SELECT path,selector FROM xml_diagnostics INDEXED BY xml_diagnostics_order ORDER BY path COLLATE BINARY,selector COLLATE BINARY")?;
    let mut rows = q.query([])?;
    while let Some(r) = rows.next()? {
        let p = r
            .get_ref(0)?
            .as_str()
            .map_err(|_| Error::Contract("diagnostic_text"))?;
        let s = r
            .get_ref(1)?
            .as_str()
            .map_err(|_| Error::Contract("diagnostic_text"))?;
        let row = row_json(p, s)?;
        count += 1;
        bytes += row.len() as u64;
        Limits::check("facts", facts, count)?;
        Limits::check("diagnostics_bytes", cap, bytes)?;
        h.update((row.len() as u64).to_be_bytes());
        h.update(row.as_bytes());
    }
    Ok(
        json!({"format":"xml-diagnostics-json-v1","domain":DOMAIN,"count":count,"bytes":bytes,"sha256":format!("{:x}",h.finalize()),"details":"xml_diagnostics","semantics":"not_analyzed"}),
    )
}
pub(crate) fn validate_saved(db: &Connection) -> Result<()> {
    let size: u64 = db.query_row(
        "SELECT length(CAST(coverage_draft AS BLOB)) FROM metadata",
        [],
        |r| r.get(0),
    )?;
    Limits::check("coverage_summary_bytes", 8192, size)?;
    let (raw, params): (String, String) = db.query_row(
        "SELECT coverage_draft,build_parameters FROM metadata",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let value: Value = serde_json::from_str(&raw)?;
    let p: Value = serde_json::from_str(&params)?;
    let summary = summarize(
        db,
        p["diagnostics"]["bytes"]
            .as_u64()
            .ok_or(Error::Contract("diagnostics_limit"))?,
        p["facts_limits"]["facts"]
            .as_u64()
            .ok_or(Error::Contract("diagnostics_facts"))?,
    )?;
    if value.get("xml_unverified").is_some() || value["xml_diagnostics"] != summary {
        return Err(Error::Contract("diagnostics_summary"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_exact_row_total_duplicates_and_domain() {
        assert!(check_limit(MAX_BYTES).is_ok());
        assert!(check_limit(MAX_BYTES + 1).is_err());
        assert!(check_limit(0).is_err());
        let overhead = row_json("p", "").unwrap().len();
        let s = "x".repeat(ROW_BYTES as usize - overhead);
        assert_eq!(row_json("p", &s).unwrap().len(), ROW_BYTES as usize);
        assert!(matches!(
            row_json("p", &(s + "x")),
            Err(Error::Limit {
                name: "diagnostic_row_bytes",
                ..
            })
        ));
        assert!(row_json("p", &"\"".repeat(9000)).is_err());
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(
            "CREATE TABLE sources(path TEXT PRIMARY KEY);INSERT INTO sources VALUES('p');",
        )
        .unwrap();
        db.execute_batch(DDL).unwrap();
        for _ in 0..2 {
            db.execute(
                "INSERT INTO xml_diagnostics(path,selector) VALUES ('p','s')",
                [],
            )
            .unwrap();
        }
        let n = row_json("p", "s").unwrap().len() as u64 * 2;
        let summary = summarize(&db, n, 2).unwrap();
        assert_eq!(summary["count"], 2);
        assert_eq!(summary["bytes"], n);
        assert!(matches!(
            summarize(&db, n - 1, 2),
            Err(Error::Limit {
                name: "diagnostics_bytes",
                ..
            })
        ));
        assert!(matches!(
            summarize(&db, n, 1),
            Err(Error::Limit { name: "facts", .. })
        ));
        let mut h = Sha256::new();
        h.update(DOMAIN.as_bytes());
        h.update([0]);
        for _ in 0..2 {
            let s = row_json("p", "s").unwrap();
            h.update((s.len() as u64).to_be_bytes());
            h.update(s.as_bytes());
        }
        assert_eq!(summary["sha256"], format!("{:x}", h.finalize()));
        db.execute("DELETE FROM xml_diagnostics WHERE diagnostic_id=1", [])
            .unwrap();
        assert_ne!(summarize(&db, n, 2).unwrap()["sha256"], summary["sha256"]);
    }
}
