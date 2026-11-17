#![cfg(feature = "synthetic-dev")]
use bslens_core::synthetic::SyntheticSnapshot;
use rusqlite::{params, Connection};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    path: PathBuf,
    golden: Value,
}
impl Fixture {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "bslens-r01-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("fixture.sqlite");
        let golden: Value = serde_json::from_str(include_str!("fixtures/schema4.json")).unwrap();
        let db = Connection::open(&path).unwrap();
        db.execute_batch("PRAGMA user_version=4;
          CREATE TABLE metadata(key TEXT PRIMARY KEY,value TEXT NOT NULL);
          CREATE TABLE sources(path TEXT PRIMARY KEY,oid TEXT NOT NULL,sha256 TEXT NOT NULL,content BLOB NOT NULL);
          CREATE TABLE positions(path TEXT NOT NULL,line INTEGER NOT NULL,start_byte INTEGER NOT NULL,end_byte INTEGER NOT NULL,PRIMARY KEY(path,line),FOREIGN KEY(path) REFERENCES sources(path));
          CREATE TABLE bsl_facts(path TEXT NOT NULL,section TEXT NOT NULL,ordinal INTEGER NOT NULL,fact TEXT NOT NULL,PRIMARY KEY(path,section,ordinal),FOREIGN KEY(path) REFERENCES sources(path));
          CREATE TABLE bsl_navigation(path TEXT NOT NULL,section TEXT NOT NULL,ordinal INTEGER NOT NULL,fact TEXT NOT NULL,PRIMARY KEY(path,section,ordinal),FOREIGN KEY(path) REFERENCES sources(path));").unwrap();
        for (key, value) in golden["metadata"].as_object().unwrap() {
            db.execute(
                "INSERT INTO metadata VALUES (?,?)",
                params![key, value.to_string()],
            )
            .unwrap();
        }
        for row in golden["tables"]["sources"].as_array().unwrap() {
            let text = row[3].as_str().unwrap();
            let raw: Vec<u8> = (0..text.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
                .collect();
            db.execute(
                "INSERT INTO sources VALUES (?,?,?,?)",
                params![row[0].as_str(), row[1].as_str(), row[2].as_str(), raw],
            )
            .unwrap();
        }
        for row in golden["tables"]["positions"].as_array().unwrap() {
            db.execute(
                "INSERT INTO positions VALUES (?,?,?,?)",
                params![
                    row[0].as_str(),
                    row[1].as_i64(),
                    row[2].as_i64(),
                    row[3].as_i64()
                ],
            )
            .unwrap();
        }
        for table in ["bsl_facts", "bsl_navigation"] {
            for row in golden["tables"][table].as_array().unwrap() {
                db.execute(
                    &format!("INSERT INTO {table} VALUES (?,?,?,?)"),
                    params![
                        row[0].as_str(),
                        row[1].as_str(),
                        row[2].as_i64(),
                        row[3].as_str()
                    ],
                )
                .unwrap();
            }
        }
        drop(db);
        Self { path, golden }
    }
    fn id(&self) -> &str {
        self.golden["metadata"]["snapshot_id"].as_str().unwrap()
    }
    fn sql(&self, sql: &str) {
        Connection::open(&self.path)
            .unwrap()
            .execute_batch(&format!("PRAGMA foreign_keys=OFF; {sql}"))
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.path.parent().unwrap());
    }
}

#[test]
fn python_oracle_exact_metadata_source_bom_crlf_unicode_and_readonly() {
    let f = Fixture::new();
    let before = std::fs::read(&f.path).unwrap();
    let s = SyntheticSnapshot::open(&f.path, f.id()).unwrap();
    assert_eq!(s.metadata(), f.golden["metadata"]);
    assert_eq!(
        s.source(f.id(), "Пример.bsl", 1, 4).unwrap(),
        f.golden["expected_source"]
    );
    assert_eq!(
        s.source(f.id(), "Пример.bsl", 1, 1).unwrap()["text"],
        "\u{feff}// 😀\r\n"
    );
    assert_eq!(std::fs::read(&f.path).unwrap(), before);
    assert_eq!(
        std::fs::read_dir(f.path.parent().unwrap()).unwrap().count(),
        1
    );
}
#[test]
fn invalid_schema_identity_content_positions_and_corruption_fail_closed() {
    for sql in [
        "PRAGMA user_version=5",
        "UPDATE metadata SET value='5' WHERE key='schema_version'",
        "UPDATE sources SET sha256='bad'",
        "UPDATE sources SET oid='bad'",
        "UPDATE positions SET end_byte=end_byte+1",
        "UPDATE metadata SET value='\"changed\"' WHERE key='source_id'",
        "UPDATE metadata SET value='\"bad\"' WHERE key='commit'",
        "DELETE FROM sources",
        "UPDATE metadata SET value='\"unsupported\"' WHERE key='profile'",
        "UPDATE metadata SET value='[]' WHERE key='inventory'",
    ] {
        let f = Fixture::new();
        f.sql(sql);
        assert!(SyntheticSnapshot::open(&f.path, f.id()).is_err(), "{sql}");
    }
    let f = Fixture::new();
    std::fs::write(&f.path, b"not sqlite").unwrap();
    assert!(SyntheticSnapshot::open(&f.path, f.id()).is_err());
}
#[test]
fn mismatch_ranges_paths_and_empty_sources() {
    let f = Fixture::new();
    assert!(SyntheticSnapshot::open(&f.path, "wrong").is_err());
    let s = SyntheticSnapshot::open(&f.path, f.id()).unwrap();
    assert!(s.source("wrong", "Пример.bsl", 1, 1).is_err());
    for p in [
        "../Пример.bsl",
        "/Пример.bsl",
        "C:/bad",
        "Unknown.bsl",
        "Empty.bsl",
    ] {
        assert!(s.source(f.id(), p, 1, 1).is_err());
    }
    for (a, b) in [(0, 1), (2, 1), (1, 101), (1, u64::MAX), (5, 5)] {
        assert!(s.source(f.id(), "Пример.bsl", a, b).is_err());
    }
}
#[test]
fn cli_labels_partial_output_and_default_production_refuses() {
    let f = Fixture::new();
    let bin = env!("CARGO_BIN_EXE_bslens-core");
    let out = std::process::Command::new(bin)
        .args([
            "synthetic-dev",
            f.path.to_str().unwrap(),
            f.id(),
            "metadata",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["validation"], "synthetic-partial-NOT-PRODUCTION");
    assert_eq!(value["data"], f.golden["metadata"]);
    let out = std::process::Command::new(bin)
        .args(["metadata", f.path.to_str().unwrap(), f.id()])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
}
#[cfg(unix)]
#[test]
fn symlinks_are_rejected() {
    let f = Fixture::new();
    let link = f.path.with_file_name("link.sqlite");
    std::os::unix::fs::symlink(&f.path, &link).unwrap();
    assert!(SyntheticSnapshot::open(&link, f.id()).is_err());
}
