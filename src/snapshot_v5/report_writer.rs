//! DataProcessor/Report: структура, команды и непрозрачные ресурсы.
use super::{companion_writer, register_writer, streaming_facts, BuiltSnapshot, Error, Result};
use crate::{git_manifest::ingest, xml221::Kind};
use rusqlite::{params, Connection};
use std::path::Path;
pub const PROFILE: &str = "linux-v5-report1-xml221-r09";
pub(crate) fn schema() -> String {
    register_writer::schema().replace(register_writer::PROFILE, PROFILE)
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(
        db, true, true, true, true, true, true, true, true, true, false, false, false, false,
        false, false, false, false, false, false, false, false, false, false, false, false, false,
        false, false, false, false, false, false, false, false, false,
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
    )
}
pub(crate) fn is_kind(k: Kind) -> bool {
    matches!(k, Kind::DataProcessor | Kind::Report)
}
pub(crate) fn is_path(path: &str) -> bool {
    matches!(path.split('/').next(), Some("DataProcessors" | "Reports"))
}
pub(crate) fn validate_path(path: &str) -> Result<()> {
    if !is_path(path) {
        return Ok(());
    }
    let p: Vec<_> = path.split('/').collect();
    let allowed = (p.len() == 2 && p[1].ends_with(".xml"))
        || (p.len() == 4
            && p[2] == "Ext"
            && ["ManagerModule.bsl", "ObjectModule.bsl", "Help.xml"].contains(&p[3]))
        || (p.len() == 5 && p[2..] == ["Ext", "Help", "ru.html"])
        || (p.len() == 6 && p[2] == "Commands" && p[4..] == ["Ext", "CommandModule.bsl"])
        || (p.len() == 4 && ["Forms", "Templates"].contains(&p[2]) && p[3].ends_with(".xml"))
        || (p.len() == 6
            && p[2] == "Forms"
            && p[4] == "Ext"
            && ["Form.xml", "Help.xml"].contains(&p[5]))
        || (p.len() == 7
            && p[2] == "Forms"
            && (p[4..] == ["Ext", "Form", "Module.bsl"] || p[4..] == ["Ext", "Help", "ru.html"]))
        || (p.len() == 6
            && p[2] == "Templates"
            && p[4] == "Ext"
            && ["Template.xml", "Template.bin", "Template.txt"].contains(&p[5]))
        || (p[0] == "DataProcessors"
            && p.len() == 7
            && p[2] == "Templates"
            && p[4..] == ["Ext", "Template", "ru.html"])
        || (p[0] == "DataProcessors"
            && p.len() == 9
            && p[2] == "Forms"
            && p[4..7] == ["Ext", "Form", "Items"]
            && !p[7].is_empty()
            && ["zip", "png", "gif", "svg"].iter().any(|e| {
                p[8].strip_suffix(&format!(".{e}"))
                    .is_some_and(|n| !n.is_empty())
            }))
        || (p[0] == "DataProcessors"
            && p.len() == 8
            && p[2] == "Forms"
            && p[4..6] == ["Ext", "Help"]
            && !p[6].is_empty()
            && p[7].strip_suffix(".png").is_some_and(|n| !n.is_empty()));
    if allowed {
        Ok(())
    } else {
        Err(Error::Contract("report_layout_unsupported"))
    }
}
pub(crate) fn source_kind(path: &str) -> &'static str {
    if !is_path(path) {
        return register_writer::source_kind(path);
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
fn exists(db: &Connection, path: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sources WHERE path=?1 AND kind='payload')",
        [path],
        |r| r.get(0),
    )?)
}
fn help_pair(db: &Connection, prefix: &str) -> Result<bool> {
    let xml = exists(db, &format!("{prefix}/Ext/Help.xml"))?;
    if xml != exists(db, &format!("{prefix}/Ext/Help/ru.html"))? {
        return Err(Error::Contract("report_help_incomplete"));
    }
    Ok(xml)
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
        if modules > 1 || !exists(db, &format!("{prefix}/Ext/Form.xml"))? {
            return Err(Error::Contract("report_form_incomplete"));
        }
        let help = help_pair(db, prefix)?;
        let assets:bool=db.query_row("SELECT EXISTS(SELECT 1 FROM sources WHERE kind='payload' AND substr(path,1,length(?1))=?1 AND path<>?2)",params![format!("{prefix}/Ext/Help/"),format!("{prefix}/Ext/Help/ru.html")],|r|r.get(0))?;
        if assets && !help {
            return Err(Error::Contract("report_help_asset_orphan"));
        }
    } else if kind == Kind::Template {
        let mut count = 0;
        for ext in ["xml", "bin", "txt"] {
            if exists(db, &format!("{prefix}/Ext/Template.{ext}"))? {
                count += 1;
            }
        }
        if count != 1 {
            return Err(Error::Contract("report_template_payload_count"));
        }
        if exists(db, &format!("{prefix}/Ext/Template/ru.html"))?
            && !exists(db, &format!("{prefix}/Ext/Template.xml"))?
        {
            return Err(Error::Contract("report_template_help_orphan"));
        }
    } else {
        return Err(Error::Contract("report_companion_kind"));
    }
    Ok(())
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
    help_pair(db, prefix)?;
    for suffix in ["Ext/Help.xml", "Ext/Help/ru.html"] {
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
