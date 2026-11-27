//! Explicit long ordinary-header search profile.
use super::{companion_writer, sidecar_writer, streaming_facts, BuiltSnapshot, Result};
use crate::git_manifest::ingest;
use rusqlite::Connection;
use std::path::Path;
pub const LEXICAL: &str = "r09-long-headers-v1";
pub const PROFILE: &str = "linux-v5-long-headers1-xml221-r09";
pub(crate) fn schema() -> String {
    super::source_limits_writer::schema().replace(super::source_limits_writer::PROFILE, PROFILE)
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(db, &super::profile_spec::LONG_HEADERS)?;
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
        &super::profile_spec::LONG_HEADERS,
        super::companion_writer::Runtime {
            global_opaque: globals,
            bin_bytes: Some(bin_bytes),
            diagnostics: Some(diagnostics_bytes),
            processor_help: None,
            omit_command_help: super::profile_spec::Feature::Off,
        },
    )
}
