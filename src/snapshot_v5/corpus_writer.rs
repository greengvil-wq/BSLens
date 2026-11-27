//! Интеграционный профиль: все известные корни, ограниченные подсистемы и явный opaque scope.
use super::{companion_writer, streaming_facts, workflow_writer, BuiltSnapshot, Error, Result};
use crate::{git_manifest::ingest, xml221::Kind};
use rusqlite::{params, Connection};
use std::path::Path;
pub const PROFILE: &str = "linux-v5-corpus1-xml221-r09";
pub(crate) fn schema() -> String {
    workflow_writer::schema()
        .replace(workflow_writer::PROFILE, PROFILE)
        .replace(
            "object_id INTEGER NOT NULL REFERENCES companions(object_id)",
            "object_id INTEGER NOT NULL REFERENCES objects(object_id)",
        )
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(db, &super::profile_spec::CORPUS)
}
pub fn build(
    repo: &Path,
    commit: &str,
    tree: &str,
    source_id: &str,
    output: &Path,
    limits: streaming_facts::Limits,
    git: ingest::Limits,
    global_opaque: &[String],
) -> Result<BuiltSnapshot> {
    companion_writer::build_mode(
        repo,
        commit,
        tree,
        source_id,
        output,
        limits,
        git,
        &super::profile_spec::CORPUS,
        super::companion_writer::Runtime {
            global_opaque: global_opaque,
            bin_bytes: None,
            diagnostics: None,
            processor_help: None,
        },
    )
}
pub const SUBSYSTEM_LEVELS: usize = 3;
pub(crate) fn is_kind(k: Kind) -> bool {
    matches!(k, Kind::Sequence | Kind::SettingsStorage | Kind::Subsystem)
}
pub(crate) fn is_path(path: &str) -> bool {
    matches!(
        path.split('/').next(),
        Some("Sequences" | "SettingsStorages" | "Subsystems")
    )
}
pub(crate) fn global_location(path: &str) -> bool {
    !path.contains('/') || path.starts_with("Ext/")
}
pub(crate) fn global_opaque(path: &str) -> bool {
    global_location(path) && [".png", ".bin", ".zip"].iter().any(|e| path.ends_with(e))
}
pub(crate) fn global_payload(path: &str) -> bool {
    (global_location(path) && path.ends_with(".xml") && path != "Configuration.xml")
        || global_opaque(path)
}
/// Явный bounded scope; порядок нормализуется до включения в identity.
pub fn parse_global_opaque(raw: &str) -> Result<Vec<String>> {
    if raw.len() > 65536 {
        return Err(Error::Contract("global_scope_bytes"));
    }
    let mut paths: Vec<String> = serde_json::from_str(raw)?;
    if paths.len() > 64 {
        return Err(Error::Contract("global_scope_count"));
    }
    for p in &paths {
        crate::xml221::path_check(p).map_err(Error::Xml)?;
        if p.len() > 1024 || !global_opaque(p) {
            return Err(Error::Contract("global_scope_path"));
        }
    }
    paths.sort();
    if paths.windows(2).any(|w| w[0] == w[1]) {
        return Err(Error::Contract("global_scope_duplicate"));
    }
    Ok(paths)
}
pub(crate) fn validate_global_sources(db: &Connection, paths: &[String]) -> Result<()> {
    let mut q =
        db.prepare("SELECT path FROM sources WHERE kind='payload' ORDER BY path COLLATE BINARY")?;
    let mut rows = q.query([])?;
    let mut observed = Vec::new();
    while let Some(r) = rows.next()? {
        let path: String = r.get(0)?;
        if global_opaque(&path) {
            if observed.len() >= 64 {
                return Err(Error::Contract("global_scope_count"));
            }
            observed.push(path);
        }
    }
    if observed != paths {
        return Err(Error::Contract("global_scope_sources"));
    }
    Ok(())
}
pub(crate) fn selected_extra(path: &str) -> bool {
    is_path(path) || (global_payload(path) && !global_opaque(path))
}
pub(crate) fn validate_path(path: &str) -> Result<()> {
    if !is_path(path) {
        return Ok(());
    }
    let p: Vec<_> = path.split('/').collect();
    let allowed = if p[0] == "Sequences" {
        p.len() == 2 && p[1].ends_with(".xml")
    } else if p[0] == "SettingsStorages" {
        (p.len() == 2 && p[1].ends_with(".xml"))
            || (p.len() == 4 && p[2..] == ["Ext", "ManagerModule.bsl"])
            || (p.len() == 4 && p[2] == "Forms" && p[3].ends_with(".xml"))
            || (p.len() == 6
                && p[2] == "Forms"
                && p[4] == "Ext"
                && ["Form.xml", "Help.xml"].contains(&p[5]))
            || (p.len() == 7
                && p[2] == "Forms"
                && (p[4..] == ["Ext", "Form", "Module.bsl"]
                    || p[4..] == ["Ext", "Help", "ru.html"]))
    } else {
        let mut i = 0;
        let mut levels = 0;
        let mut valid = false;
        while i + 1 < p.len() && p[i] == "Subsystems" {
            levels += 1;
            if levels > SUBSYSTEM_LEVELS {
                return Err(Error::Contract("subsystem_depth"));
            }
            if i + 2 == p.len() {
                valid = p[i + 1].ends_with(".xml");
                break;
            }
            if p[i + 1].ends_with(".xml") {
                break;
            }
            i += 2;
            if p[i] == "Ext" {
                valid = p[i..] == ["Ext", "CommandInterface.xml"]
                    || p[i..] == ["Ext", "Help.xml"]
                    || p[i..] == ["Ext", "Help", "ru.html"];
                break;
            }
        }
        valid
    };
    if allowed {
        Ok(())
    } else {
        Err(Error::Contract("corpus_layout_unsupported"))
    }
}
pub(crate) fn source_kind(path: &str) -> &'static str {
    if global_payload(path) {
        return "payload";
    }
    if !is_path(path) {
        return workflow_writer::source_kind(path);
    }
    let p: Vec<_> = path.split('/').collect();
    if path.ends_with(".bsl") {
        "bsl"
    } else if path.ends_with(".xml")
        && match p[0] {
            "Sequences" => p.len() == 2,
            "SettingsStorages" => p.len() == 2 || (p.len() == 4 && p[2] == "Forms"),
            "Subsystems" => {
                p.len() % 2 == 0 && p.chunks_exact(2).all(|pair| pair[0] == "Subsystems")
            }
            _ => false,
        }
    {
        "xml"
    } else {
        "payload"
    }
}
fn exists(db: &Connection, path: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sources WHERE path=?1 AND kind='payload')",
        [path],
        |r| r.get(0),
    )?)
}
fn help_pair(db: &Connection, prefix: &str) -> Result<()> {
    if exists(db, &format!("{prefix}/Ext/Help.xml"))?
        != exists(db, &format!("{prefix}/Ext/Help/ru.html"))?
    {
        return Err(Error::Contract("corpus_help_incomplete"));
    }
    Ok(())
}
pub(crate) fn check_companion(
    db: &Connection,
    path: &str,
    kind: Kind,
    modules: usize,
) -> Result<()> {
    validate_path(path)?;
    let prefix = path
        .strip_suffix(".xml")
        .ok_or(Error::Contract("xml_path"))?;
    if kind == Kind::Subsystem && path.starts_with("Subsystems/") {
        if modules != 0 {
            return Err(Error::Contract("subsystem_module"));
        }
        help_pair(db, prefix)?;
    } else if kind == Kind::Form && path.starts_with("SettingsStorages/") {
        if modules != 1 || !exists(db, &format!("{prefix}/Ext/Form.xml"))? {
            return Err(Error::Contract("settings_form_incomplete"));
        }
        help_pair(db, prefix)?;
    } else {
        return Err(Error::Contract("corpus_companion_kind"));
    }
    Ok(())
}
pub(crate) fn record_payloads<F: FnMut() -> Result<()>>(
    db: &Connection,
    path: &str,
    id: i64,
    mut charge: F,
) -> Result<()> {
    if !path.starts_with("Subsystems/") {
        return Ok(());
    }
    let prefix = path
        .strip_suffix(".xml")
        .ok_or(Error::Contract("xml_path"))?;
    help_pair(db, prefix)?;
    for suffix in [
        "Ext/CommandInterface.xml",
        "Ext/Help.xml",
        "Ext/Help/ru.html",
    ] {
        let p = format!("{prefix}/{suffix}");
        if exists(db, &p)? {
            charge()?;
            db.execute(
                "INSERT INTO companion_payloads(path,object_id,role) VALUES (?1,?2,'additional')",
                params![p, id],
            )?;
        }
    }
    Ok(())
}
pub(crate) fn record_globals<F: FnMut() -> Result<()>>(
    db: &Connection,
    mut charge: F,
) -> Result<()> {
    let mut q =
        db.prepare("SELECT path FROM sources WHERE kind='payload' ORDER BY path COLLATE BINARY")?;
    let mut rows = q.query([])?;
    while let Some(r) = rows.next()? {
        let path: String = r.get(0)?;
        if global_payload(&path) {
            charge()?;
            db.execute(
                "INSERT INTO companion_payloads(path,object_id,role) VALUES (?1,1,'additional')",
                [path],
            )?;
        }
    }
    Ok(())
}
