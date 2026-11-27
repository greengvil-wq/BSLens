//! Корневые Help.xml и Catalog Predefined.xml как непрозрачные sidecars.
use super::{companion_writer, optional_form_writer, streaming_facts, BuiltSnapshot, Result};
use crate::git_manifest::ingest;
use rusqlite::Connection;
use std::path::Path;
pub const PROFILE: &str = "linux-v5-root-sidecar1-xml221-r09";
pub(crate) fn schema() -> String {
    optional_form_writer::schema().replace(optional_form_writer::PROFILE, PROFILE)
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(db, &super::profile_spec::SIDECAR)?;
    validate_summary(db)
}
pub(super) fn validate_summary(db: &Connection) -> Result<()> {
    let raw: String = db.query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))?;
    let value: serde_json::Value = serde_json::from_str(&raw)?;
    let long: bool = db.query_row(
        "SELECT EXISTS(SELECT 1 FROM companion_payloads WHERE length(CAST(path AS BLOB))>1024)",
        [],
        |r| r.get(0),
    )?;
    if long {
        return Err(Error::Contract("sidecar_path_bytes"));
    }
    let mut q = db.prepare("SELECT path FROM companion_payloads")?;
    let mut rows = q.query([])?;
    let mut count = 0u64;
    while let Some(row) = rows.next()? {
        let p = row
            .get_ref(0)?
            .as_str()
            .map_err(|_| Error::Contract("sidecar_path"))?;
        if is_payload(p) {
            count += 1;
        }
    }
    if value["root_xml_sidecars"] != summary(count) {
        return Err(Error::Contract("root_sidecar_summary"));
    }
    Ok(())
}
pub(crate) fn summary(count: u64) -> serde_json::Value {
    serde_json::json!({"count":count,"contents":"not_analyzed","help_html":"outside_scope_not_read"})
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
        &super::profile_spec::SIDECAR,
        super::companion_writer::Runtime {
            global_opaque: globals,
            bin_bytes: Some(bin_bytes),
            diagnostics: Some(diagnostics_bytes),
            processor_help: None,
            omit_command_help: super::profile_spec::Feature::Off,
        },
    )
}

use super::Error;
use crate::xml221::Kind;
use rusqlite::params;
/// Exact root role, not a suffix or arbitrary Ext component.
pub(crate) fn is_payload(path: &str) -> bool {
    let p: Vec<_> = path.split('/').collect();
    p.len() == 4
        && !p[1].is_empty()
        && p[1] != "."
        && p[1] != ".."
        && p[2] == "Ext"
        && ((["Catalogs", "Documents", "InformationRegisters"].contains(&p[0])
            && p[3] == "Help.xml")
            || (p[0] == "Catalogs" && p[3] == "Predefined.xml"))
}
pub(crate) fn source_kind(path: &str) -> &'static str {
    if is_payload(path) {
        "payload"
    } else {
        super::corpus_writer::source_kind(path)
    }
}
pub(crate) fn record_payloads<F: FnMut() -> Result<()>>(
    db: &Connection,
    path: &str,
    kind: Kind,
    id: i64,
    mut charge: F,
) -> Result<u64> {
    if !matches!(
        kind,
        Kind::Catalog | Kind::Document | Kind::InformationRegister
    ) {
        return Ok(0);
    }
    let prefix = path
        .strip_suffix(".xml")
        .ok_or(Error::Contract("xml_path"))?;
    let mut count = 0;
    for tail in ["Help.xml", "Predefined.xml"] {
        let candidate = format!("{prefix}/Ext/{tail}");
        if !is_payload(&candidate) {
            continue;
        }
        if db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sources WHERE path=?1 AND kind='payload')",
            [&candidate],
            |r| r.get::<_, bool>(0),
        )? {
            charge()?;
            db.execute(
                "INSERT INTO companion_payloads(path,object_id,role) VALUES (?1,?2,'additional')",
                params![candidate, id],
            )?;
            count += 1;
        }
    }
    Ok(count)
}
