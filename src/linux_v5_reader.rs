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

/// S3 owns both implementations. No alternative canonical hash or parser is allowed here.
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

fn registry(connection: &Connection) -> Result<Vec<(String, String, String, Option<String>)>> {
    let mut query = connection.prepare(
        "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name COLLATE BINARY",
    )?;
    let rows = query.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
fn check_schema(connection: &Connection) -> Result<()> {
    if connection.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))? != 5 {
        return Err(Error::Version);
    }
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(include_str!("../spikes/linux-v5/schema.sql"))?;
    if registry(connection)? != registry(&reference)? {
        return Err(Error::Schema);
    }
    Ok(())
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
    ) -> Result<Vec<TreeNode>> {
        if !["configuration", "kind", "object", "unconfirmed", "module"].contains(&level) {
            return Err(Error::Range);
        }
        let mut query = self
            .connection
            .prepare(include_str!("../spikes/linux-v5/branch.sql"))?;
        let rows = query.query_map(rusqlite::named_params! {":level":level, ":parent_id":parent_id, ":kind":kind, ":module_path":module_path}, |r| Ok(TreeNode {
            node_type:r.get(0)?, key:r.get(1)?, name:r.get(2)?, status:r.get(3)?, reason:r.get(4)?
        }))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
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
        let nodes = reader.tree("unconfirmed", None, None, None).unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].status.as_deref(), Some("unparsed"));
        drop(reader);
        assert!(!copy_path.exists());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn exact_schema_and_v4_rejection() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch(include_str!("../spikes/linux-v5/schema.sql"))
            .unwrap();
        check_schema(&db).unwrap();
        db.execute_batch("CREATE VIEW extra AS SELECT 1").unwrap();
        assert!(matches!(check_schema(&db), Err(Error::Schema)));
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
