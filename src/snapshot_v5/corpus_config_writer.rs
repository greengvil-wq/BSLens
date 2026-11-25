//! Новый профиль: DOM 16384 только для корневого Configuration.xml.
use super::{companion_writer, corpus_help_writer, streaming_facts, BuiltSnapshot, Result};
use crate::git_manifest::ingest;
use rusqlite::Connection;
use std::path::Path;
pub const PROFILE: &str = "linux-v5-corpus-config1-xml221-r09";
pub(crate) fn schema() -> String {
    corpus_help_writer::schema().replace(corpus_help_writer::PROFILE, PROFILE)
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(
        db, true, true, true, true, true, true, true, true, true, true, true, true, true, true,
        false, false, false, false, false, false, false, false, false, false, false, false, false,
        false, false, false, false, false, false, false, false,
    )
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
    )
}
