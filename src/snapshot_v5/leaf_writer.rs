//! Пять корневых видов без вложений. Все payload сохраняются непрозрачно.
use super::{companion_writer, shared_writer, streaming_facts, BuiltSnapshot, Error, Result};
use crate::{git_manifest::ingest, xml221::Kind};
use rusqlite::{params, Connection};
use std::path::Path;
pub const PROFILE: &str = "linux-v5-leaf1-xml221-r09";
pub(crate) fn schema() -> String {
    companion_writer::schema().replace(companion_writer::PROFILE, PROFILE)
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(
        db, true, true, true, true, false, false, false, false, false, false, false, false, false,
        false, false, false, false, false, false, false, false, false, false, false, false, false,
        false, false, false, false, false, false, false, false, false,
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
        true,
        true,
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
    )
}
pub(crate) fn is_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::CommonPicture | Kind::Constant | Kind::Role | Kind::ScheduledJob | Kind::XDTOPackage
    )
}
pub(crate) fn is_path(path: &str) -> bool {
    matches!(
        path.split('/').next(),
        Some("CommonPictures" | "Constants" | "Roles" | "ScheduledJobs" | "XDTOPackages")
    )
}
pub(crate) fn validate_path(path: &str) -> Result<()> {
    if !is_path(path) {
        return Ok(());
    }
    let parts: Vec<_> = path.split('/').collect();
    if parts.len() == 2 && parts[1].ends_with(".xml") {
        return Ok(());
    }
    if parts.len() < 3 {
        return Err(Error::Contract("leaf_layout_unsupported"));
    }
    let suffix = parts[2..].join("/");
    let allowed = match parts[0] {
        "CommonPictures" => [
            "Ext/Picture.xml",
            "Ext/Picture/Picture.zip",
            "Ext/Picture/Picture.png",
            "Ext/Picture/Picture.svg",
            "Ext/Picture/Picture.gif",
            "Ext/Picture/Picture.ico",
        ]
        .contains(&suffix.as_str()),
        "Constants" => {
            ["Ext/ManagerModule.bsl", "Ext/ValueManagerModule.bsl"].contains(&suffix.as_str())
        }
        "Roles" => suffix == "Ext/Rights.xml",
        "ScheduledJobs" => suffix == "Ext/Schedule.xml",
        "XDTOPackages" => suffix == "Ext/Package.bin",
        _ => false,
    };
    if !allowed {
        return Err(Error::Contract("leaf_layout_unsupported"));
    }
    Ok(())
}
pub(crate) fn source_kind(path: &str) -> &'static str {
    if !is_path(path) {
        return shared_writer::source_kind(path);
    }
    let p: Vec<_> = path.split('/').collect();
    if p.len() == 2 && path.ends_with(".xml") {
        "xml"
    } else if p.len() == 4
        && p[0] == "Constants"
        && p[2] == "Ext"
        && ["ManagerModule.bsl", "ValueManagerModule.bsl"].contains(&p[3])
    {
        "bsl"
    } else {
        "payload"
    }
}
pub(crate) fn record_payloads<F: FnMut() -> Result<()>>(
    db: &Connection,
    path: &str,
    kind: Kind,
    id: i64,
    mut charge: F,
) -> Result<()> {
    let prefix = format!(
        "{}/",
        path.strip_suffix(".xml")
            .ok_or(Error::Contract("leaf_descriptor_path"))?
    );
    let mut q=db.prepare("SELECT path FROM sources WHERE kind='payload' AND substr(path,1,length(?1))=?1 ORDER BY path COLLATE BINARY")?;
    let mut rows = q.query([&prefix])?;
    let (mut envelope, mut body) = (0, 0);
    while let Some(row) = rows.next()? {
        charge()?;
        let p: String = row.get(0)?;
        validate_path(&p)?;
        let suffix = &p[prefix.len()..];
        match (kind, suffix) {
            (Kind::CommonPicture, "Ext/Picture.xml") => envelope += 1,
            (
                Kind::CommonPicture,
                "Ext/Picture/Picture.zip"
                | "Ext/Picture/Picture.png"
                | "Ext/Picture/Picture.svg"
                | "Ext/Picture/Picture.gif"
                | "Ext/Picture/Picture.ico",
            ) => body += 1,
            (Kind::Role, "Ext/Rights.xml")
            | (Kind::ScheduledJob, "Ext/Schedule.xml")
            | (Kind::XDTOPackage, "Ext/Package.bin") => body += 1,
            _ => return Err(Error::Contract("leaf_payload_unsupported")),
        }
        if body > 1 {
            return Err(Error::Contract("leaf_payload_ambiguous"));
        }
        db.execute(
            "INSERT INTO companion_payloads(path,object_id,role) VALUES (?1,?2,'additional')",
            params![p, id],
        )?;
    }
    match kind {
        Kind::CommonPicture if envelope != 1 || body != 1 => {
            Err(Error::Contract("leaf_payload_missing"))
        }
        Kind::Role | Kind::XDTOPackage if body != 1 => Err(Error::Contract("leaf_payload_missing")),
        Kind::Constant
        | Kind::ScheduledJob
        | Kind::CommonPicture
        | Kind::Role
        | Kind::XDTOPackage => Ok(()),
        _ => Err(Error::Contract("leaf_kind")),
    }
}
