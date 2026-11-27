//! Single-line async declaration and closed case-insensitive condition grammar.
use super::{companion_writer, sidecar_writer, streaming_facts, BuiltSnapshot, Result};
use crate::git_manifest::ingest;
use rusqlite::Connection;
use std::path::Path;
pub const LEXICAL: &str = "r09-async-casing-v1";
pub const PROFILE: &str = "linux-v5-async-casing1-xml221-r09";
pub(crate) fn schema() -> String {
    super::dates_members_writer::schema().replace(super::dates_members_writer::PROFILE, PROFILE)
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(db, &super::profile_spec::ASYNC_CASING)?;
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
        &super::profile_spec::ASYNC_CASING,
        super::companion_writer::Runtime {
            global_opaque: globals,
            bin_bytes: Some(bin_bytes),
            diagnostics: Some(diagnostics_bytes),
            processor_help: None,
        },
    )
}
