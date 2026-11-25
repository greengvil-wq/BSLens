//! Versioned physical conditional arms and declaration evidence; no context evaluation.
use super::{companion_writer, sidecar_writer, streaming_facts, BuiltSnapshot, Result};
use crate::git_manifest::ingest;
use rusqlite::Connection;
use std::path::Path;
pub const LEXICAL: &str = "r09-conditionals-v1";
pub const PROFILE: &str = "linux-v5-conditionals1-xml221-r09";
pub(crate) fn schema() -> String {
    sidecar_writer::schema().replace(sidecar_writer::PROFILE, PROFILE) + "\nCREATE TABLE method_branches (method_id INTEGER PRIMARY KEY REFERENCES methods(method_id), branch_path TEXT NOT NULL CHECK(length(CAST(branch_path AS BLOB)) <= 4096)) STRICT;"
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(
        db, true, true, true, true, true, true, true, true, true, true, true, true, true, true,
        true, true, true, true, true, true, false, false, false, false, false, false, false, false,
        false, false, false, false, false, false, false,
    )?;
    sidecar_writer::validate_summary(db)?;
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
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        false,
    )
}
