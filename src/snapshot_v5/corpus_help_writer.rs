//! Только точный PNG resource корневой справки DataProcessor; старые профили неизменны.
use super::{companion_writer, corpus_bin_writer, streaming_facts, BuiltSnapshot, Error, Result};
use crate::git_manifest::ingest;
use rusqlite::{params, Connection};
use std::path::Path;
pub const PROFILE: &str = "linux-v5-corpus-help1-xml221-r09";
pub(crate) fn schema() -> String {
    corpus_bin_writer::schema().replace(corpus_bin_writer::PROFILE, PROFILE)
}
pub(crate) fn root_help_path(path: &str) -> bool {
    let p: Vec<_> = path.split('/').collect();
    p.len() == 6
        && p[0] == "DataProcessors"
        && !p[1].is_empty()
        && p[2..4] == ["Ext", "Help"]
        && !p[4].is_empty()
        && p[5].strip_suffix(".png").is_some_and(|n| !n.is_empty())
}
pub(crate) fn validate_path(path: &str) -> Result<()> {
    crate::xml221::path_check(path).map_err(Error::Xml)?;
    if root_help_path(path) {
        Ok(())
    } else {
        super::report_writer::validate_path(path)
    }
}
pub(crate) fn record_payloads<F: FnMut() -> Result<()>>(
    db: &Connection,
    path: &str,
    id: i64,
    mut charge: F,
) -> Result<()> {
    let prefix = path
        .strip_suffix(".xml")
        .ok_or(Error::Contract("xml_path"))?;
    let mut q=db.prepare("SELECT path FROM sources WHERE kind='payload' AND substr(path,1,length(?1))=?1 ORDER BY path COLLATE BINARY")?;
    let mut rows = q.query([format!("{prefix}/Ext/Help/")])?;
    while let Some(r) = rows.next()? {
        let p: String = r.get(0)?;
        if !root_help_path(&p) {
            continue;
        }
        let pair: i64 = db.query_row(
            "SELECT count(*) FROM sources WHERE kind='payload' AND path IN (?1,?2)",
            params![
                format!("{prefix}/Ext/Help.xml"),
                format!("{prefix}/Ext/Help/ru.html")
            ],
            |r| r.get(0),
        )?;
        if pair != 2 {
            return Err(Error::Contract("root_help_pair_required"));
        }
        charge()?;
        db.execute(
            "INSERT INTO companion_payloads(path,object_id,role) VALUES (?1,?2,'additional')",
            params![p, id],
        )?;
    }
    Ok(())
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(db, &super::profile_spec::CORPUS_HELP)
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
        &super::profile_spec::CORPUS_HELP,
        super::companion_writer::Runtime {
            global_opaque: globals,
            bin_bytes: Some(bin_bytes),
            diagnostics: None,
            processor_help: None,
        },
    )
}
