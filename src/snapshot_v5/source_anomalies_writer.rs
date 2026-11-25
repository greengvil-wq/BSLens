//! Opt-in structural inventory qualification for bounded source anomalies.
use super::{companion_writer, sidecar_writer, streaming_facts, BuiltSnapshot, Result};
use crate::git_manifest::ingest;
use rusqlite::Connection;
use std::path::Path;
pub const LEXICAL: &str = "r09-source-anomalies-v1";
pub const PROFILE: &str = "linux-v5-source-anomalies1-xml221-r09";
pub(crate) fn schema() -> String {
    let mut schema = super::source_limits_writer::schema().replace(super::source_limits_writer::PROFILE, PROFILE);
    schema.push_str(r#"
CREATE TABLE bsl_source_anomalies (
    module_path TEXT PRIMARY KEY REFERENCES modules(path),
    code TEXT NOT NULL CHECK (code IN ('lone_cr_structural_agreement','method_terminal_semicolon','empty_opaque_physical_line')),
    physical_line INTEGER NOT NULL CHECK (physical_line >= 1),
    byte_start INTEGER NOT NULL CHECK (byte_start >= 0),
    byte_end INTEGER NOT NULL CHECK (byte_end > byte_start AND byte_end - byte_start <= 2),
    FOREIGN KEY(module_path,physical_line) REFERENCES source_lines(path,line_number)
) STRICT;
"#);
    schema
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(
        db, true, true, true, true, true, true, true, true, true, true, true, true, true, true,
        true, true, true, true, true, true, true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        false,
        false,
        true,
        false,
    )?;
    sidecar_writer::validate_summary(db)?;
    let (count, bad): (u64, bool) = db.query_row(
        "SELECT (SELECT count(*) FROM bsl_source_anomalies), EXISTS(SELECT 1 FROM bsl_source_anomalies a JOIN modules m ON m.path=a.module_path JOIN source_lines l ON l.path=a.module_path AND l.line_number=a.physical_line WHERE m.parse_status<>'accepted' OR a.byte_start<l.byte_start OR a.byte_end>l.byte_end)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let stored: String = db.query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))?;
    let summary: serde_json::Value = serde_json::from_str(&stored)?;
    let qualified = match summary["parse_counts"].get("parsed_with_source_anomalies") {
        Some(v) => v.as_u64().ok_or(super::Error::Contract("source_anomaly_inventory"))?,
        None => 0,
    };
    if bad || count != qualified {
        return Err(super::Error::Contract("source_anomaly_inventory"));
    }
    let complete: bool = db.query_row(
        "SELECT (SELECT count(*) FROM method_branches)=(SELECT count(*) FROM methods)",
        [],
        |r| r.get(0),
    )?;
    if !complete {
        return Err(super::Error::Contract("method_branch_inventory"));
    }
    Ok(())
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
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        globals,
        Some(bin_bytes),
        true,
        true,
        Some(diagnostics_bytes),
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        true,
        false,
        false,
        true,
        false,
    )
}
