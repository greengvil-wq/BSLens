//! Corpus-only opt-in: отдельный per-file BIN budget, старые профили неизменны.
use super::{companion_writer, corpus_writer, streaming_facts, BuiltSnapshot, Error, Result};
use crate::git_manifest::ingest;
use rusqlite::Connection;
use std::path::Path;
pub const PROFILE: &str = "linux-v5-corpus-bin1-xml221-r09";
pub const MAX_BIN_BYTES: u64 = 24 * 1024 * 1024;
#[derive(Clone)]
pub(crate) struct Policy {
    pub bytes: u64,
    pub root_help: bool,
    pub configuration: bool,
    pub diagnostics: Option<u64>,
    pub optional_form: bool,
    pub root_sidecars: bool,
    pub context: bool,
    pub headers: bool,
    pub conditional: bool,
    pub strings: bool,
    pub literals: bool,
    pub wrapped: bool,
    pub refined: bool,
    pub annotated_vars: bool,
    pub dates_members: bool,
    pub async_casing: bool,
    pub mobile_vars: bool,
    pub method_regions: bool,
    pub header_forms: bool,
    pub source_limits: bool,
    pub long_headers: bool,
    pub lexical_tail: bool,
    pub source_anomalies: bool,
    pub command_owners: bool,
    pub globals: Vec<String>,
}
impl Policy {
    pub fn new(bytes: u64, globals: &[String]) -> Result<Self> {
        if bytes == 0 || bytes > MAX_BIN_BYTES {
            return Err(Error::Contract("opaque_bin_bytes"));
        }
        Ok(Self {
            bytes,
            root_help: false,
            configuration: false,
            diagnostics: None,
            optional_form: false,
            root_sidecars: false,
            context: false,
            headers: false,
            conditional: false,
            strings: false,
            literals: false,
            wrapped: false,
            refined: false,
            annotated_vars: false,
            dates_members: false,
            async_casing: false,
            mobile_vars: false,
            method_regions: false,
            header_forms: false,
            source_limits: false,
            long_headers: false,
            lexical_tail: false,
            source_anomalies: false,
            command_owners: false,
            globals: corpus_writer::parse_global_opaque(&serde_json::to_string(globals)?)?,
        })
    }
    pub fn eligible(&self, path: &str) -> bool {
        let p: Vec<_> = path.split('/').collect();
        (p.len() == 4
            && ((p[0] == "CommonTemplates" && p[2..] == ["Ext", "Template.bin"])
                || (p[0] == "XDTOPackages" && p[2..] == ["Ext", "Package.bin"])))
            || (p.len() == 6
                && [
                    "Catalogs",
                    "Documents",
                    "InformationRegisters",
                    "DataProcessors",
                    "Reports",
                ]
                .contains(&p[0])
                && p[2] == "Templates"
                && p[4..] == ["Ext", "Template.bin"])
            || (path.ends_with(".bin")
                && corpus_writer::global_opaque(path)
                && self.globals.iter().any(|p| p == path))
    }
    pub fn cap(&self, path: &str, kind: &str, ordinary: u64) -> u64 {
        if kind == "payload" && self.eligible(path) {
            self.bytes
        } else {
            ordinary
        }
    }
}
pub(crate) fn schema() -> String {
    corpus_writer::schema().replace(corpus_writer::PROFILE, PROFILE)
}
pub(crate) fn stored_policy(db: &Connection) -> Result<Policy> {
    let raw: String = db.query_row("SELECT build_parameters FROM metadata", [], |r| r.get(0))?;
    let p: serde_json::Value = serde_json::from_str(&raw)?;
    let g = corpus_writer::parse_global_opaque(&p["global_scope"]["opaque"].to_string())?;
    let mut policy = Policy::new(
        p["opaque_bin"]["bytes"]
            .as_u64()
            .ok_or(Error::Contract("opaque_bin_bytes"))?,
        &g,
    )?;
    let profile: String = db.query_row("SELECT profile FROM metadata", [], |r| r.get(0))?;
    policy.command_owners = profile == super::navigator_owners_writer::PROFILE;
    let agreement = policy.command_owners || profile == super::navigator_agreement_writer::PROFILE;
    policy.source_anomalies = agreement || profile == super::source_anomalies_writer::PROFILE;
    policy.lexical_tail = agreement || profile == super::lexical_tail_writer::PROFILE;
    policy.long_headers = profile == super::long_headers_writer::PROFILE || policy.lexical_tail;
    policy.source_limits = profile == super::source_limits_writer::PROFILE
        || policy.long_headers
        || policy.source_anomalies;
    policy.header_forms = profile == super::header_forms_writer::PROFILE || policy.source_limits;
    policy.method_regions = profile == super::method_regions_writer::PROFILE || policy.header_forms;
    policy.mobile_vars = profile == super::mobile_vars_writer::PROFILE || policy.method_regions;
    policy.async_casing = profile == super::async_casing_writer::PROFILE || policy.mobile_vars;
    policy.dates_members = profile == super::dates_members_writer::PROFILE || policy.async_casing;
    policy.annotated_vars =
        profile == super::annotated_vars_writer::PROFILE || policy.dates_members;
    policy.refined = profile == super::refined_writer::PROFILE || policy.annotated_vars;
    policy.wrapped = profile == super::wrapped_writer::PROFILE || policy.refined;
    policy.literals = profile == super::literal_writer::PROFILE || policy.wrapped;
    policy.strings = profile == super::string_writer::PROFILE || policy.literals;
    policy.conditional = profile == super::conditional_writer::PROFILE || policy.strings;
    policy.headers = profile == super::parameter_writer::PROFILE || policy.conditional;
    policy.context = profile == super::context_writer::PROFILE || policy.headers;
    policy.root_sidecars = profile == super::sidecar_writer::PROFILE || policy.context;
    policy.optional_form = profile == super::optional_form_writer::PROFILE || policy.root_sidecars;
    policy.diagnostics = if profile == super::diagnostics_writer::PROFILE || policy.optional_form {
        Some(
            p["diagnostics"]["bytes"]
                .as_u64()
                .ok_or(Error::Contract("diagnostics_parameters"))?,
        )
    } else {
        None
    };
    policy.configuration =
        profile == super::corpus_config_writer::PROFILE || policy.diagnostics.is_some();
    policy.root_help = profile == super::corpus_help_writer::PROFILE || policy.configuration;
    Ok(policy)
}
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    companion_writer::validate_metadata_mode(
        db, true, true, true, true, true, true, true, true, true, true, true, true, false, false,
        false, false, false, false, false, false, false, false, false, false, false, false, false,
        false, false, false, false, false, false, false, false,
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
        true,
        globals,
        Some(bin_bytes),
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
pub(crate) fn check_sources(db: &Connection, ordinary: u64, policy: Option<&Policy>) -> Result<()> {
    if policy.is_none() {
        let largest: u64 = db.query_row(
            "SELECT coalesce(max(length(content)),0) FROM sources",
            [],
            |r| r.get(0),
        )?;
        return super::Limits::check("source_bytes", ordinary, largest);
    }
    let mut q=db.prepare("SELECT path,kind,length(content),length(CAST(path AS BLOB)),length(CAST(kind AS BLOB)) FROM sources ORDER BY path COLLATE BINARY")?;
    let mut rows = q.query([])?;
    while let Some(r) = rows.next()? {
        if r.get::<_, u64>(3)? > 1024 || r.get::<_, u64>(4)? > 7 {
            return Err(Error::Contract("source_path"));
        }
        let p: String = r.get(0)?;
        let k: String = r.get(1)?;
        if p.ends_with(".bin") && !policy.unwrap().eligible(&p) {
            return Err(Error::Contract("opaque_bin_layout_unsupported"));
        }
        let n: u64 = r.get(2)?;
        super::Limits::check(
            "source_bytes",
            policy.map_or(ordinary, |b| b.cap(&p, &k, ordinary)),
            n,
        )?;
    }
    Ok(())
}
