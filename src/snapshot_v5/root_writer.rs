//! Явный профиль Descriptor-only roots; старые профили и канонизация фактов неизменны.
use super::{companion_writer, streaming_facts, BuiltSnapshot, Result};
use crate::git_manifest::ingest;
use std::path::Path;
pub const PROFILE: &str = "linux-v5-roots1-xml221-r09";
pub(crate) fn schema() -> String {
    companion_writer::schema().replace(companion_writer::PROFILE, PROFILE)
}
pub(super) fn validate_metadata(db: &rusqlite::Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(
        db, true, true, false, false, false, false, false, false, false, false, false, false,
        false, false, false, false, false, false, false, false, false, false, false, false, false,
        false, false, false, false, false, false, false, false, false, false, false,
    )
}
pub fn build(
    repo: &Path,
    commit: &str,
    expected_tree: &str,
    source_id: &str,
    output: &Path,
    limits: streaming_facts::Limits,
    git: ingest::Limits,
) -> Result<BuiltSnapshot> {
    companion_writer::build_mode(
        repo,
        commit,
        expected_tree,
        source_id,
        output,
        limits,
        git,
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
        &[],
        None,
        false,
        false,
        None,
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
        false,
        false,
        false,
        false,
        false,
        false,
        false,
        None,
    )
}
