//! S4 reader integration boundary. Kept private until tested against the S3 builder.
#![allow(dead_code)] // Private S4 integration boundary; removed when S3 is connected.
use crate::linux_v5_staging::{self, StagedSnapshot};
use rusqlite::{Connection, DatabaseName, OpenFlags};
use std::{
    io::{Read, Seek, SeekFrom},
    path::Path,
};

#[derive(Debug)]
pub(crate) enum Error {
    Staging(linux_v5_staging::Error),
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    Version,
    Schema,
    Profile,
    Identity,
    Facts,
    ForeignKey,
    Range,
    Limit,
    Contract(String),
}
impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}
impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}
type Result<T> = std::result::Result<T, Error>;

/// S3 owns these implementations. No alternative canonical hash or parser is allowed here.
/// Open-time checks must stream facts and never collect all saved source bytes.
pub(crate) trait SharedFacts {
    fn facts_sha256(&self, connection: &Connection) -> std::result::Result<String, String>;
    fn compatible_scope(&self, connection: &Connection) -> std::result::Result<(), String>;
    /// Recompute sources' SHA, XML ownership, BSL methods and line index from saved bytes.
    /// This does not attest Git provenance.
    fn verify_saved_sources(&self, connection: &Connection) -> std::result::Result<(), String>;
}

pub(crate) struct LinuxSnapshotReader {
    // Field drop order is deliberate: close SQLite before unlinking the copy.
    connection: Connection,
    _copy: StagedSnapshot,
}

const REGISTRY_SQL: &str =
    "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name COLLATE BINARY";

fn check_schema(connection: &Connection) -> Result<()> {
    if connection.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? != 5 {
        return Err(Error::Version);
    }
    // Only this fixed, compiled-in schema is materialized in Rust.
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(include_str!("../spikes/linux-v5/schema.sql"))?;
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
            return Err(Error::Limit);
        }
        for (i, cap) in caps.iter().enumerate() {
            let size: i64 = row.get(i)?;
            if size < 0 || size as u64 > *cap as u64 {
                return Err(Error::Limit);
            }
        }
        count += 1;
    }
    if count != expected.len() {
        return Err(Error::Schema);
    }
    let mut actual_query = connection.prepare(REGISTRY_SQL)?;
    let mut actual = actual_query.query([])?;
    for expected_row in &expected {
        let row = actual.next()?.ok_or(Error::Schema)?;
        for (i, expected_value) in expected_row.iter().enumerate() {
            use rusqlite::types::ValueRef;
            match (row.get_ref(i)?, expected_value.as_deref()) {
                (ValueRef::Null, None) => {}
                (ValueRef::Text(bytes), Some(text)) if bytes == text.as_bytes() => {}
                _ => return Err(Error::Schema),
            }
        }
    }
    if actual.next()?.is_some() {
        return Err(Error::Schema);
    }
    Ok(())
}

/// Trusted caller policy; text bytes are UTF-8 payload bytes across all five fields.
/// Row count independently bounds per-node/container overhead. No partial response.
#[derive(Clone, Copy)]
pub(crate) struct TreeLimits {
    pub(crate) rows: usize,
    pub(crate) bytes: usize,
}

fn read_tree(
    connection: &Connection,
    level: &str,
    parent_id: Option<i64>,
    kind: Option<&str>,
    module_path: Option<&str>,
    limits: TreeLimits,
) -> Result<Vec<TreeNode>> {
    if !["configuration", "kind", "object", "unconfirmed", "module"].contains(&level) {
        return Err(Error::Range);
    }
    let sql = include_str!("../spikes/linux-v5/branch.sql");
    let (cte, _) = sql
        .split_once("SELECT node_type,node_key,name,status,reason_code FROM branch")
        .ok_or(Error::Schema)?;
    let preflight = format!("{cte} SELECT length(CAST(node_type AS BLOB)),length(CAST(node_key AS BLOB)),length(CAST(name AS BLOB)),coalesce(length(CAST(status AS BLOB)),0),coalesce(length(CAST(reason_code AS BLOB)),0) FROM branch LIMIT :row_limit");
    let row_limit = i64::try_from(limits.rows)
        .ok()
        .and_then(|n| n.checked_add(1))
        .ok_or(Error::Limit)?;
    let mut query = connection.prepare(&preflight)?;
    let mut rows = query.query(rusqlite::named_params! {":level":level, ":parent_id":parent_id, ":kind":kind, ":module_path":module_path, ":row_limit":row_limit})?;
    let mut count = 0usize;
    let mut bytes = 0usize;
    while let Some(row) = rows.next()? {
        if count == limits.rows {
            return Err(Error::Limit);
        }
        for i in 0..5 {
            let size = usize::try_from(row.get::<_, i64>(i)?).map_err(|_| Error::Limit)?;
            bytes = bytes
                .checked_add(size)
                .filter(|n| *n <= limits.bytes)
                .ok_or(Error::Limit)?;
        }
        count += 1;
    }
    // The immutable private copy cannot change between preflight and retrieval.
    // ORDER BY and Rust strings only see a branch already bounded in rows and bytes.
    let mut query = connection.prepare(sql)?;
    let mut rows = query.query(rusqlite::named_params! {":level":level, ":parent_id":parent_id, ":kind":kind, ":module_path":module_path})?;
    let mut nodes = Vec::new();
    let mut remaining = limits.bytes;
    while let Some(row) = rows.next()? {
        if nodes.len() == limits.rows {
            return Err(Error::Limit);
        }
        // Recheck borrowed bytes before any String allocation.
        for i in 0..5 {
            use rusqlite::types::ValueRef;
            let size = match row.get_ref(i)? {
                ValueRef::Text(v) => v.len(),
                ValueRef::Null => 0,
                _ => return Err(Error::Schema),
            };
            remaining = remaining.checked_sub(size).ok_or(Error::Limit)?;
        }
        nodes.push(TreeNode {
            node_type: row.get(0)?,
            key: row.get(1)?,
            name: row.get(2)?,
            status: row.get(3)?,
            reason: row.get(4)?,
        });
    }
    Ok(nodes)
}
impl LinuxSnapshotReader {
    pub(crate) fn open(
        path: &Path,
        staging: &Path,
        expected: Option<&str>,
        max_bytes: u64,
        shared: &impl SharedFacts,
    ) -> Result<Self> {
        let copy =
            linux_v5_staging::stage(path, staging, expected, max_bytes).map_err(Error::Staging)?;
        // The only filesystem SQLite path is a successfully hashed private copy.
        let connection = Connection::open_with_flags(
            &copy.path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")?;
        check_schema(&connection)?;
        let (version,profile,id,facts):(i64,String,String,Option<String>)=connection.query_row(
            "SELECT schema_version,profile,snapshot_id,facts_sha256 FROM metadata WHERE singleton=1",[],
            |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?;
        if version != 5 {
            return Err(Error::Version);
        }
        if profile != "linux-v5-spike-xml221-bsl-r09" {
            return Err(Error::Profile);
        }
        if id != copy.snapshot_id {
            return Err(Error::Identity);
        }
        if connection
            .prepare("PRAGMA foreign_key_check")?
            .query([])?
            .next()?
            .is_some()
        {
            return Err(Error::ForeignKey);
        }
        let actual = shared.facts_sha256(&connection).map_err(Error::Contract)?;
        if facts.as_deref() != Some(actual.as_str()) {
            return Err(Error::Facts);
        }
        shared
            .compatible_scope(&connection)
            .map_err(Error::Contract)?;
        Ok(Self {
            connection,
            _copy: copy,
        })
    }

    pub(crate) fn source(
        &self,
        path: &str,
        start: i64,
        end: i64,
        max_bytes: u64,
    ) -> Result<Vec<u8>> {
        if start < 1 || end < start {
            return Err(Error::Range);
        }
        let (rowid, first, last): (i64, i64, i64) = self.connection.query_row(
            include_str!("../spikes/linux-v5/range.sql"),
            rusqlite::named_params! {":path":path,":start_line":start,":end_line":end},
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        if first < 0 || last < first {
            return Err(Error::Range);
        }
        let count = u64::try_from(last - first).map_err(|_| Error::Range)?;
        if count > max_bytes {
            return Err(Error::Range);
        }
        let mut blob =
            self.connection
                .blob_open(DatabaseName::Main, "sources", "content", rowid, true)?;
        if last > blob.len() as i64 {
            return Err(Error::Range);
        }
        blob.seek(SeekFrom::Start(first as u64))?;
        let mut bytes = vec![0; usize::try_from(count).map_err(|_| Error::Range)?];
        blob.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    /// One SQL branch, including parse status and rejection reason; no BSL parsing.
    pub(crate) fn tree(
        &self,
        level: &str,
        parent_id: Option<i64>,
        kind: Option<&str>,
        module_path: Option<&str>,
        limits: TreeLimits,
    ) -> Result<Vec<TreeNode>> {
        read_tree(
            &self.connection,
            level,
            parent_id,
            kind,
            module_path,
            limits,
        )
    }

    pub(crate) fn verify(&self, shared: &impl SharedFacts) -> Result<()> {
        shared
            .verify_saved_sources(&self.connection)
            .map_err(Error::Contract)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_and_tree_use_saved_unparsed_bytes_without_parser() {
        use sha2::{Digest, Sha256};
        use std::{fs, os::unix::fs::PermissionsExt};
        let root = std::env::temp_dir().join(format!("bslens-s4-reader-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let raw = root.join("build.sqlite");
        let db = Connection::open(&raw).unwrap();
        db.execute_batch(include_str!("../spikes/linux-v5/schema.sql"))
            .unwrap();
        let bytes = b"#unsupported\r\nsecond\nlast";
        db.execute(
            "INSERT INTO sources VALUES('x.bsl','bsl','100644',?1,?2,?3)",
            rusqlite::params![
                "0".repeat(40),
                format!("{:x}", Sha256::digest(bytes)),
                bytes
            ],
        )
        .unwrap();
        db.execute_batch("INSERT INTO modules VALUES('x.bsl','bsl',NULL,'unparsed','DirectiveUnsupported');
          INSERT INTO source_lines VALUES('x.bsl','bsl',1,0,14),('x.bsl','bsl',2,14,21),('x.bsl','bsl',3,21,25);").unwrap();
        // This explicitly constructed fixture is not an S3 integration test.
        // Derive physical offsets for the fixture, independently of any product parser.
        let first = bytes.iter().position(|b| *b == b'\n').unwrap() + 1;
        let second = bytes[first..].iter().position(|b| *b == b'\n').unwrap() + 1 + first;
        db.execute(
            "UPDATE source_lines SET byte_end=?1 WHERE line_number=1",
            [first as i64],
        )
        .unwrap();
        db.execute(
            "UPDATE source_lines SET byte_start=?1,byte_end=?2 WHERE line_number=2",
            [first as i64, second as i64],
        )
        .unwrap();
        db.execute(
            "UPDATE source_lines SET byte_start=?1,byte_end=?2 WHERE line_number=3",
            [second as i64, bytes.len() as i64],
        )
        .unwrap();
        drop(db);
        let named = root.join(format!(
            "bslens-v5-test.{:x}.sqlite",
            Sha256::digest(fs::read(&raw).unwrap())
        ));
        fs::rename(&raw, &named).unwrap();
        let copy = linux_v5_staging::stage(&named, &root, None, 1_000_000).unwrap();
        let copy_path = copy.path.clone();
        let connection =
            Connection::open_with_flags(&copy.path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        connection
            .execute_batch("PRAGMA query_only=ON;PRAGMA trusted_schema=OFF")
            .unwrap();
        let reader = LinuxSnapshotReader {
            connection,
            _copy: copy,
        };
        assert_eq!(reader.source("x.bsl", 2, 3, 100).unwrap(), b"second\nlast");
        assert!(matches!(reader.source("x.bsl", 2, 3, 2), Err(Error::Range)));
        let nodes = reader
            .tree(
                "unconfirmed",
                None,
                None,
                None,
                TreeLimits {
                    rows: 10,
                    bytes: 1024,
                },
            )
            .unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].status.as_deref(), Some("unparsed"));
        drop(reader);
        assert!(!copy_path.exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn registry_rejects_excess_count_and_giant_sql() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../spikes/linux-v5/schema.sql"))
            .unwrap();
        for i in 0..100 {
            db.execute_batch(&format!("CREATE VIEW extra_{i} AS SELECT 1"))
                .unwrap();
        }
        assert!(matches!(check_schema(&db), Err(Error::Limit)));
        for i in 0..100 {
            db.execute_batch(&format!("DROP VIEW extra_{i}")).unwrap();
        }
        check_schema(&db).unwrap();
        // Keep the original row count, so the length check must reject this entry.
        db.execute_batch("DROP INDEX method_branch").unwrap();
        db.execute_batch(&format!(
            "CREATE VIEW extra AS SELECT '{}'",
            "x".repeat(1_000_000)
        ))
        .unwrap();
        assert!(matches!(check_schema(&db), Err(Error::Limit)));
    }

    #[test]
    fn tree_row_and_utf8_byte_limits_are_exact_and_reject_giant_field() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../spikes/linux-v5/schema.sql"))
            .unwrap();
        // Isolate branch reader from S3 and FK validation; no production fixture claim.
        db.execute_batch("PRAGMA foreign_keys=OFF").unwrap();
        db.execute(
            "INSERT INTO modules(path,parse_status) VALUES (?1,'accepted')",
            ["я.bsl"],
        )
        .unwrap();
        let exact = "module".len() + 2 * "я.bsl".len() + "accepted".len();
        let fetch = |rows, bytes| {
            read_tree(
                &db,
                "unconfirmed",
                None,
                None,
                None,
                TreeLimits { rows, bytes },
            )
        };
        assert_eq!(fetch(1, exact).unwrap().len(), 1);
        assert!(matches!(fetch(1, exact - 1), Err(Error::Limit)));
        assert!(matches!(fetch(0, exact), Err(Error::Limit)));
        for i in 0..100 {
            db.execute(
                "INSERT INTO modules(path,parse_status) VALUES (?1,'accepted')",
                [format!("{i}.bsl")],
            )
            .unwrap();
        }
        assert!(matches!(fetch(10, 1_000_000), Err(Error::Limit)));
        db.execute("DELETE FROM modules", []).unwrap();
        db.execute(
            "INSERT INTO modules(path,parse_status) VALUES (?1,'accepted')",
            ["x".repeat(1_000_000)],
        )
        .unwrap();
        assert!(matches!(fetch(1, 1024), Err(Error::Limit)));
    }

    #[test]
    fn exact_schema_and_v4_rejection() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../spikes/linux-v5/schema.sql"))
            .unwrap();
        check_schema(&db).unwrap();
        db.execute_batch("CREATE VIEW extra AS SELECT 1").unwrap();
        assert!(matches!(check_schema(&db), Err(Error::Limit)));
        db.execute_batch("DROP VIEW extra; PRAGMA user_version=4")
            .unwrap();
        assert!(matches!(check_schema(&db), Err(Error::Version)));
    }
}

#[derive(Debug)]
pub(crate) struct TreeNode {
    pub(crate) node_type: String,
    pub(crate) key: String,
    pub(crate) name: String,
    pub(crate) status: Option<String>,
    pub(crate) reason: Option<String>,
}
