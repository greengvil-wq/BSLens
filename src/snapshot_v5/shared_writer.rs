//! Общие команды/формы/макеты: строгая грамматика путей, непрозрачные payload.
use super::{companion_writer, streaming_facts, BuiltSnapshot, Result};
use crate::git_manifest::ingest;
use std::path::Path;
pub const PROFILE: &str = "linux-v5-shared1-xml221-r09";
pub(crate) fn schema() -> String {
    companion_writer::schema().replace(companion_writer::PROFILE, PROFILE)
}
pub(super) fn validate_metadata(db: &rusqlite::Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(db, &super::profile_spec::SHARED)
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
        &super::profile_spec::SHARED,
        super::companion_writer::Runtime {
            global_opaque: &[],
            bin_bytes: None,
            diagnostics: None,
            processor_help: None,
        },
    )
}

use super::Error;
use crate::xml221::Kind;
use rusqlite::{params, Connection};
pub(crate) fn is_kind(kind: Kind) -> bool {
    matches!(
        kind,
        Kind::CommonCommand | Kind::CommonForm | Kind::CommonTemplate
    )
}
pub(crate) fn is_path(path: &str) -> bool {
    matches!(
        path.split('/').next(),
        Some("CommonCommands" | "CommonForms" | "CommonTemplates")
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
        return Err(Error::Contract("shared_layout_unsupported"));
    }
    let suffix = parts[2..].join("/");
    let allowed = match parts[0] {
        "CommonCommands" => suffix == "Ext/CommandModule.bsl",
        "CommonForms" => [
            "Ext/Form.xml",
            "Ext/Form/Module.bsl",
            "Ext/Help.xml",
            "Ext/Help/ru.html",
        ]
        .contains(&suffix.as_str()),
        "CommonTemplates" => [
            "Ext/Template.xml",
            "Ext/Template.bin",
            "Ext/Template.txt",
            "Ext/Template/ru.html",
        ]
        .contains(&suffix.as_str()),
        _ => false,
    };
    if !allowed {
        return Err(Error::Contract("shared_layout_unsupported"));
    }
    Ok(())
}
pub(crate) fn source_kind(path: &str) -> &'static str {
    if !is_path(path) {
        return companion_writer::source_kind(path);
    }
    let p: Vec<_> = path.split('/').collect();
    if p.len() == 2 && path.ends_with(".xml") {
        "xml"
    } else if (p[0] == "CommonCommands" && p.len() == 4 && p[2..] == ["Ext", "CommandModule.bsl"])
        || (p[0] == "CommonForms" && p.len() == 5 && p[2..] == ["Ext", "Form", "Module.bsl"])
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
    omitted_bin: bool,
    mut charge: F,
) -> Result<()> {
    let prefix = format!(
        "{}/",
        path.strip_suffix(".xml")
            .ok_or(Error::Contract("shared_descriptor_path"))?
    );
    let mut q=db.prepare("SELECT path FROM sources WHERE kind='payload' AND substr(path,1,length(?1))=?1 ORDER BY path COLLATE BINARY")?;
    let mut rows = q.query([&prefix])?;
    let mut primary = 0;
    let mut help = 0;
    let mut template_xml = false;
    let mut template_html = false;
    while let Some(row) = rows.next()? {
        charge()?;
        let p: String = row.get(0)?;
        validate_path(&p)?;
        let suffix = &p[prefix.len()..];
        let role = match (kind, suffix) {
            (Kind::CommonForm, "Ext/Form.xml") => {
                primary += 1;
                "form"
            }
            (Kind::CommonForm, "Ext/Help.xml" | "Ext/Help/ru.html") => {
                help += 1;
                "additional"
            }
            (Kind::CommonTemplate, "Ext/Template/ru.html") => {
                template_html = true;
                "additional"
            }
            (
                Kind::CommonTemplate,
                "Ext/Template.xml" | "Ext/Template.bin" | "Ext/Template.txt",
            ) => {
                primary += 1;
                template_xml = suffix == "Ext/Template.xml";
                "template"
            }
            _ => return Err(Error::Contract("shared_payload_unsupported")),
        };
        if primary > 1 {
            return Err(Error::Contract("shared_payload_ambiguous"));
        }
        db.execute(
            "INSERT INTO companion_payloads(path,object_id,role) VALUES (?1,?2,?3)",
            params![p, id, role],
        )?;
    }
    if kind == Kind::CommonTemplate && omitted_bin {
        primary += 1;
    }
    if kind != Kind::CommonCommand && primary != 1 {
        return Err(Error::Contract("shared_payload_missing"));
    }
    if template_html && !template_xml {
        return Err(Error::Contract("shared_template_html_without_xml"));
    }
    if help == 1 {
        return Err(Error::Contract("shared_help_incomplete"));
    }
    if template_xml {
        let xml_path = format!("{prefix}Ext/Template.xml");
        let header: Vec<u8> = db.query_row(
            "SELECT substr(content,1,65536) FROM sources WHERE path=?1 AND kind='payload'",
            [xml_path],
            |r| r.get(0),
        )?;
        if template_is_help(&header)? != template_html {
            return Err(Error::Contract("shared_template_help_pair"));
        }
    }
    Ok(())
}

// Only the bounded first element is inspected to decide whether the known HTML
// companion is required. No MDClasses/version check or payload-body analysis.
fn template_is_help(header: &[u8]) -> Result<bool> {
    use quick_xml::{events::Event, name::ResolveResult, NsReader};
    let mut reader = NsReader::from_reader(header);
    for _ in 0..128 {
        let (ns, event) = reader
            .read_resolved_event()
            .map_err(|_| Error::Contract("shared_template_envelope_unsupported"))?;
        match event {
            Event::Start(tag) | Event::Empty(tag) => {
                let help = tag.local_name().as_ref() == b"Help";
                if help
                    && !matches!(ns,ResolveResult::Bound(n) if n.as_ref()==b"http://v8.1c.ru/8.3/xcf/extrnprops")
                {
                    return Err(Error::Contract("shared_template_envelope_unsupported"));
                }
                return Ok(help);
            }
            Event::Decl(_) | Event::Comment(_) | Event::PI(_) => {}
            Event::Text(t) if t.as_ref().iter().all(u8::is_ascii_whitespace) => {}
            _ => return Err(Error::Contract("shared_template_envelope_unsupported")),
        }
    }
    Err(Error::Contract("shared_template_envelope_limit"))
}
