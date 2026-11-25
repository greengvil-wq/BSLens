//! Узкий профиль: пофайловый Git ingest, прежний ограниченный producer фактов R09.
use super::{contract, derive_facts, publish, BuiltSnapshot, Error, Limits, Result};
use crate::git_manifest::ingest;
use rusqlite::{params, Connection};
use serde_json::{json, Value};
use std::{path::Path, time::Duration};

pub const PROFILE: &str = "linux-v5-ingest1-r09-facts";

/// Второй точный DDL: меняется только CHECK имени профиля. Legacy DDL неизменён.
pub(super) fn schema() -> String {
    contract::SCHEMA.replace(contract::PROFILE, PROFILE)
}

/// Не повышает пределы producer XML/BSL. Большим может быть лишь Git inventory.
fn parameters(limits: Limits, git: ingest::Limits) -> Result<Value> {
    if git.deadline != Duration::from_millis(git.deadline.as_millis() as u64) {
        return Err(Error::Contract("ingest_deadline_milliseconds"));
    }
    git.validate()
        .map_err(|_| Error::Contract("ingest_limits"))?;
    for (name, value, ceiling) in [
        (
            "source_count",
            limits.source_count,
            crate::xml221::MAX_FILES as u64,
        ),
        (
            "source_bytes",
            limits.source_bytes,
            ingest::Limits::MAX.object_bytes as u64,
        ),
        (
            "total_source_bytes",
            limits.total_source_bytes,
            crate::xml221::MAX_TOTAL_BYTES as u64,
        ),
        ("sqlite_bytes", limits.sqlite_bytes, 1024 * 1024 * 1024),
    ] {
        if value == 0 {
            return Err(Error::Contract("ingest_limits"));
        }
        Limits::check(name, ceiling, value)?;
    }
    Ok(
        json!({"limits":limits.json(),"selection":"all-lowercase-xml-bsl",
        "git_guard":"bounded-ingest-v1","git_limits":{
            "entries":git.entries,"trees":git.trees,"reads":git.reads,
            "object_bytes":git.object_bytes,"total_bytes":git.total_bytes,
            "deadline_ms":git.deadline.as_millis() as u64},
        "git_fixed":{"depth":64,"path_bytes":1024,"layout_entries":100000,"layout_depth":64,
            "excluded":"inventory-only-unread"},
        "facts_guard":"derive-facts-r09-v1","xml_guard":"r09","lexical_guard":"r09",
        "facts_limits":{"files":crate::xml221::MAX_FILES,"bytes":crate::xml221::MAX_TOTAL_BYTES},
        "xml_limits":{"bytes":crate::xml221::MAX_XML_BYTES,"depth":crate::xml221::MAX_DEPTH,"events":crate::xml221::MAX_EVENTS,"nodes":crate::xml221::MAX_NODES},
        "lexical_limits":{"bytes":crate::lexical::MAX_BYTES,"lines":crate::lexical::MAX_LINES,"tokens":crate::lexical::MAX_TOKENS,"region_depth":crate::lexical::MAX_REGION_DEPTH},
        "coverage":"temporary-r09","relations":"not_analyzed"}),
    )
}

/// Проверяется reader даже без Git. Сохранённые бюджеты не становятся его политикой.
pub(super) fn validate_metadata(db: &Connection) -> Result<()> {
    let (raw, stored): (String, String) = db.query_row(
        "SELECT build_parameters,limits_draft FROM metadata WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let p: Value = serde_json::from_str(&raw)?;
    let number = |v: &Value, k: &str| v[k].as_u64().ok_or(Error::Contract("ingest_parameters"));
    let l = &p["limits"];
    let limits = Limits {
        source_count: number(l, "source_count")?,
        source_bytes: number(l, "source_bytes")?,
        total_source_bytes: number(l, "total_source_bytes")?,
        sqlite_bytes: number(l, "sqlite_bytes")?,
    };
    let g = &p["git_limits"];
    let usize_value = |k| -> Result<usize> {
        usize::try_from(number(g, k)?).map_err(|_| Error::Contract("ingest_parameters"))
    };
    let git = ingest::Limits {
        entries: usize_value("entries")?,
        trees: usize_value("trees")?,
        reads: usize_value("reads")?,
        object_bytes: usize_value("object_bytes")?,
        total_bytes: usize_value("total_bytes")?,
        deadline: Duration::from_millis(number(g, "deadline_ms")?),
    };
    if p != parameters(limits, git)? || raw != p.to_string() || stored != limits.json().to_string()
    {
        return Err(Error::Contract("ingest_parameters"));
    }
    let (count, total, largest): (u64,u64,u64) = db.query_row(
        "SELECT count(*),coalesce(sum(length(content)),0),coalesce(max(length(content)),0) FROM sources",[],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    Limits::check("source_count", limits.source_count, count)?;
    Limits::check("source_bytes", limits.source_bytes, largest)?;
    Limits::check("total_source_bytes", limits.total_source_bytes, total)?;
    Ok(())
}

/// Только явный режим; публикация после Receipt, фактов и полного contract check.
pub fn build(
    repo: &Path,
    commit: &str,
    expected_tree: &str,
    source_id: &str,
    output: &Path,
    limits: Limits,
    git: ingest::Limits,
) -> Result<BuiltSnapshot> {
    if source_id.is_empty() {
        return Err(Error::Contract("source_id"));
    }
    let parameters = parameters(limits, git)?;
    let dir = publish::directory(output)?;
    let temp = publish::Temporary::new(&dir)?;
    let mut db = Connection::open(&temp.path)?;
    db.pragma_update(None, "page_size", 4096)?;
    let max_pages = limits.sqlite_bytes / 4096;
    if max_pages == 0 {
        return Err(Error::Limit {
            name: "sqlite_bytes",
            maximum: limits.sqlite_bytes,
            actual: 4096,
        });
    }
    db.pragma_update(None, "max_page_count", max_pages)?;
    let result = (|| -> Result<(String, String)> {
        db.execute_batch(&schema())?;
        let tx = db.transaction()?;
        let (mut count, mut total) = (0u64, 0u64);
        let mut consumer_error = None;
        let receipt = ingest::ingest(repo, commit, expected_tree, git, |entry, sha, bytes| {
            let result = (|| -> Result<()> {
                count += 1;
                total = total
                    .checked_add(bytes.len() as u64)
                    .ok_or(Error::Contract("source_size_overflow"))?;
                Limits::check("source_count", limits.source_count, count)?;
                Limits::check("source_bytes", limits.source_bytes, bytes.len() as u64)?;
                Limits::check("total_source_bytes", limits.total_source_bytes, total)?;
                tx.execute(
                    "INSERT INTO sources VALUES (?1,?2,?3,?4,?5,?6)",
                    params![
                        entry.path,
                        if entry.path.ends_with(".xml") {
                            "xml"
                        } else {
                            "bsl"
                        },
                        format!("{:o}", entry.mode),
                        entry.oid,
                        sha,
                        bytes
                    ],
                )?;
                Ok(())
            })();
            result.map_err(|e| {
                consumer_error = Some(e);
                "writer_rejected".into()
            })
        });
        if let Some(e) = consumer_error {
            return Err(e);
        }
        let receipt = receipt.map_err(Error::Ingest)?;
        let mut chosen = vec![];
        let mut excluded = vec![];
        for e in receipt.manifest.entries() {
            let entry = json!({"path":e.path,"mode":format!("{:o}",e.mode),"blob_oid":e.oid});
            if e.selected() {
                chosen.push(entry);
            } else {
                excluded.push(json!({"input":entry,"reason":"NotXmlOrBslNotRead"}));
            }
        }
        let coverage = derive_facts(&tx)?;
        let date: String =
            tx.query_row("SELECT strftime('%Y-%m-%dT%H:%M:%SZ','now')", [], |r| {
                r.get(0)
            })?;
        tx.execute("INSERT INTO metadata VALUES (1,5,?1,'bslens-v5-pending',?2,NULL,?3,?4,?5,?6,?7,'2.21',?8,?9,?10,?11,?12,?13,'not_analyzed')",
            params![PROFILE,date,source_id,receipt.manifest.commit(),receipt.manifest.tree(),contract::CORE,contract::ADAPTER,
                contract::LEXICAL,contract::SCOPE,parameters.to_string(),json!({"selected":chosen,"excluded":excluded}).to_string(),
                coverage.to_string(),limits.json().to_string()])?;
        contract::compatible_scope(&tx)?;
        let id = contract::snapshot_id(&tx)?;
        let facts = contract::facts_sha256(&tx)?;
        tx.execute(
            "UPDATE metadata SET snapshot_id=?1,facts_sha256=?2",
            params![id, facts],
        )?;
        tx.commit()?;
        Ok((id, facts))
    })();
    if matches!(&result,Err(Error::Sqlite(rusqlite::Error::SqliteFailure(e,_))) if e.code==rusqlite::ErrorCode::DiskFull)
    {
        return Err(Error::SqliteFull {
            path: temp.path.clone(),
            sqlite_bytes: limits.sqlite_bytes,
            max_pages,
        });
    }
    let (id, facts) = result?;
    db.close().map_err(|(_, e)| Error::Sqlite(e))?;
    temp.ensure_standalone()?;
    Limits::check(
        "sqlite_bytes",
        limits.sqlite_bytes,
        std::fs::metadata(&temp.path)?.len(),
    )?;
    publish::publish(&dir, temp, id, facts, limits.sqlite_bytes)
}
