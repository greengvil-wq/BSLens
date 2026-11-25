//! Три вида планов: структура и непрозрачные компаньоны, без семантики учёта.
use super::{companion_writer, service_writer, streaming_facts, BuiltSnapshot, Error, Result};
use crate::{git_manifest::ingest, xml221::Kind};
use rusqlite::{params, Connection};
use std::path::Path;
pub const PROFILE: &str = "linux-v5-chart1-xml221-r09";
pub(crate) fn schema() -> String {
    service_writer::schema()
        .replace(service_writer::PROFILE, PROFILE)
        .replace(
            "'IntegrationServiceChannel'",
            "'IntegrationServiceChannel','AccountingFlag','ExtDimensionAccountingFlag'",
        )
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(
        db, true, true, true, true, true, true, true, false, false, false, false, false, false,
        false, false, false, false, false, false, false, false,
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
    )
}
pub(crate) fn is_kind(k: Kind) -> bool {
    matches!(
        k,
        Kind::ChartOfAccounts | Kind::ChartOfCalculationTypes | Kind::ChartOfCharacteristicTypes
    )
}
pub(crate) fn is_path(path: &str) -> bool {
    matches!(
        path.split('/').next(),
        Some("ChartsOfAccounts" | "ChartsOfCalculationTypes" | "ChartsOfCharacteristicTypes")
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
            && [
                "ManagerModule.bsl",
                "ObjectModule.bsl",
                "Predefined.xml",
                "Help.xml",
            ]
            .contains(&p[3]))
        || (p.len() == 5 && p[2..] == ["Ext", "Help", "ru.html"])
        || (p.len() == 4 && p[2] == "Forms" && p[3].ends_with(".xml"))
        || (p.len() == 6
            && p[2] == "Forms"
            && p[4] == "Ext"
            && ["Form.xml", "Help.xml"].contains(&p[5]))
        || (p.len() == 7
            && p[2] == "Forms"
            && (p[4..] == ["Ext", "Form", "Module.bsl"] || p[4..] == ["Ext", "Help", "ru.html"]))
        || (p.len() == 9
            && p[0] == "ChartsOfAccounts"
            && p[2] == "Forms"
            && p[4..7] == ["Ext", "Form", "Items"]
            && !p[7].is_empty()
            && p[8].strip_suffix(".png").is_some_and(|n| !n.is_empty()));
    if allowed {
        Ok(())
    } else {
        Err(Error::Contract("chart_layout_unsupported"))
    }
}
pub(crate) fn source_kind(path: &str) -> &'static str {
    if !is_path(path) {
        return service_writer::source_kind(path);
    }
    let p: Vec<_> = path.split('/').collect();
    if (p.len() == 2 || (p.len() == 4 && p[2] == "Forms")) && path.ends_with(".xml") {
        "xml"
    } else if path.ends_with(".bsl") {
        "bsl"
    } else {
        "payload"
    }
}
fn exists(db: &Connection, p: &str, kind: &str) -> Result<bool> {
    Ok(db.query_row(
        "SELECT EXISTS(SELECT 1 FROM sources WHERE path=?1 AND kind=?2)",
        params![p, kind],
        |r| r.get(0),
    )?)
}
fn help_pair(db: &Connection, prefix: &str) -> Result<()> {
    if exists(db, &format!("{prefix}/Ext/Help.xml"), "payload")?
        != exists(db, &format!("{prefix}/Ext/Help/ru.html"), "payload")?
    {
        return Err(Error::Contract("chart_help_incomplete"));
    }
    Ok(())
}
pub(crate) fn check_form(
    db: &Connection,
    path: &str,
    kind: Kind,
    modules: usize,
    optional_form: bool,
) -> Result<()> {
    let prefix = path
        .strip_suffix(".xml")
        .ok_or(Error::Contract("xml_path"))?;
    let p: Vec<_> = path.split('/').collect();
    let optional =
        optional_form && p.len() == 4 && p[0] == "ChartsOfCharacteristicTypes" && p[2] == "Forms";
    if kind != Kind::Form
        || if optional { modules > 1 } else { modules != 1 }
        || !exists(db, &format!("{prefix}/Ext/Form.xml"), "payload")?
    {
        return Err(Error::Contract("chart_form_incomplete"));
    }
    help_pair(db, prefix)
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
    for suffix in ["Ext/Predefined.xml", "Ext/Help.xml", "Ext/Help/ru.html"] {
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
