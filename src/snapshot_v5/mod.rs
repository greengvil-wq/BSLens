//! Экспериментальный сборщик Linux v5. Не читатель и не production capability.
pub mod contract;
mod publish;
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
        let positions = lines(file.bytes());
        for (i, (a, b)) in positions.iter().enumerate() {
            db.execute("INSERT INTO source_lines(path,line_number,byte_start,byte_end) VALUES (?1,?2,?3,?4)", params![file.path,i as i64+1,*a as i64,*b as i64])?;
        }
        let analysis = lexical::analyze(file.bytes());
        let (status, code) = match &analysis {
            Ok(_) => ("accepted", None),
            Err(e) => ("unparsed", Some(reason(e.reason))),
        };
        *statuses.entry(code.unwrap_or("accepted")).or_insert(0usize) += 1;
        db.execute(
            "INSERT INTO modules(path,owner_id,parse_status,reason_code) VALUES (?1,?2,?3,?4)",
            params![file.path, owner, status, code],
        )?;
        if let Ok(analysis) = analysis {
            for fact in analysis.boundaries {
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
                // Уже принятый R09 заголовок: последний токен — Экспорт/Export либо ')'.
                let header = std::str::from_utf8(
                    &file.bytes()[fact.header_start_byte..fact.header_end_byte],
                )
                .map_err(|_| Error::Contract("header_utf8"))?;
                let exported = header.to_lowercase().ends_with("экспорт")
                    || header.to_lowercase().ends_with("export");
                db.execute("INSERT INTO methods(module_path,kind,name,exported,start_line,end_line,byte_start,byte_end) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",params![file.path,fact.kind,fact.name,exported,fact.start_line as i64,fact.end_line as i64,fact.start_byte as i64,fact.end_byte as i64])?;
            }
        }
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
