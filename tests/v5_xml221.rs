#![cfg(feature = "experimental-v5-xml221")]
use bslens_core::{
    git_manifest::ingest::Entry,
    xml221::{
        self,
        file::{self, Analysis, Error, Limits},
        Code, Kind, NS,
    },
};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
fn entry(path: &str, raw: &[u8], sha256: bool) -> (Entry, String) {
    let header = format!("blob {}\0", raw.len());
    let oid = if sha256 {
        let mut h = Sha256::new();
        h.update(header);
        h.update(raw);
        format!("{:x}", h.finalize())
    } else {
        let mut h = Sha1::new();
        h.update(header);
        h.update(raw);
        format!("{:x}", h.finalize())
    };
    (
        Entry {
            path: path.into(),
            mode: 0o100644,
            oid,
        },
        format!("{:x}", Sha256::digest(raw)),
    )
}
fn inspect(path: &str, raw: &[u8], limits: Limits) -> file::Result<file::Inspection> {
    let (entry, sha) = entry(path, raw, false);
    file::inspect(&entry, &sha, raw, limits)
}
fn metadata(body: &str) -> String {
    format!("<MetaDataObject xmlns='{NS}' version='2.21'>{body}</MetaDataObject>")
}
fn config() -> String {
    metadata(
        "<Configuration><Properties><Name>Root</Name></Properties><ChildObjects/></Configuration>",
    )
}
fn fail(raw: &[u8], code: Code) {
    assert!(
        matches!(inspect("Configuration.xml",raw,Limits::MAX),Err(Error::Xml(e)) if e.code == code)
    );
}
#[test]
fn fixture_identities_match_legacy_but_do_not_claim_global_validation() {
    let fixtures = [
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
    ];
    let blobs: BTreeMap<_, _> = fixtures
        .iter()
        .map(|(p, b)| (p.to_string(), b.to_vec()))
        .collect();
    let legacy = xml221::Xml221Subset::inspect(&blobs).unwrap();
    for (path, raw) in fixtures {
        for sha256 in [false, true] {
            let (entry, sha) = entry(path, raw, sha256);
            let result = file::inspect(&entry, &sha, raw, Limits::MAX).unwrap();
            assert_eq!(result.coverage(), "identity_only");
            assert_eq!(result.source.bytes, raw.len());
            assert_eq!(result.source.sha256, sha);
            assert_eq!(result.source.oid, entry.oid);
            let Analysis::ObjectIdentity(identity) = result.analysis else {
                panic!()
            };
            let old = legacy.objects().iter().find(|o| o.path == path).unwrap();
            assert_eq!(
                (identity.kind, identity.name, identity.uuid),
                (old.kind, old.name.clone(), old.uuid.clone())
            );
        }
    }
    // No cross-file missing-reference rejection or guessed relationships in a DTO.
    let only = inspect("Configuration.xml", fixtures[0].1, Limits::MAX).unwrap();
    assert_eq!(only.coverage(), "identity_only");
    let mut lone = BTreeMap::new();
    lone.insert("Configuration.xml".into(), fixtures[0].1.to_vec());
    assert_eq!(
        xml221::Xml221Subset::inspect(&lone).unwrap_err().code,
        Code::Missing
    );
}
#[test]
fn unknown_type_service_and_unknown_envelope_are_distinct_coverage() {
    let unknown = metadata(
        "<CommonModule><Properties><Name>DoNotInterpret</Name></Properties></CommonModule>",
    );
    let r = inspect("CommonModules/Test.xml", unknown.as_bytes(), Limits::MAX).unwrap();
    assert_eq!(r.coverage(), "unsupported_type");
    assert_eq!(
        r.analysis,
        Analysis::UnsupportedType {
            type_name: "CommonModule".into()
        }
    );
    // Synthetic service envelope, deliberately not pretending to model its schema.
    let service=b"<SyntheticService xmlns='urn:synthetic' version='uninterpreted'><data/></SyntheticService>";
    let r = inspect("ConfigDumpInfo.xml", service, Limits::MAX).unwrap();
    assert_eq!(r.analysis, Analysis::ServiceDocument);
    assert_eq!(r.coverage(), "service_document_unanalyzed");
    assert_eq!(
        r.envelope.declared_version.as_deref(),
        Some("uninterpreted")
    );
    assert_eq!(r.envelope.namespace.as_deref(), Some("urn:synthetic"));
    for path in ["Ext/Unknown.xml", "nested/ConfigDumpInfo.xml"] {
        assert_eq!(
            inspect(path, service, Limits::MAX).unwrap().analysis,
            Analysis::UnknownEnvelope
        );
    }
    let foreign = b"<MetaDataObject xmlns='urn:foreign' version='2.21'/>";
    assert_eq!(
        inspect("Other.xml", foreign, Limits::MAX)
            .unwrap()
            .coverage(),
        "unknown_envelope"
    );
    // Legacy still refuses unknown type; coverage is opt-in, not weakening inspect.
    let mut blobs = BTreeMap::new();
    blobs.insert("Configuration.xml".into(),metadata("<Configuration><Properties><Name>Root</Name></Properties><ChildObjects><CommonModule>A</CommonModule></ChildObjects></Configuration>").into_bytes());
    assert_eq!(
        xml221::Xml221Subset::inspect(&blobs).unwrap_err().code,
        Code::UnsupportedType
    );
}
#[test]
fn metadata_version_and_shape_fail_explicitly_including_unsupported_types() {
    for version in ["2.20", "2.22", ""] {
        let raw = config().replace("2.21", version);
        fail(raw.as_bytes(), Code::Version);
        let unknown = metadata("<CommonModule/>").replace("2.21", version);
        fail(unknown.as_bytes(), Code::Version);
        assert!(
            matches!(inspect("ConfigDumpInfo.xml",raw.as_bytes(),Limits::MAX),Err(Error::Xml(e)) if e.code == Code::Version)
        );
    }
    fail(
        config().replace(" version='2.21'", "").as_bytes(),
        Code::Version,
    );
    for body in [
        "",
        "text<CommonModule/>",
        "<Catalog/><Document/>",
        "<Configuration/>",
    ] {
        fail(metadata(body).as_bytes(), Code::Shape);
    }
    fail(
        metadata("<Other xmlns='urn:foreign'/>").as_bytes(),
        Code::Namespace,
    );
    fail(
        metadata("<Configuration><Properties><Name>Bad Name</Name></Properties></Configuration>")
            .as_bytes(),
        Code::Identifier,
    );
    fail(
        metadata(
            "<Configuration uuid='bad'><Properties><Name>Root</Name></Properties></Configuration>",
        )
        .as_bytes(),
        Code::Uuid,
    );
    fail(
        metadata(
            "<Configuration><Properties><Name>Root<Part/></Name></Properties></Configuration>",
        )
        .as_bytes(),
        Code::Shape,
    );
    fail(metadata("<Configuration><Properties><Name>Root</Name><Name>Duplicate</Name></Properties></Configuration>").as_bytes(),Code::Shape);
    let raw = metadata("<Catalog><Properties><Name>Actual</Name></Properties></Catalog>");
    assert!(
        matches!(inspect("Catalogs/Other.xml",raw.as_bytes(),Limits::MAX),Err(Error::Xml(e)) if e.code == Code::NameMismatch)
    );
}
#[test]
fn safety_guards_apply_before_coverage_and_match_legacy() {
    for (prefix, code) in [
        (
            "<!DOCTYPE MetaDataObject [<!ENTITY x SYSTEM 'file:///etc/passwd'>]>",
            Code::Dtd,
        ),
        ("<?danger value?>", Code::ProcessingInstruction),
        (
            "<?xml version='1.0' encoding='windows-1251'?>",
            Code::Encoding,
        ),
        ("\u{feff}\u{feff}", Code::Malformed),
    ] {
        let raw = format!("{prefix}{}", config());
        fail(raw.as_bytes(), code);
        let legacy = BTreeMap::from([("Configuration.xml".into(), raw.into_bytes())]);
        assert_eq!(
            xml221::Xml221Subset::inspect(&legacy).unwrap_err().code,
            code
        );
        let unknown = format!("{prefix}<Unknown/>");
        for path in ["Other.xml", "ConfigDumpInfo.xml"] {
            assert!(
                matches!(inspect(path,unknown.as_bytes(),Limits::MAX),Err(Error::Xml(e)) if e.code == code)
            );
        }
    }
    for raw in [
        b"<Unknown>".as_slice(),
        b"<x/><y/>",
        b"<x>&undefined;</x>",
        b"<x a='1' a='2'/>",
        b"<x>\0</x>",
    ] {
        for path in ["Other.xml", "ConfigDumpInfo.xml"] {
            assert!(
                matches!(inspect(path,raw,Limits::MAX),Err(Error::Xml(e)) if e.code == Code::Malformed)
            );
        }
    }
    fail(&[0xff], Code::Utf8);
    let raw = format!(
        "\u{feff}{}",
        config().replace("<Name>Root</Name>", "<Name>Ro<!--keep-->ot</Name>")
    );
    assert_eq!(
        inspect("Configuration.xml", raw.as_bytes(), Limits::MAX)
            .unwrap()
            .coverage(),
        "identity_only"
    );
}
#[test]
fn exact_boundaries_positive_limits_and_normalized_parameters() {
    let raw = b"<root/>";
    let limits = Limits {
        bytes: raw.len(),
        nodes: 2,
        depth: 1,
        events: 2,
    };
    assert!(inspect("Other.xml", raw, limits).is_ok());
    for small in [
        Limits {
            bytes: raw.len() - 1,
            ..limits
        },
        Limits { nodes: 1, ..limits },
        Limits {
            events: 1,
            ..limits
        },
    ] {
        assert!(
            matches!(inspect("Other.xml",raw,small),Err(Error::Xml(e)) if e.code == Code::Limit)
        );
    }
    assert!(
        matches!(inspect("Other.xml",b"<root><a/></root>",Limits {depth:1,..Limits::MAX}),Err(Error::Xml(e)) if e.code == Code::Limit)
    );
    for bad in [
        Limits {
            bytes: 0,
            ..Limits::MAX
        },
        Limits {
            nodes: 0,
            ..Limits::MAX
        },
        Limits {
            depth: 0,
            ..Limits::MAX
        },
        Limits {
            events: 0,
            ..Limits::MAX
        },
        Limits {
            bytes: Limits::MAX.bytes + 1,
            ..Limits::MAX
        },
        Limits {
            nodes: Limits::MAX.nodes + 1,
            ..Limits::MAX
        },
        Limits {
            depth: Limits::MAX.depth + 1,
            ..Limits::MAX
        },
        Limits {
            events: Limits::MAX.events + 1,
            ..Limits::MAX
        },
    ] {
        assert!(matches!(
            inspect("Other.xml", raw, bad),
            Err(Error::InvalidLimit { .. })
        ));
        assert!(matches!(
            bad.normalized_parameters(),
            Err(Error::InvalidLimit { .. })
        ));
    }
    let p = limits.normalized_parameters().unwrap();
    assert_eq!(p, limits.normalized_parameters().unwrap());
    for changed in [
        Limits {
            bytes: limits.bytes + 1,
            ..limits
        },
        Limits { nodes: 3, ..limits },
        Limits { depth: 2, ..limits },
        Limits {
            events: 3,
            ..limits
        },
    ] {
        assert_ne!(p, changed.normalized_parameters().unwrap());
    }
    assert!(p.contains(file::ADAPTER));
}
#[test]
fn identity_path_and_mode_checked_before_interpretation() {
    let raw = config();
    let (mut e, sha) = entry("Configuration.xml", raw.as_bytes(), false);
    assert!(matches!(
        file::inspect(&e, &"0".repeat(64), raw.as_bytes(), Limits::MAX),
        Err(Error::Identity)
    ));
    assert!(matches!(
        file::inspect(&e, &sha, b"changed", Limits::MAX),
        Err(Error::Identity)
    ));
    for oid in ["HEAD".into(), "0".repeat(40), e.oid.to_uppercase()] {
        e.oid = oid;
        assert!(matches!(
            file::inspect(&e, &sha, raw.as_bytes(), Limits::MAX),
            Err(Error::Identity)
        ));
    }
    let (mut e, sha) = entry("Configuration.xml", raw.as_bytes(), false);
    e.mode = 0o120000;
    assert!(
        matches!(file::inspect(&e,&sha,raw.as_bytes(),Limits::MAX),Err(Error::Xml(x)) if x.code==Code::Path)
    );
    for path in ["../x.xml", "/x.xml", "a\\b.xml", "a:xml", "a.bsl", ""] {
        assert!(
            matches!(inspect(path,raw.as_bytes(),Limits::MAX),Err(Error::Xml(x)) if x.code==Code::Path)
        );
    }
}
#[test]
fn supported_identity_is_explicitly_partial_with_unanalyzed_content() {
    let raw=metadata("<Configuration custom='unverified'><Properties extra='unverified'><Name>Root</Name><Other><Nested/></Other></Properties><ChildObjects><UnknownChild/></ChildObjects><Anything/></Configuration>");
    let r = inspect("Configuration.xml", raw.as_bytes(), Limits::MAX).unwrap();
    assert_eq!(r.coverage(), "identity_only");
    assert_eq!(
        r.analysis,
        Analysis::ObjectIdentity(file::ObjectIdentity {
            kind: Kind::Configuration,
            name: "Root".into(),
            uuid: None
        })
    );
}

#[cfg(target_os = "linux")]
mod roundtrip {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        process::Command,
        sync::atomic::{AtomicU64, Ordering},
    };
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Repo {
        path: PathBuf,
    }
    impl Repo {
        fn new(sha256: bool) -> Self {
            let path = std::env::temp_dir().join(format!(
                "bslens-r03-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            fs::create_dir(&path).unwrap();
            let r = Self { path };
            if sha256 {
                r.git(&["init", "-q", "--object-format=sha256"]);
            } else {
                r.git(&["init", "-q"]);
            }
            r
        }
        fn git(&self, args: &[&str]) -> String {
            let o = Command::new("/usr/bin/git")
                .arg("-C")
                .arg(&self.path)
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(
                o.status.success(),
                "{:?}: {}",
                args,
                String::from_utf8_lossy(&o.stderr)
            );
            String::from_utf8(o.stdout).unwrap().trim().into()
        }
        fn commit(&self) -> String {
            self.git(&["add", "-A"]);
            self.git(&[
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@invalid",
                "commit",
                "-qm",
                "fixture",
            ]);
            self.git(&["rev-parse", "HEAD"])
        }
    }
    impl Drop for Repo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn git_callback_to_blob_and_offline_reinspection() {
        let r = Repo::new(false);
        fs::create_dir(r.path.join("CommonModules")).unwrap();
        for (p, raw) in [
            ("Configuration.xml", config()),
            ("CommonModules/A.xml", metadata("<CommonModule/>")),
            ("ConfigDumpInfo.xml", "<SyntheticService/>".into()),
            ("Other.xml", "<Unknown xmlns='urn:synthetic'/>".into()),
        ] {
            fs::write(r.path.join(p), raw).unwrap();
        }
        let commit = r.commit();
        let tree = r.git(&["rev-parse", "HEAD^{tree}"]);
        let db = rusqlite::Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE blobs(path TEXT PRIMARY KEY,mode INTEGER,oid TEXT,sha TEXT,raw BLOB);BEGIN").unwrap();
        let mut expected = BTreeMap::new();
        bslens_core::git_manifest::ingest::ingest(
            &r.path,
            &commit,
            &tree,
            bslens_core::git_manifest::ingest::Limits::MAX,
            |entry, sha, raw| {
                let inspection =
                    file::inspect(entry, sha, raw, Limits::MAX).map_err(|e| e.to_string())?;
                db.execute(
                    "INSERT INTO blobs VALUES(?1,?2,?3,?4,?5)",
                    rusqlite::params![entry.path, entry.mode, entry.oid, sha, raw],
                )
                .map_err(|e| e.to_string())?;
                expected.insert(entry.path.clone(), inspection);
                Ok(())
            },
        )
        .unwrap();
        db.execute_batch("COMMIT").unwrap();
        drop(r); // No repository remains for offline pass.
        let mut stmt = db
            .prepare("SELECT path,mode,oid,sha,raw FROM blobs ORDER BY path")
            .unwrap();
        let mut rows = stmt.query([]).unwrap();
        let mut count = 0;
        while let Some(row) = rows.next().unwrap() {
            let entry = Entry {
                path: row.get(0).unwrap(),
                mode: row.get(1).unwrap(),
                oid: row.get(2).unwrap(),
            };
            let sha: String = row.get(3).unwrap();
            let bytes: Vec<u8> = row.get(4).unwrap();
            let replay = file::inspect(&entry, &sha, &bytes, Limits::MAX).unwrap();
            assert_eq!(replay, expected[&entry.path]);
            count += 1;
            assert!(matches!(
                file::inspect(&entry, &sha, b"<changed/>", Limits::MAX),
                Err(Error::Identity)
            ));
        }
        assert_eq!(count, 4);
    }
}
