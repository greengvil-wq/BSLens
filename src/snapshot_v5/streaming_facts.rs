//! Bounded calculation layer over saved sources, not a snapshot profile or CLI.
use super::{insert_bsl_mode, Error, Limits as SourceLimits, Result};
use crate::xml221::{self, Kind};
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

pub const ALGORITHM: &str = "sqlite-stream-facts-r09-v1";
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub sources: SourceLimits,
    /// Objects + modules + physical lines + methods + coverage entries.
    pub facts: u64,
    pub coverage_bytes: u64,
    /// Cooperative deadline, checked between bounded files and SQL operations.
    pub time: Duration,
}
impl Limits {
    pub(crate) fn validate(self) -> Result<()> {
        for (name, value, cap) in [
            ("source_count", self.sources.source_count, 100_000),
            ("source_bytes", self.sources.source_bytes, 16 * 1024 * 1024),
            (
                "total_source_bytes",
                self.sources.total_source_bytes,
                512 * 1024 * 1024,
            ),
            (
                "sqlite_bytes",
                self.sources.sqlite_bytes,
                1024 * 1024 * 1024,
            ),
            ("facts", self.facts, 2_000_000),
            ("coverage_bytes", self.coverage_bytes, 1024 * 1024),
        ] {
            if value == 0 {
                return Err(Error::Contract("stream_limits"));
            }
            SourceLimits::check(name, cap, value)?;
        }
        if self.time.is_zero() || self.time > Duration::from_secs(300) {
            return Err(Error::Contract("stream_deadline"));
        }
        Ok(())
    }
}
struct Budget {
    limit: Limits,
    deadline: Instant,
    facts: u64,
    coverage: u64,
    root_sidecars: u64,
    inline: bool,
    companions: bool,
    common: bool,
    roots: bool,
    shared: bool,
    leaves: bool,
    journals: bool,
    services: bool,
    charts: bool,
    registers: bool,
    reports: bool,
    workflows: bool,
    corpus: bool,
    bin_policy: Option<super::corpus_bin_writer::Policy>,
}
impl Budget {
    fn time(&self) -> Result<()> {
        if Instant::now() >= self.deadline {
            Err(Error::Contract("stream_deadline"))
        } else {
            Ok(())
        }
    }
    fn charge(&mut self, n: u64) -> Result<()> {
        self.time()?;
        self.facts = self
            .facts
            .checked_add(n)
            .ok_or(Error::Contract("fact_overflow"))?;
        SourceLimits::check("facts", self.limit.facts, self.facts)
    }
}

/// Caller supplies a scratch v5 schema with sources populated and fact tables empty.
/// Savepoint rolls back ALL newly derived facts on any late failure; sources stay.
/// Sources must be committed; page cap remains on this scratch connection.
/// Neither Git nor a full corpus Manifest is used. This is not a publish/open API.
pub fn derive(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, false, false, false, false, false, false, false, false, false, false, false,
        false, false, None,
    )
}
pub fn derive_inline(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, false, false, false, false, false, false, false, false, false, false,
        false, false, None,
    )
}
pub fn derive_companions(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, true, false, false, false, false, false, false, false, false, false,
        false, false, None,
    )
}
pub fn derive_common_modules(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, true, true, false, false, false, false, false, false, false, false,
        false, false, None,
    )
}
pub fn derive_roots(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, true, true, true, false, false, false, false, false, false, false, false,
        false, None,
    )
}
pub fn derive_shared(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, true, true, true, true, false, false, false, false, false, false, false,
        false, None,
    )
}
pub fn derive_leaves(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, true, true, true, true, true, false, false, false, false, false, false,
        false, None,
    )
}
pub fn derive_journals(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, true, true, true, true, true, true, false, false, false, false, false,
        false, None,
    )
}
pub fn derive_services(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, true, true, true, true, true, true, true, false, false, false, false,
        false, None,
    )
}
pub fn derive_charts(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, true, true, true, true, true, true, true, true, false, false, false,
        false, None,
    )
}
pub fn derive_registers(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, true, true, true, true, true, true, true, true, true, false, false,
        false, None,
    )
}
pub fn derive_reports(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, true, true, true, true, true, true, true, true, true, true, false, false,
        None,
    )
}
pub fn derive_workflows(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, true, true, true, true, true, true, true, true, true, true, true, false,
        None,
    )
}
pub fn derive_corpus(db: &mut Connection, limits: Limits) -> Result<Value> {
    derive_mode(
        db, limits, true, true, true, true, true, true, true, true, true, true, true, true, true,
        None,
    )
}
pub(crate) fn derive_corpus_bin(
    db: &mut Connection,
    limits: Limits,
    policy: super::corpus_bin_writer::Policy,
) -> Result<Value> {
    derive_mode(
        db,
        limits,
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
        true,
        true,
        Some(policy),
    )
}
fn derive_mode(
    db: &mut Connection,
    limits: Limits,
    inline: bool,
    companions: bool,
    common: bool,
    roots: bool,
    shared: bool,
    leaves: bool,
    journals: bool,
    services: bool,
    charts: bool,
    registers: bool,
    reports: bool,
    workflows: bool,
    corpus: bool,
    bin_policy: Option<super::corpus_bin_writer::Policy>,
) -> Result<Value> {
    limits.validate()?;
    if let Some(cap) = bin_policy.as_ref().and_then(|p| p.diagnostics) {
        super::diagnostics_writer::check_limit(cap)?;
    }
    if !db.is_autocommit() {
        return Err(Error::Contract("stream_requires_committed_sources"));
    }
    let deadline = Instant::now() + limits.time;
    let page_size: u64 = db.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    let pages: u64 = db.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    SourceLimits::check(
        "sqlite_bytes",
        limits.sources.sqlite_bytes,
        pages * page_size,
    )?;
    let old: u64 = db.query_row("PRAGMA max_page_count", [], |r| r.get(0))?;
    let cap = (limits.sources.sqlite_bytes / page_size).min(old);
    db.pragma_update(None, "max_page_count", cap)?;
    (|| {
        let tx = db.savepoint()?;
        let mut budget = Budget {
            limit: limits,
            deadline,
            facts: 0,
            coverage: 0,
            root_sidecars: 0,
            inline,
            companions,
            common,
            roots,
            shared,
            leaves,
            journals,
            services,
            charts,
            registers,
            reports,
            workflows,
            corpus,
            bin_policy,
        };
        let result = calculate(&tx, &mut budget)?;
        budget.time()?;
        tx.commit()?;
        Ok(result)
    })()
}
fn calculate(db: &Connection, budget: &mut Budget) -> Result<Value> {
    if budget
        .bin_policy
        .as_ref()
        .and_then(|p| p.diagnostics)
        .is_some()
        && db.query_row("SELECT EXISTS(SELECT 1 FROM xml_diagnostics)", [], |r| {
            r.get::<_, bool>(0)
        })?
    {
        return Err(Error::Contract("diagnostics_not_empty"));
    }
    for table in ["objects", "modules", "methods", "source_lines"] {
        let exists: bool =
            db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table})"), [], |r| {
                r.get(0)
            })?;
        if exists {
            return Err(Error::Contract("facts_not_empty"));
        }
    }
    if budget.inline {
        if db.query_row(
            "SELECT EXISTS(SELECT 1 FROM inline_declarations)",
            [],
            |r| r.get::<_, bool>(0),
        )? {
            return Err(Error::Contract("facts_not_empty"));
        }
    }
    if budget.companions {
        for table in ["companions", "companion_payloads"] {
            if db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table})"), [], |r| {
                r.get::<_, bool>(0)
            })? {
                return Err(Error::Contract("facts_not_empty"));
            }
        }
    }
    let (count,total,largest):(u64,u64,u64)=db.query_row(
        "SELECT count(*),coalesce(sum(length(content)),0),coalesce(max(length(content)),0) FROM sources",[],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    SourceLimits::check("source_count", budget.limit.sources.source_count, count)?;
    let _ = largest;
    super::corpus_bin_writer::check_sources(
        db,
        budget.limit.sources.source_bytes,
        budget.bin_policy.as_ref(),
    )?;
    SourceLimits::check(
        "total_source_bytes",
        budget.limit.sources.total_source_bytes,
        total,
    )?;
    // Validate each path before copying it. No list of corpus bytes is collected.
    let mut q=db.prepare("SELECT path,kind,length(CAST(path AS BLOB)),length(CAST(kind AS BLOB)) FROM sources ORDER BY path COLLATE BINARY")?;
    let mut rows = q.query([])?;
    let mut xml_count = 0usize;
    while let Some(r) = rows.next()? {
        budget.time()?;
        if r.get::<_, u64>(2)? > 1024 || r.get::<_, u64>(3)? > if budget.companions { 7 } else { 3 }
        {
            return Err(Error::Contract("source_path"));
        }
        let path: String = r.get(0)?;
        let kind: String = r.get(1)?;
        xml221::path_check(&path).map_err(Error::Xml)?;
        if budget.corpus {
            super::corpus_writer::validate_path(&path)?;
        }
        if budget.workflows {
            super::workflow_writer::validate_path(&path)?;
        }
        if budget.reports {
            if budget.bin_policy.as_ref().is_some_and(|b| b.root_help) {
                super::corpus_help_writer::validate_path(&path)?;
            } else {
                super::report_writer::validate_path(&path)?;
            }
        }
        if budget.registers {
            super::register_writer::validate_path(&path)?;
        }
        if budget.charts {
            super::chart_writer::validate_path(&path)?;
        }
        if budget.services {
            super::service_writer::validate_path(&path)?;
        }
        if budget.journals {
            super::journal_writer::validate_path(&path)?;
        }
        if budget.leaves {
            super::leaf_writer::validate_path(&path)?;
        }
        if budget.shared {
            super::shared_writer::validate_path(&path)?;
        }
        if budget.roots {
            xml221::descriptor_only_path(&path).map_err(Error::Xml)?;
        }
        if budget.companions
            && kind
                != if budget.bin_policy.as_ref().is_some_and(|p| p.root_sidecars) {
                    super::sidecar_writer::source_kind(&path)
                } else if budget.corpus {
                    super::corpus_writer::source_kind(&path)
                } else if budget.workflows {
                    super::workflow_writer::source_kind(&path)
                } else if budget.reports {
                    super::report_writer::source_kind(&path)
                } else if budget.registers {
                    super::register_writer::source_kind(&path)
                } else if budget.charts {
                    super::chart_writer::source_kind(&path)
                } else if budget.services {
                    super::service_writer::source_kind(&path)
                } else if budget.journals {
                    super::journal_writer::source_kind(&path)
                } else if budget.leaves {
                    super::leaf_writer::source_kind(&path)
                } else if budget.shared {
                    super::shared_writer::source_kind(&path)
                } else {
                    super::companion_writer::source_kind(&path)
                }
        {
            return Err(Error::Contract("source_kind"));
        }
        if budget.companions && kind == "payload" {
            continue;
        }
        if kind == "xml" && path.ends_with(".xml") {
            xml_count += 1;
        } else if kind != "bsl" || !path.ends_with(".bsl") {
            return Err(Error::Contract("source_kind"));
        }
    }
    drop(rows);
    drop(q);
    let mut uuids = BTreeSet::new();
    let mut coverage = Vec::new();
    let mut owners = BTreeMap::new();
    let config = read_xml(db, "Configuration.xml", Kind::Configuration, None, budget)?;
    let mut refs = config.references.clone();
    let root_ranges = config.root_ranges.clone();
    refs.sort_by(|a, b| a.0.cmp(&b.0));
    if !budget.companions && xml_count != refs.len() + 1 {
        return Err(Error::Contract("xml_inventory"));
    }
    let mut objects = refs.len() + 1;
    let mut pending = Vec::new();
    record_object(
        db,
        config,
        1,
        None,
        &mut uuids,
        &mut owners,
        &mut coverage,
        budget,
    )?;
    for (i, (path, kind, name)) in refs.into_iter().enumerate() {
        let parsed = read_xml(db, &path, kind, Some(&name), budget)?;
        if matches!(kind, Kind::CommonModule | Kind::CommonCommand)
            && parsed.object.modules.len() != 1
        {
            return Err(Error::Contract("common_module_source_missing"));
        }
        if budget.services
            && super::service_writer::is_kind(kind)
            && parsed.object.modules.len() != 1
        {
            return Err(Error::Contract("service_module_missing"));
        }
        if budget.corpus && kind == Kind::SettingsStorage && parsed.object.modules.len() != 1 {
            return Err(Error::Contract("settings_manager_missing"));
        }
        if budget.workflows
            && super::workflow_writer::is_kind(kind)
            && parsed.object.modules.len() != 2
        {
            return Err(Error::Contract("workflow_modules_missing"));
        }
        if budget.registers
            && kind == Kind::AccountingRegister
            && !parsed
                .object
                .modules
                .iter()
                .any(|m| m.kind == "RecordSetModule")
        {
            return Err(Error::Contract("register_recordset_missing"));
        }
        if budget.charts
            && super::chart_writer::is_kind(kind)
            && !parsed
                .object
                .modules
                .iter()
                .any(|m| m.kind == "ManagerModule")
        {
            return Err(Error::Contract("chart_manager_missing"));
        }
        for r in &parsed.companion_refs {
            budget.charge(1)?; // Reserve the companion row before retaining its reference.
            SourceLimits::check(
                "companion_references",
                budget.limit.sources.source_count,
                pending.len() as u64 + 1,
            )?;
            pending.push((i as i64 + 2, path.clone(), r.clone()));
        }
        record_object(
            db,
            parsed,
            i as i64 + 2,
            Some(1),
            &mut uuids,
            &mut owners,
            &mut coverage,
            budget,
        )?;
        if budget.bin_policy.as_ref().is_some_and(|p| p.root_sidecars) {
            let n = super::sidecar_writer::record_payloads(db, &path, kind, i as i64 + 2, || {
                budget.charge(1)
            })?;
            budget.root_sidecars += n;
        }
        if (budget.corpus && super::corpus_writer::is_kind(kind))
            || (budget.workflows && super::workflow_writer::is_kind(kind))
            || (budget.reports && super::report_writer::is_kind(kind))
            || (budget.registers && super::register_writer::is_kind(kind))
            || (budget.charts && super::chart_writer::is_kind(kind))
            || (budget.services && super::service_writer::is_kind(kind))
            || (budget.journals && super::journal_writer::is_kind(kind))
            || (budget.shared && super::shared_writer::is_kind(kind))
            || (budget.leaves && super::leaf_writer::is_kind(kind))
        {
            budget.charge(1)?;
            let (a, b) = root_ranges
                .get(&path)
                .copied()
                .ok_or(Error::Contract("shared_reference"))?;
            db.execute("INSERT INTO companions(object_id,reference_path,byte_start,byte_end) VALUES (?1,'Configuration.xml',?2,?3)",params![i as i64+2,a as i64,b as i64])?;
            if budget.corpus && super::corpus_writer::is_kind(kind) {
                super::corpus_writer::record_payloads(db, &path, i as i64 + 2, || {
                    budget.charge(1)
                })?;
            } else if budget.workflows && super::workflow_writer::is_kind(kind) {
                super::workflow_writer::record_payloads(db, &path, i as i64 + 2, || {
                    budget.charge(1)
                })?;
            } else if budget.reports && super::report_writer::is_kind(kind) {
                super::report_writer::record_payloads(db, &path, i as i64 + 2, || {
                    budget.charge(1)
                })?;
                if budget.bin_policy.as_ref().is_some_and(|b| b.root_help) {
                    super::corpus_help_writer::record_payloads(db, &path, i as i64 + 2, || {
                        budget.charge(1)
                    })?;
                }
            } else if budget.registers && super::register_writer::is_kind(kind) {
                super::register_writer::record_payloads(db, &path, i as i64 + 2, || {
                    budget.charge(1)
                })?;
            } else if budget.charts && super::chart_writer::is_kind(kind) {
                super::chart_writer::record_payloads(db, &path, i as i64 + 2, || budget.charge(1))?;
            } else if budget.services && super::service_writer::is_kind(kind) {
                // Services have inline declarations and one BSL module, no payload files.
            } else if budget.journals && super::journal_writer::is_kind(kind) {
                super::journal_writer::record_payloads(db, &path, kind, i as i64 + 2, || {
                    budget.charge(1)
                })?;
            } else if budget.leaves && super::leaf_writer::is_kind(kind) {
                super::leaf_writer::record_payloads(db, &path, kind, i as i64 + 2, || {
                    budget.charge(1)
                })?;
            } else {
                super::shared_writer::record_payloads(db, &path, kind, i as i64 + 2, || {
                    budget.charge(1)
                })?;
            }
            budget.time()?;
        }
    }
    if budget.corpus {
        super::corpus_writer::record_globals(db, || budget.charge(1))?;
    }
    let mut pending: std::collections::VecDeque<_> = pending.into();
    let mut followed = 0u64;
    while let Some((parent, reference_path, r)) = pending.pop_front() {
        followed += 1;
        if budget.corpus {
            SourceLimits::check(
                "companion_references",
                budget.limit.sources.source_count,
                followed,
            )?;
        }
        budget.time()?;
        let path = format!(
            "{}/{}/{}.xml",
            reference_path
                .strip_suffix(".xml")
                .ok_or(Error::Contract("xml_path"))?,
            r.kind.folder(),
            r.name
        );
        if !db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sources WHERE path=?1 AND kind='xml')",
            [&path],
            |row| row.get::<_, bool>(0),
        )? {
            return Err(Error::Contract("companion_descriptor_missing"));
        }
        let parsed = read_xml(db, &path, r.kind, Some(&r.name), budget)?;
        if budget.corpus && super::corpus_writer::is_path(&path) {
            super::corpus_writer::check_companion(db, &path, r.kind, parsed.object.modules.len())?;
        }
        if budget.workflows && super::workflow_writer::is_path(&path) {
            super::workflow_writer::check_companion(
                db,
                &path,
                r.kind,
                parsed.object.modules.len(),
            )?;
        }
        if budget.reports && super::report_writer::is_path(&path) {
            super::report_writer::check_companion(db, &path, r.kind, parsed.object.modules.len())?;
        }
        if budget.registers && super::register_writer::is_path(&path) {
            super::register_writer::check_companion(
                db,
                &path,
                r.kind,
                parsed.object.modules.len(),
                budget.bin_policy.as_ref().is_some_and(|p| p.optional_form),
            )?;
        }
        if budget.charts && super::chart_writer::is_path(&path) {
            super::chart_writer::check_form(
                db,
                &path,
                r.kind,
                parsed.object.modules.len(),
                budget.bin_policy.as_ref().is_some_and(|p| p.optional_form),
            )?;
        }
        if budget.journals && path.starts_with("DocumentJournals/") {
            super::journal_writer::check_companion(db, &path, r.kind, parsed.object.modules.len())?;
        }
        objects += 1;
        let id = objects as i64;
        if budget.corpus && r.kind == Kind::Subsystem {
            for nested in &parsed.companion_refs {
                budget.charge(1)?;
                SourceLimits::check(
                    "companion_references",
                    budget.limit.sources.source_count,
                    followed + pending.len() as u64 + 1,
                )?;
                pending.push_back((id, path.clone(), nested.clone()));
            }
        }
        record_object(
            db,
            parsed,
            id,
            Some(parent),
            &mut uuids,
            &mut owners,
            &mut coverage,
            budget,
        )?;
        db.execute("INSERT INTO companions(object_id,reference_path,byte_start,byte_end) VALUES (?1,?2,?3,?4)",params![id,reference_path,r.start as i64,r.end as i64])?;
        if budget.corpus && r.kind == Kind::Subsystem {
            super::corpus_writer::record_payloads(db, &path, id, || budget.charge(1))?;
            continue;
        }
        let prefix = format!("{}/", path.strip_suffix(".xml").unwrap());
        let mut q=db.prepare("SELECT path FROM sources WHERE kind='payload' AND substr(path,1,length(?1))=?1 ORDER BY path COLLATE BINARY")?;
        let mut rows = q.query(params![prefix])?;
        let mut primary = 0;
        while let Some(row) = rows.next()? {
            budget.charge(1)?;
            let p: String = row.get(0)?;
            let suffix = &p[prefix.len()..];
            let role = if r.kind == Kind::Form && suffix == "Ext/Form.xml" {
                "form"
            } else if r.kind == Kind::Template
                && ["Ext/Template.xml", "Ext/Template.bin", "Ext/Template.txt"].contains(&suffix)
            {
                "template"
            } else {
                "additional"
            };
            if role != "additional" {
                primary += 1;
            }
            if primary > 1 {
                return Err(Error::Contract("companion_payload_ambiguous"));
            }
            db.execute(
                "INSERT INTO companion_payloads(path,object_id,role) VALUES (?1,?2,?3)",
                params![p, id, role],
            )?;
        }
    }
    if budget.companions {
        if xml_count != objects {
            return Err(Error::Contract("companion_descriptor_inventory"));
        }
        if db.query_row("SELECT EXISTS(SELECT 1 FROM sources s LEFT JOIN companion_payloads p ON s.path=p.path WHERE s.kind='payload' AND p.path IS NULL)",[],|r|r.get::<_,bool>(0))? {return Err(Error::Contract("companion_payload_orphan"));}
    }
    coverage.sort_by(|a, b| (&a.path, &a.selector).cmp(&(&b.path, &b.selector)));
    let mut statuses = BTreeMap::new();
    let mut modules = 0;
    let mut unconfirmed = 0;
    let mut query = db.prepare(
        "SELECT path,content FROM sources WHERE kind='bsl' ORDER BY path COLLATE BINARY",
    )?;
    let mut rows = query.query([])?;
    while let Some(row) = rows.next()? {
        budget.time()?;
        let path: String = row.get(0)?;
        let bytes: Vec<u8> = row.get(1)?; // Only this bounded source is held.
        let owner = owners.get(&path).copied();
        if budget.corpus && super::corpus_writer::is_path(&path) && owner.is_none() {
            return Err(Error::Contract("corpus_module_owner"));
        }
        if budget.workflows && super::workflow_writer::is_path(&path) && owner.is_none() {
            return Err(Error::Contract("workflow_module_owner"));
        }
        if budget.reports && super::report_writer::is_path(&path) && owner.is_none() {
            return Err(Error::Contract("report_module_owner"));
        }
        if budget.registers && super::register_writer::is_path(&path) && owner.is_none() {
            return Err(Error::Contract("register_module_owner"));
        }
        if budget.charts && super::chart_writer::is_path(&path) && owner.is_none() {
            return Err(Error::Contract("chart_module_owner"));
        }
        if budget.services && super::service_writer::is_path(&path) && owner.is_none() {
            return Err(Error::Contract("service_module_owner"));
        }
        if budget.journals && super::journal_writer::is_path(&path) && owner.is_none() {
            return Err(Error::Contract("journal_module_owner"));
        }
        if budget.leaves && super::leaf_writer::is_path(&path) && owner.is_none() {
            return Err(Error::Contract("leaf_module_owner"));
        }
        if budget.shared && super::shared_writer::is_path(&path) && owner.is_none() {
            return Err(Error::Contract("shared_module_owner"));
        }
        if budget.common && path.starts_with("CommonModules/") && owner.is_none() {
            return Err(Error::Contract("common_module_owner"));
        }
        if budget.companions && super::companion_writer::companion_file(&path) && owner.is_none() {
            return Err(Error::Contract("companion_module_owner"));
        }
        modules += 1;
        if owner.is_none() {
            unconfirmed += 1;
        }
        let strings = budget.bin_policy.as_ref().is_some_and(|p| p.strings);
        let literals = budget.bin_policy.as_ref().is_some_and(|p| p.literals);
        let wrapped = budget.bin_policy.as_ref().is_some_and(|p| p.wrapped);
        let refined = budget.bin_policy.as_ref().is_some_and(|p| p.refined);
        let annotated_vars = budget.bin_policy.as_ref().is_some_and(|p| p.annotated_vars);
        let dates_members = budget.bin_policy.as_ref().is_some_and(|p| p.dates_members);
        let async_casing = budget.bin_policy.as_ref().is_some_and(|p| p.async_casing);
        let mobile_vars = budget.bin_policy.as_ref().is_some_and(|p| p.mobile_vars);
        let method_regions = budget.bin_policy.as_ref().is_some_and(|p| p.method_regions);
        let header_forms = budget.bin_policy.as_ref().is_some_and(|p| p.header_forms);
        let lexical_tail = budget.bin_policy.as_ref().is_some_and(|p| p.lexical_tail);
        let source_anomalies = budget.bin_policy.as_ref().is_some_and(|p| p.source_anomalies);
        let long_headers = budget.bin_policy.as_ref().is_some_and(|p| p.long_headers);
        let source_limits = budget.bin_policy.as_ref().is_some_and(|p| p.source_limits);
        let conditional = budget.bin_policy.as_ref().is_some_and(|p| p.conditional);
        let headers = budget.bin_policy.as_ref().is_some_and(|p| p.headers);
        let context = budget.bin_policy.as_ref().is_some_and(|p| p.context);
        let status = insert_bsl_mode(
            db,
            &path,
            &bytes,
            owner,
            context,
            headers,
            conditional,
            strings,
            literals,
            wrapped,
            refined,
            annotated_vars,
            dates_members,
            async_casing,
            mobile_vars,
            method_regions,
            header_forms,
            source_limits,
            long_headers,
            lexical_tail,
            source_anomalies,
            |n| budget.charge(n),
        )?;
        *statuses.entry(status).or_insert(0usize) += 1;
        budget.time()?;
    }
    let mut result = json!({"temporary":true,"objects":objects,"modules":modules,"owner_unconfirmed":unconfirmed,"parse_counts":statuses,
        "xml_unverified":coverage.iter().map(|u|json!({"path":u.path,"selector":u.selector})).collect::<Vec<_>>(),
        "relations":"not_analyzed","semantics":"not_analyzed"});
    if let Some(cap) = budget.bin_policy.as_ref().and_then(|p| p.diagnostics) {
        result.as_object_mut().unwrap().remove("xml_unverified");
        result["xml_diagnostics"] =
            super::diagnostics_writer::summarize(db, cap, budget.limit.facts)?;
    }
    if budget.bin_policy.as_ref().is_some_and(|p| p.root_sidecars) {
        result["root_xml_sidecars"] = super::sidecar_writer::summary(budget.root_sidecars);
    }
    if budget.inline {
        result["inline_declarations"] = json!(db.query_row(
            "SELECT count(*) FROM inline_declarations",
            [],
            |r| r.get::<_, u64>(0)
        )?);
        result["inline_semantics"] = json!("not_analyzed");
        result["companion_references"] = json!(if budget.companions {
            "resolved_structure_only"
        } else {
            "refused"
        });
        if budget.companions {
            result["companion_contents"] = json!("not_analyzed");
        }
    }
    if budget.corpus {
        result["corpus_structure"] = json!("complete_known_grammar");
        result["global_contents"] = json!("not_analyzed");
    }
    if budget.workflows {
        result["workflow_structure"] = json!("complete_known_grammar");
        result["workflow_semantics"] = json!("not_analyzed");
    }
    if budget.reports {
        result["report_structure"] = json!("complete_known_grammar");
        result["report_semantics"] = json!("not_analyzed");
    }
    if budget.registers {
        result["register_structure"] = json!("complete_known_grammar");
        result["register_semantics"] = json!("not_analyzed");
    }
    if budget.charts {
        result["chart_structure"] = json!("complete_known_grammar");
        result["chart_contents"] = json!("not_analyzed");
    }
    if budget.services {
        result["service_structure"] = json!("complete_known_grammar");
        result["service_semantics"] = json!("not_analyzed");
    }
    if budget.journals {
        result["journal_structure"] = json!("complete_known_grammar");
        result["journal_contents"] = json!("not_analyzed");
    }
    if budget.leaves {
        result["leaf_structure"] = json!("complete_known_grammar");
        result["leaf_contents"] = json!("not_analyzed");
    }
    if budget.shared {
        result["shared_structure"] = json!("complete_known_grammar");
        result["shared_contents"] = json!("not_analyzed");
    }
    if budget
        .bin_policy
        .as_ref()
        .and_then(|p| p.diagnostics)
        .is_some()
    {
        SourceLimits::check(
            "coverage_summary_bytes",
            8192,
            result.to_string().len() as u64,
        )?;
    }
    SourceLimits::check(
        "coverage_bytes",
        budget.limit.coverage_bytes,
        result.to_string().len() as u64,
    )?;
    Ok(result)
}
fn read_xml(
    db: &Connection,
    path: &str,
    kind: Kind,
    expected: Option<&str>,
    budget: &mut Budget,
) -> Result<xml221::ParsedFile> {
    budget.time()?;
    let bytes: Vec<u8> = db.query_row(
        "SELECT content FROM sources WHERE path=?1 AND kind='xml'",
        [path],
        |r| r.get(0),
    )?;
    let base = if kind == Kind::Configuration {
        "Ext/".to_string()
    } else {
        format!(
            "{}/Ext/",
            path.strip_suffix(".xml")
                .ok_or(Error::Contract("xml_path"))?
        )
    };
    let mut modules = Vec::new();
    for module in kind.modules() {
        let p = format!("{base}{module}.bsl");
        if db.query_row(
            "SELECT EXISTS(SELECT 1 FROM sources WHERE path=?1 AND kind='bsl')",
            [&p],
            |r| r.get::<_, bool>(0),
        )? {
            modules.push(p);
        }
    }
    let parsed = if budget.inline {
        xml221::inspect_shared_profile_configuration(
            path,
            &bytes,
            kind,
            expected,
            &modules,
            true,
            budget.companions,
            budget.common,
            budget.roots,
            budget.shared,
            budget.leaves,
            budget.journals,
            budget.services,
            budget.charts,
            budget.registers,
            budget.reports,
            budget.workflows,
            budget.corpus,
            budget.bin_policy.as_ref().is_some_and(|b| b.configuration),
        )
    } else {
        xml221::inspect_file(path, &bytes, kind, expected, &modules)
    }
    .map_err(Error::Xml)?;
    budget.time()?;
    Ok(parsed)
}
fn record_object(
    db: &Connection,
    parsed: xml221::ParsedFile,
    id: i64,
    parent: Option<i64>,
    uuids: &mut BTreeSet<String>,
    owners: &mut BTreeMap<String, i64>,
    coverage: &mut Vec<xml221::Unverified>,
    budget: &mut Budget,
) -> Result<()> {
    budget.charge(1 + parsed.unverified.len() as u64)?;
    let obj = parsed.object;
    if let Some(uuid) = &obj.uuid {
        if !uuids.insert(uuid.to_lowercase()) {
            return Err(Error::Contract("duplicate_uuid"));
        }
    }
    db.execute(
        "INSERT INTO objects(object_id,parent_id,kind,name,xml_path) VALUES (?1,?2,?3,?4,?5)",
        params![id, parent, obj.kind.name(), obj.name, obj.path],
    )?;
    if budget.inline {
        budget.charge(parsed.declarations.len() as u64)?;
        for d in parsed.declarations {
            if !uuids.insert(d.uuid.clone()) {
                return Err(Error::Contract("duplicate_uuid"));
            }
            db.execute("INSERT INTO inline_declarations(declaration_key,owner_id,parent_key,kind,name,uuid,xml_path,byte_start,byte_end) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",params![d.key,id,d.parent,d.kind,d.name,d.uuid,d.path,d.start as i64,d.end as i64])?;
            let legacy_required = (budget.journals && obj.kind == Kind::DocumentJournal)
                || (budget.reports && super::report_writer::is_kind(obj.kind))
                || (budget.workflows && obj.kind == Kind::Task);
            let exact_optional = budget.bin_policy.as_ref().is_some_and(|p| p.command_owners)
                && matches!(obj.kind, Kind::Catalog | Kind::Document | Kind::InformationRegister)
                && d.parent.is_none()
                && d.path == obj.path;
            if (legacy_required || exact_optional) && d.kind == "Command" {
                budget.charge(1)?;
                let module = format!(
                    "{}/Commands/{}/Ext/CommandModule.bsl",
                    obj.path
                        .strip_suffix(".xml")
                        .ok_or(Error::Contract("xml_path"))?,
                    d.name
                );
                if !db.query_row(
                    "SELECT EXISTS(SELECT 1 FROM sources WHERE path=?1 COLLATE BINARY AND kind='bsl')",
                    [&module],
                    |r| r.get::<_, bool>(0),
                )? {
                    if !legacy_required {
                        // A child declaration without one exact BSL source
                        // does not present a link to a guessed module.
                        continue;
                    }
                    return Err(Error::Contract(if obj.kind == Kind::Task {
                        "workflow_command_module_missing"
                    } else if super::report_writer::is_kind(obj.kind) {
                        "report_command_module_missing"
                    } else {
                        "journal_command_module_missing"
                    }));
                }
                db.execute("INSERT INTO inline_module_owners(module_path,declaration_key,owner_id) VALUES (?1,?2,?3)",params![module,d.key,id])?;
                owners.insert(module, id);
            }
        }
    }
    for module in obj.modules {
        owners.insert(module.path, id);
    }
    for item in parsed.unverified {
        if let Some(cap) = budget.bin_policy.as_ref().and_then(|p| p.diagnostics) {
            let row = super::diagnostics_writer::row_json(&item.path, &item.selector)?;
            budget.coverage += row.len() as u64;
            SourceLimits::check("diagnostics_bytes", cap, budget.coverage)?;
            budget.time()?;
            db.execute(
                "INSERT INTO xml_diagnostics(path,selector) VALUES (?1,?2)",
                params![item.path, item.selector],
            )?;
            continue;
        }
        // Conservative upper bound for JSON escaping and object punctuation.
        budget.coverage += (6 * (item.path.len() + item.selector.len()) + 128) as u64;
        SourceLimits::check(
            "coverage_bytes",
            budget.limit.coverage_bytes,
            budget.coverage,
        )?;
        coverage.push(item);
    }
    Ok(())
}
