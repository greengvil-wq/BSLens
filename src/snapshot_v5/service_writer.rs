//! Структура HTTP/Web/Integration services; семантика обработчиков не анализируется.
use super::{companion_writer, journal_writer, streaming_facts, BuiltSnapshot, Error, Result};
use crate::{git_manifest::ingest, xml221::Kind};
use rusqlite::Connection;
use std::path::Path;
pub const PROFILE: &str = "linux-v5-service1-xml221-r09";
pub(crate) fn schema() -> String {
    journal_writer::schema().replace(journal_writer::PROFILE,PROFILE).replace("'EnumValue','Column'","'EnumValue','Column','URLTemplate','Method','Operation','Parameter','IntegrationServiceChannel'")
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(db, &super::profile_spec::SERVICE)
}
pub fn build(
    repo: &Path,
    commit: &str,
    tree: &str,
    source_id: &str,
    output: &Path,
    limits: streaming_facts::Limits,
    git: ingest::Limits,
) -> Result<BuiltSnapshot> {
    companion_writer::build_mode(
        repo,
        commit,
        tree,
        source_id,
        output,
        limits,
        git,
        &super::profile_spec::SERVICE,
        super::companion_writer::Runtime {
            global_opaque: &[],
            bin_bytes: None,
            diagnostics: None,
            processor_help: None,
        },
    )
}
pub(crate) fn is_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::HTTPService | Kind::WebService | Kind::IntegrationService
    )
}
pub(crate) fn is_path(path: &str) -> bool {
    matches!(
        path.split('/').next(),
        Some("HTTPServices" | "WebServices" | "IntegrationServices")
    )
}
pub(crate) fn validate_path(path: &str) -> Result<()> {
    if !is_path(path) {
        return Ok(());
    }
    let p: Vec<_> = path.split('/').collect();
    if (p.len() == 2 && p[1].ends_with(".xml")) || (p.len() == 4 && p[2..] == ["Ext", "Module.bsl"])
    {
        Ok(())
    } else {
        Err(Error::Contract("service_layout_unsupported"))
    }
}
pub(crate) fn source_kind(path: &str) -> &'static str {
    if !is_path(path) {
        journal_writer::source_kind(path)
    } else if path.ends_with(".xml") {
        "xml"
    } else if path.ends_with(".bsl") {
        "bsl"
    } else {
        "payload"
    }
}
