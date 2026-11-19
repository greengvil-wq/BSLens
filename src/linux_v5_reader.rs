//! Experimental Linux v5 reader. Offline verification does not attest Git provenance.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub use crate::linux_v5_staging::Error as StagingError;
use crate::linux_v5_staging::{self, StagedSnapshot};
use crate::snapshot_v5::{self, contract};
use rusqlite::{Connection, DatabaseName, OpenFlags};
use std::{
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};

#[derive(Debug)]
pub enum Error {
    Staging(StagingError),
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
    SourceDigest,
    VerifyFacts,
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
impl From<snapshot_v5::Error> for Error {
    fn from(e: snapshot_v5::Error) -> Self {
        match e {
            snapshot_v5::Error::Contract("user_version") => Self::Version,
            snapshot_v5::Error::Contract("schema") => Self::Schema,
            snapshot_v5::Error::Contract(
                "schema_count" | "schema_field_budget" | "metadata_field_budget",
            ) => Self::Limit,
            snapshot_v5::Error::Contract("profile" | "incompatible_profile") => Self::Profile,
            other => Self::Contract(other.to_string()),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v5 reader: {self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

pub struct LinuxSnapshotReader {
    // Field drop order is deliberate: close SQLite before unlinking the copy.
    connection: Connection,
    _copy: StagedSnapshot,
}

/// Trusted caller policy; text bytes are UTF-8 payload bytes across all five fields.
/// Row count independently bounds per-node/container overhead. No partial response.
#[derive(Clone, Copy)]
pub struct TreeLimits {
    pub rows: usize,
    pub bytes: usize,
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
    pub fn open(
        path: &Path,
        staging: &Path,
        expected: Option<&str>,
        max_bytes: u64,
    ) -> Result<Self> {
        Self::open_with(path, staging, expected, max_bytes, |copy| {
            Connection::open_with_flags(
                copy,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )
        })
    }

    // Private seam verifies the checksum/header ordering without opening SQLite in tests.
    fn open_with(
        path: &Path,
        staging: &Path,
        expected: Option<&str>,
        max_bytes: u64,
        open_sqlite: impl FnOnce(&Path) -> rusqlite::Result<Connection>,
    ) -> Result<Self> {
        let copy =
            linux_v5_staging::stage(path, staging, expected, max_bytes).map_err(Error::Staging)?;
        copy.validate_standalone().map_err(Error::Staging)?;
        // SQLite receives only a checksum-verified, rollback-mode private copy.
        let connection = open_sqlite(&copy.path)?;
        connection.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")?;
        connection.execute_batch("BEGIN;")?;
        // One shared bounded schema/profile/metadata/compatibility validator from S3.
        contract::compatible_scope(&connection)?;
        let (id, facts): (String, Option<String>) = connection.query_row(
            "SELECT snapshot_id,facts_sha256 FROM metadata WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if id != copy.snapshot_id || contract::snapshot_id(&connection)? != id {
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
        let actual = contract::facts_sha256(&connection)?;
        if facts.as_deref() != Some(actual.as_str()) {
            return Err(Error::Facts);
        }
        Ok(Self {
            connection,
            _copy: copy,
        })
    }

    pub fn source(&self, path: &str, start: i64, end: i64, max_bytes: u64) -> Result<Vec<u8>> {
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
    pub fn tree(
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

    /// Returns scope and temporary coverage; no Git-origin claim.
    pub fn info(&self) -> Result<serde_json::Value> {
        let (id, scope, coverage): (String, String, String) = self.connection.query_row(
            "SELECT snapshot_id,scope_id,coverage_draft FROM metadata WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        Ok(
            serde_json::json!({"snapshot_id":id,"profile":contract::PROFILE,"scope_id":scope,"coverage":serde_json::from_str::<serde_json::Value>(&coverage).map_err(|e|Error::Contract(e.to_string()))?,"relations":"not_analyzed","git_provenance":"not_verified","experimental":true}),
        )
    }

    /// Explicit offline re-derivation. Limits are caller policy, never trusted from metadata.
    pub fn verify(&self, limits: snapshot_v5::Limits) -> Result<()> {
        verify_saved_sources(
            &self.connection,
            self._copy.path.parent().ok_or(Error::Range)?,
            limits,
        )
    }
}

fn verify_saved_sources(
    original: &Connection,
    staging: &Path,
    limits: snapshot_v5::Limits,
) -> Result<()> {
    use sha1::Sha1;
    use sha2::{Digest, Sha256};
    let (count, total): (u64, u64) = original.query_row(
        "SELECT count(*),coalesce(sum(length(content)),0) FROM sources",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if count > limits.source_count || total > limits.total_source_bytes {
        return Err(Error::Limit);
    }
    let (scratch_guard, handle) = linux_v5_staging::temporary(staging).map_err(Error::Staging)?;
    drop(handle);
    // Declaration order ensures transaction/connection close before the RAII unlink.
    let mut scratch =
        Connection::open_with_flags(&scratch_guard.path, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
    let pages = (limits.sqlite_bytes / 4096).min(4_294_967_294);
    if pages == 0 {
        return Err(Error::Limit);
    }
    scratch.pragma_update(None, "page_size", 4096)?;
    scratch.pragma_update(None, "max_page_count", pages)?;
    scratch.execute_batch(contract::SCHEMA)?;
    scratch.execute_batch("PRAGMA trusted_schema=OFF")?;
    let tx = scratch.transaction()?;
    let mut query=original.prepare("SELECT rowid,length(content),length(CAST(path AS BLOB)),length(CAST(kind AS BLOB)),length(CAST(mode AS BLOB)),length(CAST(blob_oid AS BLOB)),length(CAST(sha256 AS BLOB)) FROM sources ORDER BY path COLLATE BINARY")?;
    let mut rows = query.query([])?;
    let mut buffer = [0u8; 64 * 1024];
    while let Some(row) = rows.next()? {
        let rowid: i64 = row.get(0)?;
        let length: u64 = row.get(1)?;
        if length > limits.source_bytes || length > i32::MAX as u64 {
            return Err(Error::Limit);
        }
        for (i, cap) in [(2, 16_384u64), (3, 3), (4, 6), (5, 64), (6, 64)] {
            if row.get::<_, u64>(i)? > cap {
                return Err(Error::Limit);
            }
        }
        let (path, kind, mode, oid, sha): (String, String, String, String, String) = original
            .query_row(
                "SELECT path,kind,mode,blob_oid,sha256 FROM sources WHERE rowid=?1",
                [rowid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;
        if ![40, 64].contains(&oid.len()) {
            return Err(Error::SourceDigest);
        }
        tx.execute("INSERT INTO sources(path,kind,mode,blob_oid,sha256,content) VALUES (?1,?2,?3,?4,?5,zeroblob(?6))",rusqlite::params![path,kind,mode,oid,sha,length])?;
        let mut input =
            original.blob_open(DatabaseName::Main, "sources", "content", rowid, true)?;
        let mut output = tx.blob_open(
            DatabaseName::Main,
            "sources",
            "content",
            tx.last_insert_rowid(),
            false,
        )?;
        let mut content_hash = Sha256::new();
        let mut git_sha1 = Sha1::new();
        let mut git_sha256 = Sha256::new();
        let header = format!("blob {length}\0");
        git_sha1.update(header.as_bytes());
        git_sha256.update(header.as_bytes());
        loop {
            let n = input.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            content_hash.update(&buffer[..n]);
            git_sha1.update(&buffer[..n]);
            git_sha256.update(&buffer[..n]);
            output.write_all(&buffer[..n])?;
        }
        let actual_oid = if oid.len() == 40 {
            format!("{:x}", git_sha1.finalize())
        } else {
            format!("{:x}", git_sha256.finalize())
        };
        if format!("{:x}", content_hash.finalize()) != sha || actual_oid != oid {
            return Err(Error::SourceDigest);
        }
    }
    // Single producer/parser shared with S3. Its own R09 XML limits also apply.
    snapshot_v5::derive_facts(&tx)?;
    if contract::facts_sha256(&tx)? != contract::facts_sha256(original)? {
        return Err(Error::VerifyFacts);
    }
    // No need to publish or commit disposable verification results.
    drop(tx);
    scratch.close().map_err(|(_, e)| Error::Sqlite(e))?;
    drop(scratch_guard);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wal_signature_and_checksum_fail_before_sqlite_without_sidecars() {
        use sha2::{Digest, Sha256};
        use std::{fs, os::unix::fs::PermissionsExt};
        let root = std::env::temp_dir().join(format!("bslens-s4-wal-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let staging = root.join("stage");
        fs::create_dir(&staging).unwrap();
        fs::set_permissions(&staging, fs::Permissions::from_mode(0o700)).unwrap();
        let original = root.join("fixture.sqlite");
        let db = Connection::open(&original).unwrap();
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE example(value); INSERT INTO example VALUES(1); PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        drop(db);
        let wal = fs::read(&original).unwrap();
        assert_eq!(&wal[18..20], &[2, 2]);
        let check = |bytes: &[u8], corrupt: bool| {
            let path = root.join(format!(
                "bslens-v5-{}.{}.sqlite",
                "0".repeat(64),
                format!("{:x}", Sha256::digest(bytes))
            ));
            fs::write(&path, bytes).unwrap();
            if corrupt {
                fs::write(&path, b"bad checksum and bad header").unwrap();
            }
            let mut called = false;
            let result = LinuxSnapshotReader::open_with(&path, &staging, None, 1_000_000, |_| {
                called = true;
                panic!("SQLite must not be called")
            });
            assert!(!called);
            if corrupt {
                assert!(matches!(result, Err(Error::Staging(StagingError::Digest))));
            } else {
                assert!(matches!(
                    result,
                    Err(Error::Staging(StagingError::NotStandalone))
                ));
            }
            assert_eq!(
                fs::read_dir(&staging).unwrap().count(),
                0,
                "no private copies or WAL/SHM/journal sidecars"
            );
        };
        check(&wal, false);
        check(&wal, true);
        let mut bad = wal.clone();
        bad[..16].fill(0);
        bad[18] = 1;
        bad[19] = 1;
        check(&bad, false);
        let mut modes = wal.clone();
        modes[18] = 1;
        check(&modes, false);
        modes[18] = 2;
        modes[19] = 1;
        check(&modes, false);
        check(b"short", false);
        fs::remove_dir_all(root).unwrap();
    }

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
            "bslens-v5-0000000000000000000000000000000000000000000000000000000000000000.{:x}.sqlite",
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
}

#[derive(Debug)]
pub struct TreeNode {
    pub node_type: String,
    pub key: String,
    pub name: String,
    pub status: Option<String>,
    pub reason: Option<String>,
}
