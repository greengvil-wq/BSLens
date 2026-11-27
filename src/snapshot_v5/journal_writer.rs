//! Enum/DocumentJournal: плоские объявления и точные владельцы команд.
use super::{companion_writer, leaf_writer, streaming_facts, BuiltSnapshot, Error, Result};
use crate::{git_manifest::ingest, xml221::Kind};
use rusqlite::{params, Connection};
use std::path::Path;
pub const PROFILE: &str = "linux-v5-journal1-xml221-r09";
pub(crate) fn schema() -> String {
    format!(
        "{}\n{}",
        companion_writer::schema()
            .replace(companion_writer::PROFILE, PROFILE)
            .replace(
                "'Dimension','Resource'",
                "'Dimension','Resource','EnumValue','Column'"
            ),
        DDL
    )
}
const DDL: &str = r#"
CREATE TABLE inline_module_owners (
 module_path TEXT PRIMARY KEY REFERENCES modules(path) DEFERRABLE INITIALLY DEFERRED,
 declaration_key TEXT NOT NULL UNIQUE,
 owner_id INTEGER NOT NULL,
 FOREIGN KEY(declaration_key,owner_id) REFERENCES inline_declarations(declaration_key,owner_id)
) STRICT;
CREATE INDEX inline_module_declaration ON inline_module_owners(declaration_key,module_path);
"#;
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(db, &super::profile_spec::JOURNAL)
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
        &super::profile_spec::JOURNAL,
        super::companion_writer::Runtime {
            global_opaque: &[],
            bin_bytes: None,
            diagnostics: None,
            processor_help: None,
        },
    )
}
pub(crate) fn is_kind(k: Kind) -> bool {
    matches!(k, Kind::Enum | Kind::DocumentJournal)
}
pub(crate) fn is_path(path: &str) -> bool {
    matches!(path.split('/').next(), Some("Enums" | "DocumentJournals"))
}
pub(crate) fn validate_path(path: &str) -> Result<()> {
    validate_path_mode(path, false)
}

pub(crate) fn validate_path_mode(path: &str, sppr: bool) -> Result<()> {
    if !is_path(path) {
        return Ok(());
    }
    let p: Vec<_> = path.split('/').collect();
    if sppr
        && p[0] == "Enums"
        && matches!(p.as_slice(),
        [_, _, "Forms", name] if name.ends_with(".xml"))
    {
        return Ok(());
    }
    if sppr
        && p[0] == "Enums"
        && (matches!(
            p.as_slice(),
            [_, _, "Forms", _, "Ext", "Form.xml" | "Help.xml"]
        ) || matches!(
            p.as_slice(),
            [_, _, "Forms", _, "Ext", "Form", "Module.bsl"]
                | [_, _, "Forms", _, "Ext", "Help", "ru.html"]
        ))
    {
        return Ok(());
    }
    let allowed = if p.len() == 2 {
        p[1].ends_with(".xml")
    } else if p.len() == 4 && p[2] == "Ext" {
        p[3] == "ManagerModule.bsl" || (p[0] == "DocumentJournals" && p[3] == "Help.xml")
    } else if p[0] == "DocumentJournals" {
        (p.len() == 5 && p[2..] == ["Ext", "Help", "ru.html"])
            || (p.len() == 6 && p[2] == "Commands" && p[4..] == ["Ext", "CommandModule.bsl"])
            || (p.len() == 4 && ["Forms", "Templates"].contains(&p[2]) && p[3].ends_with(".xml"))
            || (p.len() == 6
                && p[2] == "Forms"
                && p[4] == "Ext"
                && ["Form.xml", "Help.xml"].contains(&p[5]))
            || (p.len() == 7
                && p[2] == "Forms"
                && (p[4..] == ["Ext", "Form", "Module.bsl"]
                    || p[4..] == ["Ext", "Help", "ru.html"]))
            || (p.len() == 6 && p[2] == "Templates" && p[4..] == ["Ext", "Template.xml"])
    } else {
        false
    };
    if !allowed {
        return Err(Error::Contract("journal_layout_unsupported"));
    }
    Ok(())
}
pub(crate) fn source_kind(path: &str) -> &'static str {
    if !is_path(path) {
        return leaf_writer::source_kind(path);
    }
    let p: Vec<_> = path.split('/').collect();
    if (p.len() == 2 || (p.len() == 4 && ["Forms", "Templates"].contains(&p[2])))
        && path.ends_with(".xml")
    {
        "xml"
    } else if path.ends_with(".bsl") {
        "bsl"
    } else {
        "payload"
    }
}
fn exists(db: &Connection, path: &str, kind: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sources WHERE path=?1 AND kind=?2)",
        params![path, kind],
        |r| r.get(0),
    )?)
}
fn help_pair(db: &Connection, prefix: &str) -> Result<()> {
    if exists(db, &format!("{prefix}/Ext/Help.xml"), "payload")?
        != exists(db, &format!("{prefix}/Ext/Help/ru.html"), "payload")?
    {
        return Err(Error::Contract("journal_help_incomplete"));
    }
    Ok(())
}
pub(crate) fn check_companion(
    db: &Connection,
    path: &str,
    kind: Kind,
    modules: usize,
) -> Result<()> {
    let prefix = path
        .strip_suffix(".xml")
        .ok_or(Error::Contract("xml_path"))?;
    if kind == Kind::Form {
        if modules != 1 || !exists(db, &format!("{prefix}/Ext/Form.xml"), "payload")? {
            return Err(Error::Contract("journal_form_incomplete"));
        }
        help_pair(db, prefix)?;
    } else if kind == Kind::Template
        && !exists(db, &format!("{prefix}/Ext/Template.xml"), "payload")?
    {
        return Err(Error::Contract("journal_template_missing"));
    }
    Ok(())
}
pub(crate) fn record_payloads<F: FnMut() -> Result<()>>(
    db: &Connection,
    path: &str,
    kind: Kind,
    id: i64,
    mut charge: F,
) -> Result<()> {
    if !is_kind(kind) {
        return Err(Error::Contract("journal_kind"));
    }
    if kind == Kind::Enum {
        return Ok(());
    }
    let prefix = path
        .strip_suffix(".xml")
        .ok_or(Error::Contract("xml_path"))?;
    help_pair(db, prefix)?;
    for suffix in ["Ext/Help.xml", "Ext/Help/ru.html"] {
        let p = format!("{prefix}/{suffix}");
        if exists(db, &p, "payload")? {
            charge()?;
            db.execute(
                "INSERT INTO companion_payloads(path,object_id,role) VALUES (?1,?2,'additional')",
                params![p, id],
            )?;
        }
    }
    Ok(())
}
