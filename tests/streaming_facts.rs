#![cfg(all(
    feature = "experimental-streaming-facts",
    target_os = "linux",
    target_arch = "x86_64"
))]
use bslens_core::{
    snapshot_v5::{
        self, contract,
        streaming_facts::{self, Limits},
    },
    xml221,
};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    path: PathBuf,
    db: Option<Connection>,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bslens-stream-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let db = Connection::open(&path).unwrap();
        db.execute_batch(contract::SCHEMA).unwrap();
        let f = Self { path, db: Some(db) };
        for (p, b) in [
            (
                "Configuration.xml",
                include_bytes!("fixtures/xml221/Configuration.xml").as_slice(),
            ),
            (
                "Catalogs/Items.xml",
                include_bytes!("fixtures/xml221/Catalogs/Items.xml").as_slice(),
            ),
            (
                "Documents/Order.xml",
                include_bytes!("fixtures/xml221/Documents/Order.xml").as_slice(),
            ),
            (
                "InformationRegisters/Events.xml",
                include_bytes!("fixtures/xml221/InformationRegisters/Events.xml").as_slice(),
            ),
        ] {
            f.insert(p, b);
        }
        f
    }
    fn insert(&self, path: &str, bytes: &[u8]) {
        let mut h = sha1::Sha1::new();
        h.update(format!("blob {}\0", bytes.len()));
        h.update(bytes);
        self.db
            .as_ref()
            .unwrap()
            .execute(
                "INSERT INTO sources VALUES (?1,?2,'100644',?3,?4,?5)",
                params![
                    path,
                    if path.ends_with(".xml") { "xml" } else { "bsl" },
                    format!("{:x}", h.finalize()),
                    format!("{:x}", Sha256::digest(bytes)),
                    bytes
                ],
            )
            .unwrap();
    }
    fn reopen(&mut self) {
        self.db.take().unwrap().close().unwrap();
        self.db = Some(Connection::open(&self.path).unwrap());
    }
    fn db(&mut self) -> &mut Connection {
        self.db.as_mut().unwrap()
    }
    fn clean(&mut self) {
        for table in ["objects", "modules", "methods", "source_lines"] {
            assert_eq!(
                self.db()
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                        .get::<_, u64>(0))
                    .unwrap(),
                0
            );
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.db.take();
        let _ = fs::remove_file(&self.path);
    }
}
fn limits() -> Limits {
    Limits {
        sources: snapshot_v5::Limits {
            source_count: 10000,
            source_bytes: 8 * 1024 * 1024,
            total_source_bytes: 64 * 1024 * 1024,
            sqlite_bytes: 128 * 1024 * 1024,
        },
        facts: 100000,
        coverage_bytes: 1024 * 1024,
        time: Duration::from_secs(30),
    }
}
const TEXT: &[u8] = "\u{feff}// Ω\r\nПроцедура Я() Экспорт\r\nКонецПроцедуры".as_bytes();
#[test]
fn parity_same_parser_owners_lines_coverage_and_canonical_facts() {
    let mut old = Fixture::new();
    let mut new = Fixture::new();
    for f in [&old, &new] {
        for (path, bytes) in [
            ("Ext/SessionModule.bsl", TEXT),
            ("Catalogs/Items/Ext/ObjectModule.bsl", TEXT),
            ("Unknown.bsl", b"\xff\r\nlast".as_slice()),
            ("Zero.bsl", b"// zero".as_slice()),
        ] {
            f.insert(path, bytes);
        }
    }
    old.reopen();
    new.reopen();
    let a = snapshot_v5::derive_facts(old.db()).unwrap();
    let b = streaming_facts::derive(new.db(), limits()).unwrap();
    assert_eq!(a, b);
    assert_eq!(
        contract::facts_sha256(old.db()).unwrap(),
        contract::facts_sha256(new.db()).unwrap()
    );
    let end: u64 = new
        .db()
        .query_row(
            "SELECT max(byte_end) FROM source_lines WHERE path='Ext/SessionModule.bsl'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(end, TEXT.len() as u64);
    assert_eq!(
        new.db()
            .query_row(
                "SELECT owner_id FROM modules WHERE path='Ext/SessionModule.bsl'",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        1
    );
}
#[test]
fn selected_count_above_legacy_and_exact_input_boundary() {
    let mut f = Fixture::new();
    for i in 0..4200 {
        f.insert(&format!("U{i:04}.bsl"), b"");
    }
    f.reopen();
    assert!(matches!(
        snapshot_v5::derive_facts(f.db()),
        Err(snapshot_v5::Error::Limit {
            name: "xml221_r09_files",
            ..
        })
    ));
    let mut l = limits();
    l.sources.source_count = 4203;
    assert!(streaming_facts::derive(f.db(), l).is_err());
    f.clean();
    l.sources.source_count = 4204;
    let coverage = streaming_facts::derive(f.db(), l).unwrap();
    assert_eq!(coverage["modules"], 4200);
    assert_eq!(coverage["parse_counts"]["accepted"], 4200);
}
#[test]
fn selected_bytes_above_legacy_and_independent_per_file_boundary() {
    let mut f = Fixture::new();
    let bytes = vec![b'x'; 4 * 1024 * 1024];
    for i in 0..9 {
        f.insert(&format!("U{i}.bsl"), &bytes);
    }
    drop(bytes);
    f.reopen();
    assert!(matches!(
        snapshot_v5::derive_facts(f.db()),
        Err(snapshot_v5::Error::Limit {
            name: "xml221_r09_total_bytes",
            ..
        })
    ));
    let total: u64 = f
        .db()
        .query_row("SELECT sum(length(content)) FROM sources", [], |r| r.get(0))
        .unwrap();
    let mut l = limits();
    l.sources.total_source_bytes = total - 1;
    assert!(streaming_facts::derive(f.db(), l).is_err());
    f.clean();
    l.sources.total_source_bytes = total;
    l.sources.source_bytes = 4 * 1024 * 1024 - 1;
    assert!(streaming_facts::derive(f.db(), l).is_err());
    f.clean();
    l.sources.source_bytes = 4 * 1024 * 1024;
    let coverage = streaming_facts::derive(f.db(), l).unwrap();
    assert_eq!(coverage["parse_counts"]["Limit"], 9);
    assert_eq!(
        f.db()
            .query_row(
                "SELECT count(*) FROM source_lines WHERE byte_start=0 AND byte_end=4194304",
                [],
                |r| r.get::<_, u64>(0)
            )
            .unwrap(),
        9
    );
}
#[test]
fn late_fact_budget_and_sqlite_full_rollback_preserve_sources() {
    let mut f = Fixture::new();
    for i in 0..4200 {
        f.insert(&format!("U{i:04}.bsl"), b"//x\n");
    }
    f.reopen();
    let mut l = limits();
    l.facts = 100;
    assert!(matches!(
        streaming_facts::derive(f.db(), l),
        Err(snapshot_v5::Error::Limit { name: "facts", .. })
    ));
    f.clean();
    let pages: u64 = f
        .db()
        .query_row("PRAGMA page_count", [], |r| r.get(0))
        .unwrap();
    l = limits();
    l.sources.sqlite_bytes = (pages + 1) * 4096;
    assert!(matches!(
        streaming_facts::derive(f.db(), l),
        Err(snapshot_v5::Error::Sqlite(_))
    ));
    f.clean();
    assert_eq!(
        f.db()
            .query_row("SELECT count(*) FROM sources", [], |r| r.get::<_, u64>(0))
            .unwrap(),
        4204
    );
}
#[test]
fn strict_nested_unsupported_and_deadline_rollback() {
    let mut f = Fixture::new();
    let raw = String::from_utf8(include_bytes!("fixtures/xml221/Documents/Order.xml").to_vec())
        .unwrap()
        .replace(
            "<ChildObjects></ChildObjects>",
            "<ChildObjects><Attribute>A</Attribute></ChildObjects>",
        );
    f.db()
        .execute(
            "UPDATE sources SET content=?1 WHERE path='Documents/Order.xml'",
            [raw.as_bytes()],
        )
        .unwrap();
    assert!(matches!(
        streaming_facts::derive(f.db(), limits()),
        Err(snapshot_v5::Error::Xml(xml221::XmlError {
            code: xml221::Code::NestedUnsupported,
            ..
        }))
    ));
    f.clean();
    let mut l = limits();
    l.time = Duration::from_nanos(1);
    assert!(matches!(
        streaming_facts::derive(f.db(), l),
        Err(snapshot_v5::Error::Contract("stream_deadline"))
    ));
    f.clean();
}

#[test]
fn cross_file_uuid_missing_orphan_and_coverage_are_fail_closed() {
    for case in ["duplicate", "missing", "orphan", "coverage"] {
        let mut f = Fixture::new();
        let mut l = limits();
        match case {
            "duplicate" => {
                let bytes = String::from_utf8(
                    include_bytes!("fixtures/xml221/Catalogs/Items.xml").to_vec(),
                )
                .unwrap()
                .replace(
                    "00000000-0000-0000-0000-000000000002",
                    "00000000-0000-0000-0000-000000000001",
                );
                f.db()
                    .execute(
                        "UPDATE sources SET content=?1 WHERE path='Catalogs/Items.xml'",
                        [bytes.as_bytes()],
                    )
                    .unwrap();
            }
            "missing" => {
                f.db()
                    .execute("DELETE FROM sources WHERE path='Catalogs/Items.xml'", [])
                    .unwrap();
                f.insert("Orphan.xml", b"invalid orphan");
            }
            "orphan" => f.insert("Orphan.xml", b"invalid orphan"),
            "coverage" => l.coverage_bytes = 1,
            _ => unreachable!(),
        }
        assert!(streaming_facts::derive(f.db(), l).is_err(), "{case}");
        f.clean();
    }
}
#[test]
fn exact_fact_boundary_and_no_outer_transaction() {
    let mut old = Fixture::new();
    old.insert("Root.bsl", TEXT);
    let coverage = snapshot_v5::derive_facts(old.db()).unwrap();
    let mut needed = coverage["xml_unverified"].as_array().unwrap().len() as u64;
    for t in ["objects", "modules", "methods", "source_lines"] {
        needed += old
            .db()
            .query_row(&format!("SELECT count(*) FROM {t}"), [], |r| {
                r.get::<_, u64>(0)
            })
            .unwrap();
    }
    let mut f = Fixture::new();
    f.insert("Root.bsl", TEXT);
    let mut l = limits();
    l.facts = needed - 1;
    assert!(streaming_facts::derive(f.db(), l).is_err());
    f.clean();
    f.db().execute_batch("BEGIN").unwrap();
    assert!(matches!(
        streaming_facts::derive(f.db(), limits()),
        Err(snapshot_v5::Error::Contract(
            "stream_requires_committed_sources"
        ))
    ));
    f.db().execute_batch("ROLLBACK").unwrap();
    l.facts = needed;
    assert_eq!(streaming_facts::derive(f.db(), l).unwrap(), coverage);
}
