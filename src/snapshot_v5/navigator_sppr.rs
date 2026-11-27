//! Справка выбранных обработок и точные ресурсы рабочих макетов СКД.
use super::{
    navigator_command_help_writer, navigator_owners_writer, streaming_facts, BuiltSnapshot, Error,
    Result,
};
use crate::{git_manifest::ingest, xml221};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::Path};

pub const PROFILE: &str = "linux-v5-navigator-sppr1-xml221-r09";
pub const DEFAULT_BIN_BYTES: u64 = ingest::MAX_TEMPLATE_BIN_BYTES as u64;
pub(crate) const REASON: &str = "SelectedProcessorRootHelpExcludedNotRead";

pub(crate) fn schema() -> String {
    navigator_owners_writer::schema().replace(navigator_owners_writer::PROFILE, PROFILE)
}

pub fn parse_owners(raw: &str) -> Result<Vec<String>> {
    let fail = || Error::Contract("processor_help_owners");
    if raw.len() > 1024 {
        return Err(fail());
    }
    let mut owners: Vec<String> = serde_json::from_str(raw)?;
    if owners.len() > 2 {
        return Err(fail());
    }
    for name in &owners {
        let mut chars = name.chars();
        if name.len() > 240
            || !chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
            || !chars.all(|c| c.is_alphanumeric() || c == '_')
        {
            return Err(fail());
        }
    }
    owners.sort();
    if owners.windows(2).any(|p| p[0] == p[1]) {
        return Err(fail());
    }
    Ok(owners)
}

pub(super) fn excluded(path: &str, owners: &[String]) -> bool {
    let mut p = path.split('/');
    matches!(
        (p.next(), p.next(), p.next(), p.next()),
        (Some("DataProcessors"), Some(owner), Some("Ext"), Some(suffix))
        if owners.iter().any(|name| name == owner)
            && match suffix {
                "Help.xml" => p.next().is_none(),
                "Help" => p.next().is_some_and(|s| !s.is_empty()) && p.all(|s| !s.is_empty()),
                _ => false
            }
    )
}

fn basename(name: &str) -> bool {
    name.strip_suffix(".svg").is_some_and(|stem| {
        !stem.is_empty()
            && stem.len() <= 64
            && stem
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
    })
}

pub(super) fn svg_path(path: &str) -> bool {
    let p: Vec<_> = path.split('/').collect();
    matches!(p.as_slice(), ["Reports", owner, "Templates", template, "Ext", "Template", name]
        if !owner.is_empty() && !template.is_empty() && basename(name))
}

pub(crate) const HELP_REASON: &str = "HelpExcludedNotRead";
pub(crate) const OVERSIZE_REASON: &str = "TemplateExceedsByteLimit";
pub(super) fn filter_companion_path(path: &str) -> bool {
    let p: Vec<_> = path.split('/').collect();
    matches!(p.as_slice(),
        ["FilterCriteria", _, "Forms", f] if f.ends_with(".xml"))
        || matches!(
            p.as_slice(),
            ["FilterCriteria", _, "Forms", _, "Ext", "Form.xml"]
                | ["FilterCriteria", _, "Forms", _, "Ext", "Form", "Module.bsl"]
                | [
                    "FilterCriteria",
                    _,
                    "Commands",
                    _,
                    "Ext",
                    "CommandModule.bsl"
                ]
        )
}
pub(crate) fn help_path(path: &str) -> bool {
    if path.ends_with(".bsl") {
        return false;
    }
    let parts: Vec<_> = path.split('/').collect();
    parts.windows(2).enumerate().any(|(i, p)| {
        p == ["Ext", "Help.xml"] && i + 2 == parts.len()
            || p == ["Ext", "Help"] && i + 2 < parts.len()
    })
}
pub(crate) fn apply_coverage(
    coverage: &mut Value,
    manifest: &Value,
    owners: &[String],
    bin_limit: u64,
) -> Result<()> {
    let fail = || Error::Contract("report_resources_manifest");
    let mut omitted = BTreeSet::new();
    let mut all_omitted = BTreeSet::new();
    let mut oversized = 0u64;
    let mut resources = BTreeSet::new();
    let mut owner_descriptors = BTreeSet::new();
    for row in manifest["excluded"].as_array().ok_or_else(fail)? {
        let path = row["input"]["path"].as_str().ok_or_else(fail)?;
        let reason = row["reason"].as_str().ok_or_else(fail)?;
        let help = matches!(
            reason,
            HELP_REASON | REASON | navigator_command_help_writer::REASON
        );
        let oversize = reason == OVERSIZE_REASON;
        if !(help || oversize || reason == "OutsideSelectionNotRead")
            || help != help_path(path)
            || excluded(path, owners) != (reason == REASON)
            || svg_path(path)
        {
            return Err(fail());
        }
        if help || oversize {
            let size = row["size_bytes"].as_u64().ok_or_else(fail)?;
            if !usable_entry(&row["input"])
                || !all_omitted.insert(path)
                || (oversize && (!ingest::binary_template_path(path) || size <= bin_limit))
            {
                return Err(fail());
            }
        }
        if oversize {
            oversized += 1;
        }
        if reason == REASON && (!usable_entry(&row["input"]) || !omitted.insert(path)) {
            return Err(fail());
        }
    }
    for row in manifest["selected"].as_array().ok_or_else(fail)? {
        let path = row["path"].as_str().ok_or_else(fail)?;
        if help_path(path) || all_omitted.contains(path) {
            return Err(fail());
        }
        if svg_path(path) && (!usable_entry(row) || !resources.insert(path)) {
            return Err(fail());
        }
        if let Some(name) = path
            .strip_prefix("DataProcessors/")
            .and_then(|p| p.strip_suffix(".xml"))
        {
            if owners.iter().any(|o| o == name) {
                owner_descriptors.insert(name);
            }
        }
    }
    for owner in owners {
        if !owner_descriptors.contains(owner.as_str())
            || !omitted.contains(format!("DataProcessors/{owner}/Ext/Help.xml").as_str())
            || !omitted.contains(format!("DataProcessors/{owner}/Ext/Help/ru.html").as_str())
        {
            return Err(fail());
        }
    }
    coverage["source_exclusions"]["processor_root_help"] = json!({
        "status":"excluded_not_read","count":omitted.len(),"reason":REASON
    });
    coverage["source_exclusions"]["help_total"] =
        json!({"count":all_omitted.len() as u64-oversized,"status":"excluded_not_read"});
    coverage["source_exclusions"]["oversize_template"] =
        json!({"count":oversized,"reason":OVERSIZE_REASON,"status":"excluded_not_read"});
    coverage["report_scd_svg"] = json!({"status":"opaque_owned_unparsed","count":resources.len()});
    Ok(())
}

fn usable_entry(entry: &Value) -> bool {
    entry["path"]
        .as_str()
        .is_some_and(|p| xml221::path_check(p).is_ok())
        && matches!(entry["mode"].as_str(), Some("100644" | "100755"))
        && entry["blob_oid"].as_str().is_some_and(|oid| {
            matches!(oid.len(), 40 | 64)
                && oid
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
}

pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    navigator_command_help_writer::validate_metadata(db)?;
    let policy = super::corpus_bin_writer::stored_policy(db)?;
    let owners = policy
        .processor_help
        .ok_or(Error::Contract("report_resources_policy"))?;
    let manifest = navigator_command_help_writer::read_manifest(db)?;
    let mut expected = json!({});
    navigator_command_help_writer::apply_coverage(&mut expected, &manifest)?;
    apply_coverage(&mut expected, &manifest, &owners, policy.bytes)?;
    let raw: String = db.query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))?;
    let actual: Value = serde_json::from_str(&raw)?;
    if actual["source_exclusions"] != expected["source_exclusions"]
        || actual["report_scd_svg"] != expected["report_scd_svg"]
    {
        return Err(Error::Contract("report_resources_coverage"));
    }
    let mut q = db.prepare("SELECT path,kind,mode,blob_oid FROM sources WHERE path GLOB 'DataProcessors/*' OR path GLOB 'Reports/*'")?;
    let mut rows = q.query([])?;
    let mut stored = BTreeSet::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        if excluded(&path, &owners) {
            return Err(Error::Contract("processor_help_present"));
        }
        if svg_path(&path) {
            let kind: String = row.get(1)?;
            if kind != "payload" {
                return Err(Error::Contract("report_svg_kind"));
            }
            stored.insert((path, row.get::<_, String>(2)?, row.get::<_, String>(3)?));
        }
    }
    let declared = manifest["selected"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|r| {
            let path = r["path"].as_str().filter(|p| svg_path(p))?;
            Some((path, r["mode"].as_str()?, r["blob_oid"].as_str()?))
        })
        .collect::<BTreeSet<_>>();
    if !stored
        .iter()
        .map(|(path, mode, oid)| (path.as_str(), mode.as_str(), oid.as_str()))
        .eq(declared.into_iter())
    {
        return Err(Error::Contract("report_svg_inventory"));
    }
    Ok(())
}

fn element(node: roxmltree::Node<'_, '_>, ns: &str, tag: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(ns) && node.tag_name().name() == tag
}
fn exact_text<'a, 'input: 'a>(node: roxmltree::Node<'a, 'input>) -> Option<&'a str> {
    let mut children = node.children();
    let text = children.next()?;
    if text.is_text() && children.next().is_none() {
        text.text()
    } else {
        None
    }
}
pub(super) fn template_kind(bytes: &[u8], descriptor: &str, expected: &str) -> Result<()> {
    const MD: &str = "http://v8.1c.ru/8.3/MDClasses";
    let doc = xml221::parse(bytes, descriptor).map_err(Error::Xml)?;
    let root = doc.root_element();
    let mut values = root
        .children()
        .filter(|n| element(*n, MD, "Template"))
        .flat_map(|n| n.children().filter(|n| element(*n, MD, "Properties")))
        .flat_map(|n| n.children().filter(|n| element(*n, MD, "TemplateType")));
    if !element(root, MD, "MetaDataObject")
        || values.next().and_then(exact_text) != Some(expected)
        || values.next().is_some()
    {
        return Err(Error::Contract("report_svg_not_scd"));
    }
    Ok(())
}

pub(super) fn validate_scd_svg<F: FnMut() -> Result<()>>(
    db: &Connection,
    descriptor: &str,
    reference: &str,
    mut charge: F,
) -> Result<()> {
    let prefix = descriptor
        .strip_suffix(".xml")
        .ok_or(Error::Contract("report_svg_owner"))?;
    let base = format!("{prefix}/Ext/Template/");
    // In byte order, prefix + '0' is the exclusive end for prefix + '/'.
    let upper = format!("{prefix}/Ext/Template0");
    let mut stmt = db.prepare("SELECT path FROM sources WHERE path>=?1 COLLATE BINARY AND path<?2 COLLATE BINARY AND kind='payload' ORDER BY path COLLATE BINARY")?;
    let mut rows = stmt.query(params![base, upper])?;
    let mut resources = BTreeSet::new();
    while let Some(row) = rows.next()? {
        charge()?;
        let path: String = row.get(0)?;
        if svg_path(&path) {
            resources.insert(path[base.len()..].to_string());
        }
    }
    if resources.is_empty() {
        return Ok(());
    }
    let parts: Vec<_> = prefix.split('/').collect();
    if !matches!(parts.as_slice(), ["Reports", owner, "Templates", _] if reference == format!("Reports/{owner}.xml"))
    {
        return Err(Error::Contract("report_svg_owner"));
    }
    let read = |path: &str, kind: &str| -> Result<Vec<u8>> {
        db.query_row(
            "SELECT length(content) FROM sources WHERE path=?1 AND kind=?2",
            params![path, kind],
            |r| r.get::<_, u64>(0),
        )
        .map_err(|_| Error::Contract("report_svg_xml_required"))
        .and_then(|len| {
            if len > xml221::MAX_XML_BYTES as u64 {
                Err(Error::Contract("report_svg_xml_limit"))
            } else {
                Ok(db.query_row(
                    "SELECT content FROM sources WHERE path=?1 AND kind=?2",
                    params![path, kind],
                    |r| r.get(0),
                )?)
            }
        })
    };
    template_kind(
        &read(descriptor, "xml")?,
        descriptor,
        "DataCompositionSchema",
    )?;
    let xml = format!("{prefix}/Ext/Template.xml");
    let content = read(&xml, "payload")?;
    let doc = xml221::parse_sppr_scd(&content, &xml).map_err(Error::Xml)?;
    const S: &str = "http://v8.1c.ru/8.1/data-composition-system/schema";
    const A: &str = "http://v8.1c.ru/8.1/data-composition-system/area-template";
    const C: &str = "http://v8.1c.ru/8.1/data-composition-system/core";
    const R: &str = "http://v8.1c.ru/8.3/xcf/readable";
    let root = doc.root_element();
    if !element(root, S, "DataCompositionSchema") {
        return Err(Error::Contract("report_svg_scd_xml"));
    }
    let mut verified = BTreeSet::new();
    for node in root.descendants().filter(|n| element(*n, R, "Abs")) {
        charge()?;
        let mut parent = node.parent();
        let valid = [
            (C, "value"),
            (C, "item"),
            (A, "appearance"),
            (A, "tableCell"),
            (A, "item"),
            (S, "template"),
            (S, "template"),
            (S, "DataCompositionSchema"),
        ]
        .into_iter()
        .enumerate()
        .all(|(i, (ns, tag))| {
            let Some(n) = parent else {
                return false;
            };
            parent = n.parent();
            element(n, ns, tag) && (i != 7 || n == root)
        });
        if !valid {
            continue;
        }
        if let Some(name) = exact_text(node).filter(|name| basename(name)) {
            verified.insert(name);
        }
    }
    if !resources
        .iter()
        .map(String::as_str)
        .eq(verified.into_iter())
    {
        return Err(Error::Contract("report_svg_reference_inventory"));
    }
    Ok(())
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
    owners: &[String],
) -> Result<BuiltSnapshot> {
    let owners = parse_owners(&serde_json::to_string(owners)?)?;
    navigator_owners_writer::build_with(&navigator_owners_writer::BuildOptions {
        repo,
        commit,
        tree,
        source_id,
        output,
        limits,
        git,
        globals,
        bin_bytes,
        diagnostics_bytes,
        profile: &super::profile_spec::NAVIGATOR_SPPR,
        processor_help_owners: Some(&owners),
    })
}
