//! Экспериментальный сборщик Linux x86_64 v5. Не читатель и не production capability.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub mod chart_writer;
pub mod common_writer;
pub mod companion_writer;
pub mod contract;
pub mod ingest_writer;
pub mod inline_writer;
pub mod journal_writer;
pub mod leaf_writer;
mod publish;
pub mod root_writer;
pub mod service_writer;
pub mod shared_writer;
pub mod streaming_facts;
pub mod streaming_writer;
use crate::{
    git_manifest::{self, Manifest},
    lexical,
    xml221::Xml221Subset,
};
use rusqlite::{params, Connection};
use serde_json::json;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    Sqlite(rusqlite::Error),
    Json(serde_json::Error),
    Git(git_manifest::Code),
    Ingest(git_manifest::ingest::Error),
    Xml(crate::xml221::XmlError),
    Limit {
        name: &'static str,
        maximum: u64,
        actual: u64,
    },
    SqliteFull {
        path: PathBuf,
        sqlite_bytes: u64,
        max_pages: u64,
    },
    Contract(&'static str),
    NonReproducible,
    PublicationConflict,
    PublishedSync {
        path: PathBuf,
        error: std::io::Error,
    },
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SqliteFull {path,sqlite_bytes,max_pages} => write!(f,
                "v5: диск или лимит страниц SQLite: {}; sqlite_bytes={sqlite_bytes}, max_pages={max_pages}",path.display()),
            _ => write!(f, "v5: {self:?}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}
impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
pub type Result<T> = std::result::Result<T, Error>;

/// Все четыре лимита обязательны; production-значений по умолчанию нет.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub source_count: u64,
    pub source_bytes: u64,
    pub total_source_bytes: u64,
    pub sqlite_bytes: u64,
}
impl Limits {
    fn check(name: &'static str, maximum: u64, actual: u64) -> Result<()> {
        if actual > maximum {
            Err(Error::Limit {
                name,
                maximum,
                actual,
            })
        } else {
            Ok(())
        }
    }
    fn json(self) -> serde_json::Value {
        json!({"source_count":self.source_count,"source_bytes":self.source_bytes,
            "total_source_bytes":self.total_source_bytes,"sqlite_bytes":self.sqlite_bytes})
    }
}
#[derive(Debug)]
pub struct BuiltSnapshot {
    pub path: PathBuf,
    pub snapshot_id: String,
    pub facts_sha256: String,
    pub file_sha256: String,
    pub reused: bool,
}

/// Читает только проверенные commit/tree/blobs, никогда рабочие XML/BSL.
pub fn build(
    repo: &Path,
    commit: &str,
    source_id: &str,
    output: &Path,
    limits: Limits,
) -> Result<BuiltSnapshot> {
    if source_id.is_empty() {
        return Err(Error::Contract("source_id"));
    }
    let manifest = Manifest::read(repo, commit, git_manifest::Limits::default())
        .map_err(|e| Error::Git(e.code))?;
    let selected: Vec<_> = manifest
        .files()
        .iter()
        .filter(|f| selected(&f.path))
        .collect();
    Limits::check("source_count", limits.source_count, selected.len() as u64)?;
    let mut total = 0u64;
    for file in &selected {
        Limits::check(
            "source_bytes",
            limits.source_bytes,
            file.bytes().len() as u64,
        )?;
        total = total
            .checked_add(file.bytes().len() as u64)
            .ok_or(Error::Contract("source_size_overflow"))?;
        Limits::check("total_source_bytes", limits.total_source_bytes, total)?;
    }
    let dir = publish::directory(output)?;
    let temp = publish::Temporary::new(&dir)?;
    let mut db = Connection::open(&temp.path)?;
    // SQLite прекращает рост до превышения заданного числа полных страниц.
    db.pragma_update(None, "page_size", 4096)?;
    let max_pages = (limits.sqlite_bytes / 4096).min(4_294_967_294);
    if max_pages == 0 {
        return Err(Error::Limit {
            name: "sqlite_bytes",
            maximum: limits.sqlite_bytes,
            actual: 4096,
        });
    }
    db.pragma_update(None, "max_page_count", max_pages)?;
    let result = fill(&mut db, &manifest, source_id, limits, None);
    if matches!(&result, Err(Error::Sqlite(rusqlite::Error::SqliteFailure(e, _))) if e.code == rusqlite::ErrorCode::DiskFull)
    {
        // SQLITE_FULL не различает ENOSPC и max_page_count: actual неизвестен.
        return Err(Error::SqliteFull {
            path: temp.path.clone(),
            sqlite_bytes: limits.sqlite_bytes,
            max_pages,
        });
    }
    let (snapshot_id, facts_sha256) = result?;
    db.close().map_err(|(_, e)| Error::Sqlite(e))?;
    temp.ensure_standalone()?;
    let size = std::fs::metadata(&temp.path)?.len();
    Limits::check("sqlite_bytes", limits.sqlite_bytes, size)?;
    publish::publish(&dir, temp, snapshot_id, facts_sha256, limits.sqlite_bytes)
}
fn selected(path: &str) -> bool {
    path.ends_with(".xml") || path.ends_with(".bsl")
}
fn reason(reason: lexical::Reason) -> &'static str {
    use lexical::Reason::*;
    match reason {
        Limit => "Limit",
        Utf8 => "Utf8",
        UnsupportedCharacter => "UnsupportedCharacter",
        InvalidControl => "InvalidControl",
        ReservedName => "ReservedName",
        MultilineString => "MultilineString",
        DirectiveUnsupported => "DirectiveUnsupported",
        RegionHeader => "RegionHeader",
        RegionInsideMethod => "RegionInsideMethod",
        RegionUnmatchedEnd => "RegionUnmatchedEnd",
        RegionMissingEnd => "RegionMissingEnd",
        AnnotationUnsupported => "AnnotationUnsupported",
        AsyncUnsupported => "AsyncUnsupported",
        UnsupportedHeader => "UnsupportedHeader",
        AmbiguousBoundary => "AmbiguousBoundary",
        Delimiters => "Delimiters",
        MissingEnd => "MissingEnd",
        UnmatchedEnd => "UnmatchedEnd",
    }
}
fn lines(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut result = vec![];
    let mut start = 0;
    for (i, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            result.push((start, i + 1));
            start = i + 1;
        }
    }
    if start < bytes.len() {
        result.push((start, bytes.len()));
    }
    result
}
fn fill(
    db: &mut Connection,
    manifest: &Manifest,
    source_id: &str,
    limits: Limits,
    date: Option<&str>,
) -> Result<(String, String)> {
    db.execute_batch(contract::SCHEMA)?;
    let tx = db.transaction()?;
    let date: String = match date {
        Some(d) => d.into(),
        None => tx.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| {
            r.get(0)
        })?,
    };
    let parameters = json!({"limits":limits.json(),"selection":"all-lowercase-xml-bsl","git_guard":"r09","xml_guard":"r09","lexical_guard":"r09"});
    let mut chosen = vec![];
    let mut excluded = vec![];
    for file in manifest.files() {
        let entry = json!({"path":file.path,"mode":format!("{:o}",file.mode),"blob_oid":file.oid});
        if !selected(&file.path) {
            excluded.push(json!({"input":entry,"reason":"NotXmlOrBsl"}));
            continue;
        }
        chosen.push(entry);
        let kind = if file.path.ends_with(".xml") {
            "xml"
        } else {
            "bsl"
        };
        tx.execute(
            "INSERT INTO sources VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                file.path,
                kind,
                format!("{:o}", file.mode),
                file.oid,
                file.sha256,
                file.bytes()
            ],
        )?;
    }
    let coverage = derive_facts(&tx)?;
    tx.execute("INSERT INTO metadata VALUES (1,5,?1,'bslens-v5-pending',?2,NULL,?3,?4,?5,?6,?7,'2.21',?8,?9,?10,?11,?12,?13,'not_analyzed')",params![contract::PROFILE,date,source_id,manifest.commit(),manifest.tree(),contract::CORE,contract::ADAPTER,contract::LEXICAL,contract::SCOPE,parameters.to_string(),json!({"selected":chosen,"excluded":excluded}).to_string(),coverage.to_string(),limits.json().to_string()])?;
    contract::validate_schema_profile(&tx)?;
    let snapshot = contract::snapshot_id(&tx)?;
    let facts = contract::facts_sha256(&tx)?;
    tx.execute(
        "UPDATE metadata SET snapshot_id=?1,facts_sha256=?2",
        params![snapshot, facts],
    )?;
    tx.commit()?;
    Ok((snapshot, facts))
}

/// Общий producer фактов S3 и будущего offline verify: вызывается на отдельной
/// scratch БД с точной схемой и уже заполненной sources, пустыми таблицами фактов.
/// Не открывает файлы, не удаляет старые факты и не проверяет происхождение Git.
/// Вызывающий оборачивает заполнение в транзакцию; возвращается временное покрытие.
pub fn derive_facts(db: &Connection) -> Result<serde_json::Value> {
    for name in ["objects", "modules", "methods", "source_lines"] {
        let count: i64 = db.query_row(&format!("SELECT count(*) FROM {name}"), [], |r| r.get(0))?;
        if count != 0 {
            return Err(Error::Contract("facts_not_empty"));
        }
    }
    let (count, bytes): (u64, u64) = db.query_row(
        "SELECT count(*),coalesce(sum(length(content)),0) FROM sources",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Limits::check("xml221_r09_files", crate::xml221::MAX_FILES as u64, count)?;
    Limits::check(
        "xml221_r09_total_bytes",
        crate::xml221::MAX_TOTAL_BYTES as u64,
        bytes,
    )?;
    let mut query = db.prepare("SELECT path,content FROM sources ORDER BY path COLLATE BINARY")?;
    let inputs = query
        .query_map([], |r| {
            Ok(FactInput {
                path: r.get(0)?,
                data: r.get(1)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let blobs = inputs
        .iter()
        .map(|f| (f.path.clone(), f.data.clone()))
        .collect();
    let subset = Xml221Subset::inspect(&blobs).map_err(Error::Xml)?;
    let mut ids = BTreeMap::new();
    let config = subset
        .objects()
        .iter()
        .find(|o| o.kind == crate::xml221::Kind::Configuration)
        .ok_or(Error::Contract("configuration"))?;
    ids.insert(config.path.as_str(), 1i64);
    db.execute(
        "INSERT INTO objects(object_id,parent_id,kind,name,xml_path) VALUES (1,NULL,?1,?2,?3)",
        params![config.kind.name(), config.name, config.path],
    )?;
    for object in subset
        .objects()
        .iter()
        .filter(|o| o.kind != crate::xml221::Kind::Configuration)
    {
        let id = ids.len() as i64 + 1;
        ids.insert(object.path.as_str(), id);
        db.execute(
            "INSERT INTO objects(object_id,parent_id,kind,name,xml_path) VALUES (?1,1,?2,?3,?4)",
            params![id, object.kind.name(), object.name, object.path],
        )?;
    }
    let owners: BTreeMap<_, _> = subset
        .objects()
        .iter()
        .flat_map(|o| {
            o.modules
                .iter()
                .map(|m| (m.path.as_str(), ids[o.path.as_str()]))
        })
        .collect();
    let mut statuses = BTreeMap::new();
    let mut modules = 0;
    let mut unconfirmed = 0;
    for file in inputs.iter().filter(|f| f.path.ends_with(".bsl")) {
        modules += 1;
        let owner = owners.get(file.path.as_str());
        if owner.is_none() {
            unconfirmed += 1;
        }
        let status = insert_bsl(db, &file.path, file.bytes(), owner.copied(), |_| Ok(()))?;
        *statuses.entry(status).or_insert(0usize) += 1;
    }

    let coverage = json!({"temporary":true,"objects":ids.len(),"modules":modules,"owner_unconfirmed":unconfirmed,"parse_counts":statuses,
        "xml_unverified":subset.unverified().iter().map(|u| json!({"path":u.path,"selector":u.selector})).collect::<Vec<_>>(),
        "relations":"not_analyzed","semantics":"not_analyzed"});
    Ok(coverage)
}
struct FactInput {
    path: String,
    data: Vec<u8>,
}
impl FactInput {
    fn bytes(&self) -> &[u8] {
        &self.data
    }
}

/// Shared per-source fact producer; caller charges rows before allocations/inserts.
pub(super) fn insert_bsl(
    db: &Connection,
    path: &str,
    bytes: &[u8],
    owner: Option<i64>,
    charge: impl FnMut(u64) -> Result<()>,
) -> Result<&'static str> {
    insert_bsl_mode(
        db, path, bytes, owner, false, false, false, false, false, false, false, false, false,
        false, false, false, false, false, false, false, false, charge,
    )
}
pub(super) fn insert_bsl_mode(
    db: &Connection,
    path: &str,
    bytes: &[u8],
    owner: Option<i64>,
    context: bool,
    headers: bool,
    conditional: bool,
    strings: bool,
    literals: bool,
    wrapped: bool,
    refined: bool,
    annotated_vars: bool,
    dates_members: bool,
    async_casing: bool,
    mobile_vars: bool,
    method_regions: bool,
    header_forms: bool,
    source_limits: bool,
    long_headers: bool,
    lexical_tail: bool,
    source_anomalies: bool,
    mut charge: impl FnMut(u64) -> Result<()>,
) -> Result<&'static str> {
    let line_count = bytes.iter().filter(|b| **b == b'\n').count()
        + usize::from(!bytes.is_empty() && bytes.last() != Some(&b'\n'));
    charge(line_count as u64 + 1)?;
    let positions = lines(bytes);
    for (i, (a, b)) in positions.iter().enumerate() {
        charge(0)?;
        db.execute(
            "INSERT INTO source_lines(path,line_number,byte_start,byte_end) VALUES (?1,?2,?3,?4)",
            params![path, i as i64 + 1, *a as i64, *b as i64],
        )?;
    }
    let mut anomaly = None;
    let analysis = if source_anomalies {
        charge(0)?;
        let checked = if lexical_tail {
            lexical::analyze_navigator_agreement_v1(bytes)
        } else {
            lexical::analyze_source_anomalies_v1(bytes)
        };
        charge(0)?;
        checked.map(|(result, site)| {
            anomaly = site;
            result
        })
    } else if lexical_tail {
        lexical::analyze_lexical_tail_v1(bytes)
    } else if long_headers {
        lexical::analyze_long_headers_v1(bytes)
    } else if source_limits {
        lexical::analyze_source_limits_v1(bytes)
    } else if header_forms {
        lexical::analyze_header_forms_v1(bytes)
    } else if method_regions {
        lexical::analyze_method_regions_v1(bytes)
    } else if mobile_vars {
        lexical::analyze_mobile_vars_v1(bytes)
    } else if async_casing {
        lexical::analyze_async_casing_v1(bytes)
    } else if dates_members {
        lexical::analyze_dates_members_v1(bytes)
    } else if annotated_vars {
        lexical::analyze_annotated_vars_v1(bytes)
    } else if refined {
        lexical::analyze_corpus_refinements_v1(bytes)
    } else if wrapped {
        lexical::analyze_wrapped_headers_v1(bytes)
    } else if literals {
        lexical::analyze_literal_defaults_v1(bytes)
    } else if strings {
        lexical::analyze_pipe_strings_v1(bytes)
    } else if conditional {
        lexical::analyze_conditionals_v1(bytes)
    } else if headers {
        lexical::analyze_parameters_v1(bytes)
    } else if context {
        lexical::analyze_context_v1(bytes)
    } else {
        lexical::analyze(bytes)
    };
    let (status, code) = match &analysis {
        Ok(_) => ("accepted", None),
        Err(e) => ("unparsed", Some(reason(e.reason))),
    };

    db.execute(
        "INSERT INTO modules(path,owner_id,parse_status,reason_code) VALUES (?1,?2,?3,?4)",
        params![path, owner, status, code],
    )?;
    if let Some(site) = anomaly {
        charge(1)?;
        db.execute(
            "INSERT INTO bsl_source_anomalies VALUES (?1,?2,?3,?4,?5)",
            params![
                path,
                site.code,
                site.line as i64,
                site.start as i64,
                site.end as i64
            ],
        )?;
    }
    if let Ok(analysis) = analysis {
        charge(analysis.boundaries.len() as u64)?;
        for fact in analysis.boundaries {
            charge(0)?;
            let first = positions
                .get(fact.start_line - 1)
                .ok_or(Error::Contract("method_line"))?;
            let last = positions
                .get(fact.end_line - 1)
                .ok_or(Error::Contract("method_line"))?;
            if !(first.0 <= fact.start_byte
                && fact.start_byte < first.1
                && last.0 < fact.end_byte
                && fact.end_byte <= last.1
                && fact.start_byte < fact.end_byte)
            {
                return Err(Error::Contract("method_range"));
            }
            let header = std::str::from_utf8(&bytes[fact.header_start_byte..fact.header_end_byte])
                .map_err(|_| Error::Contract("header_utf8"))?;
            // Only the literal profile admits one final separator after an otherwise
            // complete header. The lexer has already checked placement and cardinality.
            let header = if literals {
                header
                    .strip_suffix(';')
                    .map_or(header, |without_separator| {
                        without_separator.trim_end_matches([' ', '\t'])
                    })
            } else {
                header
            };
            let header = header.to_lowercase();
            let exported = header.ends_with("экспорт") || header.ends_with("export");
            db.execute("INSERT INTO methods(module_path,kind,name,exported,start_line,end_line,byte_start,byte_end) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",params![path,fact.kind,fact.name,exported,fact.start_line as i64,fact.end_line as i64,fact.start_byte as i64,fact.end_byte as i64])?;
            if conditional {
                charge(1)?;
                let path = serde_json::to_string(&fact.branch_path.iter().map(|b|json!({"block_line":b.block_line,"arm":b.arm,"kind":b.kind,"arm_line":b.arm_line,"directive_start_byte":b.directive_start_byte,"directive_end_byte":b.directive_end_byte})).collect::<Vec<_>>())?;
                if path.len() > 4096 {
                    return Err(Error::Limit {
                        name: "branch_path",
                        maximum: 4096,
                        actual: path.len() as u64,
                    });
                }
                db.execute(
                    "INSERT INTO method_branches VALUES (?1,?2)",
                    params![db.last_insert_rowid(), path],
                )?;
            }
        }
    }
    Ok(code.unwrap_or(if anomaly.is_some() {
        "parsed_with_source_anomalies"
    } else {
        "accepted"
    }))
}

pub mod register_writer;

pub mod report_writer;

pub mod workflow_writer;

pub mod corpus_bin_writer;
pub mod corpus_config_writer;
pub mod corpus_help_writer;
pub mod corpus_writer;

pub mod diagnostics_writer;

pub mod optional_form_writer;

pub mod sidecar_writer;

pub mod context_writer;

pub mod parameter_writer;

pub mod conditional_writer;

pub mod annotated_vars_writer;
pub mod async_casing_writer;
pub mod dates_members_writer;
pub mod header_forms_writer;
pub mod lexical_tail_writer;
pub mod literal_writer;
pub mod long_headers_writer;
pub mod method_regions_writer;
pub mod mobile_vars_writer;
pub mod navigator_agreement_writer;
pub mod navigator_owners_writer;
pub mod refined_writer;
pub mod source_anomalies_writer;
pub mod source_limits_writer;
pub mod wrapped_writer;

pub mod navigator_command_help_writer;
pub mod navigator_sppr;
pub mod string_writer;
