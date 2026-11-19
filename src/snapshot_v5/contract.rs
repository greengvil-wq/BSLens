//! Общий контракт S3/S4. Вызывающий отвечает за доверенную приватную копию и транзакцию.
use super::{Error, Result};
use rusqlite::{types::ValueRef, Connection};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = include_str!("../../spikes/linux-v5/schema.sql");
pub const PROFILE: &str = "linux-v5-spike-xml221-bsl-r09";
pub const SCOPE: &str = "xml221-r09-all-xml-bsl";
pub const CORE: &str = env!("CARGO_PKG_VERSION");
pub const ADAPTER: &str = "xml221-r09";
pub const LEXICAL: &str = "r09-regions";

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn frame(h: &mut Sha256, tag: u8, bytes: &[u8]) {
    h.update([tag]);
    h.update((bytes.len() as u64).to_be_bytes());
    h.update(bytes);
}
fn table(h: &mut Sha256, db: &Connection, name: &str, sql: &str) -> Result<()> {
    frame(h, b'T', name.as_bytes());
    let mut query = db.prepare(sql)?;
    let count = query.column_count();
    h.update((count as u64).to_be_bytes());
    for name in query.column_names() {
        frame(h, b'F', name.as_bytes());
    }
    let mut rows = query.query([])?;
    while let Some(row) = rows.next()? {
        h.update(b"R");
        for i in 0..count {
            match row.get_ref(i)? {
                ValueRef::Null => h.update(b"N"),
                ValueRef::Integer(n) => {
                    h.update(b"I");
                    h.update(n.to_be_bytes());
                }
                ValueRef::Text(t) => {
                    std::str::from_utf8(t).map_err(|_| Error::Contract("canonical_utf8"))?;
                    frame(h, b'S', t);
                }
                _ => return Err(Error::Contract("canonical_type")),
            }
        }
    }
    h.update(b"E");
    Ok(())
}

/// Хеш логических фактов; ID заменяются стабильными XML/BSL-ключами.
/// Перед вызовом: validate_schema_profile и одна read-транзакция на стороне S4.
pub fn facts_sha256(db: &Connection) -> Result<String> {
    if db
        .prepare("PRAGMA foreign_key_check")?
        .query([])?
        .next()?
        .is_some()
    {
        return Err(Error::Contract("foreign_key"));
    }
    let mut h = Sha256::new();
    h.update(b"BSLens-v5/facts/1\0");
    table(&mut h, db, "objects", "SELECT o.xml_path AS xml_path,p.xml_path AS parent_xml_path,o.kind AS kind,o.name AS name,o.source_kind AS source_kind FROM objects o LEFT JOIN objects p ON p.object_id=o.parent_id ORDER BY o.xml_path COLLATE BINARY")?;
    table(&mut h, db, "modules", "SELECT m.path AS path,m.source_kind AS source_kind,o.xml_path AS owner_xml_path,m.parse_status AS parse_status,m.reason_code AS reason_code FROM modules m LEFT JOIN objects o ON o.object_id=m.owner_id ORDER BY m.path COLLATE BINARY")?;
    table(&mut h, db, "methods", "SELECT module_path,parse_status,kind,name,exported,start_line,end_line,byte_start,byte_end FROM methods ORDER BY module_path COLLATE BINARY,start_line")?;
    table(&mut h, db, "source_lines", "SELECT path,source_kind,line_number,byte_start,byte_end FROM source_lines ORDER BY path COLLATE BINARY,line_number")?;
    Ok(hex(&h.finalize()))
}

/// Входы сборки, без даты, фактов и физического SQLite-файла.
pub fn snapshot_id(db: &Connection) -> Result<String> {
    let raw: String = db.query_row(
        "SELECT build_parameters FROM metadata WHERE singleton=1",
        [],
        |r| r.get(0),
    )?;
    let json: serde_json::Value = serde_json::from_str(&raw)?;
    if !json.is_object() || serde_json::to_string(&json)? != raw {
        return Err(Error::Contract("parameters_not_normalized"));
    }
    let mut h = Sha256::new();
    h.update(b"BSLens-v5/inputs/1\0");
    table(&mut h, db, "inputs", "SELECT source_id,commit_oid,tree_oid,schema_version,core_version,adapter_version,xml_version,lexical_version,profile,scope_id,build_parameters FROM metadata WHERE singleton=1")?;
    Ok(format!("bslens-v5-{}", hex(&h.finalize())))
}

const REGISTRY_SQL: &str =
    "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name COLLATE BINARY";

pub fn validate_schema(connection: &Connection) -> Result<()> {
    if connection.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? != 5 {
        return Err(Error::Contract("user_version"));
    }
    // Only this fixed, compiled-in schema is materialized in Rust.
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(SCHEMA)?;
    let mut expected_query = reference.prepare(REGISTRY_SQL)?;
    let expected = expected_query
        .query_map([], |r| {
            Ok([
                r.get::<_, Option<String>>(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
            ])
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let caps: Vec<usize> = (0..4)
        .map(|i| {
            expected
                .iter()
                .map(|r| r[i].as_ref().map_or(0, String::len))
                .max()
                .unwrap_or(0)
        })
        .collect();
    // No sorting or text values until the complete registry's count and byte lengths
    // have passed. LIMIT includes one overflow row; no untrusted Vec<String> exists.
    let mut lengths = connection.prepare("SELECT length(CAST(type AS BLOB)),length(CAST(name AS BLOB)),length(CAST(tbl_name AS BLOB)),coalesce(length(CAST(sql AS BLOB)),0) FROM sqlite_schema LIMIT ?1")?;
    let mut rows = lengths.query([expected.len() as i64 + 1])?;
    let mut count = 0;
    while let Some(row) = rows.next()? {
        if count == expected.len() {
            return Err(Error::Contract("schema_limit"));
        }
        for (i, cap) in caps.iter().enumerate() {
            let size: i64 = row.get(i)?;
            if size < 0 || size as u64 > *cap as u64 {
                return Err(Error::Contract("schema_limit"));
            }
        }
        count += 1;
    }
    if count != expected.len() {
        return Err(Error::Contract("schema"));
    }
    let mut actual_query = connection.prepare(REGISTRY_SQL)?;
    let mut actual = actual_query.query([])?;
    for expected_row in &expected {
        let row = actual.next()?.ok_or(Error::Contract("schema"))?;
        for (i, expected_value) in expected_row.iter().enumerate() {
            use rusqlite::types::ValueRef;
            match (row.get_ref(i)?, expected_value.as_deref()) {
                (ValueRef::Null, None) => {}
                (ValueRef::Text(bytes), Some(text)) if bytes == text.as_bytes() => {}
                _ => return Err(Error::Contract("schema")),
            }
        }
    }
    if actual.next()?.is_some() {
        return Err(Error::Contract("schema"));
    }
    Ok(())
}

/// Точное определение схемы S2, user_version, единственная metadata и известный профиль.
/// Не открывает путь, не пересчитывает BSL и не создаёт ValidatedSnapshot.
pub fn validate_schema_profile(db: &Connection) -> Result<()> {
    validate_schema(db)?;
    let row: (i64, i64, String, String) = db.query_row(
        "SELECT count(*),schema_version,profile,relations FROM metadata",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    if row != (1, 5, PROFILE.into(), "not_analyzed".into()) {
        return Err(Error::Contract("profile"));
    }
    Ok(())
}

/// Совместимость известного профиля. Другой непустой scope_id сам по себе не отказ.
/// Покрытие остаётся временным JSON и должно быть показано потребителю S4.
pub fn compatible_scope(db: &Connection) -> Result<()> {
    validate_schema_profile(db)?;
    let (core,adapter,xml,lexical,scope,coverage):(String,String,String,String,String,String)=db.query_row(
        "SELECT core_version,adapter_version,xml_version,lexical_version,scope_id,coverage_draft FROM metadata",[],
        |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;
    let coverage: serde_json::Value = serde_json::from_str(&coverage)?;
    if core != CORE
        || adapter != ADAPTER
        || xml != "2.21"
        || lexical != LEXICAL
        || scope.is_empty()
        || coverage.get("temporary") != Some(&serde_json::Value::Bool(true))
    {
        return Err(Error::Contract("incompatible_profile"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn digest(sql: &str) -> String {
        let db = Connection::open_in_memory().unwrap();
        let mut h = Sha256::new();
        table(&mut h, &db, "test", sql).unwrap();
        hex(&h.finalize())
    }
    #[test]
    fn typed_frames_and_row_boundaries_do_not_alias() {
        assert_ne!(digest("SELECT NULL AS value"), digest("SELECT '' AS value"));
        assert_ne!(digest("SELECT 1 AS value"), digest("SELECT '1' AS value"));
        assert_ne!(
            digest("SELECT 'ab' AS a,'c' AS b"),
            digest("SELECT 'a' AS a,'bc' AS b")
        );
        assert_ne!(
            digest("SELECT 'a' AS value UNION ALL SELECT 'b'"),
            digest("SELECT 'ab' AS value")
        );
        assert_ne!(digest("SELECT 'x' AS a"), digest("SELECT 'x' AS b"));
    }
}
