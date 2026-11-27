//! Exact owner bindings for three families of inline commands.
use super::{companion_writer, sidecar_writer, streaming_facts, BuiltSnapshot, Result};
use crate::git_manifest::ingest;
use rusqlite::Connection;
use std::path::Path;
pub const PROFILE: &str = "linux-v5-navigator-owners1-xml221-r09";
pub(crate) fn schema() -> String {
    super::navigator_agreement_writer::schema()
        .replace(super::navigator_agreement_writer::PROFILE, PROFILE)
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    validate_metadata_for(db, false)
}

pub(super) fn validate_metadata_for(db: &Connection, sppr: bool) -> Result<()> {
    companion_writer::validate_metadata_mode(
        db,
        if sppr {
            &super::profile_spec::NAVIGATOR_SPPR
        } else {
            &super::profile_spec::NAVIGATOR_OWNERS
        },
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
        Some(v) => v
            .as_u64()
            .ok_or(super::Error::Contract("source_anomaly_inventory"))?,
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
    build_for(
        repo,
        commit,
        tree,
        source_id,
        output,
        limits,
        git,
        globals,
        bin_bytes,
        diagnostics_bytes,
        None,
    )
}

pub(super) fn build_for(
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
    processor_help: Option<&[String]>,
) -> Result<BuiltSnapshot> {
    companion_writer::build_mode(
        repo,
        commit,
        tree,
        source_id,
        output,
        limits,
        git,
        if processor_help.is_some() {
            &super::profile_spec::NAVIGATOR_SPPR
        } else {
            &super::profile_spec::NAVIGATOR_OWNERS
        },
        super::companion_writer::Runtime {
            global_opaque: globals,
            bin_bytes: Some(bin_bytes),
            diagnostics: Some(diagnostics_bytes),
            processor_help: processor_help,
            omit_command_help: if processor_help.is_some() {
                super::profile_spec::Feature::On
            } else {
                super::profile_spec::Feature::Off
            },
        },
    )
}
