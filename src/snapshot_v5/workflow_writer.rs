//! ExchangePlan/BusinessProcess/Task: только структура и непрозрачные компаньоны.
use super::{companion_writer, report_writer, streaming_facts, BuiltSnapshot, Error, Result};
use crate::{git_manifest::ingest, xml221::Kind};
use rusqlite::{params, Connection};
use std::path::Path;
pub const PROFILE: &str = "linux-v5-workflow1-xml221-r09";
pub(crate) fn schema() -> String {
    report_writer::schema()
        .replace(report_writer::PROFILE, PROFILE)
        .replace(
            "'Attribute','Command'",
            "'AddressingAttribute','Attribute','Command'",
        )
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(
        db, true, true, true, true, true, true, true, true, true, true, false, false, false, false,
        false, false, false, false, false, false, false,
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
        true,
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
    matches!(k, Kind::ExchangePlan | Kind::BusinessProcess | Kind::Task)
}
pub(crate) fn is_path(path: &str) -> bool {
    matches!(
        path.split('/').next(),
        Some("ExchangePlans" | "BusinessProcesses" | "Tasks")
    )
}
pub(crate) fn validate_path(path: &str) -> Result<()> {
    if !is_path(path) {
        return Ok(());
    }
    let p: Vec<_> = path.split('/').collect();
    let allowed = (p.len() == 2 && p[1].ends_with(".xml"))
        || (p.len() == 4
            && p[2] == "Ext"
            && (["ManagerModule.bsl", "ObjectModule.bsl", "Help.xml"].contains(&p[3])
                || (p[0] == "ExchangePlans" && p[3] == "Content.xml")
                || (p[0] == "BusinessProcesses" && p[3] == "Flowchart.xml")))
        || (p.len() == 5 && p[2..] == ["Ext", "Help", "ru.html"])
        || (p.len() == 6
            && p[0] == "Tasks"
            && p[2] == "Commands"
            && p[4..] == ["Ext", "CommandModule.bsl"])
        || (p.len() == 4
            && p[3].ends_with(".xml")
            && (p[2] == "Forms" || (p[0] == "ExchangePlans" && p[2] == "Templates")))
        || (p.len() == 6
            && p[2] == "Forms"
            && p[4] == "Ext"
            && ["Form.xml", "Help.xml"].contains(&p[5]))
        || (p.len() == 7
            && p[2] == "Forms"
            && (p[4..] == ["Ext", "Form", "Module.bsl"] || p[4..] == ["Ext", "Help", "ru.html"]))
        || (p.len() == 6
            && p[0] == "ExchangePlans"
            && p[2] == "Templates"
            && p[4..] == ["Ext", "Template.txt"]);
    if allowed {
        Ok(())
    } else {
        Err(Error::Contract("workflow_layout_unsupported"))
    }
}
pub(crate) fn source_kind(path: &str) -> &'static str {
    if !is_path(path) {
        return report_writer::source_kind(path);
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
fn help_pair(db: &Connection, prefix: &str) -> Result<()> {
    if exists(db, &format!("{prefix}/Ext/Help.xml"))?
        != exists(db, &format!("{prefix}/Ext/Help/ru.html"))?
    {
        return Err(Error::Contract("workflow_help_incomplete"));
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
        if modules > 1
            || (!path.starts_with("ExchangePlans/") && modules != 1)
            || !exists(db, &format!("{prefix}/Ext/Form.xml"))?
        {
            return Err(Error::Contract("workflow_form_incomplete"));
        }
        help_pair(db, prefix)?;
    } else if kind == Kind::Template && path.starts_with("ExchangePlans/") {
        if !exists(db, &format!("{prefix}/Ext/Template.txt"))? {
            return Err(Error::Contract("workflow_template_missing"));
        }
    } else {
        return Err(Error::Contract("workflow_companion_kind"));
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
    let required = if path.starts_with("ExchangePlans/") {
        Some("Ext/Content.xml")
    } else if path.starts_with("BusinessProcesses/") {
        Some("Ext/Flowchart.xml")
    } else {
        None
    };
    if let Some(s) = required {
        if !exists(db, &format!("{prefix}/{s}"))? {
            return Err(Error::Contract("workflow_payload_missing"));
        }
    }
    for suffix in ["Ext/Help.xml", "Ext/Help/ru.html"]
        .into_iter()
        .chain(required)
    {
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
