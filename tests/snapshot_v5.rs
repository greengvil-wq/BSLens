#![cfg(all(feature = "experimental-snapshot-v5", target_os = "linux"))]
use bslens_core::snapshot_v5::{build, contract, Error, Limits};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const ACCEPTED: &str = "Catalogs/Items/Ext/ObjectModule.bsl";
const UNPARSED: &str = "Documents/Order/Ext/ObjectModule.bsl";
const ORPHAN: &str = "Unknown.bsl";
fn accepted() -> Vec<u8> {
    [b"\xef\xbb\xbf".as_slice(),"  Процедура ЯПервый() Экспорт\r\n  КонецПроцедуры\r\n  Функция АВторой()\r\n  КонецФункции\r\n".as_bytes()].concat()
}
fn unparsed() -> Vec<u8> {
    [
        b"\xef\xbb\xbf".as_slice(),
        "// Синтетика\r\n#Область\r\nПоследняяСтрока".as_bytes(),
    ]
    .concat()
}
struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    output: PathBuf,
    commit: String,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "bslens-v5-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let repo = root.join("repo");
        fs::create_dir(&repo).unwrap();
        let mut f = Self {
            output: root.join("output"),
            root,
            repo,
            commit: String::new(),
        };
        f.git(&["init", "-q"]);
        for (path, bytes) in [
            (
                "Configuration.xml",
                include_bytes!("fixtures/xml221/Configuration.xml").to_vec(),
            ),
            (
                "Catalogs/Items.xml",
                include_bytes!("fixtures/xml221/Catalogs/Items.xml").to_vec(),
            ),
            (
                "Documents/Order.xml",
                include_bytes!("fixtures/xml221/Documents/Order.xml").to_vec(),
            ),
            (
                "InformationRegisters/Events.xml",
                include_bytes!("fixtures/xml221/InformationRegisters/Events.xml").to_vec(),
            ),
            (ACCEPTED, accepted()),
            (UNPARSED, unparsed()),
            (ORPHAN, b"// unconfirmed\n".to_vec()),
            ("Ext/SessionModule.bsl", b"// root\n".to_vec()),
            ("README.txt", b"synthetic excluded\n".to_vec()),
        ] {
            let p = f.repo.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, bytes).unwrap();
        }
        f.git(&["add", "."]);
        f.git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@invalid",
            "commit",
            "-qm",
            "synthetic-v5",
        ]);
        f.commit = f.git(&["rev-parse", "HEAD"]);
        f
    }
    fn git(&self, args: &[&str]) -> String {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&self.repo)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_DATE", "2026-11-19T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2026-11-19T00:00:00Z")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().into()
    }
    fn build(&self) -> bslens_core::snapshot_v5::BuiltSnapshot {
        build(
            &self.repo,
            &self.commit,
            "synthetic:s3",
            &self.output,
            limits(),
        )
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
fn limits() -> Limits {
    Limits {
        source_count: 100,
        source_bytes: 1_000_000,
        total_source_bytes: 4_000_000,
        sqlite_bytes: 2_000_000,
    }
}
fn sha(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

#[test]
fn exact_git_bytes_statuses_lines_and_lexical_ranges() {
    let f = Fixture::new();
    fs::write(f.repo.join(ACCEPTED), "not the committed file").unwrap();
    let built = f.build();
    let db = Connection::open(&built.path).unwrap();
    contract::validate_schema_profile(&db).unwrap();
    assert_eq!(contract::facts_sha256(&db).unwrap(), built.facts_sha256);
    assert_eq!(contract::snapshot_id(&db).unwrap(), built.snapshot_id);
    assert_eq!(sha(&built.path), built.file_sha256);
    assert_eq!(
        db.query_row(
            "SELECT content FROM sources WHERE path=?",
            [ACCEPTED],
            |r| r.get::<_, Vec<u8>>(0)
        )
        .unwrap(),
        accepted()
    );
    assert_eq!(
        db.query_row(
            "SELECT parse_status,reason_code FROM modules WHERE path=?",
            [UNPARSED],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        )
        .unwrap(),
        ("unparsed".into(), "RegionHeader".into())
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM methods WHERE module_path=?",
            [UNPARSED],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        db.query_row("SELECT owner_id FROM modules WHERE path=?", [ORPHAN], |r| r
            .get::<_, Option<i64>>(0))
            .unwrap(),
        None
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM objects WHERE kind='InformationRegister'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    let data = unparsed();
    let (start,end):(usize,usize)=db.query_row("SELECT a.byte_start,b.byte_end FROM source_lines a JOIN source_lines b ON a.path=b.path WHERE a.path=? AND a.line_number=2 AND b.line_number=3",[UNPARSED],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(&data[start..end], "#Область\r\nПоследняяСтрока".as_bytes());
    let coords:(usize,usize,usize,usize,i64)=db.query_row("SELECT m.byte_start,m.byte_end,a.byte_start,b.byte_end,m.exported FROM methods m JOIN source_lines a ON m.module_path=a.path AND m.start_line=a.line_number JOIN source_lines b ON m.module_path=b.path AND m.end_line=b.line_number WHERE m.start_line=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).unwrap();
    assert_eq!(coords.0, 5);
    assert_eq!(coords.2, 0);
    assert_eq!(coords.1 + 2, coords.3);
    assert_eq!(coords.4, 1);
    assert!(std::str::from_utf8(&accepted()[coords.0..coords.1])
        .unwrap()
        .ends_with("КонецПроцедуры"));
    let manifest: String = db
        .query_row("SELECT input_manifest FROM metadata", [], |r| r.get(0))
        .unwrap();
    assert!(manifest.contains("README.txt"));
    assert!(manifest.contains("NotXmlOrBsl"));
    assert_eq!(
        db.query_row("SELECT count(*) FROM sources", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        8
    );
    assert_eq!(
        db.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "delete"
    );
    assert_eq!(
        fs::metadata(&built.path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!fs::read_dir(&f.output).unwrap().any(|e| {
        let n = e.unwrap().file_name().to_string_lossy().into_owned();
        n.ends_with("-wal") || n.ends_with("-shm") || n.ends_with("-journal") || n.ends_with(".tmp")
    }));
}

#[test]
fn golden_and_logical_identity_ignore_surrogates_order_and_time() {
    let f = Fixture::new();
    let built = f.build();
    let db = Connection::open(&built.path).unwrap();
    let facts = contract::facts_sha256(&db).unwrap();
    let id = contract::snapshot_id(&db).unwrap();
    assert_eq!(
        facts,
        "c84407ce028a22d8acb975d1c255280ed81eec64efa49a82267bae7bbb25a85d"
    );
    assert_eq!(
        id,
        "bslens-v5-4a945e206f7e08d35b51ed24f3f2dfcc5bf4a6ef2afd11237de06f668a4b2d5d"
    );
    db.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
    for (name, order) in [
        ("objects", "xml_path"),
        ("modules", "path"),
        ("methods", "start_line"),
        ("source_lines", "line_number"),
    ] {
        db.execute_batch(&format!("CREATE TEMP TABLE reorder AS SELECT * FROM {name}; DELETE FROM {name}; INSERT INTO {name} SELECT * FROM reorder ORDER BY {order} DESC; DROP TABLE reorder;")).unwrap();
    }
    db.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    assert_eq!(contract::facts_sha256(&db).unwrap(), facts);
    db.execute_batch("PRAGMA foreign_keys=OFF; UPDATE objects SET object_id=object_id+100; UPDATE objects SET parent_id=parent_id+100 WHERE parent_id IS NOT NULL; UPDATE modules SET owner_id=owner_id+100 WHERE owner_id IS NOT NULL; UPDATE methods SET method_id=1000-method_id; UPDATE metadata SET built_at_utc='2030-01-01T00:00:00Z'; PRAGMA foreign_keys=ON;").unwrap();
    assert_eq!(contract::facts_sha256(&db).unwrap(), facts);
    assert_eq!(contract::snapshot_id(&db).unwrap(), id);
    db.execute("UPDATE methods SET name='Changed' WHERE start_line=1", [])
        .unwrap();
    assert_ne!(contract::facts_sha256(&db).unwrap(), facts);
    assert_eq!(contract::snapshot_id(&db).unwrap(), id);
    let changed = contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET source_id='other-source'", [])
        .unwrap();
    assert_eq!(contract::facts_sha256(&db).unwrap(), changed);
    assert_ne!(contract::snapshot_id(&db).unwrap(), id);
}
#[test]
fn repeat_and_concurrent_publication_never_replace() {
    let f = Fixture::new();
    let results = std::thread::scope(|s| {
        let jobs: Vec<_> = (0..4).map(|_| s.spawn(|| f.build())).collect();
        jobs.into_iter()
            .map(|j| j.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert_eq!(results.iter().filter(|b| !b.reused).count(), 1);
    assert!(results.iter().all(|b| b.path == results[0].path));
    let before = fs::read(&results[0].path).unwrap();
    let again = f.build();
    assert!(again.reused);
    assert_eq!(before, fs::read(&again.path).unwrap());
    assert_eq!(
        fs::read_dir(&f.output)
            .unwrap()
            .filter(|e| e
                .as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|e| e == "sqlite"))
            .count(),
        1
    );
}
#[test]
fn each_limit_names_failure_and_publishes_nothing() {
    for name in [
        "source_count",
        "source_bytes",
        "total_source_bytes",
        "sqlite_bytes",
    ] {
        let f = Fixture::new();
        let mut cap = limits();
        match name {
            "source_count" => cap.source_count = 1,
            "source_bytes" => cap.source_bytes = 1,
            "total_source_bytes" => cap.total_source_bytes = 1,
            _ => cap.sqlite_bytes = 4096,
        }
        let error = build(&f.repo, &f.commit, "synthetic:s3", &f.output, cap).unwrap_err();
        if name == "sqlite_bytes" {
            assert!(
                matches!(&error,Error::SqliteFull {path,sqlite_bytes:4096,max_pages:1} if path.parent()==Some(f.output.as_path())),
                "{error:?}"
            );
            assert!(error.to_string().contains("диск или лимит страниц"));
            assert!(!error.to_string().contains("actual"));
        } else {
            assert!(
                matches!(error,Error::Limit {name:n,..} if n==name),
                "{error:?}"
            );
        }
        if f.output.exists() {
            assert_eq!(fs::read_dir(&f.output).unwrap().count(), 0);
        }
    }
}
#[test]
fn same_input_with_different_facts_is_not_overwritten() {
    let f = Fixture::new();
    let old = f.build();
    let db = Connection::open(&old.path).unwrap();
    db.execute(
        "UPDATE methods SET name='DifferentFact' WHERE start_line=1",
        [],
    )
    .unwrap();
    let altered = contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?", [altered])
        .unwrap();
    drop(db);
    let altered_path = f
        .output
        .join(format!("{}.{}.sqlite", old.snapshot_id, sha(&old.path)));
    fs::rename(&old.path, &altered_path).unwrap();
    let bytes = fs::read(&altered_path).unwrap();
    assert!(matches!(
        build(&f.repo, &f.commit, "synthetic:s3", &f.output, limits()),
        Err(Error::NonReproducible)
    ));
    assert_eq!(bytes, fs::read(altered_path).unwrap());
}
#[test]
fn schema_and_profile_are_exact_and_foreign_keys_checked() {
    let f = Fixture::new();
    let built = f.build();
    let db = Connection::open(&built.path).unwrap();
    db.execute_batch("PRAGMA user_version=4;").unwrap();
    assert!(contract::validate_schema_profile(&db).is_err());
    db.execute_batch("PRAGMA user_version=5; CREATE VIEW extra AS SELECT 1;")
        .unwrap();
    assert!(contract::validate_schema_profile(&db).is_err());
    db.execute_batch("DROP VIEW extra; PRAGMA ignore_check_constraints=ON; UPDATE metadata SET profile='unknown';").unwrap();
    assert!(contract::validate_schema_profile(&db).is_err());
    db.execute("UPDATE metadata SET profile=?", [contract::PROFILE])
        .unwrap();
    contract::validate_schema_profile(&db).unwrap();
    db.execute_batch("PRAGMA foreign_keys=OFF; UPDATE modules SET owner_id=999;")
        .unwrap();
    assert!(contract::facts_sha256(&db).is_err());
}
#[test]
fn invalid_xml_and_noncommit_inputs_publish_nothing() {
    let f = Fixture::new();
    assert!(build(&f.repo, "HEAD", "synthetic:s3", &f.output, limits()).is_err());
    fs::write(f.repo.join("Configuration.xml"), b"not XML").unwrap();
    f.git(&["add", "."]);
    f.git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@invalid",
        "commit",
        "-qm",
        "bad",
    ]);
    let commit = f.git(&["rev-parse", "HEAD"]);
    assert!(matches!(
        build(&f.repo, &commit, "synthetic:s3", &f.output, limits()),
        Err(Error::Xml(_))
    ));
    if f.output.exists() {
        assert_eq!(fs::read_dir(&f.output).unwrap().count(), 0);
    }
}

#[test]
fn shared_derivation_matches_without_git_and_refuses_nonempty_facts() {
    let f = Fixture::new();
    let built = f.build();
    let original = Connection::open(&built.path).unwrap();
    let expected = contract::facts_sha256(&original).unwrap();
    let copy = Connection::open_in_memory().unwrap();
    copy.execute_batch(contract::SCHEMA).unwrap();
    copy.execute("ATTACH DATABASE ? AS input", [built.path.to_str().unwrap()])
        .unwrap();
    copy.execute("INSERT INTO sources SELECT * FROM input.sources", [])
        .unwrap();
    copy.execute_batch("DETACH DATABASE input;").unwrap();
    fs::remove_dir_all(&f.repo).unwrap();
    let coverage = bslens_core::snapshot_v5::derive_facts(&copy).unwrap();
    assert_eq!(coverage["owner_unconfirmed"], 1);
    assert_eq!(contract::facts_sha256(&copy).unwrap(), expected);
    assert!(bslens_core::snapshot_v5::derive_facts(&copy).is_err());
}
#[test]
fn build_time_changes_only_file_hash_and_other_scope_is_compatible() {
    let f = Fixture::new();
    let built = f.build();
    let db = Connection::open(&built.path).unwrap();
    contract::compatible_scope(&db).unwrap();
    db.execute(
        "UPDATE metadata SET built_at_utc='2000-01-01T00:00:00Z'",
        [],
    )
    .unwrap();
    assert_ne!(sha(&built.path), built.file_sha256);
    assert_eq!(contract::snapshot_id(&db).unwrap(), built.snapshot_id);
    assert_eq!(contract::facts_sha256(&db).unwrap(), built.facts_sha256);
    db.execute(
        "UPDATE metadata SET scope_id='another-compatible-scope'",
        [],
    )
    .unwrap();
    contract::compatible_scope(&db).unwrap();
    db.execute("UPDATE metadata SET lexical_version='unknown'", [])
        .unwrap();
    assert!(contract::compatible_scope(&db).is_err());
}
#[test]
fn source_line_facts_cover_invalid_utf8_and_empty_bsl() {
    let f = Fixture::new();
    fs::write(f.repo.join(ORPHAN), [0xff, b'\r', b'\n', b'x']).unwrap();
    fs::write(f.repo.join("Empty.bsl"), []).unwrap();
    f.git(&["add", "."]);
    f.git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@invalid",
        "commit",
        "-qm",
        "bytes",
    ]);
    let built = build(
        &f.repo,
        &f.git(&["rev-parse", "HEAD"]),
        "synthetic:s3",
        &f.output,
        limits(),
    )
    .unwrap();
    let db = Connection::open(built.path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT reason_code FROM modules WHERE path=?",
            [ORPHAN],
            |r| r.get::<_, String>(0)
        )
        .unwrap(),
        "Utf8"
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM source_lines WHERE path=?",
            [ORPHAN],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM source_lines WHERE path='Empty.bsl'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
}

#[test]
fn schema_catalog_budgets_precede_text_comparison() {
    let f = Fixture::new();
    let built = f.build();
    let db = Connection::open(&built.path).unwrap();
    db.execute_batch("CREATE VIEW extra AS SELECT 1;").unwrap();
    assert!(matches!(
        contract::validate_schema_profile(&db),
        Err(Error::Contract("schema_count"))
    ));
    db.execute_batch("DROP VIEW extra; PRAGMA writable_schema=ON;")
        .unwrap();
    // Число строк то же; SQL раздувается валидным комментарием.
    db.execute(
        "UPDATE sqlite_schema SET sql=sql||? WHERE name='methods'",
        [format!(" /*{}*/", "x".repeat(1024 * 1024))],
    )
    .unwrap();
    assert!(matches!(
        contract::validate_schema_profile(&db),
        Err(Error::Contract("schema_field_budget"))
    ));
}
#[test]
fn metadata_budgets_cover_identity_compatibility_and_embedded_nul() {
    let f = Fixture::new();
    let built = f.build();
    let db = Connection::open(&built.path).unwrap();
    for (field, value) in [
        ("source_id", format!("x{}", "\0".repeat(1025))),
        ("scope_id", "x".repeat(1025)),
        ("core_version", "x".repeat(129)),
        (
            "build_parameters",
            serde_json::json!({"x":"a".repeat(16*1024)}).to_string(),
        ),
        (
            "coverage_draft",
            serde_json::json!({"temporary":true,"x":"a".repeat(1024*1024)}).to_string(),
        ),
        (
            "limits_draft",
            serde_json::json!({"x":"a".repeat(4096)}).to_string(),
        ),
        (
            "input_manifest",
            serde_json::json!({"x":"a".repeat(16*1024*1024)}).to_string(),
        ),
    ] {
        db.execute_batch("SAVEPOINT oversized;").unwrap();
        db.execute(&format!("UPDATE metadata SET {field}=?"), [value])
            .unwrap();
        for result in [
            contract::validate_schema_profile(&db),
            contract::compatible_scope(&db),
            contract::snapshot_id(&db).map(|_| ()),
        ] {
            assert!(
                matches!(result, Err(Error::Contract("metadata_field_budget"))),
                "{field}: {result:?}"
            );
        }
        db.execute_batch("ROLLBACK TO oversized; RELEASE oversized;")
            .unwrap();
    }
    contract::compatible_scope(&db).unwrap();
    assert_eq!(contract::snapshot_id(&db).unwrap(), built.snapshot_id);
}
