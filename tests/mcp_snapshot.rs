#![cfg(all(
    feature = "experimental-snapshot-mcp",
    target_os = "linux",
    target_arch = "x86_64"
))]
use bslens_core::{
    git_manifest,
    mcp_transport::{MAX_BODY, PROTOCOL},
    pipeline::GitStructure,
    snapshot_v5::{build, BuiltSnapshot, Limits},
    structure::{Node, NodeKind},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
const ROOT: &str = "Ext/SessionModule.bsl";
const ACCEPTED: &str = "Catalogs/Items/Ext/ObjectModule.bsl";
const REFUSED: &str = "Documents/Order/Ext/ObjectModule.bsl";
const TEXT: &str =
    "\u{feff}// Ω\r\nПроцедура ЯПервый()\r\nКонецПроцедуры\r\nФункция АВторой()\r\nКонецФункции";
const BAD: &str = "\u{feff}#Область\r\nlast without LF";
const BINARY: &[u8] = &[0xff, b'\r', b'\n', 0, b'x'];
const CORPUS_GLOBALS: &[(&str, &[u8])] = &[
    ("Global.xml", b"<Uninterpreted/>"),
    ("Ext/Global/Other.xml", b"not parsed XML payload"),
    ("Splash.png", b"\x89PNG\x00opaque"),
    ("Ext/First.bin", b"\x00\xff\x01"),
    ("Ext/Second.bin", b"\x00\x80"),
    ("Ext/Archive.zip", b"PK\x03\x04opaque"),
];
fn corpus_opaque_json() -> String {
    json!(CORPUS_GLOBALS
        .iter()
        .filter(|(p, _)| !p.ends_with(".xml"))
        .map(|(p, _)| *p)
        .collect::<Vec<_>>())
    .to_string()
}
const DESCRIPTOR_ROOTS: &[(&str, &str)] = &[
    ("CommandGroup", "CommandGroups"),
    ("CommonAttribute", "CommonAttributes"),
    ("DefinedType", "DefinedTypes"),
    ("EventSubscription", "EventSubscriptions"),
    ("FilterCriterion", "FilterCriteria"),
    ("FunctionalOption", "FunctionalOptions"),
    ("FunctionalOptionsParameter", "FunctionalOptionsParameters"),
    ("Language", "Languages"),
    ("PaletteColor", "PaletteColors"),
    ("SessionParameter", "SessionParameters"),
    ("StyleItem", "StyleItems"),
];

const SHARED_OBJECTS: &[(&str, &str, &str)] = &[
    ("CommonCommand", "CommonCommands", "Act"),
    ("CommonCommand", "CommonCommands", "Refused"),
    ("CommonForm", "CommonForms", "Card"),
    ("CommonForm", "CommonForms", "БезКода"),
    ("CommonTemplate", "CommonTemplates", "Binary"),
    ("CommonTemplate", "CommonTemplates", "Text"),
    ("CommonTemplate", "CommonTemplates", "Help"),
    ("CommonTemplate", "CommonTemplates", "Sheet"),
    ("CommonTemplate", "CommonTemplates", "DCS"),
];
const SHARED_PAYLOADS: &[(&str, &[u8])] = &[
    (
        "CommonForms/Card/Ext/Form.xml",
        b"<Form xmlns=\"http://v8.1c.ru/8.3/xcf/logform\" version=\"2.21\"/>",
    ),
    (
        "CommonForms/БезКода/Ext/Form.xml",
        b"<Form xmlns=\"http://v8.1c.ru/8.3/xcf/logform\" version=\"2.21\"/>",
    ),
    (
        "CommonForms/Card/Ext/Help.xml",
        b"<Help xmlns=\"http://v8.1c.ru/8.3/xcf/extrnprops\" version=\"2.21\"/>",
    ),
    (
        "CommonForms/Card/Ext/Help/ru.html",
        b"\xef\xbb\xbf<html>opaque &amp; help</html>\r\n",
    ),
    (
        "CommonTemplates/Help/Ext/Template.xml",
        b"<Help xmlns=\"http://v8.1c.ru/8.3/xcf/extrnprops\" version=\"2.21\"/>",
    ),
    (
        "CommonTemplates/Help/Ext/Template/ru.html",
        b"<html>template help</html>\r\nlast",
    ),
    ("CommonTemplates/Binary/Ext/Template.bin", BINARY),
    (
        "CommonTemplates/Text/Ext/Template.txt",
        b"\xef\xbb\xbftemplate\r\nlast without LF",
    ),
    (
        "CommonTemplates/Sheet/Ext/Template.xml",
        b"<document xmlns=\"http://v8.1c.ru/8.2/data/spreadsheet\"/>",
    ),
    (
        "CommonTemplates/DCS/Ext/Template.xml",
        b"<DataCompositionSchema xmlns=\"http://v8.1c.ru/8.1/data-composition-system/schema\"/>",
    ),
];
const LEAF_OBJECTS: &[(&str, &str, &str)] = &[
    ("CommonPicture", "CommonPictures", "Zip"),
    ("CommonPicture", "CommonPictures", "Png"),
    ("CommonPicture", "CommonPictures", "Svg"),
    ("CommonPicture", "CommonPictures", "Gif"),
    ("CommonPicture", "CommonPictures", "Ico"),
    ("Constant", "Constants", "Both"),
    ("Constant", "Constants", "Manager"),
    ("Constant", "Constants", "Value"),
    ("Constant", "Constants", "None"),
    ("Role", "Roles", "Reader"),
    ("ScheduledJob", "ScheduledJobs", "With"),
    ("ScheduledJob", "ScheduledJobs", "Without"),
    ("XDTOPackage", "XDTOPackages", "Package"),
];
const LEAF_PAYLOADS: &[(&str, &[u8])] = &[
    ("CommonPictures/Zip/Ext/Picture.xml", b"opaque envelope"),
    (
        "CommonPictures/Zip/Ext/Picture/Picture.zip",
        b"PK\x00\xff\r\n",
    ),
    ("CommonPictures/Png/Ext/Picture.xml", b"opaque envelope"),
    (
        "CommonPictures/Png/Ext/Picture/Picture.png",
        b"\x89PNG\x00\r\n",
    ),
    ("CommonPictures/Svg/Ext/Picture.xml", b"opaque envelope"),
    ("CommonPictures/Svg/Ext/Picture/Picture.svg", b"<svg/>\r\n"),
    ("CommonPictures/Gif/Ext/Picture.xml", b"opaque envelope"),
    ("CommonPictures/Gif/Ext/Picture/Picture.gif", b"GIF89a\x00"),
    ("CommonPictures/Ico/Ext/Picture.xml", b"opaque envelope"),
    ("CommonPictures/Ico/Ext/Picture/Picture.ico", BINARY),
    (
        "Roles/Reader/Ext/Rights.xml",
        b"\xef\xbb\xbf<opaqueRights/>\r\n",
    ),
    ("ScheduledJobs/With/Ext/Schedule.xml", b"<opaqueSchedule/>"),
    ("XDTOPackages/Package/Ext/Package.bin", BINARY),
];
const LEAF_BSL: &[(&str, &[u8])] = &[
    ("Constants/Both/Ext/ManagerModule.bsl", TEXT.as_bytes()),
    ("Constants/Both/Ext/ValueManagerModule.bsl", BINARY),
    ("Constants/Manager/Ext/ManagerModule.bsl", TEXT.as_bytes()),
    (
        "Constants/Value/Ext/ValueManagerModule.bsl",
        TEXT.as_bytes(),
    ),
];

const JOURNAL_OBJECTS: &[(&str, &str, &str)] = &[
    ("Enum", "Enums", "With"),
    ("Enum", "Enums", "Without"),
    ("DocumentJournal", "DocumentJournals", "Full"),
    ("DocumentJournal", "DocumentJournals", "Thin"),
];
const JOURNAL_INLINE: &[(&str, &str, &str, u32)] = &[
    ("Enums/With.xml", "EnumValue", "First", 900),
    ("Enums/With.xml", "EnumValue", "Second", 901),
    ("Enums/Without.xml", "EnumValue", "Only", 902),
    ("DocumentJournals/Full.xml", "Column", "Number", 903),
    ("DocumentJournals/Full.xml", "Command", "Run", 904),
    ("DocumentJournals/Full.xml", "Command", "Refused", 905),
    ("DocumentJournals/Thin.xml", "Column", "Number", 906),
    ("DocumentJournals/Thin.xml", "Command", "Run", 907),
];
fn journal_inline(kind: &str, name: &str, n: u32) -> String {
    format!("<{kind} uuid=\"20000000-0000-0000-0000-{n:012}\"><Properties><Name>{name}</Name><Synonym/><Comment/><Color>opaque</Color></Properties></{kind}>")
}
const JOURNAL_PAYLOADS: &[(&str, &[u8])] = &[
    ("DocumentJournals/Full/Ext/Help.xml", b"<Help/>"),
    (
        "DocumentJournals/Full/Ext/Help/ru.html",
        b"\xef\xbb\xbfhelp\r\nlast",
    ),
    (
        "DocumentJournals/Full/Forms/Card/Ext/Form.xml",
        b"<opaqueForm/>",
    ),
    ("DocumentJournals/Full/Forms/Card/Ext/Help.xml", b"<Help/>"),
    ("DocumentJournals/Full/Forms/Card/Ext/Help/ru.html", BINARY),
    (
        "DocumentJournals/Thin/Forms/Card/Ext/Form.xml",
        b"<opaqueForm/>",
    ),
    (
        "DocumentJournals/Full/Templates/Report/Ext/Template.xml",
        b"<DataCompositionSchema/>",
    ),
];
const JOURNAL_BSL: &[(&str, &[u8])] = &[
    ("Enums/With/Ext/ManagerModule.bsl", TEXT.as_bytes()),
    (
        "DocumentJournals/Full/Ext/ManagerModule.bsl",
        TEXT.as_bytes(),
    ),
    (
        "DocumentJournals/Full/Commands/Run/Ext/CommandModule.bsl",
        TEXT.as_bytes(),
    ),
    (
        "DocumentJournals/Full/Commands/Refused/Ext/CommandModule.bsl",
        BINARY,
    ),
    (
        "DocumentJournals/Thin/Commands/Run/Ext/CommandModule.bsl",
        TEXT.as_bytes(),
    ),
    (
        "DocumentJournals/Full/Forms/Card/Ext/Form/Module.bsl",
        TEXT.as_bytes(),
    ),
    (
        "DocumentJournals/Thin/Forms/Card/Ext/Form/Module.bsl",
        TEXT.as_bytes(),
    ),
];

const SERVICE_ROOTS: &[(&str, &str, &str)] = &[
    ("HTTPService", "HTTPServices", "Http"),
    ("WebService", "WebServices", "Web"),
    ("IntegrationService", "IntegrationServices", "Integration"),
];
fn service_node(kind: &str, name: &str, id: u32, extra: &str) -> String {
    format!("<{kind} uuid=\"40000000-0000-0000-0000-{id:012}\"><Properties><Name>{name}</Name><Handler>not_analyzed</Handler></Properties>{extra}</{kind}>")
}
fn service_body(kind: &str) -> String {
    match kind {
        "HTTPService" => service_node(
            "URLTemplate",
            "Endpoint",
            1001,
            &format!(
                "<ChildObjects>{}{}</ChildObjects>",
                service_node("Method", "Get", 1002, ""),
                service_node("Method", "Post", 1003, "")
            ),
        ),
        "WebService" => format!(
            "{}{}",
            service_node(
                "Operation",
                "WithParameter",
                1004,
                &format!(
                    "<ChildObjects>{}</ChildObjects>",
                    service_node("Parameter", "Input", 1005, "")
                )
            ),
            service_node("Operation", "Empty", 1006, "<ChildObjects/>")
        ),
        "IntegrationService" => service_node(
            "IntegrationServiceChannel",
            "Channel",
            1007,
            "<InternalInfo><Opaque/></InternalInfo>",
        ),
        _ => unreachable!(),
    }
}

const CHART_ROOTS: &[(&str, &str, &str)] = &[
    ("ChartOfAccounts", "ChartsOfAccounts", "Accounts"),
    (
        "ChartOfCalculationTypes",
        "ChartsOfCalculationTypes",
        "Calculation",
    ),
    (
        "ChartOfCharacteristicTypes",
        "ChartsOfCharacteristicTypes",
        "Characteristics",
    ),
];
fn chart_node(kind: &str, name: &str, id: u32, extra: &str) -> String {
    format!("<{kind} uuid=\"50000000-0000-0000-0000-{id:012}\"><Properties><Name>{name}</Name><UnknownProperty>not_analyzed</UnknownProperty></Properties>{extra}</{kind}>")
}
fn chart_body(kind: &str) -> String {
    let inline = match kind {
        "ChartOfAccounts" => format!(
            "{}{}{}",
            chart_node("AccountingFlag", "Flag", 1201, ""),
            chart_node("ExtDimensionAccountingFlag", "DimensionFlag", 1202, ""),
            chart_node("Attribute", "Attribute", 1203, "")
        ),
        "ChartOfCalculationTypes" => String::new(),
        "ChartOfCharacteristicTypes" => format!(
            "{}{}",
            chart_node("Attribute", "Direct", 1204, ""),
            chart_node(
                "TabularSection",
                "Rows",
                1205,
                &format!(
                    "<InternalInfo><Opaque/></InternalInfo><ChildObjects>{}</ChildObjects>",
                    chart_node("Attribute", "Child", 1206, "")
                )
            )
        ),
        _ => unreachable!(),
    };
    format!("{inline}<Form>Card</Form>")
}
const CHART_PAYLOADS: &[(&str, &[u8])] = &[
    (
        "ChartsOfAccounts/Accounts/Ext/Predefined.xml",
        b"<opaquePredefined/>",
    ),
    (
        "ChartsOfCharacteristicTypes/Characteristics/Ext/Predefined.xml",
        BINARY,
    ),
    ("ChartsOfAccounts/Accounts/Ext/Help.xml", b"<Help/>"),
    ("ChartsOfAccounts/Accounts/Ext/Help/ru.html", BINARY),
    (
        "ChartsOfCharacteristicTypes/Characteristics/Ext/Help.xml",
        b"<Help/>",
    ),
    (
        "ChartsOfCharacteristicTypes/Characteristics/Ext/Help/ru.html",
        b"\xef\xbb\xbfhelp\r\nlast",
    ),
    (
        "ChartsOfAccounts/Accounts/Forms/Card/Ext/Form.xml",
        b"<opaqueForm/>",
    ),
    (
        "ChartsOfCalculationTypes/Calculation/Forms/Card/Ext/Form.xml",
        b"<opaqueForm/>",
    ),
    (
        "ChartsOfCharacteristicTypes/Characteristics/Forms/Card/Ext/Form.xml",
        b"<opaqueForm/>",
    ),
    (
        "ChartsOfCharacteristicTypes/Characteristics/Forms/Card/Ext/Help.xml",
        b"<Help/>",
    ),
    (
        "ChartsOfCharacteristicTypes/Characteristics/Forms/Card/Ext/Help/ru.html",
        BINARY,
    ),
    (
        "ChartsOfAccounts/Accounts/Forms/Card/Ext/Form/Items/Item/Picture.png",
        b"\x89PNG\r\n\x1a\n\x00\xff",
    ),
];

struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    stage: PathBuf,
    built: BuiltSnapshot,
    old: GitStructure,
    read_limit: &'static str,
}
impl Fixture {
    fn new(extra: &[(&str, Vec<u8>)]) -> Self {
        Self::with_ingest(extra, false)
    }
    fn with_ingest(extra: &[(&str, Vec<u8>)], ingest_mode: bool) -> Self {
        Self::with_mode(extra, if ingest_mode { 1 } else { 0 })
    }
    fn with_mode(extra: &[(&str, Vec<u8>)], mode: u8) -> Self {
        let ingest_mode = mode == 1;
        let root = std::env::temp_dir().join(format!(
            "bslens-s6-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let repo = root.join("repo");
        fs::create_dir(&repo).unwrap();
        let stage = root.join("stage");
        fs::create_dir(&stage).unwrap();
        fs::set_permissions(&stage, fs::Permissions::from_mode(0o700)).unwrap();
        let git = |args: &[&str]| {
            let out = Command::new("/usr/bin/git")
                .arg("-C")
                .arg(&repo)
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8(out.stdout).unwrap().trim().to_owned()
        };
        git(&["init", "-q"]);
        let mut files = vec![
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
            (ROOT, TEXT.as_bytes().to_vec()),
            (ACCEPTED, TEXT.as_bytes().to_vec()),
            (REFUSED, BAD.as_bytes().to_vec()),
            ("Binary.bsl", BINARY.to_vec()),
            ("Zero.bsl", b"// accepted, zero methods\n".to_vec()),
        ];
        files.extend_from_slice(extra);
        for (path, bytes) in files {
            let p = repo.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, bytes).unwrap();
        }
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@invalid",
            "commit",
            "-qm",
            "S6 synthetic",
        ]);
        let sha = git(&["rev-parse", "HEAD"]);
        let old = GitStructure::read(&repo, &sha, git_manifest::Limits::default()).unwrap();
        let mut built = build(
            &repo,
            &sha,
            "synthetic:s6",
            &root.join("output"),
            Limits {
                source_count: 100,
                source_bytes: 1_000_000,
                total_source_bytes: 4_000_000,
                sqlite_bytes: 4_000_000,
            },
        )
        .unwrap();
        if ingest_mode {
            // Inventory exceeds both old Git ceilings, selected XML/BSL remains small.
            for i in 0..5 {
                fs::write(
                    repo.join(format!("ignored-{i}.bin")),
                    vec![b'x'; 7 * 1024 * 1024],
                )
                .unwrap();
            }
            git(&["add", "."]);
            git(&[
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@invalid",
                "commit",
                "-qm",
                "large excluded bodies",
            ]);
            let bytes_sha = git(&["rev-parse", "HEAD"]);
            assert!(git_manifest::Manifest::read(
                &repo,
                &bytes_sha,
                git_manifest::Limits::default()
            )
            .is_err());
            for i in 0..4200 {
                fs::write(repo.join(format!("inventory-{i:04}.txt")), b"inventory").unwrap();
            }
            git(&["add", "."]);
            git(&[
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@invalid",
                "commit",
                "-qm",
                "large inventory",
            ]);
            let sha = git(&["rev-parse", "HEAD"]);
            let tree = git(&["rev-parse", "HEAD^{tree}"]);
            assert!(
                git_manifest::Manifest::read(&repo, &sha, git_manifest::Limits::default()).is_err()
            );
            let run = |dir: &str, source_count: &str, sqlite: &str, git_total: &str| {
                Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
                    .args([
                        "build-ingest",
                        repo.to_str().unwrap(),
                        &sha,
                        &tree,
                        "synthetic:ingest",
                        root.join(dir).to_str().unwrap(),
                        source_count,
                        "1000000",
                        "4000000",
                        sqlite,
                        "5000",
                        "100",
                        "5000",
                        "1000000",
                        git_total,
                        "30000",
                    ])
                    .output()
                    .unwrap()
            };
            let receipt = bslens_core::git_manifest::ingest::ingest(
                &repo,
                &sha,
                &tree,
                bslens_core::git_manifest::ingest::Limits {
                    entries: 5000,
                    trees: 100,
                    reads: 5000,
                    object_bytes: 1000000,
                    total_bytes: 4000000,
                    deadline: Duration::from_secs(30),
                },
                |_, _, _| Ok(()),
            )
            .unwrap();
            assert!(receipt.selected > 1);
            let late_budget = (receipt.bytes - 1).to_string();
            // Count fails after one INSERT; Git budget fails on the last selected blob.
            // SQLite's small page limit may fail earlier while creating the schema.
            for (dir, count, sqlite, total) in [
                ("fail-count", "1", "4000000", "4000000"),
                ("fail-pages", "100", "4096", "4000000"),
                ("fail-git", "100", "4000000", late_budget.as_str()),
            ] {
                let out = run(dir, count, sqlite, total);
                assert!(!out.status.success(), "unexpected success: {dir}");
                if dir == "fail-git" {
                    assert!(String::from_utf8_lossy(&out.stderr).contains("total_bytes"));
                }
                if dir == "fail-count" {
                    assert!(String::from_utf8_lossy(&out.stderr).contains("source_count"));
                }
                assert_eq!(
                    fs::read_dir(root.join(dir)).unwrap().count(),
                    0,
                    "leftovers {dir}"
                );
            }
            let above_facts_ceiling = run("above-facts-ceiling", "4097", "4000000", "4000000");
            assert!(!above_facts_ceiling.status.success());
            assert!(String::from_utf8_lossy(&above_facts_ceiling.stderr).contains("source_count"));
            assert!(!root.join("above-facts-ceiling").exists());
            let out = run("ingest-output", "100", "4000000", "4000000");
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let value: Value = serde_json::from_slice(&out.stdout).unwrap();
            let repeat = run("ingest-output", "100", "4000000", "4000000");
            assert!(repeat.status.success());
            let repeat: Value = serde_json::from_slice(&repeat.stdout).unwrap();
            assert_eq!(repeat["snapshot_id"], value["snapshot_id"]);
            assert_eq!(repeat["reused"], true);
            let changed = run("other-budget", "101", "4000000", "4000000");
            assert!(changed.status.success());
            let changed: Value = serde_json::from_slice(&changed.stdout).unwrap();
            assert_ne!(changed["snapshot_id"], value["snapshot_id"]);
            assert_ne!(built.snapshot_id, value["snapshot_id"]);
            built = BuiltSnapshot {
                path: PathBuf::from(value["path"].as_str().unwrap()),
                snapshot_id: value["snapshot_id"].as_str().unwrap().into(),
                facts_sha256: value["facts_sha256"].as_str().unwrap().into(),
                file_sha256: value["file_sha256"].as_str().unwrap().into(),
                reused: false,
            };
        }
        if mode == 2 || mode == 3 {
            let mut args: Vec<String> = vec![
                "build-stream",
                repo.to_str().unwrap(),
                &sha,
                &git(&["rev-parse", "HEAD^{tree}"]),
                "synthetic:s6",
                root.join("stream-small").to_str().unwrap(),
                "5000",
                "8388608",
                "67108864",
                "134217728",
                "10000",
                "100",
                "12000",
                "8388608",
                "70000000",
                "30000",
                "100000",
                "1048576",
                "30000",
            ]
            .into_iter()
            .map(String::from)
            .collect();
            if mode >= 14 {
                args[6] = "400".into();
            }
            if mode == 16 {
                args[6] = "600".into();
                args.push(corpus_opaque_json());
                let inventory = Command::new("git")
                    .arg("-C")
                    .arg(&repo)
                    .args(["ls-tree", "-r", "-z", "HEAD"])
                    .output()
                    .unwrap();
                assert!(inventory.status.success());
                let entries: Vec<Value>=inventory.stdout.split(|b| *b==0).filter(|b| !b.is_empty()).map(|b| {
                    let (header,path)=b.split_at(b.iter().position(|b| *b==b'\t').unwrap());
                    let h=std::str::from_utf8(header).unwrap().split(' ').collect::<Vec<_>>();
                    json!({"path":std::str::from_utf8(&path[1..]).unwrap(),"mode":h[0],"blob_oid":h[2]})
                }).collect();
                fs::write(
                    root.join("git-inventory.json"),
                    serde_json::to_vec(&entries).unwrap(),
                )
                .unwrap();
            }
            let run = |args: &[String]| {
                let args = if args.len() == 20 && args[0] != "build-corpus" {
                    &args[..19]
                } else {
                    args
                };
                Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
                    .args(args)
                    .output()
                    .unwrap()
            };
            let out = run(&args);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let small: Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(small["facts_sha256"], built.facts_sha256);
            assert_ne!(small["snapshot_id"], built.snapshot_id);
            if mode == 2 {
                for i in 0..4200 {
                    fs::write(repo.join(format!("Stream{i:04}.bsl")), b"").unwrap();
                }
            } else {
                let bytes = vec![b'x'; 4 * 1024 * 1024];
                for i in 0..9 {
                    fs::write(repo.join(format!("Stream{i:04}.bsl")), &bytes).unwrap();
                }
            }
            git(&["add", "."]);
            git(&[
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@invalid",
                "commit",
                "-qm",
                "selected streaming corpus",
            ]);
            args[2] = git(&["rev-parse", "HEAD"]);
            args[3] = git(&["rev-parse", "HEAD^{tree}"]);
            let legacy = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
                .args([
                    "build",
                    repo.to_str().unwrap(),
                    &args[2],
                    "synthetic:s6",
                    root.join("legacy-large").to_str().unwrap(),
                    "5000",
                    "8388608",
                    "67108864",
                    "134217728",
                ])
                .output()
                .unwrap();
            assert!(!legacy.status.success());
            let limited = [
                ("stream-facts-fail", 16, "5"),
                ("stream-count-fail", 6, "5"),
                ("stream-pages-fail", 9, "4096"),
                ("stream-bytes-fail", 8, "1"),
            ];
            for (dir, index, value) in limited {
                let mut bad = args.clone();
                bad[5] = root.join(dir).to_str().unwrap().into();
                bad[index] = value.into();
                let out = run(&bad);
                assert!(!out.status.success(), "{dir}");
                assert_eq!(fs::read_dir(root.join(dir)).unwrap().count(), 0);
            }
            args[5] = root.join("stream-large").to_str().unwrap().into();
            let out = run(&args);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let value: Value = serde_json::from_slice(&out.stdout).unwrap();
            let out = run(&args);
            assert!(out.status.success());
            assert_eq!(
                serde_json::from_slice::<Value>(&out.stdout).unwrap()["reused"],
                true
            );
            let mut changed = args.clone();
            changed[5] = root.join("stream-other-policy").to_str().unwrap().into();
            changed[16] = "100001".into();
            let out = run(&changed);
            assert!(out.status.success());
            assert_ne!(
                serde_json::from_slice::<Value>(&out.stdout).unwrap()["snapshot_id"],
                value["snapshot_id"]
            );
            built = BuiltSnapshot {
                path: PathBuf::from(value["path"].as_str().unwrap()),
                snapshot_id: value["snapshot_id"].as_str().unwrap().into(),
                facts_sha256: value["facts_sha256"].as_str().unwrap().into(),
                file_sha256: value["file_sha256"].as_str().unwrap().into(),
                reused: false,
            };
            let xml = fs::read_to_string(repo.join("Documents/Order.xml"))
                .unwrap()
                .replace(
                    "<ChildObjects></ChildObjects>",
                    "<ChildObjects><Attribute>A</Attribute></ChildObjects>",
                );
            fs::write(repo.join("Documents/Order.xml"), xml).unwrap();
            git(&["add", "."]);
            git(&[
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@invalid",
                "commit",
                "-qm",
                "strict nested refusal",
            ]);
            args[2] = git(&["rev-parse", "HEAD"]);
            args[3] = git(&["rev-parse", "HEAD^{tree}"]);
            args[5] = root.join("stream-nested-fail").to_str().unwrap().into();
            let out = run(&args);
            assert!(!out.status.success());
            assert!(String::from_utf8_lossy(&out.stderr).contains("NestedUnsupported"));
            assert_eq!(
                fs::read_dir(root.join("stream-nested-fail"))
                    .unwrap()
                    .count(),
                0
            );
        }
        if mode == 4 {
            let decl = |tag: &str, name: &str, id: u64, children: &str| {
                format!("<{tag} uuid=\"00000000-0000-0000-0000-{id:012}\"><Properties><Name>{name}</Name><Uninterpreted>value</Uninterpreted></Properties>{children}</{tag}>")
            };
            for (path, base_id) in [
                ("Catalogs/Items.xml", 100u64),
                ("Documents/Order.xml", 200),
                ("InformationRegisters/Events.xml", 300),
            ] {
                let mut children = decl("Attribute", "ОбщийРеквизит", base_id, "")
                    + &decl("Command", "Команда", base_id + 1, "");
                if base_id != 300 {
                    let nested = decl("Attribute", "Колонка", base_id + 3, "");
                    children+=&decl("TabularSection","Строки",base_id+2,&format!("<AdditionalSetting><NotAnalyzed/></AdditionalSetting><ChildObjects>{nested}</ChildObjects>"));
                } else {
                    children += &decl("Dimension", "Измерение", base_id + 2, "");
                    children += &decl("Resource", "Ресурс", base_id + 3, "");
                }
                let xml = fs::read_to_string(repo.join(path)).unwrap();
                let xml = xml.replace(
                    "<ChildObjects></ChildObjects>",
                    &format!("<ChildObjects>{children}</ChildObjects>"),
                );
                assert!(xml.contains("ОбщийРеквизит"));
                fs::write(
                    repo.join(path),
                    if base_id == 100 {
                        format!("\u{feff}{xml}")
                    } else {
                        xml
                    },
                )
                .unwrap();
            }
            git(&["add", "."]);
            git(&[
                "-c",
                "user.name=Synthetic",
                "-c",
                "user.email=synthetic@invalid",
                "commit",
                "-qm",
                "confirmed inline synthetic",
            ]);
            let mut args: Vec<String> = vec![
                "build-inline",
                repo.to_str().unwrap(),
                &git(&["rev-parse", "HEAD"]),
                &git(&["rev-parse", "HEAD^{tree}"]),
                "synthetic:inline",
                root.join("inline-output").to_str().unwrap(),
                "100",
                "1000000",
                "4000000",
                "4000000",
                "5000",
                "100",
                "5000",
                "1000000",
                "4000000",
                "30000",
                "100000",
                "1048576",
                "30000",
            ]
            .into_iter()
            .map(String::from)
            .collect();
            let run = |a: &[String]| {
                Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
                    .args(a)
                    .output()
                    .unwrap()
            };
            let out = run(&args);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let value: Value = serde_json::from_slice(&out.stdout).unwrap();
            let mut budget_fail = args.clone();
            budget_fail[5] = root.join("inline-budget-fail").to_str().unwrap().into();
            budget_fail[16] = "1".into();
            let refused = run(&budget_fail);
            assert!(!refused.status.success());
            assert_eq!(fs::read_dir(&budget_fail[5]).unwrap().count(), 0);
            let repeat = run(&args);
            assert!(repeat.status.success());
            assert_eq!(
                serde_json::from_slice::<Value>(&repeat.stdout).unwrap()["reused"],
                true
            );
            let mut legacy = args[..16].to_vec();
            legacy[0] = "build-stream".into();
            legacy.extend_from_slice(&args[16..]);
            legacy[5] = root.join("old-inline-refused").to_str().unwrap().into();
            let out = run(&legacy);
            assert!(!out.status.success());
            assert!(String::from_utf8_lossy(&out.stderr).contains("NestedUnsupported"));
            assert_eq!(
                fs::read_dir(root.join("old-inline-refused"))
                    .unwrap()
                    .count(),
                0
            );
            let new_path = PathBuf::from(value["path"].as_str().unwrap());
            let parity_path = root.join("parity.sqlite");
            fs::copy(&new_path, &parity_path).unwrap();
            let db = rusqlite::Connection::open(&parity_path).unwrap();
            db.execute_batch("PRAGMA foreign_keys=OFF;DROP TABLE inline_declarations;")
                .unwrap();
            assert_eq!(
                bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap(),
                built.facts_sha256
            );
            drop(db);
            fs::remove_file(parity_path).unwrap();
            built = BuiltSnapshot {
                path: new_path,
                snapshot_id: value["snapshot_id"].as_str().unwrap().into(),
                facts_sha256: value["facts_sha256"].as_str().unwrap().into(),
                file_sha256: value["file_sha256"].as_str().unwrap().into(),
                reused: false,
            };
            let original = fs::read_to_string(repo.join("Catalogs/Items.xml")).unwrap();
            for (i,replacement) in ["<Form>Форма</Form>","<Template>Макет</Template>","<Unknown/>","<Attribute uuid=\"bad\"><Properties><Name>A</Name></Properties></Attribute>","<Resource uuid=\"00000000-0000-0000-0000-000000000999\"><Properties><Name>A</Name></Properties></Resource>"].iter().enumerate() {
                let xml=original.replace("</ChildObjects></TabularSection>",&format!("{replacement}</ChildObjects></TabularSection>"));
                assert_ne!(xml,original);fs::write(repo.join("Catalogs/Items.xml"),xml).unwrap();git(&["add","."]);git(&["-c","user.name=Synthetic","-c","user.email=synthetic@invalid","commit","-qm","refused inline shape"]);
                args[2]=git(&["rev-parse","HEAD"]);args[3]=git(&["rev-parse","HEAD^{tree}"]);args[5]=root.join(format!("inline-refused-{i}")).to_str().unwrap().into();let out=run(&args);assert!(!out.status.success(),"{replacement}");assert_eq!(fs::read_dir(&args[5]).unwrap().count(),0);
            }
        }

        if (5..=16).contains(&mode) {
            let legacy_path = built.path.clone();
            let root_xml = format!(
                "{}{}",
                char::from_u32(0xfeff).unwrap(),
                fs::read_to_string(repo.join("Catalogs/Items.xml")).unwrap()
            );
            let updated=root_xml.replace("<ChildObjects></ChildObjects>","<ChildObjects><Form>Card</Form><Form>Empty</Form><Template>Print</Template><Template>MissingPayload</Template></ChildObjects>");
            assert_ne!(updated, root_xml);
            fs::write(repo.join("Catalogs/Items.xml"), &updated).unwrap();
            let descriptor = |kind: &str, name: &str, n: u32| {
                format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{kind} uuid=\"10000000-0000-0000-0000-{n:012}\"><Properties><Name>{name}</Name></Properties></{kind}></MetaDataObject>")
            };
            for (path, body) in [
                (
                    "Catalogs/Items/Forms/Card.xml",
                    descriptor("Form", "Card", 1).into_bytes(),
                ),
                (
                    "Catalogs/Items/Forms/Empty.xml",
                    descriptor("Form", "Empty", 2).into_bytes(),
                ),
                (
                    "Catalogs/Items/Templates/Print.xml",
                    descriptor("Template", "Print", 3).into_bytes(),
                ),
                (
                    "Catalogs/Items/Templates/MissingPayload.xml",
                    descriptor("Template", "MissingPayload", 4).into_bytes(),
                ),
                (
                    "Catalogs/Items/Forms/Card/Ext/Form.xml",
                    b"<OpaqueForm/>".to_vec(),
                ),
                (
                    "Catalogs/Items/Forms/Card/Ext/Form/Module.bsl",
                    TEXT.as_bytes().to_vec(),
                ),
                ("Catalogs/Items/Forms/Card/Ext/Extra.bin", BINARY.to_vec()),
                (
                    "Catalogs/Items/Forms/Card/Other.bsl",
                    TEXT.as_bytes().to_vec(),
                ),
                ("Catalogs/Items/Forms/Card/Big.bin", vec![0xff; 50000]),
                (
                    "Catalogs/Items/Templates/Print/Ext/Template.bin",
                    BINARY.to_vec(),
                ),
            ] {
                let path = repo.join(path);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, body).unwrap();
            }
            for (owner, kind, name, n, payload) in [
                ("Documents/Order", "Template", "Текст", 5, "Template.txt"),
                (
                    "InformationRegisters/Events",
                    "Template",
                    "Xml",
                    6,
                    "Template.xml",
                ),
            ] {
                let source = repo.join(format!("{owner}.xml"));
                let original = fs::read_to_string(&source).unwrap();
                let updated = original.replace(
                    "<ChildObjects></ChildObjects>",
                    &format!("<ChildObjects><Template>{name}</Template></ChildObjects>"),
                );
                assert_ne!(original, updated);
                fs::write(source, updated).unwrap();
                let file = repo.join(format!("{owner}/Templates/{name}.xml"));
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(file, descriptor(kind, name, n)).unwrap();
                let file = repo.join(format!("{owner}/Templates/{name}/Ext/{payload}"));
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(file, b"opaque, not parsed").unwrap();
            }
            if (6..=16).contains(&mode) {
                let cfg = repo.join("Configuration.xml");
                let xml = fs::read_to_string(&cfg).unwrap();
                fs::write(cfg,xml.replace("</ChildObjects>","<CommonModule>Shared</CommonModule><CommonModule>Отказ</CommonModule></ChildObjects>")).unwrap();
                for (name, n, bytes) in [("Shared", 100, TEXT.as_bytes()), ("Отказ", 101, BINARY)]
                {
                    let xml = repo.join(format!("CommonModules/{name}.xml"));
                    fs::create_dir_all(xml.parent().unwrap()).unwrap();
                    fs::write(xml, descriptor("CommonModule", name, n)).unwrap();
                    let module = repo.join(format!("CommonModules/{name}/Ext/Module.bsl"));
                    fs::create_dir_all(module.parent().unwrap()).unwrap();
                    fs::write(module, bytes).unwrap();
                }
            }
            if (7..=16).contains(&mode) {
                let cfg = repo.join("Configuration.xml");
                let mut xml = fs::read_to_string(&cfg).unwrap();
                for (i, (kind, folder)) in DESCRIPTOR_ROOTS.iter().enumerate() {
                    xml = xml.replace(
                        "</ChildObjects>",
                        &format!("<{kind}>Пример</{kind}></ChildObjects>"),
                    );
                    let path = repo.join(format!("{folder}/Пример.xml"));
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path,format!("{}{}",char::from_u32(0xfeff).unwrap(),descriptor(kind,"Пример",200+i as u32).replace("<Properties>","<InternalInfo><UnknownSemanticProperty/></InternalInfo><Properties>"))).unwrap();
                }
                fs::write(cfg, xml).unwrap();
            }
            if mode >= 8 {
                let cfg = repo.join("Configuration.xml");
                let mut xml = fs::read_to_string(&cfg).unwrap();
                for (i, (kind, folder, name)) in SHARED_OBJECTS.iter().enumerate() {
                    xml = xml.replace(
                        "</ChildObjects>",
                        &format!("<{kind}>{name}</{kind}></ChildObjects>"),
                    );
                    let path = repo.join(format!("{folder}/{name}.xml"));
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, descriptor(kind, name, 400 + i as u32)).unwrap();
                }
                fs::write(cfg, format!("{}{}", char::from_u32(0xfeff).unwrap(), xml)).unwrap();
                for (path, bytes) in SHARED_PAYLOADS.iter().copied().chain([
                    ("CommonCommands/Act/Ext/CommandModule.bsl", TEXT.as_bytes()),
                    ("CommonCommands/Refused/Ext/CommandModule.bsl", BINARY),
                    ("CommonForms/Card/Ext/Form/Module.bsl", TEXT.as_bytes()),
                ]) {
                    let path = repo.join(path);
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, bytes).unwrap();
                }
            }
            if mode >= 9 {
                let cfg = repo.join("Configuration.xml");
                let mut xml = fs::read_to_string(&cfg).unwrap();
                for (i, (kind, folder, name)) in LEAF_OBJECTS.iter().enumerate() {
                    xml = xml.replace(
                        "</ChildObjects>",
                        &format!("<{kind}>{name}</{kind}></ChildObjects>"),
                    );
                    let path = repo.join(format!("{folder}/{name}.xml"));
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, descriptor(kind, name, 600 + i as u32)).unwrap();
                }
                fs::write(cfg, xml).unwrap();
                for (path, bytes) in LEAF_PAYLOADS.iter().chain(LEAF_BSL) {
                    let p = repo.join(path);
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    fs::write(p, bytes).unwrap();
                }
            }
            if mode >= 10 {
                let cfg = repo.join("Configuration.xml");
                let mut xml = fs::read_to_string(&cfg).unwrap();
                for (i, (kind, folder, name)) in JOURNAL_OBJECTS.iter().enumerate() {
                    xml = xml.replace(
                        "</ChildObjects>",
                        &format!("<{kind}>{name}</{kind}></ChildObjects>"),
                    );
                    let relative = format!("{folder}/{name}.xml");
                    let path = repo.join(&relative);
                    let mut body = String::new();
                    for (p, k, n, u) in JOURNAL_INLINE {
                        if *p == relative {
                            body.push_str(&journal_inline(k, n, *u));
                        }
                    }
                    if *kind == "DocumentJournal" {
                        body.push_str("<Form>Card</Form>");
                        if *name == "Full" {
                            body.push_str("<Template>Report</Template>");
                        }
                    }
                    let content=descriptor(kind,name,800+i as u32).replace("</Properties>","<DefaultForm>unverified</DefaultForm><AuxiliaryForm/><RegisteredDocuments/></Properties>").replace(&format!("</{kind}>"),&format!("<ChildObjects>{body}</ChildObjects></{kind}>"));
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, format!("\u{feff}{content}")).unwrap();
                }
                fs::write(cfg, xml).unwrap();
                for (path, kind, name, n) in [
                    ("DocumentJournals/Full/Forms/Card.xml", "Form", "Card", 850),
                    ("DocumentJournals/Thin/Forms/Card.xml", "Form", "Card", 851),
                    (
                        "DocumentJournals/Full/Templates/Report.xml",
                        "Template",
                        "Report",
                        852,
                    ),
                ] {
                    let p = repo.join(path);
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    fs::write(p, descriptor(kind, name, n)).unwrap();
                }
                for (path, bytes) in JOURNAL_PAYLOADS.iter().chain(JOURNAL_BSL) {
                    let p = repo.join(path);
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    fs::write(p, bytes).unwrap();
                }
            }
            if mode >= 11 {
                let cfg = repo.join("Configuration.xml");
                let mut xml = fs::read_to_string(&cfg).unwrap();
                for (i, (kind, folder, name)) in SERVICE_ROOTS.iter().enumerate() {
                    xml = xml.replace(
                        "</ChildObjects>",
                        &format!("<{kind}>{name}</{kind}></ChildObjects>"),
                    );
                    let p = repo.join(format!("{folder}/{name}.xml"));
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    let internal = if *kind == "IntegrationService" {
                        "<InternalInfo><Opaque/></InternalInfo>"
                    } else {
                        ""
                    };
                    let content = descriptor(kind, name, 1100 + i as u32).replace(
                        &format!("</{kind}>"),
                        &format!(
                            "{internal}<ChildObjects>{}</ChildObjects></{kind}>",
                            service_body(kind)
                        ),
                    );
                    fs::write(p, format!("\u{feff}{content}")).unwrap();
                    let m = repo.join(format!("{folder}/{name}/Ext/Module.bsl"));
                    fs::create_dir_all(m.parent().unwrap()).unwrap();
                    fs::write(
                        m,
                        if *kind == "IntegrationService" {
                            BINARY
                        } else {
                            TEXT.as_bytes()
                        },
                    )
                    .unwrap();
                }
                fs::write(cfg, xml).unwrap();
            }
            if mode >= 12 {
                let cfg = repo.join("Configuration.xml");
                let mut xml = fs::read_to_string(&cfg).unwrap();
                for (i, (kind, folder, name)) in CHART_ROOTS.iter().enumerate() {
                    xml = xml.replace(
                        "</ChildObjects>",
                        &format!("<{kind}>{name}</{kind}></ChildObjects>"),
                    );
                    let path = repo.join(format!("{folder}/{name}.xml"));
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    let content=descriptor(kind,name,1300+i as u32).replace(&format!("</{kind}>"),&format!("<InternalInfo><Opaque/></InternalInfo><ChildObjects>{}</ChildObjects></{kind}>",chart_body(kind)));
                    fs::write(path, format!("\u{feff}{content}")).unwrap();
                    let module = repo.join(format!("{folder}/{name}/Ext/ManagerModule.bsl"));
                    fs::create_dir_all(module.parent().unwrap()).unwrap();
                    fs::write(module, TEXT).unwrap();
                    if *kind != "ChartOfCalculationTypes" {
                        fs::write(
                            repo.join(format!("{folder}/{name}/Ext/ObjectModule.bsl")),
                            if *kind == "ChartOfAccounts" {
                                TEXT.as_bytes()
                            } else {
                                BINARY
                            },
                        )
                        .unwrap();
                    }
                    let form = repo.join(format!("{folder}/{name}/Forms/Card.xml"));
                    fs::create_dir_all(form.parent().unwrap()).unwrap();
                    fs::write(form, descriptor("Form", "Card", 1350 + i as u32)).unwrap();
                    let module =
                        repo.join(format!("{folder}/{name}/Forms/Card/Ext/Form/Module.bsl"));
                    fs::create_dir_all(module.parent().unwrap()).unwrap();
                    fs::write(module, TEXT).unwrap();
                }
                fs::write(cfg, xml).unwrap();
                for (path, bytes) in CHART_PAYLOADS {
                    let path = repo.join(path);
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, bytes).unwrap();
                }
            }
            if mode >= 13 {
                let cfg = repo.join("Configuration.xml");
                let mut xml = fs::read_to_string(&cfg).unwrap();
                for (i, (kind, folder, name)) in REGISTER_ROOTS.iter().enumerate() {
                    xml = xml.replace(
                        "</ChildObjects>",
                        &format!("<{kind}>{name}</{kind}></ChildObjects>"),
                    );
                    let path = repo.join(format!("{folder}/{name}.xml"));
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    let content = register_descriptor(
                        kind,
                        name,
                        1500 + i as u32,
                        &register_body(kind, name),
                    );
                    fs::write(path, format!("\u{feff}{content}")).unwrap();
                    let ext = repo.join(format!("{folder}/{name}/Ext"));
                    fs::create_dir_all(&ext).unwrap();
                    if *kind == "AccountingRegister" {
                        fs::write(ext.join("RecordSetModule.bsl"), TEXT).unwrap();
                    }
                    if *name == "WithCode" {
                        fs::write(ext.join("RecordSetModule.bsl"), BINARY).unwrap();
                        fs::write(ext.join("ManagerModule.bsl"), TEXT).unwrap();
                        fs::write(ext.join("Aggregates.xml"), b"<OpaqueAggregate/>").unwrap();
                    }
                    fs::write(ext.join("Help.xml"), b"<Help opaque='true'/>").unwrap();
                    fs::create_dir_all(ext.join("Help")).unwrap();
                    fs::write(ext.join("Help/ru.html"), b"<html>help</html>").unwrap();
                    let form = repo.join(format!("{folder}/{name}/Forms/Card.xml"));
                    fs::create_dir_all(form.parent().unwrap()).unwrap();
                    fs::write(form, descriptor("Form", "Card", 1550 + i as u32)).unwrap();
                    let form_ext = repo.join(format!("{folder}/{name}/Forms/Card/Ext"));
                    fs::create_dir_all(form_ext.join("Form")).unwrap();
                    fs::write(form_ext.join("Form.xml"), b"<Form opaque='true'/>").unwrap();
                    fs::write(form_ext.join("Form/Module.bsl"), TEXT).unwrap();
                }
                fs::write(cfg, xml).unwrap();
                let p = repo.join("CalculationRegisters/Calc/Recalculations/Rebuild.xml");
                fs::create_dir_all(p.parent().unwrap()).unwrap();
                fs::write(
                    p,
                    format!(
                        "\u{feff}{}",
                        register_descriptor(
                            "Recalculation",
                            "Rebuild",
                            1600,
                            &recalculation_body()
                        )
                    ),
                )
                .unwrap();
            }
            if mode >= 14 {
                let cfg = repo.join("Configuration.xml");
                let mut xml = fs::read_to_string(&cfg).unwrap();
                for (i, (kind, folder, name)) in REPORT_ROOTS.iter().enumerate() {
                    xml = xml.replace(
                        "</ChildObjects>",
                        &format!("<{kind}>{name}</{kind}></ChildObjects>"),
                    );
                    let prefix = format!("{folder}/{name}");
                    let path = repo.join(format!("{prefix}.xml"));
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(
                        path,
                        format!(
                            "\u{feff}{}",
                            register_descriptor(kind, name, 6100 + i as u32, &report_body(i))
                        ),
                    )
                    .unwrap();
                    if *name == "Empty" {
                        continue;
                    }
                    let ext = repo.join(format!("{prefix}/Ext"));
                    fs::create_dir_all(&ext).unwrap();
                    for module in report_modules(name) {
                        fs::write(ext.join(format!("{module}.bsl")), TEXT).unwrap();
                    }
                    fs::write(ext.join("Help.xml"), b"<Help/>").unwrap();
                    fs::create_dir_all(ext.join("Help")).unwrap();
                    fs::write(ext.join("Help/ru.html"), b"<html>root</html>").unwrap();
                    let command = repo.join(format!("{prefix}/Commands/Act/Ext/CommandModule.bsl"));
                    fs::create_dir_all(command.parent().unwrap()).unwrap();
                    fs::write(
                        command,
                        if *name == "Overview" {
                            BINARY
                        } else {
                            TEXT.as_bytes()
                        },
                    )
                    .unwrap();
                    for (j, form) in ["Card", "NoCode"].iter().enumerate() {
                        let path = repo.join(format!("{prefix}/Forms/{form}.xml"));
                        fs::create_dir_all(path.parent().unwrap()).unwrap();
                        fs::write(
                            path,
                            descriptor("Form", form, 7000 + i as u32 * 10 + j as u32),
                        )
                        .unwrap();
                        let ext = repo.join(format!("{prefix}/Forms/{form}/Ext"));
                        fs::create_dir_all(ext.join("Form")).unwrap();
                        fs::write(ext.join("Form.xml"), b"<Form opaque='true'/>").unwrap();
                        if j == 0 {
                            fs::write(ext.join("Form/Module.bsl"), TEXT).unwrap();
                            fs::write(ext.join("Help.xml"), b"<Help/>").unwrap();
                            fs::create_dir_all(ext.join("Help")).unwrap();
                            fs::write(ext.join("Help/ru.html"), b"<html>form</html>").unwrap();
                        }
                    }
                    for (j, (name, extension, payload)) in REPORT_TEMPLATES.iter().enumerate() {
                        let path = repo.join(format!("{prefix}/Templates/{name}.xml"));
                        fs::create_dir_all(path.parent().unwrap()).unwrap();
                        fs::write(
                            path,
                            descriptor("Template", name, 8000 + i as u32 * 10 + j as u32),
                        )
                        .unwrap();
                        let ext = repo.join(format!("{prefix}/Templates/{name}/Ext"));
                        fs::create_dir_all(&ext).unwrap();
                        fs::write(ext.join(format!("Template.{extension}")), payload).unwrap();
                    }
                }
                fs::write(cfg, xml).unwrap();
                for (path, bytes) in REPORT_ASSETS {
                    let p = repo.join(path);
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    fs::write(p, bytes).unwrap();
                }
            }
            if mode >= 15 {
                let cfg = repo.join("Configuration.xml");
                let mut xml = fs::read_to_string(&cfg).unwrap();
                for (i, (kind, folder, name)) in WORKFLOW_ROOTS.iter().enumerate() {
                    xml = xml.replace(
                        "</ChildObjects>",
                        &format!("<{kind}>{name}</{kind}></ChildObjects>"),
                    );
                    let prefix = format!("{folder}/{name}");
                    let path = repo.join(format!("{prefix}.xml"));
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(
                        path,
                        format!(
                            "\u{feff}{}",
                            register_descriptor(kind, name, 41000 + i as u32, &workflow_body(i))
                        ),
                    )
                    .unwrap();
                    let ext = repo.join(format!("{prefix}/Ext"));
                    fs::create_dir_all(&ext).unwrap();
                    for m in ["ManagerModule", "ObjectModule"] {
                        fs::write(ext.join(format!("{m}.bsl")), TEXT).unwrap();
                    }
                    if *kind == "ExchangePlan" {
                        fs::write(ext.join("Content.xml"), b"<OpaqueContent/>").unwrap();
                    } else if *kind == "BusinessProcess" {
                        fs::write(ext.join("Flowchart.xml"), b"<OpaqueFlowchart/>").unwrap();
                    }
                    if *name == "Empty" {
                        continue;
                    }
                    fs::write(ext.join("Help.xml"), b"<Help/>").unwrap();
                    fs::create_dir_all(ext.join("Help")).unwrap();
                    fs::write(ext.join("Help/ru.html"), b"<html>root</html>").unwrap();
                    for (j, form) in ["Card", "NoCode"].iter().enumerate() {
                        if j == 1 && *kind != "ExchangePlan" {
                            continue;
                        }
                        let path = repo.join(format!("{prefix}/Forms/{form}.xml"));
                        fs::create_dir_all(path.parent().unwrap()).unwrap();
                        fs::write(
                            path,
                            descriptor("Form", form, 42000 + i as u32 * 10 + j as u32),
                        )
                        .unwrap();
                        let ext = repo.join(format!("{prefix}/Forms/{form}/Ext"));
                        fs::create_dir_all(ext.join("Form")).unwrap();
                        fs::write(ext.join("Form.xml"), b"<OpaqueForm/>").unwrap();
                        if j == 0 {
                            fs::write(ext.join("Form/Module.bsl"), TEXT).unwrap();
                        }
                        fs::write(ext.join("Help.xml"), b"<Help/>").unwrap();
                        fs::create_dir_all(ext.join("Help")).unwrap();
                        fs::write(ext.join("Help/ru.html"), b"<html>form</html>").unwrap();
                    }
                    if *kind == "ExchangePlan" {
                        let path = repo.join(format!("{prefix}/Templates/Text.xml"));
                        fs::create_dir_all(path.parent().unwrap()).unwrap();
                        fs::write(path, descriptor("Template", "Text", 43000)).unwrap();
                        let ext = repo.join(format!("{prefix}/Templates/Text/Ext"));
                        fs::create_dir_all(&ext).unwrap();
                        fs::write(ext.join("Template.txt"), b"opaque\r\nlast").unwrap();
                    }
                    if *kind == "Task" {
                        for n in 0..4 {
                            let path = repo
                                .join(format!("{prefix}/Commands/Act{n}/Ext/CommandModule.bsl"));
                            fs::create_dir_all(path.parent().unwrap()).unwrap();
                            fs::write(path, if n == 3 { BINARY } else { TEXT.as_bytes() }).unwrap();
                        }
                    }
                }
                fs::write(cfg, xml).unwrap();
            }
            if mode == 16 {
                let cfg = repo.join("Configuration.xml");
                let mut xml = fs::read_to_string(&cfg).unwrap();
                for (kind, name) in [
                    ("Sequence", "Order"),
                    ("SettingsStorage", "Ext"),
                    ("Subsystem", "Ext"),
                ] {
                    xml = xml.replace(
                        "</ChildObjects>",
                        &format!("<{kind}>{name}</{kind}></ChildObjects>"),
                    );
                }
                fs::write(cfg, xml).unwrap();
                for (path, kind, name, id, body) in corpus_descriptors() {
                    let p = repo.join(path);
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    fs::write(
                        p,
                        format!("\u{feff}{}", register_descriptor(&kind, &name, id, &body)),
                    )
                    .unwrap();
                }
                let filter = repo.join("FilterCriteria/Пример.xml");
                let x = fs::read_to_string(&filter).unwrap();
                fs::write(
                    filter,
                    x.replace("</FilterCriterion>", "<ChildObjects/></FilterCriterion>"),
                )
                .unwrap();
                for module in [
                    "ManagedApplicationModule",
                    "OrdinaryApplicationModule",
                    "ExternalConnectionModule",
                ] {
                    fs::write(
                        repo.join(format!("Ext/{module}.bsl")),
                        if module == "ExternalConnectionModule" {
                            BINARY
                        } else {
                            TEXT.as_bytes()
                        },
                    )
                    .unwrap();
                }
                for (path, bytes) in CORPUS_GLOBALS {
                    let p = repo.join(path);
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    fs::write(&p, bytes).unwrap();
                    if *path == "Ext/First.bin" {
                        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
                    }
                }
                fs::write(repo.join("outside.dat"), b"explicitly excluded").unwrap();
                let manager = repo.join("SettingsStorages/Ext/Ext/ManagerModule.bsl");
                fs::create_dir_all(manager.parent().unwrap()).unwrap();
                fs::write(manager, TEXT).unwrap();
                for n in 0..13 {
                    let prefix = format!("SettingsStorages/Ext/Forms/Form{n}");
                    let p = repo.join(format!("{prefix}.xml"));
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    fs::write(p, descriptor("Form", &format!("Form{n}"), 61000 + n)).unwrap();
                    let ext = repo.join(format!("{prefix}/Ext"));
                    fs::create_dir_all(ext.join("Form")).unwrap();
                    fs::write(ext.join("Form.xml"), b"<OpaqueForm/>").unwrap();
                    fs::write(ext.join("Form/Module.bsl"), TEXT).unwrap();
                    if n < 8 {
                        fs::write(ext.join("Help.xml"), b"<Help/>").unwrap();
                        fs::create_dir_all(ext.join("Help")).unwrap();
                        fs::write(ext.join("Help/ru.html"), b"<html>form</html>").unwrap();
                    }
                }
                for (path, bytes) in CORPUS_SUBSYSTEM_PAYLOADS {
                    let p = repo.join(path);
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    fs::write(p, bytes).unwrap();
                }
            }
            let commit = || {
                git(&["add", "."]);
                git(&[
                    "-c",
                    "user.name=Synthetic",
                    "-c",
                    "user.email=synthetic@invalid",
                    "commit",
                    "-qm",
                    "companions fixture",
                ]);
            };
            commit();
            let mut args: Vec<String> = vec![
                if mode == 16 {
                    "build-corpus"
                } else if mode >= 15 {
                    "build-workflows"
                } else if mode >= 14 {
                    "build-reports"
                } else if mode >= 13 {
                    "build-registers"
                } else if mode >= 12 {
                    "build-charts"
                } else if mode >= 11 {
                    "build-services"
                } else if mode >= 10 {
                    "build-journals"
                } else if mode >= 9 {
                    "build-leaves"
                } else if mode >= 8 {
                    "build-shared"
                } else if mode == 7 {
                    "build-roots"
                } else if mode == 6 {
                    "build-common-modules"
                } else {
                    "build-companions"
                }
                .into(),
                repo.to_str().unwrap().into(),
                git(&["rev-parse", "HEAD"]),
                git(&["rev-parse", "HEAD^{tree}"]),
                "synthetic:companions".into(),
                root.join("companions").to_str().unwrap().into(),
            ];
            args.extend(
                [
                    "200", "1000000", "4000000", "4000000", "1000", "1000", "1000", "1000000",
                    "4000000", "30000", "100000", "1048576", "30000",
                ]
                .map(String::from),
            );
            if mode >= 14 {
                args[6] = "400".into();
            }
            if mode == 16 {
                args[6] = "600".into();
                args.push(corpus_opaque_json());
                let inventory = Command::new("git")
                    .arg("-C")
                    .arg(&repo)
                    .args(["ls-tree", "-r", "-z", "HEAD"])
                    .output()
                    .unwrap();
                assert!(inventory.status.success());
                let entries: Vec<Value>=inventory.stdout.split(|b| *b==0).filter(|b| !b.is_empty()).map(|b| {
                    let (header,path)=b.split_at(b.iter().position(|b| *b==b'\t').unwrap());
                    let h=std::str::from_utf8(header).unwrap().split(' ').collect::<Vec<_>>();
                    json!({"path":std::str::from_utf8(&path[1..]).unwrap(),"mode":h[0],"blob_oid":h[2]})
                }).collect();
                fs::write(
                    root.join("git-inventory.json"),
                    serde_json::to_vec(&entries).unwrap(),
                )
                .unwrap();
            }
            let run = |args: &[String]| {
                let args = if args.len() == 20 && args[0] != "build-corpus" {
                    &args[..19]
                } else {
                    args
                };
                Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
                    .args(args)
                    .output()
                    .unwrap()
            };
            let out = run(&args);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let v: Value = serde_json::from_slice(&out.stdout).unwrap();
            let second = run(&args);
            assert!(second.status.success());
            let v2: Value = serde_json::from_slice(&second.stdout).unwrap();
            assert_eq!(v["snapshot_id"], v2["snapshot_id"]);
            assert_eq!(v2["reused"], true);
            if mode == 16 {
                let mut normalized = args.clone();
                let mut scope: Vec<String> = serde_json::from_str(&args[19]).unwrap();
                scope.reverse();
                normalized[19] = serde_json::to_string(&scope).unwrap();
                let again = run(&normalized);
                assert!(again.status.success());
                let again: Value = serde_json::from_slice(&again.stdout).unwrap();
                assert_eq!(again["snapshot_id"], v["snapshot_id"]);
                assert_eq!(again["reused"], true);
            }
            built = BuiltSnapshot {
                path: PathBuf::from(v["path"].as_str().unwrap()),
                snapshot_id: v["snapshot_id"].as_str().unwrap().into(),
                facts_sha256: v["facts_sha256"].as_str().unwrap().into(),
                file_sha256: v["file_sha256"].as_str().unwrap().into(),
                reused: false,
            };
            let legacy = rusqlite::Connection::open(&legacy_path).unwrap();
            let fresh = rusqlite::Connection::open(&built.path).unwrap();
            for sql in ["SELECT json_array(m.path,m.parse_status,m.reason_code,o.xml_path) FROM modules m LEFT JOIN objects o ON m.owner_id=o.object_id WHERE m.path NOT LIKE '%/Forms/%' AND m.path NOT LIKE 'CommonModules/%' AND m.path NOT LIKE 'CommonCommands/%' AND m.path NOT LIKE 'CommonForms/%' AND m.path NOT LIKE 'Constants/%' AND m.path NOT LIKE 'Enums/%' AND m.path NOT LIKE 'DocumentJournals/%' AND m.path NOT LIKE 'HTTPServices/%' AND m.path NOT LIKE 'WebServices/%' AND m.path NOT LIKE 'IntegrationServices/%' AND m.path NOT LIKE 'ChartsOfAccounts/%' AND m.path NOT LIKE 'ChartsOfCalculationTypes/%' AND m.path NOT LIKE 'ChartsOfCharacteristicTypes/%' AND path NOT LIKE 'AccountingRegisters/%' AND path NOT LIKE 'AccumulationRegisters/%' AND path NOT LIKE 'CalculationRegisters/%' AND path NOT LIKE 'DataProcessors/%' AND path NOT LIKE 'Reports/%' AND path NOT LIKE 'ExchangePlans/%' AND path NOT LIKE 'BusinessProcesses/%' AND path NOT LIKE 'Tasks/%' AND path NOT LIKE 'Sequences/%' AND path NOT LIKE 'SettingsStorages/%' AND path NOT LIKE 'Subsystems/%' AND path NOT IN ('Ext/ManagedApplicationModule.bsl','Ext/OrdinaryApplicationModule.bsl','Ext/ExternalConnectionModule.bsl') AND m.path NOT LIKE 'AccountingRegisters/%' AND m.path NOT LIKE 'AccumulationRegisters/%' AND m.path NOT LIKE 'CalculationRegisters/%' AND m.path NOT LIKE 'DataProcessors/%' AND m.path NOT LIKE 'Reports/%' AND m.path NOT LIKE 'ExchangePlans/%' AND m.path NOT LIKE 'BusinessProcesses/%' AND m.path NOT LIKE 'Tasks/%' AND m.path NOT LIKE 'Sequences/%' AND m.path NOT LIKE 'SettingsStorages/%' AND m.path NOT LIKE 'Subsystems/%' AND m.path NOT IN ('Ext/ManagedApplicationModule.bsl','Ext/OrdinaryApplicationModule.bsl','Ext/ExternalConnectionModule.bsl') ORDER BY m.path", "SELECT json_array(path,line_number,byte_start,byte_end) FROM source_lines WHERE path NOT LIKE '%/Forms/%' AND path NOT LIKE 'CommonModules/%' AND path NOT LIKE 'CommonCommands/%' AND path NOT LIKE 'CommonForms/%' AND path NOT LIKE 'Constants/%' AND path NOT LIKE 'Enums/%' AND path NOT LIKE 'DocumentJournals/%' AND path NOT LIKE 'HTTPServices/%' AND path NOT LIKE 'WebServices/%' AND path NOT LIKE 'IntegrationServices/%' AND path NOT LIKE 'ChartsOfAccounts/%' AND path NOT LIKE 'ChartsOfCalculationTypes/%' AND path NOT LIKE 'ChartsOfCharacteristicTypes/%' AND path NOT LIKE 'AccountingRegisters/%' AND path NOT LIKE 'AccumulationRegisters/%' AND path NOT LIKE 'CalculationRegisters/%' AND path NOT LIKE 'DataProcessors/%' AND path NOT LIKE 'Reports/%' AND path NOT LIKE 'ExchangePlans/%' AND path NOT LIKE 'BusinessProcesses/%' AND path NOT LIKE 'Tasks/%' AND path NOT LIKE 'Sequences/%' AND path NOT LIKE 'SettingsStorages/%' AND path NOT LIKE 'Subsystems/%' AND path NOT IN ('Ext/ManagedApplicationModule.bsl','Ext/OrdinaryApplicationModule.bsl','Ext/ExternalConnectionModule.bsl') ORDER BY path,line_number", "SELECT json_array(module_path,name,start_line,end_line,byte_start,byte_end) FROM methods WHERE module_path NOT LIKE '%/Forms/%' AND module_path NOT LIKE 'CommonModules/%' AND module_path NOT LIKE 'CommonCommands/%' AND module_path NOT LIKE 'CommonForms/%' AND module_path NOT LIKE 'Constants/%' AND module_path NOT LIKE 'Enums/%' AND module_path NOT LIKE 'DocumentJournals/%' AND module_path NOT LIKE 'HTTPServices/%' AND module_path NOT LIKE 'WebServices/%' AND module_path NOT LIKE 'IntegrationServices/%' AND module_path NOT LIKE 'ChartsOfAccounts/%' AND module_path NOT LIKE 'ChartsOfCalculationTypes/%' AND module_path NOT LIKE 'ChartsOfCharacteristicTypes/%' AND module_path NOT LIKE 'AccountingRegisters/%' AND module_path NOT LIKE 'AccumulationRegisters/%' AND module_path NOT LIKE 'CalculationRegisters/%' AND module_path NOT LIKE 'DataProcessors/%' AND module_path NOT LIKE 'Reports/%' AND module_path NOT LIKE 'ExchangePlans/%' AND module_path NOT LIKE 'BusinessProcesses/%' AND module_path NOT LIKE 'Tasks/%' AND module_path NOT LIKE 'Sequences/%' AND module_path NOT LIKE 'SettingsStorages/%' AND module_path NOT LIKE 'Subsystems/%' AND module_path NOT IN ('Ext/ManagedApplicationModule.bsl','Ext/OrdinaryApplicationModule.bsl','Ext/ExternalConnectionModule.bsl') ORDER BY module_path,start_line", "SELECT json_array(kind,name,xml_path,parent_id) FROM objects WHERE kind IN ('Configuration','Catalog','Document','InformationRegister') ORDER BY xml_path"] {
                let read=|db:&rusqlite::Connection|db.prepare(sql).unwrap().query_map([],|r|r.get::<_,String>(0)).unwrap().collect::<Result<Vec<_>,_>>().unwrap();assert_eq!(read(&legacy),read(&fresh));
            }
            drop(legacy);
            drop(fresh);
            let mut old_args = args.clone();
            old_args[0] = "build-inline".into();
            old_args[5] = root
                .join("old-inline-refuses-companions")
                .to_str()
                .unwrap()
                .into();
            assert!(!run(&old_args).status.success());
            assert_eq!(fs::read_dir(&old_args[5]).unwrap().count(), 0);
            if mode == 6 {
                let mut old = args.clone();
                old[0] = "build-companions".into();
                old[5] = root.join("old-companion-refused").to_str().unwrap().into();
                assert!(!run(&old).status.success());
                assert_eq!(fs::read_dir(&old[5]).unwrap().count(), 0);
                let config_path = repo.join("Configuration.xml");
                let config = fs::read(&config_path).unwrap();
                let descriptor_path = repo.join("CommonModules/Shared.xml");
                let original = fs::read(&descriptor_path).unwrap();
                let module = repo.join("CommonModules/Shared/Ext/Module.bsl");
                let extra = repo.join("CommonModules/Shared/Ext/Other.bsl");
                for (i, case) in [
                    "missing_descriptor",
                    "missing_module",
                    "orphan",
                    "extra_module",
                    "name",
                    "uuid",
                    "version",
                    "namespace",
                    "childobjects",
                    "wrong_type",
                    "duplicate_reference",
                ]
                .iter()
                .enumerate()
                {
                    fs::write(&config_path, &config).unwrap();
                    fs::write(&descriptor_path, &original).unwrap();
                    fs::write(&module, TEXT.as_bytes()).unwrap();
                    if extra.exists() {
                        fs::remove_file(&extra).unwrap();
                    }
                    let xml = String::from_utf8(original.clone()).unwrap();
                    match *case {
                        "missing_descriptor"=>fs::remove_file(&descriptor_path).unwrap(),"missing_module"=>fs::remove_file(&module).unwrap(),
                        "orphan"=>fs::write(&config_path,String::from_utf8(config.clone()).unwrap().replace("<CommonModule>Shared</CommonModule>","")).unwrap(),
                        "extra_module"=>fs::write(&extra,TEXT.as_bytes()).unwrap(),
                        "name"=>fs::write(&descriptor_path,xml.replace("<Name>Shared</Name>","<Name>Wrong</Name>")).unwrap(),
                        "uuid"=>fs::write(&descriptor_path,descriptor("CommonModule","Shared",101)).unwrap(),
                        "version"=>fs::write(&descriptor_path,xml.replace("2.21","2.20")).unwrap(),
                        "namespace"=>fs::write(&descriptor_path,xml.replace("http://v8.1c.ru/8.3/MDClasses","urn:wrong")).unwrap(),
                        "childobjects"=>fs::write(&descriptor_path,xml.replace("</CommonModule>","<ChildObjects/></CommonModule>")).unwrap(),
                        "wrong_type"=>fs::write(&descriptor_path,xml.replace("CommonModule","Catalog")).unwrap(),
                        _=>fs::write(&config_path,String::from_utf8(config.clone()).unwrap().replace("<CommonModule>Shared</CommonModule>","<CommonModule>Shared</CommonModule><CommonModule>Shared</CommonModule>")).unwrap()
                    }
                    commit();
                    let mut bad = args.clone();
                    bad[2] = git(&["rev-parse", "HEAD"]);
                    bad[3] = git(&["rev-parse", "HEAD^{tree}"]);
                    bad[5] = root
                        .join(format!("common-refused-{i}"))
                        .to_str()
                        .unwrap()
                        .into();
                    let out = run(&bad);
                    assert!(!out.status.success(), "{case}");
                    assert_eq!(fs::read_dir(&bad[5]).unwrap().count(), 0, "{case}");
                }
                fs::write(&config_path, config).unwrap();
                fs::write(descriptor_path, original).unwrap();
                fs::write(module, TEXT.as_bytes()).unwrap();
                if extra.exists() {
                    fs::remove_file(extra).unwrap();
                }
                commit();
            }
            if mode == 7 {
                let mut old = args.clone();
                old[0] = "build-common-modules".into();
                old[5] = root
                    .join("old-common-refused-roots")
                    .to_str()
                    .unwrap()
                    .into();
                assert!(!run(&old).status.success());
                assert_eq!(fs::read_dir(&old[5]).unwrap().count(), 0);
                let cfg = repo.join("Configuration.xml");
                let config = fs::read_to_string(&cfg).unwrap();
                let path = repo.join("StyleItems/Пример.xml");
                let original = fs::read_to_string(&path).unwrap();
                let extra = repo.join("StyleItems/Пример/Ext/Payload.bin");
                let wrong = repo.join("FilterCriterions/Пример.xml");
                let correct = repo.join("FilterCriteria/Пример.xml");
                let filter = fs::read(&correct).unwrap();
                for (i, case) in [
                    "unknown_root",
                    "missing",
                    "type",
                    "name",
                    "uuid",
                    "namespace",
                    "childobjects",
                    "unknown_nested",
                    "duplicate_uuid",
                    "binary_companion",
                    "bsl_companion",
                    "plural_guess",
                    "case",
                    "duplicate_reference",
                ]
                .iter()
                .enumerate()
                {
                    fs::write(&cfg, &config).unwrap();
                    fs::write(&path, &original).unwrap();
                    fs::write(&correct, &filter).unwrap();
                    if extra.exists() {
                        fs::remove_file(&extra).unwrap();
                    }
                    let bsl = extra.with_extension("bsl");
                    if bsl.exists() {
                        fs::remove_file(&bsl).unwrap();
                    }
                    if wrong.exists() {
                        fs::remove_file(&wrong).unwrap();
                    }
                    match *case {
                        "unknown_root" => fs::write(
                            &cfg,
                            config.replace(
                                "</ChildObjects>",
                                "<UnsupportedRoot>X</UnsupportedRoot></ChildObjects>",
                            ),
                        )
                        .unwrap(),
                        "missing" => fs::remove_file(&path).unwrap(),
                        "type" => {
                            fs::write(&path, original.replace("StyleItem", "Language")).unwrap()
                        }
                        "name" => fs::write(
                            &path,
                            original.replace("<Name>Пример</Name>", "<Name>Другое</Name>"),
                        )
                        .unwrap(),
                        "uuid" => fs::write(
                            &path,
                            original.replace("uuid=\"10000000", "uuid=\"zzzzzzzz"),
                        )
                        .unwrap(),
                        "namespace" => fs::write(
                            &path,
                            original.replace("http://v8.1c.ru/8.3/MDClasses", "urn:wrong"),
                        )
                        .unwrap(),
                        "childobjects" => fs::write(
                            &path,
                            original.replace("</StyleItem>", "<ChildObjects/></StyleItem>"),
                        )
                        .unwrap(),
                        "unknown_nested" => fs::write(
                            &path,
                            original.replace(
                                "</StyleItem>",
                                "<Unknown><Attribute/></Unknown></StyleItem>",
                            ),
                        )
                        .unwrap(),
                        "duplicate_uuid" => {
                            fs::write(&path, descriptor("StyleItem", "Пример", 200)).unwrap()
                        }
                        "binary_companion" => {
                            fs::create_dir_all(extra.parent().unwrap()).unwrap();
                            fs::write(&extra, b"opaque excluded file must not be ignored").unwrap();
                        }
                        "bsl_companion" => {
                            fs::create_dir_all(bsl.parent().unwrap()).unwrap();
                            fs::write(bsl, TEXT.as_bytes()).unwrap();
                        }
                        "plural_guess" => {
                            fs::create_dir_all(wrong.parent().unwrap()).unwrap();
                            fs::rename(&correct, &wrong).unwrap();
                        }
                        "case" => fs::write(
                            &cfg,
                            config
                                .replace("<StyleItem>", "<styleitem>")
                                .replace("</StyleItem>", "</styleitem>"),
                        )
                        .unwrap(),
                        _ => fs::write(
                            &cfg,
                            config.replace(
                                "<StyleItem>Пример</StyleItem>",
                                "<StyleItem>Пример</StyleItem><StyleItem>Пример</StyleItem>",
                            ),
                        )
                        .unwrap(),
                    }
                    commit();
                    let mut bad = args.clone();
                    bad[2] = git(&["rev-parse", "HEAD"]);
                    bad[3] = git(&["rev-parse", "HEAD^{tree}"]);
                    bad[5] = root
                        .join(format!("roots-refused-{i}"))
                        .to_str()
                        .unwrap()
                        .into();
                    let out = run(&bad);
                    assert!(!out.status.success(), "{case}");
                    assert_eq!(fs::read_dir(&bad[5]).unwrap().count(), 0, "{case}");
                }
                fs::write(cfg, config).unwrap();
                fs::write(path, original).unwrap();
                fs::write(correct, filter).unwrap();
                if extra.exists() {
                    fs::remove_file(&extra).unwrap();
                }
                let bsl = extra.with_extension("bsl");
                if bsl.exists() {
                    fs::remove_file(bsl).unwrap();
                }
                if wrong.exists() {
                    fs::remove_file(wrong).unwrap();
                }
                commit();
            }
            if mode >= 8 {
                let mut old = args.clone();
                old[0] = "build-roots".into();
                old[5] = root
                    .join("old-roots-refused-shared")
                    .to_str()
                    .unwrap()
                    .into();
                assert!(!run(&old).status.success());
                assert_eq!(fs::read_dir(&old[5]).unwrap().count(), 0);
                let cfg = repo.join("Configuration.xml");
                let config = fs::read(&cfg).unwrap();
                let desc = repo.join("CommonForms/Card.xml");
                let original = fs::read_to_string(&desc).unwrap();
                let cases = [
                    (
                        "command_missing",
                        "CommonCommands/Act/Ext/CommandModule.bsl",
                    ),
                    ("form_payload_missing", "CommonForms/БезКода/Ext/Form.xml"),
                    ("help_xml_missing", "CommonForms/Card/Ext/Help.xml"),
                    ("help_html_missing", "CommonForms/Card/Ext/Help/ru.html"),
                    ("template_missing", "CommonTemplates/Text/Ext/Template.txt"),
                    (
                        "template_help_html_missing",
                        "CommonTemplates/Help/Ext/Template/ru.html",
                    ),
                    ("header_budget", "CommonTemplates/Help/Ext/Template.xml"),
                    (
                        "template_html_without_xml",
                        "CommonTemplates/Help/Ext/Template.xml",
                    ),
                    ("ambiguous", "CommonTemplates/Binary/Ext/Template.xml"),
                    (
                        "unsupported_html",
                        "CommonTemplates/Text/Ext/Template/ru.html",
                    ),
                    ("unknown_extra", "CommonTemplates/Binary/Ext/Extra.bin"),
                    ("unknown_form", "CommonForms/Card/Ext/Other.xml"),
                    ("unknown_command", "CommonCommands/Act/Ext/Extra.bsl"),
                    ("descriptor_missing", "CommonForms/Card.xml"),
                    ("descriptor_name", "CommonForms/Card.xml"),
                    ("descriptor_type", "CommonForms/Card.xml"),
                    ("descriptor_uuid", "CommonForms/Card.xml"),
                    ("nested", "CommonForms/Card.xml"),
                    ("orphan", "Configuration.xml"),
                ];
                for (i, (case, target)) in cases.iter().enumerate() {
                    let path = repo.join(target);
                    let previous = fs::read(&path).ok();
                    match *case {
                        "header_budget" => fs::write(&path, vec![b' '; 65537]).unwrap(),
                        "descriptor_name" => fs::write(
                            &path,
                            original.replace("<Name>Card</Name>", "<Name>Wrong</Name>"),
                        )
                        .unwrap(),
                        "descriptor_type" => {
                            fs::write(&path, original.replace("CommonForm", "CommonTemplate"))
                                .unwrap()
                        }
                        "descriptor_uuid" => fs::write(
                            &path,
                            original.replace("uuid=\"10000000", "uuid=\"zzzzzzzz"),
                        )
                        .unwrap(),
                        "nested" => fs::write(
                            &path,
                            original.replace("</CommonForm>", "<ChildObjects/></CommonForm>"),
                        )
                        .unwrap(),
                        "orphan" => fs::write(
                            &path,
                            String::from_utf8(config.clone())
                                .unwrap()
                                .replace("<CommonForm>Card</CommonForm>", ""),
                        )
                        .unwrap(),
                        "ambiguous" | "unsupported_html" | "unknown_extra" | "unknown_form"
                        | "unknown_command" => {
                            fs::create_dir_all(path.parent().unwrap()).unwrap();
                            fs::write(&path, b"opaque unexpected source").unwrap();
                        }
                        _ => fs::remove_file(&path).unwrap(),
                    }
                    commit();
                    let mut bad = args.clone();
                    bad[2] = git(&["rev-parse", "HEAD"]);
                    bad[3] = git(&["rev-parse", "HEAD^{tree}"]);
                    bad[5] = root
                        .join(format!("shared-refused-{i}"))
                        .to_str()
                        .unwrap()
                        .into();
                    let out = run(&bad);
                    assert!(
                        !out.status.success(),
                        "unexpected {case}: {}",
                        String::from_utf8_lossy(&out.stdout)
                    );
                    assert_eq!(fs::read_dir(&bad[5]).unwrap().count(), 0, "{case}");
                    if let Some(bytes) = previous {
                        fs::write(path, bytes).unwrap();
                    } else {
                        fs::remove_file(path).unwrap();
                    }
                }
                commit();
            }
            if mode >= 9 {
                let mut old = args.clone();
                old[0] = "build-shared".into();
                old[5] = root.join("shared-refuses-leaves").to_str().unwrap().into();
                assert!(!run(&old).status.success());
                assert_eq!(fs::read_dir(&old[5]).unwrap().count(), 0);
                let cfg = repo.join("Configuration.xml");
                let config = fs::read_to_string(&cfg).unwrap();
                for (i, (case, target)) in [
                    ("missing", "CommonPictures/Zip/Ext/Picture.xml"),
                    ("missing", "CommonPictures/Png/Ext/Picture/Picture.png"),
                    ("missing", "Roles/Reader/Ext/Rights.xml"),
                    ("missing", "XDTOPackages/Package/Ext/Package.bin"),
                    ("extra", "CommonPictures/Zip/Ext/Picture/Picture.png"),
                    ("extra", "CommonPictures/Ico/Ext/Picture/Picture.jpg"),
                    ("extra", "Constants/None/Ext/Module.bsl"),
                    ("extra", "Roles/Reader/Ext/Other.bin"),
                    ("extra", "ScheduledJobs/Without/Ext/Other.txt"),
                    ("extra", "XDTOPackages/Package/Ext/Package.xml"),
                    ("missing", "Constants/None.xml"),
                    ("nested", "ScheduledJobs/Without.xml"),
                    ("name", "Roles/Reader.xml"),
                    ("type", "Roles/Reader.xml"),
                    ("uuid", "Roles/Reader.xml"),
                    ("namespace", "Roles/Reader.xml"),
                    ("orphan", "Configuration.xml"),
                    ("module_orphan", "Configuration.xml"),
                    ("unknown_root", "Configuration.xml"),
                ]
                .iter()
                .enumerate()
                {
                    let path = repo.join(target);
                    let previous = fs::read(&path).ok();
                    match *case {
                        "missing" => fs::remove_file(&path).unwrap(),
                        "extra" => {
                            fs::create_dir_all(path.parent().unwrap()).unwrap();
                            fs::write(&path, BINARY).unwrap();
                        }
                        "orphan" => fs::write(
                            &path,
                            config.replace("<XDTOPackage>Package</XDTOPackage>", ""),
                        )
                        .unwrap(),
                        "module_orphan" => {
                            fs::write(&path, config.replace("<Constant>Both</Constant>", ""))
                                .unwrap()
                        }
                        "unknown_root" => fs::write(
                            &path,
                            config.replace(
                                "</ChildObjects>",
                                "<UnknownLeaf>Unknown</UnknownLeaf></ChildObjects>",
                            ),
                        )
                        .unwrap(),
                        _ => {
                            let x = String::from_utf8(previous.clone().unwrap()).unwrap();
                            let changed = match *case {
                                "nested" => {
                                    x.replace("</ScheduledJob>", "<ChildObjects/></ScheduledJob>")
                                }
                                "name" => x.replace("<Name>Reader</Name>", "<Name>Wrong</Name>"),
                                "type" => x.replace("Role", "ScheduledJob"),
                                "uuid" => x.replace("uuid=\"10000000", "uuid=\"zzzzzzzz"),
                                "namespace" => {
                                    x.replace("http://v8.1c.ru/8.3/MDClasses", "urn:wrong")
                                }
                                _ => unreachable!(),
                            };
                            fs::write(&path, changed).unwrap();
                        }
                    }
                    commit();
                    let mut bad = args.clone();
                    bad[2] = git(&["rev-parse", "HEAD"]);
                    bad[3] = git(&["rev-parse", "HEAD^{tree}"]);
                    bad[5] = root
                        .join(format!("leaf-refused-{i}"))
                        .to_str()
                        .unwrap()
                        .into();
                    let result = run(&bad);
                    assert!(
                        !result.status.success(),
                        "unexpected success: {case} {target}"
                    );
                    assert_eq!(fs::read_dir(&bad[5]).unwrap().count(), 0, "{case}");
                    if *target == "XDTOPackages/Package/Ext/Package.bin" && *case == "missing" {
                        assert!(String::from_utf8_lossy(&result.stderr)
                            .contains("leaf_payload_missing"));
                    }
                    if let Some(bytes) = previous {
                        fs::write(path, bytes).unwrap();
                    } else {
                        fs::remove_file(path).unwrap();
                    }
                }
                commit();
                // Independent bounded fact failure after ingestion: no published file.
                let mut bad = args.clone();
                bad[2] = git(&["rev-parse", "HEAD"]);
                bad[3] = git(&["rev-parse", "HEAD^{tree}"]);
                bad[5] = root.join("leaf-fact-limit").to_str().unwrap().into();
                bad[16] = "20".into();
                assert!(!run(&bad).status.success());
                assert_eq!(fs::read_dir(&bad[5]).unwrap().count(), 0);
            }
            if mode >= 10 {
                let mut old = args.clone();
                old[0] = "build-leaves".into();
                old[5] = root.join("leaves-refuse-journals").to_str().unwrap().into();
                assert!(!run(&old).status.success());
                assert_eq!(fs::read_dir(&old[5]).unwrap().count(), 0);
                for (i, (case, target)) in [
                    (
                        "missing",
                        "DocumentJournals/Thin/Commands/Run/Ext/CommandModule.bsl",
                    ),
                    ("missing", "DocumentJournals/Full/Forms/Card.xml"),
                    ("missing", "DocumentJournals/Thin/Forms/Card/Ext/Form.xml"),
                    (
                        "missing",
                        "DocumentJournals/Thin/Forms/Card/Ext/Form/Module.bsl",
                    ),
                    (
                        "missing",
                        "DocumentJournals/Full/Templates/Report/Ext/Template.xml",
                    ),
                    ("missing", "DocumentJournals/Full/Ext/Help.xml"),
                    ("missing", "DocumentJournals/Full/Ext/Help/ru.html"),
                    ("missing", "DocumentJournals/Full/Forms/Card/Ext/Help.xml"),
                    (
                        "missing",
                        "DocumentJournals/Full/Forms/Card/Ext/Help/ru.html",
                    ),
                    ("extra", "Enums/Without/Ext/Extra.bin"),
                    (
                        "extra",
                        "DocumentJournals/Thin/Commands/Orphan/Ext/CommandModule.bsl",
                    ),
                    (
                        "extra",
                        "DocumentJournals/Full/Templates/Report/Ext/Template.bin",
                    ),
                    ("extra", "DocumentJournals/Full/Forms/Card/Ext/Other.xml"),
                    ("nested", "Enums/With.xml"),
                    ("unknown", "Enums/With.xml"),
                    ("enum_form", "Enums/With.xml"),
                    ("uuid", "Enums/With.xml"),
                    ("duplicate", "DocumentJournals/Full.xml"),
                    ("unknown", "DocumentJournals/Full.xml"),
                    ("orphan", "DocumentJournals/Full.xml"),
                    ("bad_name", "DocumentJournals/Full.xml"),
                ]
                .iter()
                .enumerate()
                {
                    let path = repo.join(target);
                    let previous = fs::read(&path).ok();
                    match *case {
                        "missing" => fs::remove_file(&path).unwrap(),
                        "extra" => {
                            fs::create_dir_all(path.parent().unwrap()).unwrap();
                            fs::write(&path, BINARY).unwrap();
                        }
                        _ => {
                            let x = String::from_utf8(previous.clone().unwrap()).unwrap();
                            let changed = match *case {
                                "nested" => {
                                    x.replace("</EnumValue>", "<ChildObjects/></EnumValue>")
                                }
                                "unknown" => {
                                    x.replace("</ChildObjects>", "<Unknown/></ChildObjects>")
                                }
                                "enum_form" => {
                                    x.replace("</ChildObjects>", "<Form>Card</Form></ChildObjects>")
                                }
                                "uuid" => x.replace("uuid=\"20000000", "uuid=\"zzzzzzzz"),
                                "duplicate" => x.replace(
                                    "</ChildObjects>",
                                    &format!(
                                        "{}</ChildObjects>",
                                        journal_inline("Command", "Run", 904)
                                    ),
                                ),
                                "orphan" => {
                                    x.replace(&journal_inline("Command", "Refused", 905), "")
                                }
                                "bad_name" => x.replace("<Name>Run</Name>", "<Name>../Run</Name>"),
                                _ => unreachable!(),
                            };
                            fs::write(&path, changed).unwrap();
                        }
                    }
                    commit();
                    let mut bad = args.clone();
                    bad[2] = git(&["rev-parse", "HEAD"]);
                    bad[3] = git(&["rev-parse", "HEAD^{tree}"]);
                    bad[5] = root
                        .join(format!("journal-refused-{i}"))
                        .to_str()
                        .unwrap()
                        .into();
                    let result = run(&bad);
                    assert!(!result.status.success(), "unexpected {case}: {target}");
                    assert_eq!(fs::read_dir(&bad[5]).unwrap().count(), 0);
                    if i == 0 {
                        assert!(String::from_utf8_lossy(&result.stderr)
                            .contains("journal_command_module_missing"));
                    }
                    if let Some(bytes) = previous {
                        fs::write(path, bytes).unwrap();
                    } else {
                        fs::remove_file(path).unwrap();
                    }
                }
                commit();
            }
            if mode >= 11 {
                let mut old = args.clone();
                old[0] = "build-journals".into();
                old[5] = root
                    .join("journal-refuses-services")
                    .to_str()
                    .unwrap()
                    .into();
                assert!(!run(&old).status.success());
                assert_eq!(fs::read_dir(&old[5]).unwrap().count(), 0);
                for (i, (case, target)) in [
                    ("missing", "HTTPServices/Http/Ext/Module.bsl"),
                    ("missing", "WebServices/Web/Ext/Module.bsl"),
                    ("missing", "IntegrationServices/Integration/Ext/Module.bsl"),
                    ("extra", "HTTPServices/Http/URLTemplates/Endpoint.xml"),
                    ("extra", "WebServices/Web/Ext/Other.bin"),
                    (
                        "extra",
                        "IntegrationServices/Integration/Channels/Channel/Ext/Module.bsl",
                    ),
                    ("empty_url", "HTTPServices/Http.xml"),
                    ("missing_url_children", "HTTPServices/Http.xml"),
                    ("duplicate_url_children", "HTTPServices/Http.xml"),
                    ("missing_operation_children", "WebServices/Web.xml"),
                    ("unknown", "WebServices/Web.xml"),
                    ("wrong_role", "HTTPServices/Http.xml"),
                    ("nested_method", "HTTPServices/Http.xml"),
                    ("nested_parameter", "WebServices/Web.xml"),
                    ("nested_channel", "IntegrationServices/Integration.xml"),
                    ("uuid", "HTTPServices/Http.xml"),
                    ("qualified_uuid", "HTTPServices/Http.xml"),
                    ("name", "WebServices/Web.xml"),
                    ("namespace", "HTTPServices/Http.xml"),
                    ("internal_root", "HTTPServices/Http.xml"),
                    ("internal_leaf", "WebServices/Web.xml"),
                    ("extra_wrapper", "IntegrationServices/Integration.xml"),
                    ("orphan", "Configuration.xml"),
                ]
                .iter()
                .enumerate()
                {
                    let path = repo.join(target);
                    let previous = fs::read(&path).ok();
                    match *case {
                        "missing" => fs::remove_file(&path).unwrap(),
                        "extra" => {
                            fs::create_dir_all(path.parent().unwrap()).unwrap();
                            fs::write(&path, BINARY).unwrap();
                        }
                        _ => {
                            let x = String::from_utf8(previous.clone().unwrap()).unwrap();
                            let changed = match *case {
                                "empty_url" => x.replace(
                                    &service_node(
                                        "URLTemplate",
                                        "Endpoint",
                                        1001,
                                        &format!(
                                            "<ChildObjects>{}{}</ChildObjects>",
                                            service_node("Method", "Get", 1002, ""),
                                            service_node("Method", "Post", 1003, "")
                                        ),
                                    ),
                                    &service_node(
                                        "URLTemplate",
                                        "Endpoint",
                                        1001,
                                        "<ChildObjects/>",
                                    ),
                                ),
                                "missing_url_children" => x.replace(
                                    &service_body("HTTPService"),
                                    &service_node("URLTemplate", "Endpoint", 1001, ""),
                                ),
                                "duplicate_url_children" => {
                                    x.replace("</URLTemplate>", "<ChildObjects/></URLTemplate>")
                                }
                                "missing_operation_children" => {
                                    x.replace("<ChildObjects/></Operation>", "</Operation>")
                                }
                                "unknown" => x.replace("</Operation>", "<Unknown/></Operation>"),
                                "wrong_role" => x.replace("Method", "Parameter"),
                                "nested_method" => {
                                    x.replace("</Method>", "<ChildObjects/></Method>")
                                }
                                "nested_parameter" => {
                                    x.replace("</Parameter>", "<ChildObjects/></Parameter>")
                                }
                                "nested_channel" => x.replace(
                                    "</IntegrationServiceChannel>",
                                    "<ChildObjects/></IntegrationServiceChannel>",
                                ),
                                "uuid" => x.replace("uuid=\"40000000", "uuid=\"zzzzzzzz"),
                                "qualified_uuid" => x.replace(
                                    "<URLTemplate uuid=",
                                    "<URLTemplate xmlns:q=\"urn:test\" q:uuid=",
                                ),
                                "name" => x.replace("<Name>Input</Name>", "<Name><Bad/></Name>"),
                                "namespace" => {
                                    x.replace("<Method uuid=", "<Method xmlns=\"urn:wrong\" uuid=")
                                }
                                "internal_root" => {
                                    x.replace("</HTTPService>", "<InternalInfo/></HTTPService>")
                                }
                                "internal_leaf" => {
                                    x.replace("</Parameter>", "<InternalInfo/></Parameter>")
                                }
                                "extra_wrapper" => x.replace(
                                    "</IntegrationServiceChannel>",
                                    "<Unknown/></IntegrationServiceChannel>",
                                ),
                                "orphan" => x.replace("<HTTPService>Http</HTTPService>", ""),
                                _ => unreachable!(),
                            };
                            assert_ne!(x, changed, "{case}");
                            fs::write(&path, changed).unwrap();
                        }
                    }
                    commit();
                    let mut bad = args.clone();
                    bad[2] = git(&["rev-parse", "HEAD"]);
                    bad[3] = git(&["rev-parse", "HEAD^{tree}"]);
                    bad[5] = root
                        .join(format!("service-refused-{i}"))
                        .to_str()
                        .unwrap()
                        .into();
                    let result = run(&bad);
                    assert!(!result.status.success(), "{case}");
                    assert_eq!(fs::read_dir(&bad[5]).unwrap().count(), 0, "{case}");
                    if i < 3 {
                        assert!(String::from_utf8_lossy(&result.stderr)
                            .contains("service_module_missing"));
                    }
                    if let Some(bytes) = previous {
                        fs::write(path, bytes).unwrap();
                    } else {
                        fs::remove_file(path).unwrap();
                    }
                }
                commit();
            }
            if mode >= 12 {
                let mut old = args.clone();
                old[0] = "build-services".into();
                old[5] = root.join("services-refuse-charts").to_str().unwrap().into();
                assert!(!run(&old).status.success());
                assert_eq!(fs::read_dir(&old[5]).unwrap().count(), 0);
                for (i,(case,target)) in [
                    ("missing","ChartsOfAccounts/Accounts/Ext/ManagerModule.bsl"),("missing","ChartsOfCalculationTypes/Calculation/Ext/ManagerModule.bsl"),("missing","ChartsOfCharacteristicTypes/Characteristics/Ext/ManagerModule.bsl"),
                    ("missing","ChartsOfAccounts/Accounts/Forms/Card.xml"),("missing","ChartsOfAccounts/Accounts/Forms/Card/Ext/Form.xml"),("missing","ChartsOfAccounts/Accounts/Forms/Card/Ext/Form/Module.bsl"),
                    ("missing","ChartsOfAccounts/Accounts/Ext/Help.xml"),("missing","ChartsOfAccounts/Accounts/Ext/Help/ru.html"),("missing","ChartsOfCharacteristicTypes/Characteristics/Forms/Card/Ext/Help.xml"),("missing","ChartsOfCharacteristicTypes/Characteristics/Forms/Card/Ext/Help/ru.html"),
                    ("extra","ChartsOfAccounts/Accounts/Forms/Card/Ext/Form/Items/Item/Picture.jpg"),("extra","ChartsOfAccounts/Accounts/Forms/Card/Ext/Form/Items/Picture.png"),
                    ("extra","ChartsOfCharacteristicTypes/Characteristics/Forms/Card/Ext/Form/Items/Item/Picture.png"),("extra","ChartsOfAccounts/Accounts/Forms/Orphan/Ext/Form/Items/Item/Picture.png"),
                    ("extra","ChartsOfAccounts/Accounts/Ext/Other.xml"),("extra","ChartsOfCalculationTypes/Calculation/Ext/Module.bsl"),
                    ("nested","ChartsOfAccounts/Accounts.xml"),("wrong_role","ChartsOfCalculationTypes/Calculation.xml"),("template","ChartsOfAccounts/Accounts.xml"),
                    ("empty_tabular","ChartsOfCharacteristicTypes/Characteristics.xml"),("unknown","ChartsOfCharacteristicTypes/Characteristics.xml"),("uuid","ChartsOfAccounts/Accounts.xml"),
                    ("name","ChartsOfAccounts/Accounts.xml"),("duplicate","ChartsOfAccounts/Accounts.xml"),("orphan","Configuration.xml"),
                ].iter().enumerate() {
                    let path=repo.join(target);let previous=fs::read(&path).ok();
                    match *case {
                        "missing"=>fs::remove_file(&path).unwrap(),
                        "extra"=>{fs::create_dir_all(path.parent().unwrap()).unwrap();fs::write(&path,BINARY).unwrap();},
                        _=>{let x=String::from_utf8(previous.clone().unwrap()).unwrap();let changed=match *case {
                            "nested"=>x.replace("</AccountingFlag>","<ChildObjects/></AccountingFlag>"),
                            "wrong_role"=>x.replace("</ChildObjects>",&format!("{}</ChildObjects>",chart_node("Attribute","Wrong",1400,""))),
                            "template"=>x.replace("<Form>Card</Form>","<Template>Card</Template>"),
                            "empty_tabular"=>x.replace(&chart_node("Attribute","Child",1206,""),""),
                            "unknown"=>x.replace("</TabularSection>","<Unknown/></TabularSection>"),
                            "uuid"=>x.replace("uuid=\"50000000","uuid=\"zzzzzzzz"),
                            "name"=>x.replace("<Name>Flag</Name>","<Name><Bad/></Name>"),
                            "duplicate"=>x.replace("</ChildObjects>",&format!("{}</ChildObjects>",chart_node("AccountingFlag","Flag",1201,""))),
                            "orphan"=>x.replace("<ChartOfAccounts>Accounts</ChartOfAccounts>",""),_=>unreachable!(),
                        };assert_ne!(x,changed,"{case}");fs::write(&path,changed).unwrap();}
                    }
                    commit();let mut bad=args.clone();bad[2]=git(&["rev-parse","HEAD"]);bad[3]=git(&["rev-parse","HEAD^{tree}"]);bad[5]=root.join(format!("chart-refused-{i}")).to_str().unwrap().into();let result=run(&bad);assert!(!result.status.success(),"unexpected {case}: {target}");assert_eq!(fs::read_dir(&bad[5]).unwrap().count(),0,"{case}");if i<3 {assert!(String::from_utf8_lossy(&result.stderr).contains("chart_manager_missing"));}
                    if let Some(bytes)=previous {fs::write(path,bytes).unwrap();}else{fs::remove_file(path).unwrap();}
                }
                commit();
            }
            if mode >= 13 {
                let mut old = args.clone();
                old[0] = "build-charts".into();
                old[5] = root
                    .join("charts-refuse-registers")
                    .to_str()
                    .unwrap()
                    .into();
                assert!(!run(&old).status.success());
                assert_eq!(fs::read_dir(&old[5]).unwrap().count(), 0);
                for (i, (case, target)) in [
                    (
                        "missing",
                        "AccountingRegisters/Books/Ext/RecordSetModule.bsl",
                    ),
                    (
                        "missing",
                        "CalculationRegisters/Calc/Recalculations/Rebuild.xml",
                    ),
                    ("missing", "CalculationRegisters/Calc/Forms/Card.xml"),
                    (
                        "missing",
                        "CalculationRegisters/Calc/Forms/Card/Ext/Form.xml",
                    ),
                    (
                        "missing",
                        "CalculationRegisters/Calc/Forms/Card/Ext/Form/Module.bsl",
                    ),
                    ("missing", "AccountingRegisters/Books/Ext/Help/ru.html"),
                    ("missing", "AccumulationRegisters/WithCode/Ext/Help.xml"),
                    ("extra", "CalculationRegisters/Calc/Ext/ManagerModule.bsl"),
                    ("extra", "AccountingRegisters/Books/Ext/ManagerModule.bsl"),
                    (
                        "extra",
                        "CalculationRegisters/Calc/Recalculations/Rebuild/Ext/Module.bsl",
                    ),
                    (
                        "extra",
                        "CalculationRegisters/Calc/Recalculations/Rebuild/Ext/Help.xml",
                    ),
                    ("extra", "CalculationRegisters/Calc/Ext/Aggregates.xml"),
                    ("extra", "AccumulationRegisters/WithCode/Ext/Other.xml"),
                    (
                        "orphan_descriptor",
                        "CalculationRegisters/Calc/Recalculations/Orphan.xml",
                    ),
                    ("unknown", "AccountingRegisters/Books.xml"),
                    ("attribute", "AccumulationRegisters/WithCode.xml"),
                    ("wrong_ref", "AccountingRegisters/Books.xml"),
                    ("template", "CalculationRegisters/Calc.xml"),
                    ("nonscalar", "CalculationRegisters/Calc.xml"),
                    ("duplicate", "CalculationRegisters/Calc.xml"),
                    (
                        "mismatch",
                        "CalculationRegisters/Calc/Recalculations/Rebuild.xml",
                    ),
                    (
                        "uuid",
                        "CalculationRegisters/Calc/Recalculations/Rebuild.xml",
                    ),
                    (
                        "empty",
                        "CalculationRegisters/Calc/Recalculations/Rebuild.xml",
                    ),
                    (
                        "depth",
                        "CalculationRegisters/Calc/Recalculations/Rebuild.xml",
                    ),
                    (
                        "form_ref",
                        "CalculationRegisters/Calc/Recalculations/Rebuild.xml",
                    ),
                    (
                        "namespace",
                        "CalculationRegisters/Calc/Recalculations/Rebuild.xml",
                    ),
                    ("orphan", "Configuration.xml"),
                ]
                .iter()
                .enumerate()
                {
                    let path = repo.join(target);
                    let previous = fs::read(&path).ok();
                    match *case {
                        "missing" => fs::remove_file(&path).unwrap(),
                        "extra" => {
                            fs::create_dir_all(path.parent().unwrap()).unwrap();
                            fs::write(&path, BINARY).unwrap();
                        }
                        "orphan_descriptor" => {
                            fs::write(
                                &path,
                                register_descriptor(
                                    "Recalculation",
                                    "Orphan",
                                    1900,
                                    &recalculation_body(),
                                ),
                            )
                            .unwrap();
                        }
                        _ => {
                            let x = String::from_utf8(previous.clone().unwrap()).unwrap();
                            let changed = match *case {
                                "unknown" => {
                                    x.replace("</ChildObjects>", "<Unknown/></ChildObjects>")
                                }
                                "attribute" => x.replace(
                                    "</ChildObjects>",
                                    &format!(
                                        "{}</ChildObjects>",
                                        chart_node("Attribute", "Wrong", 1901, "")
                                    ),
                                ),
                                "wrong_ref" => x.replace(
                                    "</ChildObjects>",
                                    "<Recalculation>Rebuild</Recalculation></ChildObjects>",
                                ),
                                "template" => {
                                    x.replace("<Form>Card</Form>", "<Template>Card</Template>")
                                }
                                "nonscalar" => x.replace(
                                    "<Recalculation>Rebuild</Recalculation>",
                                    "<Recalculation><Name>Rebuild</Name></Recalculation>",
                                ),
                                "duplicate" => x.replace(
                                    "</ChildObjects>",
                                    "<Recalculation>Rebuild</Recalculation></ChildObjects>",
                                ),
                                "mismatch" => {
                                    x.replace("<Name>Rebuild</Name>", "<Name>Different</Name>")
                                }
                                "uuid" => x.replace("uuid=\"00000000", "uuid=\"zzzzzzzz"),
                                "empty" => x.replace(&recalculation_body(), ""),
                                "depth" => x.replace("</Dimension>", "<ChildObjects/></Dimension>"),
                                "form_ref" => {
                                    x.replace("</ChildObjects>", "<Form>Card</Form></ChildObjects>")
                                }
                                "namespace" => {
                                    x.replace("<Dimension ", "<Dimension xmlns=\"urn:wrong\" ")
                                }
                                "orphan" => {
                                    x.replace("<CalculationRegister>Calc</CalculationRegister>", "")
                                }
                                _ => unreachable!(),
                            };
                            assert_ne!(x, changed, "{case}");
                            fs::write(&path, changed).unwrap();
                        }
                    }
                    commit();
                    let mut bad = args.clone();
                    bad[2] = git(&["rev-parse", "HEAD"]);
                    bad[3] = git(&["rev-parse", "HEAD^{tree}"]);
                    bad[5] = root
                        .join(format!("register-refused-{i}"))
                        .to_str()
                        .unwrap()
                        .into();
                    let result = run(&bad);
                    assert!(!result.status.success(), "unexpected {case}: {target}");
                    assert_eq!(fs::read_dir(&bad[5]).unwrap().count(), 0, "{case}");
                    if let Some(bytes) = previous {
                        fs::write(path, bytes).unwrap();
                    } else {
                        fs::remove_file(path).unwrap();
                    }
                }
                commit();
            }
            if mode >= 14 {
                let mut old = args.clone();
                old[0] = "build-registers".into();
                old[5] = root
                    .join("registers-refuse-reports")
                    .to_str()
                    .unwrap()
                    .into();
                assert!(!run(&old).status.success());
                assert_eq!(fs::read_dir(&old[5]).unwrap().count(), 0);
                for (i, (case, target)) in [
                    (
                        "missing",
                        "DataProcessors/Process/Commands/Act/Ext/CommandModule.bsl",
                    ),
                    (
                        "missing",
                        "Reports/Overview/Commands/Act/Ext/CommandModule.bsl",
                    ),
                    ("missing", "DataProcessors/Process/Forms/NoCode.xml"),
                    (
                        "missing",
                        "DataProcessors/Process/Forms/NoCode/Ext/Form.xml",
                    ),
                    (
                        "missing",
                        "Reports/Overview/Templates/Binary/Ext/Template.bin",
                    ),
                    ("missing", "DataProcessors/Process/Ext/Help.xml"),
                    (
                        "missing",
                        "DataProcessors/Process/Forms/Card/Ext/Help/ru.html",
                    ),
                    (
                        "extra",
                        "DataProcessors/Process/Commands/Orphan/Ext/CommandModule.bsl",
                    ),
                    (
                        "extra",
                        "DataProcessors/Process/Commands/Act/Ext/Module.bsl",
                    ),
                    ("extra", "DataProcessors/Process/Ext/Other.xml"),
                    (
                        "extra",
                        "DataProcessors/Process/Forms/Card/Ext/Form/Items/File.zip",
                    ),
                    (
                        "extra",
                        "DataProcessors/Process/Forms/Card/Ext/Form/Items/Item/Extra/File.png",
                    ),
                    (
                        "extra",
                        "DataProcessors/Process/Forms/Card/Ext/Form/Items/Item/File.jpg",
                    ),
                    (
                        "extra",
                        "DataProcessors/Process/Forms/Card/Ext/Help/Extra/Deep/File.png",
                    ),
                    (
                        "extra",
                        "DataProcessors/Process/Forms/Card/Ext/Help/Asset/File.svg",
                    ),
                    (
                        "extra",
                        "Reports/Overview/Forms/Card/Ext/Form/Items/Item/File.png",
                    ),
                    (
                        "extra",
                        "Reports/Overview/Templates/Xml/Ext/Template/ru.html",
                    ),
                    (
                        "extra",
                        "DataProcessors/Process/Templates/Binary/Ext/Template/ru.html",
                    ),
                    (
                        "extra",
                        "DataProcessors/Process/Templates/Xml/Ext/Template.bin",
                    ),
                    (
                        "extra",
                        "DataProcessors/Process/Forms/Orphan/Ext/Form/Items/Item/File.zip",
                    ),
                    ("dimension", "Reports/Overview.xml"),
                    ("unknown", "DataProcessors/Process.xml"),
                    ("empty_tabular", "DataProcessors/Process.xml"),
                    ("depth", "Reports/Overview.xml"),
                    ("recalc", "Reports/Overview.xml"),
                    ("uuid", "DataProcessors/Process.xml"),
                    ("name", "DataProcessors/Process.xml"),
                    ("duplicate", "DataProcessors/Process.xml"),
                    ("orphan", "Configuration.xml"),
                ]
                .iter()
                .enumerate()
                {
                    let path = repo.join(target);
                    let previous = fs::read(&path).ok();
                    match *case {
                        "missing" => fs::remove_file(&path).unwrap(),
                        "extra" => {
                            fs::create_dir_all(path.parent().unwrap()).unwrap();
                            fs::write(&path, BINARY).unwrap();
                        }
                        _ => {
                            let x = String::from_utf8(previous.clone().unwrap()).unwrap();
                            let changed = match *case {
                                "dimension" => x.replace(
                                    "</ChildObjects>",
                                    &format!(
                                        "{}</ChildObjects>",
                                        chart_node("Dimension", "Wrong", 9990, "")
                                    ),
                                ),
                                "unknown" => {
                                    x.replace("</TabularSection>", "<Unknown/></TabularSection>")
                                }
                                "empty_tabular" => {
                                    x.replace(&chart_node("Attribute", "Cell", 6004, ""), "")
                                }
                                "depth" => x.replace("</Attribute>", "<ChildObjects/></Attribute>"),
                                "recalc" => x.replace(
                                    "</ChildObjects>",
                                    "<Recalculation>Wrong</Recalculation></ChildObjects>",
                                ),
                                "uuid" => x.replace("uuid=\"50000000", "uuid=\"zzzzzzzz"),
                                "name" => x.replace("<Name>Act</Name>", "<Name><Wrong/></Name>"),
                                "duplicate" => x.replace(
                                    "</ChildObjects>",
                                    &format!(
                                        "{}</ChildObjects>",
                                        chart_node("Command", "Act", 6002, "")
                                    ),
                                ),
                                "orphan" => x.replace("<DataProcessor>Process</DataProcessor>", ""),
                                _ => unreachable!(),
                            };
                            assert_ne!(x, changed, "{case}");
                            fs::write(&path, changed).unwrap();
                        }
                    }
                    commit();
                    let mut bad = args.clone();
                    bad[2] = git(&["rev-parse", "HEAD"]);
                    bad[3] = git(&["rev-parse", "HEAD^{tree}"]);
                    bad[5] = root
                        .join(format!("report-refused-{i}"))
                        .to_str()
                        .unwrap()
                        .into();
                    let result = run(&bad);
                    assert!(!result.status.success(), "unexpected {case}: {target}");
                    assert_eq!(fs::read_dir(&bad[5]).unwrap().count(), 0, "{case}");
                    if let Some(bytes) = previous {
                        fs::write(path, bytes).unwrap();
                    } else {
                        fs::remove_file(path).unwrap();
                    }
                }
                commit();
            }
            if mode >= 15 {
                let mut old = args.clone();
                old[0] = "build-reports".into();
                old[5] = root
                    .join("reports-refuse-workflows")
                    .to_str()
                    .unwrap()
                    .into();
                assert!(!run(&old).status.success());
                assert_eq!(fs::read_dir(&old[5]).unwrap().count(), 0);
                for (i, (case, target)) in [
                    ("missing", "ExchangePlans/Exchange/Ext/ManagerModule.bsl"),
                    ("missing", "ExchangePlans/Exchange/Ext/ObjectModule.bsl"),
                    ("missing", "BusinessProcesses/Process/Ext/ManagerModule.bsl"),
                    ("missing", "BusinessProcesses/Process/Ext/ObjectModule.bsl"),
                    ("missing", "Tasks/Work/Ext/ManagerModule.bsl"),
                    ("missing", "Tasks/Work/Ext/ObjectModule.bsl"),
                    ("missing", "ExchangePlans/Exchange/Ext/Content.xml"),
                    ("missing", "BusinessProcesses/Process/Ext/Flowchart.xml"),
                    ("missing", "Tasks/Work/Commands/Act0/Ext/CommandModule.bsl"),
                    ("missing", "Tasks/Work/Commands/Act3/Ext/CommandModule.bsl"),
                    ("missing", "ExchangePlans/Exchange/Forms/NoCode.xml"),
                    (
                        "missing",
                        "ExchangePlans/Exchange/Forms/NoCode/Ext/Form.xml",
                    ),
                    (
                        "missing",
                        "BusinessProcesses/Process/Forms/Card/Ext/Form/Module.bsl",
                    ),
                    ("missing", "Tasks/Work/Forms/Card/Ext/Form/Module.bsl"),
                    (
                        "missing",
                        "ExchangePlans/Exchange/Templates/Text/Ext/Template.txt",
                    ),
                    ("missing", "Tasks/Work/Ext/Help.xml"),
                    ("missing", "Tasks/Work/Forms/Card/Ext/Help/ru.html"),
                    ("extra", "Tasks/Work/Commands/Orphan/Ext/CommandModule.bsl"),
                    (
                        "extra",
                        "ExchangePlans/Exchange/Commands/Act/Ext/CommandModule.bsl",
                    ),
                    ("extra", "ExchangePlans/Exchange/Ext/Flowchart.xml"),
                    ("extra", "BusinessProcesses/Process/Ext/Content.xml"),
                    (
                        "extra",
                        "ExchangePlans/Exchange/Templates/Text/Ext/Template.xml",
                    ),
                    (
                        "extra",
                        "Tasks/Work/Forms/Card/Ext/Form/Items/Item/Picture.png",
                    ),
                    ("unknown", "ExchangePlans/Exchange.xml"),
                    ("address", "BusinessProcesses/Process.xml"),
                    ("tabular", "Tasks/Work.xml"),
                    ("template", "Tasks/Work.xml"),
                    ("empty_tabular", "ExchangePlans/Exchange.xml"),
                    ("uuid", "Tasks/Work.xml"),
                    ("name", "Tasks/Work.xml"),
                    ("depth", "Tasks/Work.xml"),
                    ("orphan", "Configuration.xml"),
                ]
                .iter()
                .enumerate()
                {
                    let path = repo.join(target);
                    let previous = fs::read(&path).ok();
                    match *case {
                        "missing" => fs::remove_file(&path).unwrap(),
                        "extra" => {
                            fs::create_dir_all(path.parent().unwrap()).unwrap();
                            fs::write(&path, BINARY).unwrap();
                        }
                        _ => {
                            let x = String::from_utf8(previous.clone().unwrap()).unwrap();
                            let changed = match *case {
                                "unknown" => {
                                    x.replace("</TabularSection>", "<Unknown/></TabularSection>")
                                }
                                "address" => x.replace(
                                    "</ChildObjects>",
                                    &format!(
                                        "{}</ChildObjects>",
                                        chart_node("AddressingAttribute", "Wrong", 49000, "")
                                    ),
                                ),
                                "tabular" => x.replace(
                                    "</ChildObjects>",
                                    &format!(
                                        "{}</ChildObjects>",
                                        chart_node(
                                            "TabularSection",
                                            "Wrong",
                                            49001,
                                            "<ChildObjects/>"
                                        )
                                    ),
                                ),
                                "template" => {
                                    x.replace("<Form>Card</Form>", "<Template>Wrong</Template>")
                                }
                                "empty_tabular" => {
                                    x.replace(&chart_node("Attribute", "Cell", 40003, ""), "")
                                }
                                "uuid" => x.replace("uuid=\"50000000", "uuid=\"zzzzzzzz"),
                                "name" => x.replace("<Name>Act0</Name>", "<Name><Bad/></Name>"),
                                "depth" => x.replace(
                                    "</AddressingAttribute>",
                                    "<ChildObjects/></AddressingAttribute>",
                                ),
                                "orphan" => x.replace("<Task>Work</Task>", ""),
                                _ => unreachable!(),
                            };
                            assert_ne!(x, changed, "{case}");
                            fs::write(&path, changed).unwrap();
                        }
                    }
                    commit();
                    let mut bad = args.clone();
                    bad[2] = git(&["rev-parse", "HEAD"]);
                    bad[3] = git(&["rev-parse", "HEAD^{tree}"]);
                    bad[5] = root
                        .join(format!("workflow-refused-{i}"))
                        .to_str()
                        .unwrap()
                        .into();
                    let result = run(&bad);
                    assert!(!result.status.success(), "unexpected {case}: {target}");
                    assert_eq!(fs::read_dir(&bad[5]).unwrap().count(), 0, "{case}");
                    if let Some(bytes) = previous {
                        fs::write(path, bytes).unwrap();
                    } else {
                        fs::remove_file(path).unwrap();
                    }
                }
                commit();
            }
            if mode == 16 {
                let mut old = args.clone();
                old[0] = "build-workflows".into();
                old.pop();
                old[5] = root
                    .join("workflows-refuse-corpus")
                    .to_str()
                    .unwrap()
                    .into();
                assert!(!run(&old).status.success());
                assert_eq!(fs::read_dir(&old[5]).unwrap().count(), 0);
                for (n, scope) in [
                    "[]".to_string(),
                    "null".into(),
                    "[1]".into(),
                    "[\"Ext/Missing.bin\"]".into(),
                    "[\"Splash.png\",\"Splash.png\"]".into(),
                    "[\"../Bad.bin\"]".into(),
                    "[\"Other/Bad.bin\"]".into(),
                    "[\"Ext/Bad.bsl\"]".into(),
                    json!(vec!["Ext/A.bin"; 65]).to_string(),
                    " ".repeat(65537),
                ]
                .iter()
                .enumerate()
                {
                    let mut invalid = args.clone();
                    invalid[5] = root
                        .join(format!("global-scope-refuse-{n}"))
                        .to_str()
                        .unwrap()
                        .into();
                    invalid[19] = scope.clone();
                    assert!(!run(&invalid).status.success());
                    assert!(
                        !std::path::Path::new(&invalid[5]).exists()
                            || fs::read_dir(&invalid[5]).unwrap().count() == 0
                    );
                }
                // Canonical parameters, not the larger CLI JSON envelope, cap the scope.
                let db = rusqlite::Connection::open(&built.path).unwrap();
                let raw: String = db
                    .query_row("SELECT build_parameters FROM metadata", [], |r| r.get(0))
                    .unwrap();
                drop(db);
                let mut parameters: Value = serde_json::from_str(&raw).unwrap();
                let mut scope: Vec<String> = serde_json::from_str(&args[19]).unwrap();
                let original_len = scope.len();
                let path_of_len = |len: usize, index: usize| {
                    let mut p = format!("Ext/boundary{index}/");
                    while len - p.len() - 4 > 180 {
                        p.push_str(&"x".repeat(179));
                        p.push('/');
                    }
                    p.push_str(&"x".repeat(len - p.len() - 4));
                    p.push_str(".bin");
                    p
                };
                while 16384 - parameters.to_string().len() > 1000 {
                    scope.push(path_of_len(800, scope.len()));
                    parameters["global_scope"]["opaque"] = json!(scope);
                }
                let last_len = 16384 - parameters.to_string().len() - 3;
                scope.push(path_of_len(last_len, scope.len()));
                scope.sort();
                parameters["global_scope"]["opaque"] = json!(scope);
                assert_eq!(parameters.to_string().len(), 16384);
                assert!(scope.len() <= 64 && scope.iter().all(|p| p.len() <= 1024));
                let originals: Vec<String> = serde_json::from_str(&args[19]).unwrap();
                let extra: Vec<String> = scope
                    .iter()
                    .filter(|p| !originals.contains(p))
                    .cloned()
                    .collect();
                assert_eq!(scope.len(), original_len + extra.len());
                for p in &extra {
                    let p = repo.join(p);
                    fs::create_dir_all(p.parent().unwrap()).unwrap();
                    fs::write(p, BINARY).unwrap();
                }
                commit();
                let mut boundary = args.clone();
                boundary[2] = git(&["rev-parse", "HEAD"]);
                boundary[3] = git(&["rev-parse", "HEAD^{tree}"]);
                boundary[5] = root.join("parameters-16384").to_str().unwrap().into();
                boundary[19] = serde_json::to_string(&scope).unwrap();
                assert!(boundary[19].len() < 65536);
                let result = run(&boundary);
                assert!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                fs::write(root.join("parameters-16384.json"), &result.stdout).unwrap();
                let result: Value = serde_json::from_slice(&result.stdout).unwrap();
                let db = rusqlite::Connection::open(result["path"].as_str().unwrap()).unwrap();
                assert_eq!(
                    db.query_row(
                        "SELECT length(CAST(build_parameters AS BLOB)) FROM metadata",
                        [],
                        |r| r.get::<_, u64>(0)
                    )
                    .unwrap(),
                    16384
                );
                drop(db);
                let before = extra.last().unwrap();
                let after = before.replace(".bin", "x.bin");
                fs::rename(repo.join(before), repo.join(&after)).unwrap();
                *scope.iter_mut().find(|p| *p == before).unwrap() = after.clone();
                scope.sort();
                parameters["global_scope"]["opaque"] = json!(scope);
                assert_eq!(parameters.to_string().len(), 16385);
                commit();
                boundary[2] = git(&["rev-parse", "HEAD"]);
                boundary[3] = git(&["rev-parse", "HEAD^{tree}"]);
                boundary[5] = root.join("parameters-16385").to_str().unwrap().into();
                boundary[19] = serde_json::to_string(&scope).unwrap();
                let result = run(&boundary);
                assert!(!result.status.success());
                assert!(
                    String::from_utf8_lossy(&result.stderr).contains("metadata_field_budget"),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                assert_eq!(fs::read_dir(&boundary[5]).unwrap().count(), 0);
                fs::rename(repo.join(&after), repo.join(before)).unwrap();
                for p in &extra {
                    fs::remove_file(repo.join(p)).unwrap();
                }
                commit();
                // Absence of an optional Configuration module is visible, not invented.
                let module = repo.join("Ext/OrdinaryApplicationModule.bsl");
                let saved = fs::read(&module).unwrap();
                fs::remove_file(&module).unwrap();
                commit();
                let mut optional = args.clone();
                optional[2] = git(&["rev-parse", "HEAD"]);
                optional[3] = git(&["rev-parse", "HEAD^{tree}"]);
                optional[5] = root.join("config-absent").to_str().unwrap().into();
                let result = run(&optional);
                assert!(
                    result.status.success(),
                    "{}",
                    String::from_utf8_lossy(&result.stderr)
                );
                fs::write(root.join("config-absent.json"), &result.stdout).unwrap();
                fs::write(module, saved).unwrap();
                commit();
                for (i, (case, target)) in [
                    ("missing", "Ext/First.bin"),
                    ("missing", "SettingsStorages/Ext/Ext/ManagerModule.bsl"),
                    ("missing", "SettingsStorages/Ext/Forms/Form12.xml"),
                    ("missing", "SettingsStorages/Ext/Forms/Form12/Ext/Form.xml"),
                    (
                        "missing",
                        "SettingsStorages/Ext/Forms/Form12/Ext/Form/Module.bsl",
                    ),
                    ("missing", "Subsystems/Ext/Subsystems/Alpha.xml"),
                    (
                        "missing",
                        "Subsystems/Ext/Subsystems/Alpha/Subsystems/Leaf.xml",
                    ),
                    (
                        "missing",
                        "Subsystems/Ext/Subsystems/Alpha/Ext/Help/ru.html",
                    ),
                    ("extra", "Sequences/Order/Ext/Module.bsl"),
                    ("extra", "SettingsStorages/Ext/Ext/ObjectModule.bsl"),
                    ("extra", "Subsystems/Ext/Ext/Module.bsl"),
                    (
                        "extra",
                        "Subsystems/Ext/Subsystems/Alpha/Subsystems/Leaf/Subsystems/TooDeep.xml",
                    ),
                    ("extra", "Subsystems/Ext/Ext/Other.xml"),
                    ("extra", "Subsystems/Ext/Subsystems/Orphan.xml"),
                    ("subtype", "Subsystems/Ext.xml"),
                    ("nonscalar", "Subsystems/Ext.xml"),
                    ("duplicate", "Subsystems/Ext.xml"),
                    ("filter_nonempty", "FilterCriteria/Пример.xml"),
                    ("filter_attr", "FilterCriteria/Пример.xml"),
                    ("filter_text", "FilterCriteria/Пример.xml"),
                    ("filter_double", "FilterCriteria/Пример.xml"),
                    ("other_descriptor", "CommandGroups/Пример.xml"),
                    ("sequence_role", "Sequences/Order.xml"),
                    ("sequence_depth", "Sequences/Order.xml"),
                    ("settings_role", "SettingsStorages/Ext.xml"),
                ]
                .iter()
                .enumerate()
                {
                    let path = repo.join(target);
                    let previous = fs::read(&path).ok();
                    match *case {
                        "missing" => fs::remove_file(&path).unwrap(),
                        "extra" => {
                            fs::create_dir_all(path.parent().unwrap()).unwrap();
                            fs::write(&path, BINARY).unwrap();
                        }
                        _ => {
                            let x = String::from_utf8(previous.clone().unwrap()).unwrap();
                            let changed = match *case {
                                "subtype" => {
                                    x.replace("<Subsystem>Alpha</Subsystem>", "<Form>Alpha</Form>")
                                }
                                "nonscalar" => x.replace(
                                    "<Subsystem>Alpha</Subsystem>",
                                    "<Subsystem><Name>Alpha</Name></Subsystem>",
                                ),
                                "duplicate" => x.replace(
                                    "</ChildObjects>",
                                    "<Subsystem>Alpha</Subsystem></ChildObjects>",
                                ),
                                "filter_nonempty" => x.replace(
                                    "<ChildObjects/>",
                                    "<ChildObjects><Unknown/></ChildObjects>",
                                ),
                                "filter_attr" => {
                                    x.replace("<ChildObjects/>", "<ChildObjects bad='1'/>")
                                }
                                "filter_text" => {
                                    x.replace("<ChildObjects/>", "<ChildObjects>bad</ChildObjects>")
                                }
                                "filter_double" => {
                                    x.replace("<ChildObjects/>", "<ChildObjects/><ChildObjects/>")
                                }
                                "other_descriptor" => {
                                    x.replace("</CommandGroup>", "<ChildObjects/></CommandGroup>")
                                }
                                "sequence_role" => x.replace("Dimension", "Attribute"),
                                "sequence_depth" => {
                                    x.replace("</Dimension>", "<ChildObjects/></Dimension>")
                                }
                                "settings_role" => {
                                    x.replace("<Form>Form0</Form>", "<Template>Form0</Template>")
                                }
                                _ => unreachable!(),
                            };
                            assert_ne!(x, changed, "{case}");
                            fs::write(&path, changed).unwrap();
                        }
                    }
                    commit();
                    let mut bad = args.clone();
                    bad[2] = git(&["rev-parse", "HEAD"]);
                    bad[3] = git(&["rev-parse", "HEAD^{tree}"]);
                    bad[5] = root
                        .join(format!("corpus-refused-{i}"))
                        .to_str()
                        .unwrap()
                        .into();
                    let result = run(&bad);
                    assert!(!result.status.success(), "unexpected {case}: {target}");
                    assert_eq!(fs::read_dir(&bad[5]).unwrap().count(), 0, "{case}");
                    if let Some(bytes) = previous {
                        fs::write(path, bytes).unwrap();
                    } else {
                        fs::remove_file(path).unwrap();
                    }
                }
                commit();
            }
            let descriptor_path = repo.join("Catalogs/Items/Forms/Card.xml");
            let original = fs::read(&descriptor_path).unwrap();
            for (i, kind) in [
                "missing",
                "mismatch",
                "payload_as_descriptor",
                "unknown",
                "duplicate",
                "ambiguous",
                "childobjects",
                "namespace",
                "uuid",
                "orphan",
                "late_limit",
            ]
            .iter()
            .enumerate()
            {
                fs::write(&descriptor_path, &original).unwrap();
                fs::write(repo.join("Catalogs/Items.xml"), &updated).unwrap();
                let extra = repo.join("Catalogs/Items/Templates/Print/Ext/Template.txt");
                if extra.exists() {
                    fs::remove_file(&extra).unwrap();
                }
                match *kind {
                    "missing" => fs::remove_file(&descriptor_path).unwrap(),
                    "mismatch" => {
                        fs::write(&descriptor_path, descriptor("Form", "Wrong", 1)).unwrap()
                    }
                    "payload_as_descriptor" => {
                        fs::write(&descriptor_path, b"<OpaqueForm/>").unwrap()
                    }
                    "unknown" => fs::write(
                        repo.join("Catalogs/Items.xml"),
                        updated.replace("<Form>Card</Form>", "<Unknown>Card</Unknown>"),
                    )
                    .unwrap(),
                    "duplicate" => fs::write(
                        repo.join("Catalogs/Items.xml"),
                        updated.replace("<Form>Card</Form>", "<Form>Card</Form><Form>Card</Form>"),
                    )
                    .unwrap(),
                    "ambiguous" => fs::write(&extra, b"opaque").unwrap(),
                    "childobjects" => fs::write(
                        &descriptor_path,
                        descriptor("Form", "Card", 1).replace("</Form>", "<ChildObjects/></Form>"),
                    )
                    .unwrap(),
                    "namespace" => fs::write(
                        &descriptor_path,
                        descriptor("Form", "Card", 1)
                            .replace("http://v8.1c.ru/8.3/MDClasses", "urn:wrong"),
                    )
                    .unwrap(),
                    "uuid" => fs::write(&descriptor_path, descriptor("Form", "Card", 2)).unwrap(),
                    "orphan" => fs::write(
                        repo.join("Catalogs/Items.xml"),
                        updated.replace("<Form>Card</Form>", ""),
                    )
                    .unwrap(),
                    _ => {}
                }
                commit();
                args[2] = git(&["rev-parse", "HEAD"]);
                args[3] = git(&["rev-parse", "HEAD^{tree}"]);
                args[5] = root
                    .join(format!("companion-refused-{i}"))
                    .to_str()
                    .unwrap()
                    .into();
                args[16] = if *kind == "late_limit" {
                    "10"
                } else {
                    "100000"
                }
                .into();
                let out = run(&args);
                assert!(!out.status.success(), "unexpected {kind}");
                assert_eq!(fs::read_dir(&args[5]).unwrap().count(), 0, "{kind}");
            }
        }
        // Only a repository created by this test is removed. Runtime has no repository.
        fs::remove_dir_all(&repo).unwrap();
        if ingest_mode {
            let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
                .env("PATH", "")
                .args([
                    "verify",
                    built.path.to_str().unwrap(),
                    "--staging-dir",
                    stage.to_str().unwrap(),
                    "--max-sqlite-bytes",
                    "4000000",
                    "--expected-sha256",
                    &built.file_sha256,
                    "--max-sources",
                    "100",
                    "--max-source-bytes",
                    "1000000",
                    "--max-total-bytes",
                    "4000000",
                    "--max-scratch-bytes",
                    "4000000",
                ])
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let v: Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(v["offline_facts_verified"], true);
            assert_eq!(
                v["snapshot"]["profile"],
                bslens_core::snapshot_v5::ingest_writer::PROFILE
            );
            assert_eq!(v["git_provenance"], "not_verified");
        }

        if mode >= 2 {
            let args = [
                "verify",
                built.path.to_str().unwrap(),
                "--staging-dir",
                stage.to_str().unwrap(),
                "--max-sqlite-bytes",
                "134217728",
                "--expected-sha256",
                &built.file_sha256,
                "--max-sources",
                "5000",
                "--max-source-bytes",
                "8388608",
                "--max-total-bytes",
                "67108864",
                "--max-scratch-bytes",
                "134217728",
                "--max-facts",
                "100000",
                "--max-coverage-bytes",
                "1048576",
                "--facts-time-ms",
                "30000",
            ];
            let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
                .env("PATH", "")
                .args(args)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let v: Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(v["offline_facts_verified"], true);
            assert_eq!(
                v["snapshot"]["profile"],
                if mode == 16 {
                    bslens_core::snapshot_v5::corpus_writer::PROFILE
                } else if mode >= 15 {
                    bslens_core::snapshot_v5::workflow_writer::PROFILE
                } else if mode >= 14 {
                    bslens_core::snapshot_v5::report_writer::PROFILE
                } else if mode >= 13 {
                    bslens_core::snapshot_v5::register_writer::PROFILE
                } else if mode >= 12 {
                    bslens_core::snapshot_v5::chart_writer::PROFILE
                } else if mode >= 11 {
                    bslens_core::snapshot_v5::service_writer::PROFILE
                } else if mode >= 10 {
                    bslens_core::snapshot_v5::journal_writer::PROFILE
                } else if mode >= 9 {
                    bslens_core::snapshot_v5::leaf_writer::PROFILE
                } else if mode >= 8 {
                    bslens_core::snapshot_v5::shared_writer::PROFILE
                } else if mode == 7 {
                    bslens_core::snapshot_v5::root_writer::PROFILE
                } else if mode == 6 {
                    bslens_core::snapshot_v5::common_writer::PROFILE
                } else if mode == 5 {
                    bslens_core::snapshot_v5::companion_writer::PROFILE
                } else if mode == 4 {
                    bslens_core::snapshot_v5::inline_writer::PROFILE
                } else {
                    bslens_core::snapshot_v5::streaming_writer::PROFILE
                }
            );
            let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
                .env("PATH", "")
                .args([
                    "open",
                    built.path.to_str().unwrap(),
                    "--staging-dir",
                    stage.to_str().unwrap(),
                    "--max-sqlite-bytes",
                    "134217728",
                    "--expected-sha256",
                    &built.file_sha256,
                ])
                .output()
                .unwrap();
            assert!(out.status.success());
        }
        Self {
            root,
            repo,
            stage,
            built,
            old,
            read_limit: if mode >= 2 { "134217728" } else { "4000000" },
        }
    }
    fn command_without_expected(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_bslens-mcp"));
        c.args([
            "--snapshot",
            self.built.path.to_str().unwrap(),
            "--staging-dir",
            self.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            self.read_limit,
        ])
        .env("PATH", "");
        c
    }
    fn command(&self) -> Command {
        let mut c = self.command_without_expected();
        c.args(["--expected-sha256", &self.built.file_sha256]);
        c
    }
    fn alter(&mut self, sql: &str) {
        let db = rusqlite::Connection::open(&self.built.path).unwrap();
        db.execute_batch(sql).unwrap();
        drop(db);
        self.built.file_sha256 =
            format!("{:x}", Sha256::digest(fs::read(&self.built.path).unwrap()));
        let path = self.built.path.parent().unwrap().join(format!(
            "{}.{}.sqlite",
            self.built.snapshot_id, self.built.file_sha256
        ));
        fs::rename(&self.built.path, &path).unwrap();
        self.built.path = path;
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
struct Peer {
    child: Child,
    input: Option<ChildStdin>,
    rx: mpsc::Receiver<Vec<u8>>,
    id: u64,
}
impl Peer {
    fn start(f: &Fixture) -> Self {
        let mut child = f
            .command()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take();
        let out = child.stdout.take().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut reader = BufReader::new(out);
            loop {
                let mut line = vec![];
                if reader.read_until(b'\n', &mut line).unwrap_or(0) == 0 {
                    break;
                }
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut peer = Self {
            child,
            input,
            rx,
            id: 1,
        };
        let initialized=peer.request("initialize",json!({"protocolVersion":PROTOCOL,"capabilities":{},"clientInfo":{"name":"s6","version":"1"}}));
        assert_eq!(initialized["result"]["protocolVersion"], PROTOCOL);
        peer.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
        peer
    }
    fn send(&mut self, v: Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{v}").unwrap();
        input.flush().unwrap();
    }
    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.id;
        self.id += 1;
        self.send(json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}));
        let bytes = self
            .rx
            .recv_timeout(Duration::from_secs(10))
            .expect("MCP response timeout");
        assert!(bytes.len() <= MAX_BODY + 1);
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["id"], id);
        v
    }
    fn call(&mut self, name: &str, args: Value) -> Value {
        self.request("tools/call", json!({"name":name,"arguments":args}))
    }
    fn data(&mut self, name: &str, args: Value) -> Value {
        let v = self.call(name, args);
        assert_eq!(v["result"]["isError"], false, "{v}");
        let content = &v["result"]["content"][0]["text"];
        let parsed: Value = serde_json::from_str(content.as_str().unwrap()).unwrap();
        assert_eq!(parsed, v["result"]["structuredContent"]);
        parsed
    }
    fn finish(&mut self) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                break;
            }
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(5));
        }
        let mut stderr = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert!(stderr.is_empty(), "{stderr}");
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn old_shape(n: &Node) -> Value {
    match n.kind {
        NodeKind::Module => json!(["module", n.address]),
        NodeKind::ObjectType => json!(["kind", n.label]),
        NodeKind::UnassignedModules => json!(["unconfirmed", "unconfirmed"]),
        NodeKind::Object => json!(["object", n.label]),
        NodeKind::Method => json!(["method", n.label]),
        _ => panic!("unexpected node"),
    }
}
fn new_shape(n: &Value) -> Value {
    match n["type"].as_str().unwrap() {
        "module" => json!(["module", n["key"]]),
        "unconfirmed" => json!(["unconfirmed", "unconfirmed"]),
        kind => json!([kind, n["name"]]),
    }
}
fn bytes(v: &Value) -> Vec<u8> {
    match v["encoding"].as_str().unwrap() {
        "utf-8" => v["data"].as_str().unwrap().as_bytes().to_vec(),
        "base64" => {
            // Independent test decoder, including canonical padding check.
            let text = v["data"].as_str().unwrap();
            assert_eq!(text.len() % 4, 0);
            let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
            let mut output = vec![];
            for c in text.as_bytes().chunks_exact(4) {
                let a = alphabet.iter().position(|x| *x == c[0]).unwrap() as u32;
                let b = alphabet.iter().position(|x| *x == c[1]).unwrap() as u32;
                let d = if c[2] == b'=' {
                    0
                } else {
                    alphabet.iter().position(|x| *x == c[2]).unwrap() as u32
                };
                let e = if c[3] == b'=' {
                    0
                } else {
                    alphabet.iter().position(|x| *x == c[3]).unwrap() as u32
                };
                let n = a << 18 | b << 12 | d << 6 | e;
                output.push((n >> 16) as u8);
                if c[2] != b'=' {
                    output.push((n >> 8) as u8);
                }
                if c[3] != b'=' {
                    output.push(n as u8);
                }
            }
            output
        }
        _ => panic!("unmarked encoding"),
    }
}
#[test]
fn actual_mcp_offline_tree_parity_status_and_exact_sources_one_reader() {
    check_offline_mcp(Fixture::new(&[]));
}
#[test]
fn actual_ingest_writer_large_inventory_build_verify_mcp_and_rollback() {
    check_offline_mcp(Fixture::with_ingest(&[], true));
}
fn check_offline_mcp(f: Fixture) {
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let list = p.request("tools/list", json!({}));
    assert_eq!(
        list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec!["metadata", "tree", "source"]
    );
    let metadata = p.data("metadata", json!({}));
    assert_eq!(metadata["snapshot"]["snapshot_id"], f.built.snapshot_id);
    assert_eq!(metadata["snapshot"]["relations"], "not_analyzed");
    assert_eq!(metadata["snapshot"]["git_provenance"], "not_verified");
    assert_eq!(metadata["snapshot"]["coverage"]["temporary"], true);
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 1);
    // Remove the published input too, after session startup: tools must retain one copy.
    fs::remove_file(&f.built.path).unwrap();
    let root = p.data("tree", json!({}));
    let items = root["data"]["items"].as_array().unwrap();
    let old = f.old.page(None, 0, 20).unwrap();
    assert_eq!(
        items.iter().map(new_shape).collect::<Vec<_>>(),
        old.page.items.iter().map(old_shape).collect::<Vec<_>>()
    );
    assert_eq!(
        metadata["snapshot"]["coverage"]["owner_unconfirmed"],
        old.page.coverage.unassigned_modules
    );
    assert_eq!(
        metadata["snapshot"]["coverage"]["xml_unverified_count"]
            .as_u64()
            .unwrap() as usize,
        old.page.coverage.unverified_entries
    );
    for (node, old_node) in items.iter().zip(&old.page.items) {
        if node["type"] == "module" || node["type"] == "kind" || node["type"] == "unconfirmed" {
            let branch = p.data("tree", node["children"].clone());
            let old_branch = f.old.page(Some(&old_node.id), 0, 20).unwrap();
            assert_eq!(
                branch["data"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(new_shape)
                    .collect::<Vec<_>>(),
                old_branch
                    .page
                    .items
                    .iter()
                    .map(old_shape)
                    .collect::<Vec<_>>()
            );
        }
    }
    let catalog = p.data(
        "tree",
        json!({"level":"kind","parent_id":root["data"]["branch"]["parent_id"],"kind":"Catalog"}),
    );
    let object = p.data("tree", catalog["data"]["items"][0]["children"].clone());
    assert_eq!(object["data"]["items"][0]["key"], ACCEPTED);
    let methods = p.data("tree", object["data"]["items"][0]["children"].clone());
    assert_eq!(methods["data"]["branch"]["status"], "accepted");
    assert_eq!(methods["data"]["items"][0]["name"], "япервый");
    assert_eq!(methods["data"]["items"][1]["name"], "авторой");
    let method_source = p.data("source", methods["data"]["items"][0]["source"].clone());
    assert_eq!(
        bytes(&method_source["data"]),
        "Процедура ЯПервый()\r\nКонецПроцедуры\r\n".as_bytes()
    );
    for (path, expected, last, status) in [
        (ACCEPTED, TEXT.as_bytes(), 5, "accepted"),
        (REFUSED, BAD.as_bytes(), 2, "unparsed"),
        ("Binary.bsl", BINARY, 2, "unparsed"),
    ] {
        let result = p.data(
            "source",
            json!({"path":path,"start_line":1,"end_line":last}),
        );
        let source = &result["data"];
        assert_eq!(bytes(source), expected);
        assert_eq!(source["byte_length"], expected.len());
        assert_eq!(source["start_byte"], 0);
        assert_eq!(source["end_byte"], expected.len());
        assert_eq!(source["parse_status"], status);
        assert_eq!(source["line_units"], "physical_1_based_inclusive");
        assert_eq!(source["byte_units"], "0_based_end_exclusive");
        assert_eq!(source["sha256"], format!("{:x}", Sha256::digest(expected)));
        if path == "Binary.bsl" {
            assert_eq!(source["encoding"], "base64");
        }
    }
    for (path, status, reason) in [
        ("Zero.bsl", "accepted", Value::Null),
        (REFUSED, "unparsed", json!("RegionHeader")),
        ("Binary.bsl", "unparsed", json!("Utf8")),
    ] {
        let branch = p.data("tree", json!({"level":"module","module_path":path}));
        assert_eq!(branch["data"]["branch"]["status"], status);
        assert_eq!(branch["data"]["branch"]["reason"], reason);
        assert!(branch["data"]["items"].as_array().unwrap().is_empty());
    }
    let empty=p.data("tree",json!({"level":"kind","parent_id":root["data"]["branch"]["parent_id"],"kind":"InformationRegister"}));
    let empty = p.data("tree", empty["data"]["items"][0]["children"].clone());
    assert!(empty["data"]["items"].as_array().unwrap().is_empty());
    for (tool, args) in [
        (
            "tree",
            json!({"level":"module","module_path":"Missing.bsl"}),
        ),
        (
            "source",
            json!({"path":"Missing.bsl","start_line":1,"end_line":1}),
        ),
    ] {
        assert_eq!(
            p.call(tool, args)["result"]["structuredContent"]["code"],
            "not_found"
        );
    }
    assert_eq!(
        p.call("tree", json!({"max_nodes":1}))["result"]["structuredContent"]["code"],
        "budget_exceeded"
    );
    assert_eq!(p.call("exec", json!({}))["error"]["code"], -32602);
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 1);
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}

#[test]
fn full_serialized_frame_limits_escape_base64_and_raw_budget_without_partial_payload() {
    let f = Fixture::new(&[
        ("Plain.bsl", vec![b'x'; 20_000]),
        ("Escaped.bsl", vec![b'"'; 20_000]),
        ("Envelope.bsl", vec![b'x'; 32_700]),
        ("LargeBinary.bsl", vec![0xff; 50_000]),
        ("TooLarge.bsl", vec![b'x'; 65_537]),
    ]);
    let mut p = Peer::start(&f);
    let plain = p.data(
        "source",
        json!({"path":"Plain.bsl","start_line":1,"end_line":1}),
    );
    assert_eq!(bytes(&plain["data"]), vec![b'x'; 20_000]);
    for name in ["Escaped.bsl", "LargeBinary.bsl", "Envelope.bsl"] {
        let v = p.call("source", json!({"path":name,"start_line":1,"end_line":1}));
        assert_eq!(v["error"]["code"], -32001);
        assert_eq!(v["error"]["message"], "Response exceeds byte limit");
        assert!(v.get("result").is_none());
        assert!(v.to_string().len() < 200);
    }
    let v = p.call(
        "source",
        json!({"path":"TooLarge.bsl","start_line":1,"end_line":1}),
    );
    assert_eq!(v["result"]["structuredContent"]["code"], "budget_exceeded");
    assert!(v["result"]["structuredContent"].get("data").is_none());
    assert_eq!(
        bytes(
            &p.data(
                "source",
                json!({"path":"Binary.bsl","start_line":1,"end_line":2})
            )["data"]
        ),
        BINARY
    );
    p.finish();
}

#[test]
fn startup_version_profile_checksum_and_mode_conflicts_fail_closed() {
    for (sql, code) in [
        ("PRAGMA user_version=4", "snapshot_version"),
        (
            "PRAGMA ignore_check_constraints=ON;UPDATE metadata SET profile='unknown'",
            "snapshot_profile",
        ),
    ] {
        let mut f = Fixture::new(&[]);
        f.alter(sql);
        let out = f.command().output().unwrap();
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert_eq!(out.stderr, format!("{code}\n").as_bytes());
        assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    }
    let f = Fixture::new(&[]);
    let out = f.command_without_expected().output().unwrap();
    assert!(out.status.success());
    assert!(out.stdout.is_empty());
    assert!(out.stderr.is_empty());
    let out = f
        .command_without_expected()
        .args(["--expected-sha256", &"0".repeat(64)])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(out.stderr, b"snapshot_checksum\n");
    fs::write(&f.built.path, b"not SQLite").unwrap();
    let out = f.command().output().unwrap();
    assert!(!out.status.success());
    assert!(out.stdout.is_empty());
    assert_eq!(out.stderr, b"snapshot_checksum\n");
    for args in [
        vec!["--repo", "/unused"],
        vec!["--commit", "unused"],
        vec!["--cache-dir", "/unused"],
        vec!["--rebuild-cache"],
        vec!["--snapshot", "/unused"],
        vec!["--max-sqlite-bytes", "1"],
    ] {
        let out = f.command().args(args).output().unwrap();
        assert!(!out.status.success());
        assert!(out.stdout.is_empty());
        assert_eq!(out.stderr, b"snapshot_arguments\n");
    }
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}

#[test]
fn omitted_arguments_are_empty_but_explicit_wrong_types_are_invalid() {
    let f = Fixture::new(&[]);
    let mut p = Peer::start(&f);
    for name in ["metadata", "tree"] {
        let absent = p.request("tools/call", json!({"name":name}));
        let empty = p.call(name, json!({}));
        assert_eq!(absent["result"]["isError"], false);
        assert_eq!(absent["result"], empty["result"]);
        for invalid in [Value::Null, json!(false), json!(7), json!(""), json!([])] {
            assert_eq!(
                p.request("tools/call", json!({"name":name,"arguments":invalid}))["error"]["code"],
                -32602
            );
        }
    }
    assert_eq!(
        p.request("tools/call", json!({"name":"source"}))["error"]["code"],
        -32602
    );
    p.finish();
}

#[test]
fn generated_large_coverage_is_summarized_and_unknown_fields_never_echoed() {
    let xml =
        String::from_utf8(include_bytes!("fixtures/xml221/Configuration.xml").to_vec()).unwrap();
    let extras = (0..700)
        .map(|i| format!("<UnverifiedProperty{i:04}/>"))
        .collect::<String>();
    let xml = xml.replace("<Synonym/>", &extras);
    let mut f = Fixture::new(&[
        ("Configuration.xml", xml.into_bytes()),
        ("One.bsl", b"x".to_vec()),
        ("Escape.bsl", vec![b'"'; 20_000]),
    ]);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    let raw: String = db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    assert!(raw.len() > 65_536);
    let coverage: Value = serde_json::from_str(&raw).unwrap();
    drop(db);
    let mut p = Peer::start(&f);
    let metadata = p.data("metadata", json!({}));
    let summary = &metadata["snapshot"]["coverage"];
    assert_eq!(summary["details_omitted"], true);
    assert_eq!(summary["summary_format"], "allowlist-v1");
    assert_eq!(
        summary["xml_unverified_count"],
        coverage["xml_unverified"].as_array().unwrap().len()
    );
    assert_eq!(
        summary["coverage_sha256"],
        format!("{:x}", Sha256::digest(coverage.to_string().as_bytes()))
    );
    assert!(summary.to_string().len() < 1024);
    assert!(summary.get("xml_unverified").is_none());
    assert!(!metadata.to_string().contains("UnverifiedProperty"));
    assert_eq!(p.data("tree", json!({}))["snapshot"], metadata["snapshot"]);
    let one = p.data(
        "source",
        json!({"path":"One.bsl","start_line":1,"end_line":1}),
    );
    assert_eq!(bytes(&one["data"]), b"x");
    assert_eq!(one["snapshot"], metadata["snapshot"]);
    let over = p.call(
        "source",
        json!({"path":"Escape.bsl","start_line":1,"end_line":1}),
    );
    assert_eq!(over["error"]["code"], -32001);
    assert_eq!(over["error"]["message"], "Response exceeds byte limit");
    assert!(over.get("result").is_none());
    p.finish();

    // Forward-compatible/incorrect temporary fields must never become a wire schema.
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    let mut extended = coverage;
    extended["future_private_details"] = json!("private-marker".repeat(20_000));
    extended["objects"] = json!("wrong-type-marker".repeat(5_000));
    extended["parse_counts"]["future_reason"] = json!("unknown-marker".repeat(5_000));
    let scope = "\u{0001}".repeat(1024); // maximum shared-contract size, worst JSON escaping
    db.execute(
        "UPDATE metadata SET coverage_draft=?1,scope_id=?2",
        rusqlite::params![extended.to_string(), scope],
    )
    .unwrap();
    let id = bslens_core::snapshot_v5::contract::snapshot_id(&db).unwrap();
    db.execute("UPDATE metadata SET snapshot_id=?1", [&id])
        .unwrap();
    drop(db);
    f.built.snapshot_id = id;
    f.alter("");
    let mut p = Peer::start(&f);
    let metadata = p.data("metadata", json!({}));
    assert_eq!(metadata["snapshot"]["scope_id"], scope);
    assert_eq!(metadata["snapshot"]["snapshot_id"], f.built.snapshot_id);
    assert!(metadata["snapshot"]["coverage"]["objects"].is_null());
    for needle in [
        "private-marker",
        "wrong-type-marker",
        "unknown-marker",
        "UnverifiedProperty",
    ] {
        assert!(!metadata.to_string().contains(needle));
    }
    assert_eq!(p.data("tree", json!({}))["snapshot"], metadata["snapshot"]);
    assert_eq!(
        bytes(
            &p.data(
                "source",
                json!({"path":"One.bsl","start_line":1,"end_line":1})
            )["data"]
        ),
        b"x"
    );
    p.finish();
}

#[test]
fn ingest_profile_rejects_unrecognized_parameters_and_schema_profile_mismatch() {
    let mut f = Fixture::with_ingest(&[], true);
    let original = fs::read(&f.built.path).unwrap();
    for sql in [
        "UPDATE metadata SET build_parameters=json_set(build_parameters,'$.extra',1)",
        "UPDATE metadata SET build_parameters=json_set(build_parameters,'$.git_limits.entries',100001)",
        "UPDATE metadata SET limits_draft='{}'",
        "PRAGMA ignore_check_constraints=ON;UPDATE metadata SET profile='linux-v5-spike-xml221-bsl-r09'",
    ] {
        fs::write(&f.built.path,&original).unwrap();
        f.alter(sql);
        let out=f.command().output().unwrap();
        assert!(!out.status.success(),"{sql}");
        assert!(out.stdout.is_empty());
        assert_eq!(fs::read_dir(&f.stage).unwrap().count(),0);
    }
}

#[test]
fn stream_cli_selected_count_and_bytes_offline_verify_real_mcp() {
    for mode in [2, 3] {
        let mut f = Fixture::with_mode(&[], mode);
        assert!(!f.repo.exists());
        let db = rusqlite::Connection::open(&f.built.path).unwrap();
        let (count, total): (u64, u64) = db
            .query_row(
                "SELECT count(*),sum(length(content)) FROM sources",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        if mode == 2 {
            assert!(count > 4096);
            assert!(total < 32 * 1024 * 1024);
        } else {
            assert!(count < 4096);
            assert!(total > 32 * 1024 * 1024);
        }
        drop(db);
        if mode == 2 {
            let verify = |f: &Fixture, facts: &str, extra: bool| {
                let mut c = Command::new(env!("CARGO_BIN_EXE_bslens-v5"));
                c.env("PATH", "").args([
                    "verify",
                    f.built.path.to_str().unwrap(),
                    "--staging-dir",
                    f.stage.to_str().unwrap(),
                    "--max-sqlite-bytes",
                    "134217728",
                    "--expected-sha256",
                    &f.built.file_sha256,
                    "--max-sources",
                    "5000",
                    "--max-source-bytes",
                    "8388608",
                    "--max-total-bytes",
                    "67108864",
                    "--max-scratch-bytes",
                    "134217728",
                ]);
                if extra {
                    c.args([
                        "--max-facts",
                        facts,
                        "--max-coverage-bytes",
                        "1048576",
                        "--facts-time-ms",
                        "30000",
                    ]);
                }
                c.output().unwrap()
            };
            assert!(!verify(&f, "100000", false).status.success());
            assert!(!verify(&f, "1", true).status.success());
            assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
            let original = fs::read(&f.built.path).unwrap();
            let db = rusqlite::Connection::open(&f.built.path).unwrap();
            db.execute("UPDATE methods SET name='WrongButWellFormed' WHERE method_id=(SELECT min(method_id) FROM methods)",[]).unwrap();
            let hash = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
            db.execute("UPDATE metadata SET facts_sha256=?1", [hash])
                .unwrap();
            drop(db);
            f.alter("");
            let out = f.command().output().unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert!(!verify(&f, "100000", true).status.success());
            assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
            fs::write(&f.built.path, &original).unwrap();
            f.alter("");
            for sql in ["UPDATE metadata SET build_parameters=json_set(build_parameters,'$.facts_limits.facts',2000001)","UPDATE metadata SET build_parameters=json_set(build_parameters,'$.unknown',1)","PRAGMA ignore_check_constraints=ON;UPDATE metadata SET profile='linux-v5-ingest1-r09-facts'"] {
                f.alter(sql);let out=f.command().output().unwrap();assert!(!out.status.success());assert!(out.stdout.is_empty());assert_eq!(fs::read_dir(&f.stage).unwrap().count(),0);
                fs::write(&f.built.path,&original).unwrap();f.alter("");
            }
        }
        let mut p = Peer::start(&f);
        let meta = p.data("metadata", json!({}));
        assert_eq!(
            meta["snapshot"]["profile"],
            bslens_core::snapshot_v5::streaming_writer::PROFILE
        );
        assert_eq!(meta["snapshot"]["git_provenance"], "not_verified");
        assert_eq!(
            meta["snapshot"]["coverage"]["modules"],
            if mode == 2 { 4205 } else { 14 }
        );
        let root = p.data("tree", json!({}));
        assert!(root["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["type"] == "unconfirmed"));
        for (path, end, expected) in [(ROOT, 5, TEXT.as_bytes()), ("Binary.bsl", 2, BINARY)] {
            let source = p.data("source", json!({"path":path,"start_line":1,"end_line":end}));
            assert_eq!(bytes(&source["data"]), expected);
        }
        let branch = p.data(
            "tree",
            json!({"level":"module","module_path":"Stream0000.bsl"}),
        );
        assert_eq!(
            branch["data"]["branch"]["status"],
            if mode == 2 { "accepted" } else { "unparsed" }
        );
        p.finish();
        assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    }
}

#[test]
fn inline_cli_verify_mcp_parent_and_exact_xml_evidence() {
    let mut f = Fixture::with_mode(&[], 4);
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let meta = p.data("metadata", json!({}));
    assert_eq!(
        meta["snapshot"]["profile"],
        bslens_core::snapshot_v5::inline_writer::PROFILE
    );
    assert_eq!(meta["snapshot"]["coverage"]["inline_declarations"], 12);
    assert_eq!(
        meta["snapshot"]["coverage"]["inline_semantics"],
        "not_analyzed"
    );
    let root = p.data("tree", json!({}));
    let catalog = root["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["key"] == "Catalog")
        .unwrap();
    let objects = p.data("tree", catalog["children"].clone());
    let owner = objects["data"]["items"][0]["key"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let object = p.data("tree", objects["data"]["items"][0]["children"].clone());
    let group = object["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["type"] == "inline_group")
        .unwrap();
    let declarations = p.data("tree", group["children"].clone());
    assert_eq!(declarations["data"]["items"].as_array().unwrap().len(), 3);
    let ts = declarations["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["kind"] == "TabularSection")
        .unwrap();
    let children = p.data("tree", ts["children"].clone());
    let child = &children["data"]["items"][0];
    assert_eq!(child["kind"], "Attribute");
    assert_eq!(child["parent_key"], ts["key"]);
    assert_eq!(child["owner_id"], owner);
    let evidence = p.data("source", child["source"].clone());
    assert_eq!(evidence["data"]["path"], "Catalogs/Items.xml");
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    let source: Vec<u8> = db
        .query_row(
            "SELECT content FROM sources WHERE path='Catalogs/Items.xml'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(source.starts_with(&[0xef, 0xbb, 0xbf]));
    let a = child["start_byte"].as_u64().unwrap() as usize;
    let b = child["end_byte"].as_u64().unwrap() as usize;
    assert_eq!(bytes(&evidence["data"]), source[a..b]);
    assert!(String::from_utf8(bytes(&evidence["data"]))
        .unwrap()
        .contains("<Name>Колонка</Name>"));
    drop(db);
    let bsl = p.data(
        "source",
        json!({"path":ACCEPTED,"start_line":1,"end_line":5}),
    );
    assert_eq!(bytes(&bsl["data"]), TEXT.as_bytes());
    assert_eq!(
        p.call(
            "tree",
            json!({"level":"inline","parent_id":owner,"max_nodes":1})
        )["result"]["structuredContent"]["code"],
        "budget_exceeded"
    );
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    // Well-formed wrong direct parent: SHA/facts updated, open succeeds, verify rejects.
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    db.execute(
        "UPDATE inline_declarations SET parent_key=NULL WHERE declaration_key=?1",
        [child["key"].as_str().unwrap()],
    )
    .unwrap();
    let hash = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [hash])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "4000000",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "100",
            "--max-source-bytes",
            "1000000",
            "--max-total-bytes",
            "4000000",
            "--max-scratch-bytes",
            "4000000",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}

#[test]
fn companions_cli_offline_verify_mcp_exact_code_payload_and_reference() {
    let mut f = Fixture::with_mode(&[], 5);
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let meta = p.data("metadata", json!({}));
    assert_eq!(
        meta["snapshot"]["profile"],
        bslens_core::snapshot_v5::companion_writer::PROFILE
    );
    let root = p.data("tree", json!({}));
    let id = root["data"]["branch"]["parent_id"].clone();
    let catalogs = p.data(
        "tree",
        json!({"level":"kind","parent_id":id,"kind":"Catalog"}),
    );
    let owner = catalogs["data"]["items"][0]["key"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let branch = p.data("tree", json!({"level":"companions","parent_id":owner}));
    let items = branch["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 4);
    assert_eq!(items[0]["name"], "Card");
    let form = items[0]["object_id"].as_i64().unwrap();
    let formtree = p.data("tree", json!({"level":"object","parent_id":form}));
    assert_eq!(
        formtree["data"]["items"][0]["key"],
        "Catalogs/Items/Forms/Card/Ext/Form/Module.bsl"
    );
    let empty_form = items.iter().find(|v| v["name"] == "Empty").unwrap()["object_id"].clone();
    let empty = p.data("tree", json!({"level":"object","parent_id":empty_form}));
    assert!(empty["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|v| v["type"] != "module"));
    for kind in ["Document", "InformationRegister"] {
        let objects = p.data("tree", json!({"level":"kind","parent_id":id,"kind":kind}));
        let owner = objects["data"]["items"][0]["key"]
            .as_str()
            .unwrap()
            .parse::<i64>()
            .unwrap();
        let refs = p.data("tree", json!({"level":"companions","parent_id":owner}));
        assert_eq!(refs["data"]["items"].as_array().unwrap().len(), 1);
        let object = refs["data"]["items"][0]["object_id"].clone();
        let content = p.data("tree", json!({"level":"companions","parent_id":object}));
        let payload = content["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|v| v["role"] == "template")
            .unwrap();
        let source = p.data("source", payload["source"].clone());
        assert_eq!(source["data"]["encoding"], "base64");
        assert_eq!(bytes(&source["data"]), b"opaque, not parsed");
    }
    let code = p.data(
        "source",
        json!({"path":"Catalogs/Items/Forms/Card/Ext/Form/Module.bsl","start_line":1,"end_line":5}),
    );
    assert_eq!(bytes(&code["data"]), TEXT.as_bytes());
    let evidence = p.data("tree", json!({"level":"companions","parent_id":form}));
    for item in evidence["data"]["items"].as_array().unwrap() {
        if item["source"]["end_byte"] == 50000 {
            let refused = p.call("source", item["source"].clone());
            assert_eq!(refused["error"]["code"], -32001);
            assert_eq!(refused["error"]["message"], "Response exceeds byte limit");
            assert!(refused.get("result").is_none());
            let mut part = item["source"].clone();
            part["end_byte"] = json!(1);
            assert_eq!(bytes(&p.data("source", part)["data"]), [0xff]);
            continue;
        }
        let source = p.data("source", item["source"].clone());
        if item["role"] == "reference" {
            assert_eq!(bytes(&source["data"]), b"<Form>Card</Form>");
        } else {
            assert_eq!(source["data"]["encoding"], "base64");
        }
    }
    let template = items.iter().find(|v| v["name"] == "Print").unwrap()["object_id"].clone();
    let evidence = p.data("tree", json!({"level":"companions","parent_id":template}));
    let payload = evidence["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["role"] == "template")
        .unwrap();
    let got = p.data("source", payload["source"].clone());
    assert_eq!(bytes(&got["data"]), BINARY);
    assert_eq!(got["data"]["kind"], "payload");
    let missing = items
        .iter()
        .find(|v| v["name"] == "MissingPayload")
        .unwrap()["object_id"]
        .clone();
    let absent = p.data("tree", json!({"level":"companions","parent_id":missing}));
    assert_eq!(absent["data"]["items"].as_array().unwrap().len(), 1);
    assert_eq!(absent["data"]["contents"], "not_analyzed");
    assert_eq!(
        p.call(
            "tree",
            json!({"level":"companions","parent_id":owner,"max_nodes":1})
        )["result"]["structuredContent"]["code"],
        "budget_exceeded"
    );
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM modules WHERE path='Catalogs/Items/Forms/Card/Other.bsl'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    // Existing root facts retain parity; form code is owned by the form, not Catalog.
    assert_eq!(
        db.query_row(
            "SELECT owner_id FROM modules WHERE path=?1",
            ["Catalogs/Items/Forms/Card/Ext/Form/Module.bsl"],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        form
    );
    db.execute(
        "UPDATE modules SET owner_id=?1 WHERE path='Catalogs/Items/Forms/Card/Ext/Form/Module.bsl'",
        [owner],
    )
    .unwrap();
    let digest = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [digest])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "4000000",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "100",
            "--max-source-bytes",
            "1000000",
            "--max-total-bytes",
            "4000000",
            "--max-scratch-bytes",
            "4000000",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("VerifyFacts"));
}

#[test]
fn common_modules_cli_offline_mcp_exact_method_unparsed_owner_and_rollback() {
    let mut f = Fixture::with_mode(&[], 6);
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let meta = p.data("metadata", json!({}));
    assert_eq!(
        meta["snapshot"]["profile"],
        bslens_core::snapshot_v5::common_writer::PROFILE
    );
    let root = p.data("tree", json!({}));
    let group = root["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["key"] == "CommonModule")
        .unwrap();
    let objects = p.data("tree", group["children"].clone());
    let items = objects["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["name"], "Shared");
    assert_eq!(items[1]["name"], "Отказ");
    let owner = items[0]["key"].as_str().unwrap().parse::<i64>().unwrap();
    let object = p.data("tree", items[0]["children"].clone());
    let module = &object["data"]["items"][0];
    assert_eq!(module["key"], "CommonModules/Shared/Ext/Module.bsl");
    let methods = p.data("tree", module["children"].clone());
    assert_eq!(methods["data"]["items"].as_array().unwrap().len(), 2);
    let source = p.data("source", methods["data"]["items"][0]["source"].clone());
    assert_eq!(
        bytes(&source["data"]),
        "Процедура ЯПервый()\r\nКонецПроцедуры\r\n".as_bytes()
    );
    let full = p.data(
        "source",
        json!({"path":"CommonModules/Shared/Ext/Module.bsl","start_line":1,"end_line":5}),
    );
    assert_eq!(bytes(&full["data"]), TEXT.as_bytes());
    let refused = p.data("tree", items[1]["children"].clone());
    let module = &refused["data"]["items"][0];
    assert_eq!(module["parse_status"], "unparsed");
    let branch = p.data("tree", module["children"].clone());
    assert_eq!(branch["data"]["branch"]["status"], "unparsed");
    assert!(branch["data"]["items"].as_array().unwrap().is_empty());
    let raw = p.data(
        "source",
        json!({"path":"CommonModules/Отказ/Ext/Module.bsl","start_line":1,"end_line":2}),
    );
    assert_eq!(bytes(&raw["data"]), BINARY);
    assert_eq!(raw["data"]["encoding"], "base64");
    let tail = p.data(
        "source",
        json!({"path":"CommonModules/Отказ/Ext/Module.bsl","start_line":2,"end_line":2}),
    );
    assert_eq!(bytes(&tail["data"]), &BINARY[3..]);
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT owner_id FROM modules WHERE path='CommonModules/Shared/Ext/Module.bsl'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        owner
    );
    db.execute(
        "UPDATE modules SET owner_id=1 WHERE path='CommonModules/Shared/Ext/Module.bsl'",
        [],
    )
    .unwrap();
    let hash = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [hash])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "4000000",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "100",
            "--max-source-bytes",
            "1000000",
            "--max-total-bytes",
            "4000000",
            "--max-scratch-bytes",
            "4000000",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("VerifyFacts"));
}

#[test]
fn root_locators_cli_offline_mcp_descriptors_exact_xml_and_strict_refusals() {
    let mut f = Fixture::with_mode(&[], 7);
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let meta = p.data("metadata", json!({}));
    assert_eq!(
        meta["snapshot"]["profile"],
        bslens_core::snapshot_v5::root_writer::PROFILE
    );
    let root = p.data("tree", json!({}));
    let nodes = root["data"]["items"].as_array().unwrap();
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    for (kind, folder) in DESCRIPTOR_ROOTS {
        let group = nodes.iter().find(|n| n["key"] == *kind).unwrap();
        let objects = p.data("tree", group["children"].clone());
        assert_eq!(objects["data"]["items"].as_array().unwrap().len(), 1);
        let object = p.data("tree", objects["data"]["items"][0]["children"].clone());
        assert_eq!(object["data"]["branch"]["descriptor_only"], true);
        assert!(object["data"]["items"].as_array().unwrap().is_empty());
        let source = p.data("source", object["data"]["branch"]["source"].clone());
        let path = format!("{folder}/Пример.xml");
        assert_eq!(source["data"]["path"], path);
        let expected: Vec<u8> = db
            .query_row("SELECT content FROM sources WHERE path=?1", [&path], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(bytes(&source["data"]), expected);
        assert!(expected.starts_with(&[0xef, 0xbb, 0xbf]));
    }
    assert_eq!(
        db.query_row("SELECT count(*) FROM objects WHERE parent_id=1", [], |r| {
            r.get::<_, usize>(0)
        })
        .unwrap(),
        DESCRIPTOR_ROOTS.len() + 5
    );
    drop(db);
    let code = p.data(
        "source",
        json!({"path":"CommonModules/Shared/Ext/Module.bsl","start_line":1,"end_line":5}),
    );
    assert_eq!(bytes(&code["data"]), TEXT.as_bytes());
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    db.execute(
        "UPDATE objects SET name='Неверно' WHERE kind='StyleItem'",
        [],
    )
    .unwrap();
    let digest = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [digest])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "4000000",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "100",
            "--max-source-bytes",
            "1000000",
            "--max-total-bytes",
            "4000000",
            "--max-scratch-bytes",
            "4000000",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("VerifyFacts"));
}

#[test]
fn shared_objects_cli_offline_mcp_owners_optional_form_code_exact_payloads_and_rollback() {
    let mut f = Fixture::with_mode(&[], 8);
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let meta = p.data("metadata", json!({}));
    assert_eq!(
        meta["snapshot"]["profile"],
        bslens_core::snapshot_v5::shared_writer::PROFILE
    );
    assert_eq!(
        meta["snapshot"]["coverage"]["shared_structure"],
        "complete_known_grammar"
    );
    assert_eq!(
        meta["snapshot"]["coverage"]["shared_contents"],
        "not_analyzed"
    );
    let root = p.data("tree", json!({}));
    let root_nodes = root["data"]["items"].as_array().unwrap();
    let mut actual_owners = Vec::new();
    for (kind, folder, name) in SHARED_OBJECTS {
        let group = root_nodes.iter().find(|n| n["key"] == *kind).unwrap();
        let objects = p.data("tree", group["children"].clone());
        let object = objects["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == *name)
            .unwrap();
        let id = object["key"].as_str().unwrap().parse::<i64>().unwrap();
        let branch = p.data("tree", object["children"].clone());
        let modules: Vec<_> = branch["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["type"] == "module")
            .collect();
        let module = match *kind {
            "CommonCommand" => Some(format!("{folder}/{name}/Ext/CommandModule.bsl")),
            "CommonForm" if *name == "Card" => Some(format!("{folder}/{name}/Ext/Form/Module.bsl")),
            _ => None,
        };
        if let Some(module) = module {
            assert_eq!(modules.len(), 1);
            assert_eq!(modules[0]["key"], module);
            actual_owners.push((module.clone(), id));
            let methods = p.data("tree", modules[0]["children"].clone());
            if *name == "Refused" {
                assert_eq!(methods["data"]["branch"]["status"], "unparsed");
                assert!(methods["data"]["items"].as_array().unwrap().is_empty());
                assert_eq!(
                    bytes(
                        &p.data("source", json!({"path":module,"start_line":1,"end_line":2}))
                            ["data"]
                    ),
                    BINARY
                );
            } else {
                assert_eq!(methods["data"]["items"].as_array().unwrap().len(), 2);
                let got = p.data("source", methods["data"]["items"][0]["source"].clone());
                assert_eq!(
                    bytes(&got["data"]),
                    "Процедура ЯПервый()\r\nКонецПроцедуры\r\n".as_bytes()
                );
                assert_eq!(
                    bytes(
                        &p.data("source", json!({"path":module,"start_line":1,"end_line":5}))
                            ["data"]
                    ),
                    TEXT.as_bytes()
                );
            }
        } else {
            assert!(modules.is_empty());
        }
        let payloads = p.data("tree", json!({"level":"companions","parent_id":id}));
        let items = payloads["data"]["items"].as_array().unwrap();
        let reference = items.iter().find(|n| n["role"] == "reference").unwrap();
        assert_eq!(
            bytes(&p.data("source", reference["source"].clone())["data"]),
            format!("<{kind}>{name}</{kind}>").as_bytes()
        );
        let expected: Vec<_> = SHARED_PAYLOADS
            .iter()
            .filter(|(path, _)| path.starts_with(&format!("{folder}/{name}/")))
            .collect();
        assert_eq!(items.len(), expected.len() + 1);
        for (path, expected) in expected {
            let item = items.iter().find(|n| n["path"] == *path).unwrap();
            let got = p.data("source", item["source"].clone());
            assert_eq!(got["data"]["encoding"], "base64");
            assert_eq!(got["data"]["kind"], "payload");
            assert_eq!(bytes(&got["data"]), *expected);
        }
    }
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    for (path, id) in actual_owners {
        assert_eq!(
            db.query_row("SELECT owner_id FROM modules WHERE path=?1", [path], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            id
        );
    }
    db.execute(
        "UPDATE modules SET owner_id=1 WHERE path='CommonForms/Card/Ext/Form/Module.bsl'",
        [],
    )
    .unwrap();
    let digest = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [digest])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "4000000",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "100",
            "--max-source-bytes",
            "1000000",
            "--max-total-bytes",
            "4000000",
            "--max-scratch-bytes",
            "4000000",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("VerifyFacts"));
}

#[test]
fn leaf_objects_cli_offline_mcp_exact_owners_optional_modules_payloads_and_rollback() {
    let mut f = Fixture::with_mode(&[], 9);
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let meta = p.data("metadata", json!({}));
    assert_eq!(
        meta["snapshot"]["profile"],
        bslens_core::snapshot_v5::leaf_writer::PROFILE
    );
    assert_eq!(
        meta["snapshot"]["coverage"]["leaf_structure"],
        "complete_known_grammar"
    );
    assert_eq!(
        meta["snapshot"]["coverage"]["leaf_contents"],
        "not_analyzed"
    );
    let root = p.data("tree", json!({}));
    let root_nodes = root["data"]["items"].as_array().unwrap();
    let mut actual_owners = Vec::new();
    for (kind, folder, name) in LEAF_OBJECTS {
        let group = root_nodes.iter().find(|n| n["key"] == *kind).unwrap();
        let objects = p.data("tree", group["children"].clone());
        let object = objects["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == *name)
            .unwrap();
        let id = object["key"].as_str().unwrap().parse::<i64>().unwrap();
        let branch = p.data("tree", object["children"].clone());
        let modules: Vec<_> = branch["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["type"] == "module")
            .collect();
        if *kind == "Constant" {
            for module in ["ManagerModule", "ValueManagerModule"] {
                let path = format!("{folder}/{name}/Ext/{module}.bsl");
                let status = match LEAF_BSL.iter().find(|(p, _)| *p == path) {
                    Some((_, b)) if *b == BINARY => "unparsed",
                    Some(_) => "accepted",
                    None => "absent",
                };
                assert_eq!(branch["data"]["branch"]["module_presence"][module], status);
            }
        }
        if *kind == "ScheduledJob" {
            assert_eq!(
                branch["data"]["branch"]["schedule_presence"],
                if *name == "With" {
                    "present_unparsed"
                } else {
                    "absent"
                }
            );
        }
        let expected_modules: Vec<_> = LEAF_BSL
            .iter()
            .filter(|(path, _)| path.starts_with(&format!("{folder}/{name}/")))
            .collect();
        assert_eq!(modules.len(), expected_modules.len());
        for (module, expected) in expected_modules {
            let node = modules.iter().find(|n| n["key"] == *module).unwrap();
            actual_owners.push((module.to_string(), id));
            let methods = p.data("tree", node["children"].clone());
            if *expected == BINARY {
                assert_eq!(methods["data"]["branch"]["status"], "unparsed");
                assert!(methods["data"]["items"].as_array().unwrap().is_empty());
                assert_eq!(
                    bytes(
                        &p.data("source", json!({"path":module,"start_line":1,"end_line":2}))
                            ["data"]
                    ),
                    BINARY
                );
            } else {
                assert_eq!(methods["data"]["branch"]["status"], "accepted");
                assert_eq!(methods["data"]["items"].as_array().unwrap().len(), 2);
                assert_eq!(
                    bytes(&p.data("source", methods["data"]["items"][0]["source"].clone())["data"]),
                    "Процедура ЯПервый()\r\nКонецПроцедуры\r\n".as_bytes()
                );
                assert_eq!(
                    bytes(
                        &p.data("source", json!({"path":module,"start_line":1,"end_line":5}))
                            ["data"]
                    ),
                    TEXT.as_bytes()
                );
            }
        }
        let payloads = p.data("tree", json!({"level":"companions","parent_id":id}));
        let items = payloads["data"]["items"].as_array().unwrap();
        let reference = items.iter().find(|n| n["role"] == "reference").unwrap();
        assert_eq!(
            bytes(&p.data("source", reference["source"].clone())["data"]),
            format!("<{kind}>{name}</{kind}>").as_bytes()
        );
        let expected: Vec<_> = LEAF_PAYLOADS
            .iter()
            .filter(|(path, _)| path.starts_with(&format!("{folder}/{name}/")))
            .collect();
        assert_eq!(items.len(), expected.len() + 1);
        assert_eq!(
            payloads["data"]["payload_presence"],
            if expected.is_empty() {
                "absent"
            } else {
                "present_unparsed"
            }
        );
        for (path, expected) in expected {
            let item = items.iter().find(|n| n["path"] == *path).unwrap();
            let got = p.data("source", item["source"].clone());
            assert_eq!(got["data"]["encoding"], "base64");
            assert_eq!(got["data"]["kind"], "payload");
            assert_eq!(bytes(&got["data"]), *expected);
        }
    }
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    for (path, id) in actual_owners {
        assert_eq!(
            db.query_row("SELECT owner_id FROM modules WHERE path=?1", [path], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            id
        );
    }
    db.execute(
        "UPDATE modules SET owner_id=1 WHERE path='Constants/Both/Ext/ValueManagerModule.bsl'",
        [],
    )
    .unwrap();
    let digest = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [digest])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "4000000",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "200",
            "--max-source-bytes",
            "1000000",
            "--max-total-bytes",
            "4000000",
            "--max-scratch-bytes",
            "4000000",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("VerifyFacts"));
}

#[test]
fn enum_journal_cli_offline_mcp_inline_owners_exact_bytes_and_rollback() {
    let mut f = Fixture::with_mode(&[], 10);
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let meta = p.data("metadata", json!({}));
    assert_eq!(
        meta["snapshot"]["profile"],
        bslens_core::snapshot_v5::journal_writer::PROFILE
    );
    assert_eq!(
        meta["snapshot"]["coverage"]["journal_structure"],
        "complete_known_grammar"
    );
    assert_eq!(
        meta["snapshot"]["coverage"]["journal_contents"],
        "not_analyzed"
    );
    let root = p.data("tree", json!({}));
    let mut owners = Vec::new();
    for (kind, folder, name) in JOURNAL_OBJECTS {
        let group = root["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["key"] == *kind)
            .unwrap();
        let objects = p.data("tree", group["children"].clone());
        let object = objects["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == *name)
            .unwrap();
        let id = object["key"].as_str().unwrap().parse::<i64>().unwrap();
        let path = format!("{folder}/{name}.xml");
        owners.push((path.clone(), id));
        let branch = p.data("tree", object["children"].clone());
        let descriptor =
            bytes(&p.data("source", branch["data"]["branch"]["source"].clone())["data"]);
        assert!(descriptor.starts_with(&[0xef, 0xbb, 0xbf]));
        let inline = p.data("tree", json!({"level":"inline","parent_id":id}));
        let items = inline["data"]["items"].as_array().unwrap();
        let expected: Vec<_> = JOURNAL_INLINE
            .iter()
            .filter(|(p, _, _, _)| *p == path)
            .collect();
        assert_eq!(items.len(), expected.len());
        let order: Vec<_> = items
            .iter()
            .map(|n| (n["kind"].as_str().unwrap(), n["name"].as_str().unwrap()))
            .collect();
        assert!(order.windows(2).all(|w| w[0] <= w[1]));
        for (_, k, n, u) in expected {
            let node = items
                .iter()
                .find(|i| i["kind"] == *k && i["name"] == *n)
                .unwrap();
            assert_eq!(node["owner_id"], id);
            assert!(node["parent_key"].is_null());
            assert_eq!(node["path"], path);
            let exact = bytes(&p.data("source", node["source"].clone())["data"]);
            assert_eq!(exact, journal_inline(k, n, *u).as_bytes());
            let a = node["start_byte"].as_u64().unwrap() as usize;
            let b = node["end_byte"].as_u64().unwrap() as usize;
            assert_eq!(&descriptor[a..b], exact);
            if *k == "Command" {
                let module = format!("{folder}/{name}/Commands/{n}/Ext/CommandModule.bsl");
                assert_eq!(node["module_path"], module);
                let listed = branch["data"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|n| n["key"] == module)
                    .unwrap();
                assert_eq!(listed["owner_declaration"], node["key"]);
                assert_eq!(listed["container_object_id"], id);
                let methods = p.data("tree", node["children"].clone());
                assert_eq!(methods["data"]["branch"]["owner_declaration"], node["key"]);
                assert_eq!(methods["data"]["branch"]["container_object_id"], id);
                if *n == "Refused" {
                    assert_eq!(methods["data"]["branch"]["status"], "unparsed");
                    assert!(methods["data"]["items"].as_array().unwrap().is_empty());
                } else {
                    assert_eq!(methods["data"]["items"].as_array().unwrap().len(), 2);
                    assert_eq!(
                        bytes(
                            &p.data("source", methods["data"]["items"][0]["source"].clone())
                                ["data"]
                        ),
                        "Процедура ЯПервый()\r\nКонецПроцедуры\r\n".as_bytes()
                    );
                }
            } else {
                assert!(node["children"].is_null());
            }
        }
        let companions = p.data("tree", json!({"level":"companions","parent_id":id}));
        let nodes = companions["data"]["items"].as_array().unwrap();
        let reference = nodes.iter().find(|n| n["role"] == "reference").unwrap();
        assert_eq!(
            bytes(&p.data("source", reference["source"].clone())["data"]),
            format!("<{kind}>{name}</{kind}>").as_bytes()
        );
        if *kind == "DocumentJournal" {
            for node in nodes.iter().filter(|n| n["role"] == "descriptor") {
                let child = p.data("tree", node["children"].clone());
                let child_id = node["object_id"].as_i64().unwrap();
                let child_path = node["path"].as_str().unwrap().to_string();
                owners.push((child_path.clone(), child_id));
                let saved = p.data("tree", json!({"level":"companions","parent_id":child_id}));
                let payloads: Vec<_> = JOURNAL_PAYLOADS
                    .iter()
                    .filter(|(p, _)| {
                        p.starts_with(&format!("{}/", child_path.strip_suffix(".xml").unwrap()))
                    })
                    .collect();
                assert_eq!(
                    saved["data"]["items"].as_array().unwrap().len(),
                    payloads.len() + 1
                );
                if child_path.contains("/Forms/") {
                    assert_eq!(
                        child["data"]["items"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .filter(|n| n["type"] == "module")
                            .count(),
                        1
                    );
                }
                for (path, expected) in payloads {
                    let item = saved["data"]["items"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|n| n["path"] == *path)
                        .unwrap();
                    assert_eq!(
                        bytes(&p.data("source", item["source"].clone())["data"]),
                        *expected
                    );
                }
            }
        }
    }
    for (path, expected) in JOURNAL_BSL {
        let last = if *expected == BINARY { 2 } else { 5 };
        assert_eq!(
            bytes(
                &p.data(
                    "source",
                    json!({"path":path,"start_line":1,"end_line":last})
                )["data"]
            ),
            *expected
        );
    }
    for (path, expected) in JOURNAL_PAYLOADS {
        let got = p.data(
            "source",
            json!({"saved_path":path,"start_byte":0,"end_byte":expected.len()}),
        );
        assert_eq!(got["data"]["encoding"], "base64");
        assert_eq!(got["data"]["kind"], "payload");
        assert_eq!(bytes(&got["data"]), *expected);
    }
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    for (path, _) in JOURNAL_BSL {
        let descriptor = if let Some((prefix, _)) = path.split_once("/Commands/") {
            format!("{prefix}.xml")
        } else {
            format!("{}.xml", path.split_once("/Ext/").unwrap().0)
        };
        let id = owners.iter().find(|(p, _)| *p == descriptor).unwrap().1;
        assert_eq!(
            db.query_row("SELECT owner_id FROM modules WHERE path=?1", [path], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            id
        );
    }
    let coverage: String = db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    for prop in [
        "DefaultForm",
        "AuxiliaryForm",
        "RegisteredDocuments",
        "Color",
        "Synonym",
        "Comment",
    ] {
        assert!(coverage.contains(prop));
    }
    // Structurally valid wrong immediate owner: canonical sums alone cannot prove derivation.
    db.execute("UPDATE inline_module_owners SET declaration_key=(SELECT declaration_key FROM inline_declarations WHERE xml_path='DocumentJournals/Full.xml' AND kind='Column') WHERE module_path='DocumentJournals/Full/Commands/Run/Ext/CommandModule.bsl'",[]).unwrap();
    let hash = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [hash])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "4000000",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "200",
            "--max-source-bytes",
            "1000000",
            "--max-total-bytes",
            "4000000",
            "--max-scratch-bytes",
            "4000000",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("VerifyFacts"));
}

#[test]
fn exact_name_cli_804_modules_large_methods_offline_stdio() {
    let mut f = Fixture::new(&[]);
    let repo = f.root.join("exact-name-repo");
    fs::create_dir(&repo).unwrap();
    let git = |args: &[&str]| {
        let o = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let descriptor = |kind: &str, name: &str, n: usize, body: &str| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{kind} uuid=\"30000000-0000-0000-0000-{n:012}\"><Properties><Name>{name}</Name></Properties>{body}</{kind}></MetaDataObject>")
    };
    let mut references = String::new();
    let mut source = String::from("\u{feff}// exact names\r\n");
    for i in 0..129 {
        source.push_str(&format!("Процедура M{i:03}()\r\nКонецПроцедуры\r\n"));
    }
    for _ in 0..2 {
        source.push_str("Процедура Duplicate()\r\nКонецПроцедуры\r\n");
    }
    for _ in 0..129 {
        source.push_str("Процедура Many()\r\nКонецПроцедуры\r\n");
    }
    let method = "Процедура Цель()\r\nКонецПроцедуры";
    source.push_str(method);
    for i in 0..804 {
        let name = if i == 803 {
            "ZzЦель".to_string()
        } else {
            format!("M{i:03}")
        };
        references.push_str(&format!("<CommonModule>{name}</CommonModule>"));
        let d = repo.join(format!("CommonModules/{name}.xml"));
        fs::create_dir_all(d.parent().unwrap()).unwrap();
        fs::write(d, descriptor("CommonModule", &name, i + 2, "")).unwrap();
        let m = repo.join(format!("CommonModules/{name}/Ext/Module.bsl"));
        fs::create_dir_all(m.parent().unwrap()).unwrap();
        fs::write(
            m,
            if i == 803 {
                source.as_bytes()
            } else if i == 0 {
                BINARY
            } else {
                b"// accepted without methods"
            },
        )
        .unwrap();
    }
    fs::write(
        repo.join("Configuration.xml"),
        descriptor(
            "Configuration",
            "Synthetic",
            1,
            &format!("<ChildObjects>{references}</ChildObjects>"),
        ),
    )
    .unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@invalid",
        "commit",
        "-qm",
        "exact names",
    ]);
    let commit = git(&["rev-parse", "HEAD"]);
    let tree = git(&["rev-parse", "HEAD^{tree}"]);
    let output = f.root.join("exact-name-snapshot");
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .args([
            "build-journals",
            repo.to_str().unwrap(),
            &commit,
            &tree,
            "synthetic:exact-name",
            output.to_str().unwrap(),
            "2000",
            "1000000",
            "16000000",
            "32000000",
            "5000",
            "5000",
            "5000",
            "1000000",
            "16000000",
            "30000",
            "100000",
            "1048576",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    f.built = BuiltSnapshot {
        path: PathBuf::from(v["path"].as_str().unwrap()),
        snapshot_id: v["snapshot_id"].as_str().unwrap().into(),
        facts_sha256: v["facts_sha256"].as_str().unwrap().into(),
        file_sha256: v["file_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.read_limit = "32000000";
    fs::remove_dir_all(&repo).unwrap();
    assert!(!repo.exists());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "32000000",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "2000",
            "--max-source-bytes",
            "1000000",
            "--max-total-bytes",
            "16000000",
            "--max-scratch-bytes",
            "32000000",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut p = Peer::start(&f);
    let advertised = p.request("tools/list", json!({}));
    assert_eq!(
        advertised["result"]["tools"][1]["inputSchema"]["properties"]["name_exact"]["type"],
        "string"
    );
    // Client starts with visible kind/object/method names only; every ID/path comes from responses.
    let root = p.data("tree", json!({}));
    let group = root["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "CommonModule")
        .unwrap();
    let kind_args = group["children"].clone();
    let denied = p.call("tree", kind_args.clone());
    assert_eq!(
        denied["result"]["structuredContent"]["code"],
        "budget_exceeded"
    );
    let mut search = kind_args.clone();
    search["name_exact"] = json!("ZzЦель");
    let found = p.data("tree", search.clone());
    assert_eq!(found["data"]["branch"]["match_status"], "matched");
    assert_eq!(found["data"]["complete_branch"], false);
    assert_eq!(found["data"]["complete_matches"], true);
    assert_eq!(found["data"]["items"].as_array().unwrap().len(), 1);
    let object = p.data("tree", found["data"]["items"][0]["children"].clone());
    let module = object["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["type"] == "module")
        .unwrap();
    let module_args = module["children"].clone();
    assert_eq!(
        p.call("tree", module_args.clone())["result"]["structuredContent"]["code"],
        "budget_exceeded"
    );
    let mut methods = module_args.clone();
    methods["name_exact"] = json!("цель");
    let found = p.data("tree", methods.clone());
    assert_eq!(found["data"]["branch"]["match_status"], "matched");
    let got = p.data("source", found["data"]["items"][0]["source"].clone());
    assert_eq!(bytes(&got["data"]), method.as_bytes());
    for absent in ["Цель", "цель ", "Missing", "%", "Цел_", "Це\u{301}ль"] {
        methods["name_exact"] = json!(absent);
        let got = p.data("tree", methods.clone());
        assert_eq!(got["data"]["branch"]["match_status"], "no_match");
        assert!(got["data"]["items"].as_array().unwrap().is_empty());
    }
    for absent in ["zzЦель", "Zzцель", "Missing", "%"] {
        search["name_exact"] = json!(absent);
        let got = p.data("tree", search.clone());
        assert_eq!(got["data"]["branch"]["match_status"], "no_match");
    }
    methods["name_exact"] = json!("duplicate");
    let duplicate = p.data("tree", methods.clone());
    assert_eq!(duplicate["data"]["branch"]["match_status"], "ambiguous");
    let items = duplicate["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_ne!(items[0]["key"], items[1]["key"]);
    assert!(items[0]["source"]["start_line"].as_u64() < items[1]["source"]["start_line"].as_u64());
    methods["max_nodes"] = json!(1);
    assert_eq!(
        p.call("tree", methods.clone())["result"]["structuredContent"]["code"],
        "budget_exceeded"
    );
    methods.as_object_mut().unwrap().remove("max_nodes");
    methods["name_exact"] = json!("many");
    assert_eq!(
        p.call("tree", methods.clone())["result"]["structuredContent"]["code"],
        "budget_exceeded"
    );
    for (name, status) in [("M000", "unparsed"), ("M001", "no_match")] {
        search["name_exact"] = json!(name);
        let found = p.data("tree", search.clone());
        let object = p.data("tree", found["data"]["items"][0]["children"].clone());
        let module = object["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["type"] == "module")
            .unwrap();
        let mut args = module["children"].clone();
        args["name_exact"] = json!("цель");
        let got = p.data("tree", args);
        assert_eq!(got["data"]["branch"]["match_status"], status);
        assert_eq!(got["data"]["complete_matches"], status != "unparsed");
    }
    for value in [json!(null), json!(1), json!(""), json!("x".repeat(1025))] {
        search["name_exact"] = value;
        assert_eq!(p.call("tree", search.clone())["error"]["code"], -32602);
    }
    assert_eq!(
        p.call("tree", json!({"name_exact":"ZzЦель"}))["error"]["code"],
        -32602
    );
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let reader = bslens_core::linux_v5_reader::LinuxSnapshotReader::open(
        &f.built.path,
        &f.stage,
        Some(&f.built.file_sha256),
        32000000,
    )
    .unwrap();
    let parent = kind_args["parent_id"].as_i64().unwrap();
    assert!(matches!(
        reader.tree_named(
            "kind",
            Some(parent),
            Some("CommonModule"),
            None,
            Some("ZzЦель"),
            bslens_core::linux_v5_reader::TreeLimits {
                rows: 128,
                bytes: 1
            }
        ),
        Err(bslens_core::linux_v5_reader::Error::Limit)
    ));
}

#[test]
fn services_cli_offline_stdio_nested_xml_owners_bytes_and_lookup() {
    let mut f = Fixture::with_mode(&[], 11);
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let metadata = p.data("metadata", json!({}));
    assert_eq!(
        metadata["snapshot"]["profile"],
        bslens_core::snapshot_v5::service_writer::PROFILE
    );
    assert_eq!(
        metadata["snapshot"]["coverage"]["service_structure"],
        "complete_known_grammar"
    );
    assert_eq!(
        metadata["snapshot"]["coverage"]["service_semantics"],
        "not_analyzed"
    );
    let root = p.data("tree", json!({}));
    let mut owners = Vec::new();
    let mut seen = 0;
    for (kind, folder, name) in SERVICE_ROOTS {
        let group = root["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == *kind)
            .unwrap();
        let mut args = group["children"].clone();
        args["name_exact"] = json!(name);
        let objects = p.data("tree", args);
        assert_eq!(objects["data"]["branch"]["match_status"], "matched");
        let object = &objects["data"]["items"][0];
        let id = object["key"].as_str().unwrap().parse::<i64>().unwrap();
        let path = format!("{folder}/{name}.xml");
        owners.push((format!("{folder}/{name}/Ext/Module.bsl"), id));
        let branch = p.data("tree", object["children"].clone());
        let descriptor =
            bytes(&p.data("source", branch["data"]["branch"]["source"].clone())["data"]);
        assert!(descriptor.starts_with(&[0xef, 0xbb, 0xbf]));
        let modules: Vec<_> = branch["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["type"] == "module")
            .collect();
        assert_eq!(modules.len(), 1);
        let mut args = modules[0]["children"].clone();
        args["name_exact"] = json!("япервый");
        let method = p.data("tree", args);
        if *kind == "IntegrationService" {
            assert_eq!(method["data"]["branch"]["match_status"], "unparsed");
            assert_eq!(method["data"]["complete_matches"], false);
            assert!(method["data"]["items"].as_array().unwrap().is_empty());
        } else {
            assert_eq!(method["data"]["branch"]["match_status"], "matched");
            assert_eq!(
                bytes(&p.data("source", method["data"]["items"][0]["source"].clone())["data"]),
                "Процедура ЯПервый()\r\nКонецПроцедуры\r\n".as_bytes()
            );
        }
        let last = if *kind == "IntegrationService" { 2 } else { 5 };
        assert_eq!(
            bytes(
                &p.data(
                    "source",
                    json!({"path":modules[0]["key"],"start_line":1,"end_line":last})
                )["data"]
            ),
            if *kind == "IntegrationService" {
                BINARY
            } else {
                TEXT.as_bytes()
            }
        );
        let mut stack = vec![(json!({"level":"inline","parent_id":id}), None::<String>)];
        while let Some((args, parent)) = stack.pop() {
            let branch = p.data("tree", args);
            let nodes = branch["data"]["items"].as_array().unwrap();
            if nodes.is_empty() {
                assert!(parent.as_deref().unwrap().ends_with("000000001006"));
            }
            for node in nodes {
                seen += 1;
                assert_eq!(node["owner_id"], id);
                assert_eq!(node["parent_key"], json!(parent));
                assert_eq!(node["path"], path);
                assert_eq!(node["semantics"], "not_analyzed");
                let key = node["key"].as_str().unwrap();
                let n = key.rsplit('-').next().unwrap().parse::<u32>().unwrap();
                let expected = match n {
                    1001 => service_body("HTTPService"),
                    1002 => service_node("Method", "Get", 1002, ""),
                    1003 => service_node("Method", "Post", 1003, ""),
                    1004 => service_node(
                        "Operation",
                        "WithParameter",
                        1004,
                        &format!(
                            "<ChildObjects>{}</ChildObjects>",
                            service_node("Parameter", "Input", 1005, "")
                        ),
                    ),
                    1005 => service_node("Parameter", "Input", 1005, ""),
                    1006 => service_node("Operation", "Empty", 1006, "<ChildObjects/>"),
                    1007 => service_body("IntegrationService"),
                    _ => panic!("unexpected {n}"),
                };
                let raw = bytes(&p.data("source", node["source"].clone())["data"]);
                assert_eq!(raw, expected.as_bytes());
                let a = node["start_byte"].as_u64().unwrap() as usize;
                let b = node["end_byte"].as_u64().unwrap() as usize;
                assert_eq!(&descriptor[a..b], raw);
                if [1002, 1003, 1005, 1007].contains(&n) {
                    assert!(node["children"].is_null());
                } else {
                    assert!(!node["children"].is_null());
                    stack.push((node["children"].clone(), Some(key.to_string())));
                }
                assert!(node.get("module_path").is_none());
            }
        }
        let refs = p.data("tree", json!({"level":"companions","parent_id":id}));
        assert_eq!(refs["data"]["items"].as_array().unwrap().len(), 1);
        let r = &refs["data"]["items"][0];
        assert_eq!(r["role"], "reference");
        assert_eq!(
            bytes(&p.data("source", r["source"].clone())["data"]),
            format!("<{kind}>{name}</{kind}>").as_bytes()
        );
    }
    assert_eq!(seen, 7);
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    for (path, id) in owners {
        assert_eq!(
            db.query_row("SELECT owner_id FROM modules WHERE path=?1", [path], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            id
        );
    }
    let coverage: String = db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    assert!(coverage.contains("Handler"));
    assert!(coverage.contains("InternalInfo"));
    db.execute("UPDATE inline_declarations SET parent_key=(SELECT declaration_key FROM inline_declarations WHERE xml_path='HTTPServices/Http.xml' AND name='Post') WHERE xml_path='HTTPServices/Http.xml' AND name='Get'",[]).unwrap();
    let hash = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [hash])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "4000000",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "200",
            "--max-source-bytes",
            "1000000",
            "--max-total-bytes",
            "4000000",
            "--max-scratch-bytes",
            "4000000",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("VerifyFacts"));
}

#[test]
fn charts_cli_offline_stdio_inline_forms_png_owners_and_rollback() {
    let mut f = Fixture::with_mode(&[], 12);
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let metadata = p.data("metadata", json!({}));
    assert_eq!(
        metadata["snapshot"]["profile"],
        bslens_core::snapshot_v5::chart_writer::PROFILE
    );
    assert_eq!(
        metadata["snapshot"]["coverage"]["chart_structure"],
        "complete_known_grammar"
    );
    assert_eq!(
        metadata["snapshot"]["coverage"]["chart_contents"],
        "not_analyzed"
    );
    let root = p.data("tree", json!({}));
    let mut owners = Vec::new();
    let mut payload_owners = Vec::new();
    let mut count = 0;
    let mut account_id = 0;
    for (kind, folder, name) in CHART_ROOTS {
        let group = root["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == *kind)
            .unwrap();
        let mut args = group["children"].clone();
        args["name_exact"] = json!(name);
        let found = p.data("tree", args);
        assert_eq!(found["data"]["branch"]["match_status"], "matched");
        let object = &found["data"]["items"][0];
        let id = object["key"].as_str().unwrap().parse::<i64>().unwrap();
        if *kind == "ChartOfAccounts" {
            account_id = id;
        }
        let branch = p.data("tree", object["children"].clone());
        let raw = bytes(&p.data("source", branch["data"]["branch"]["source"].clone())["data"]);
        assert!(raw.starts_with(&[0xef, 0xbb, 0xbf]));
        let modules: Vec<_> = branch["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["type"] == "module")
            .collect();
        assert_eq!(
            modules.len(),
            if *kind == "ChartOfCalculationTypes" {
                1
            } else {
                2
            }
        );
        for module in modules {
            let path = module["key"].as_str().unwrap();
            owners.push((path.to_string(), id));
            let refused =
                *kind == "ChartOfCharacteristicTypes" && path.ends_with("ObjectModule.bsl");
            let mut args = module["children"].clone();
            args["name_exact"] = json!("япервый");
            let methods = p.data("tree", args);
            assert_eq!(
                methods["data"]["branch"]["match_status"],
                if refused { "unparsed" } else { "matched" }
            );
            assert_eq!(
                bytes(
                    &p.data(
                        "source",
                        json!({"path":path,"start_line":1,"end_line":if refused{2}else{5}})
                    )["data"]
                ),
                if refused { BINARY } else { TEXT.as_bytes() }
            );
        }
        let mut stack = vec![(json!({"level":"inline","parent_id":id}), None::<String>)];
        while let Some((args, parent)) = stack.pop() {
            let response = p.data("tree", args);
            let nodes = response["data"]["items"].as_array().unwrap();
            if *kind == "ChartOfCalculationTypes" {
                assert!(nodes.is_empty());
            }
            for node in nodes {
                count += 1;
                assert_eq!(node["owner_id"], id);
                assert_eq!(node["parent_key"], json!(parent));
                assert_eq!(node["path"], format!("{folder}/{name}.xml"));
                let key = node["key"].as_str().unwrap();
                let n = key.rsplit('-').next().unwrap().parse::<u32>().unwrap();
                let expected = match n {
                    1201 => chart_node("AccountingFlag", "Flag", 1201, ""),
                    1202 => chart_node("ExtDimensionAccountingFlag", "DimensionFlag", 1202, ""),
                    1203 => chart_node("Attribute", "Attribute", 1203, ""),
                    1204 => chart_node("Attribute", "Direct", 1204, ""),
                    1205 => chart_node(
                        "TabularSection",
                        "Rows",
                        1205,
                        &format!(
                            "<InternalInfo><Opaque/></InternalInfo><ChildObjects>{}</ChildObjects>",
                            chart_node("Attribute", "Child", 1206, "")
                        ),
                    ),
                    1206 => chart_node("Attribute", "Child", 1206, ""),
                    _ => panic!("{n}"),
                };
                let exact = bytes(&p.data("source", node["source"].clone())["data"]);
                assert_eq!(exact, expected.as_bytes());
                let a = node["start_byte"].as_u64().unwrap() as usize;
                let b = node["end_byte"].as_u64().unwrap() as usize;
                assert_eq!(&raw[a..b], exact);
                if n == 1205 {
                    stack.push((node["children"].clone(), Some(key.to_string())));
                } else {
                    assert!(node["children"].is_null());
                }
            }
        }
        let companions = p.data("tree", json!({"level":"companions","parent_id":id}));
        assert_eq!(
            companions["data"]["payload_presence"],
            if *kind == "ChartOfCalculationTypes" {
                "absent"
            } else {
                "present_unparsed"
            }
        );
        let nodes = companions["data"]["items"].as_array().unwrap();
        let reference = nodes.iter().find(|n| n["role"] == "reference").unwrap();
        assert_eq!(
            bytes(&p.data("source", reference["source"].clone())["data"]),
            format!("<{kind}>{name}</{kind}>").as_bytes()
        );
        let form = nodes.iter().find(|n| n["role"] == "descriptor").unwrap();
        let form_id = form["object_id"].as_i64().unwrap();
        let form_branch = p.data("tree", form["children"].clone());
        let module = form_branch["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["type"] == "module")
            .unwrap();
        owners.push((module["key"].as_str().unwrap().to_string(), form_id));
        assert_eq!(
            bytes(
                &p.data(
                    "source",
                    json!({"path":module["key"],"start_line":1,"end_line":5})
                )["data"]
            ),
            TEXT.as_bytes()
        );
        let form_payloads = p.data("tree", json!({"level":"companions","parent_id":form_id}));
        let form_ref = form_payloads["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["role"] == "reference")
            .unwrap();
        assert_eq!(
            bytes(&p.data("source", form_ref["source"].clone())["data"]),
            b"<Form>Card</Form>"
        );
        for (path, expected) in CHART_PAYLOADS
            .iter()
            .filter(|(p, _)| p.starts_with(&format!("{folder}/{name}/")))
        {
            let (response, owner) = if path.contains("/Forms/") {
                (&form_payloads, form_id)
            } else {
                (&companions, id)
            };
            let item = response["data"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|n| n["path"] == *path)
                .unwrap();
            assert_eq!(item["object_id"], owner);
            payload_owners.push((path.to_string(), owner));
            let got = p.data("source", item["source"].clone());
            assert_eq!(got["data"]["kind"], "payload");
            assert_eq!(got["data"]["encoding"], "base64");
            assert_eq!(bytes(&got["data"]), *expected);
        }
    }
    assert_eq!(count, 6);
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    for (path, id) in owners {
        assert_eq!(
            db.query_row("SELECT owner_id FROM modules WHERE path=?1", [path], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            id
        );
    }
    for (path, id) in payload_owners {
        assert_eq!(
            db.query_row(
                "SELECT object_id FROM companion_payloads WHERE path=?1",
                [path],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            id
        );
    }
    let coverage: String = db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    assert!(coverage.contains("InternalInfo"));
    assert!(coverage.contains("UnknownProperty"));
    db.execute("UPDATE companion_payloads SET object_id=?1 WHERE path='ChartsOfAccounts/Accounts/Forms/Card/Ext/Form/Items/Item/Picture.png'",[account_id]).unwrap();
    let hash = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [hash])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "4000000",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "200",
            "--max-source-bytes",
            "1000000",
            "--max-total-bytes",
            "4000000",
            "--max-scratch-bytes",
            "4000000",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("VerifyFacts"));
}

const REGISTER_ROOTS: &[(&str, &str, &str)] = &[
    ("AccountingRegister", "AccountingRegisters", "Books"),
    ("AccumulationRegister", "AccumulationRegisters", "WithCode"),
    (
        "AccumulationRegister",
        "AccumulationRegisters",
        "WithoutCode",
    ),
    ("CalculationRegister", "CalculationRegisters", "Calc"),
];
fn register_descriptor(kind: &str, name: &str, id: u32, body: &str) -> String {
    format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{kind} uuid=\"00000000-0000-0000-0000-{id:012}\"><Properties><Name>{name}</Name></Properties></{kind}></MetaDataObject>").replace(
        &format!("</{kind}>"),
        &format!(
            "<InternalInfo><Opaque/></InternalInfo><ChildObjects>{body}</ChildObjects></{kind}>"
        ),
    )
}
fn register_body(kind: &str, name: &str) -> String {
    let offset = match name {
        "Books" => 0,
        "WithCode" => 10,
        "WithoutCode" => 20,
        _ => 30,
    };
    format!(
        "{}{}{}<Form>Card</Form>{}",
        if kind == "AccountingRegister" {
            chart_node("Attribute", "Attr", 1700, "")
        } else {
            String::new()
        },
        chart_node("Dimension", "Dim", 1701 + offset, ""),
        chart_node("Resource", "Value", 1702 + offset, ""),
        if kind == "CalculationRegister" {
            "<Recalculation>Rebuild</Recalculation>"
        } else {
            ""
        }
    )
}
fn recalculation_body() -> String {
    format!(
        "{}{}",
        chart_node("Dimension", "First", 1801, ""),
        chart_node("Dimension", "Second", 1802, "")
    )
}

#[test]
fn registers_cli_offline_stdio_recalculation_identity_bytes_and_rollback() {
    let mut f = Fixture::with_mode(&[], 13);
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let meta = p.data("metadata", json!({}));
    assert_eq!(
        meta["snapshot"]["profile"],
        bslens_core::snapshot_v5::register_writer::PROFILE
    );
    assert_eq!(
        meta["snapshot"]["coverage"]["register_structure"],
        "complete_known_grammar"
    );
    assert_eq!(
        meta["snapshot"]["coverage"]["register_semantics"],
        "not_analyzed"
    );
    let root = p.data("tree", json!({}));
    let mut owners = Vec::new();
    let mut calc = 0;
    let mut recalc = 0;
    for (kind, folder, name) in REGISTER_ROOTS {
        let group = root["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == *kind)
            .unwrap();
        let mut a = group["children"].clone();
        a["name_exact"] = json!(name);
        let found = p.data("tree", a);
        assert_eq!(found["data"]["branch"]["match_status"], "matched");
        let object = &found["data"]["items"][0];
        let id = object["key"].as_str().unwrap().parse::<i64>().unwrap();
        if *kind == "CalculationRegister" {
            calc = id;
        }
        let branch = p.data("tree", object["children"].clone());
        let raw = bytes(&p.data("source", branch["data"]["branch"]["source"].clone())["data"]);
        assert!(raw.starts_with(&[0xef, 0xbb, 0xbf]));
        let mods: Vec<_> = branch["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["type"] == "module")
            .collect();
        assert_eq!(
            mods.len(),
            match *name {
                "Books" => 1,
                "WithCode" => 2,
                _ => 0,
            }
        );
        for m in mods {
            let path = m["key"].as_str().unwrap();
            let refused = *name == "WithCode" && path.ends_with("RecordSetModule.bsl");
            let mut a = m["children"].clone();
            a["name_exact"] = json!("япервый");
            let methods = p.data("tree", a);
            owners.push((path.to_string(), id));
            if !refused {
                assert_eq!(
                    bytes(&p.data("source", methods["data"]["items"][0]["source"].clone())["data"]),
                    "Процедура ЯПервый()\r\nКонецПроцедуры\r\n".as_bytes()
                );
            }
            assert_eq!(
                methods["data"]["branch"]["match_status"],
                if refused { "unparsed" } else { "matched" }
            );
            assert_eq!(
                bytes(
                    &p.data(
                        "source",
                        json!({"path":path,"start_line":1,"end_line":if refused{2}else{5}})
                    )["data"]
                ),
                if refused { BINARY } else { TEXT.as_bytes() }
            );
        }
        let inline = p.data("tree", json!({"level":"inline","parent_id":id}));
        assert_eq!(
            inline["data"]["items"].as_array().unwrap().len(),
            if *kind == "AccountingRegister" { 3 } else { 2 }
        );
        for n in inline["data"]["items"].as_array().unwrap() {
            assert_eq!(n["owner_id"], id);
            assert!(n["parent_key"].is_null());
            assert!(n["children"].is_null());
            let a = n["start_byte"].as_u64().unwrap() as usize;
            let b = n["end_byte"].as_u64().unwrap() as usize;
            assert_eq!(
                bytes(&p.data("source", n["source"].clone())["data"]),
                raw[a..b]
            );
        }
        let companions = p.data("tree", json!({"level":"companions","parent_id":id}));
        assert_eq!(companions["data"]["payload_presence"], "present_unparsed");
        for n in companions["data"]["items"].as_array().unwrap() {
            let path = n["path"].as_str().unwrap();
            if n["role"] == "descriptor" {
                let child = n["object_id"].as_i64().unwrap();
                let b = p.data("tree", n["children"].clone());
                let childraw =
                    bytes(&p.data("source", b["data"]["branch"]["source"].clone())["data"]);
                if path.contains("/Recalculations/") {
                    recalc = child;
                    assert_eq!(
                        childraw,
                        format!(
                            "\u{feff}{}",
                            register_descriptor(
                                "Recalculation",
                                "Rebuild",
                                1600,
                                &recalculation_body()
                            )
                        )
                        .as_bytes()
                    );
                    assert!(b["data"]["items"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|n| n["type"] != "module"));
                    let r = p.data("tree", json!({"level":"inline","parent_id":child}));
                    assert_eq!(r["data"]["items"].as_array().unwrap().len(), 2);
                    for n in r["data"]["items"].as_array().unwrap() {
                        assert_eq!(n["owner_id"], child);
                        assert!(n["parent_key"].is_null());
                        assert_eq!(n["path"], path);
                        let a = n["start_byte"].as_u64().unwrap() as usize;
                        let z = n["end_byte"].as_u64().unwrap() as usize;
                        assert_eq!(
                            bytes(&p.data("source", n["source"].clone())["data"]),
                            childraw[a..z]
                        );
                    }
                    let c = p.data("tree", json!({"level":"companions","parent_id":child}));
                    assert_eq!(c["data"]["items"].as_array().unwrap().len(), 1);
                    assert_eq!(
                        bytes(&p.data("source", c["data"]["items"][0]["source"].clone())["data"]),
                        b"<Recalculation>Rebuild</Recalculation>"
                    );
                } else {
                    let m = b["data"]["items"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|n| n["type"] == "module")
                        .unwrap();
                    let mb = p.data("tree", m["children"].clone());
                    assert_eq!(mb["data"]["branch"]["status"], "accepted");
                    owners.push((m["key"].as_str().unwrap().to_string(), child));
                    assert_eq!(
                        bytes(
                            &p.data(
                                "source",
                                json!({"path":m["key"],"start_line":1,"end_line":5})
                            )["data"]
                        ),
                        TEXT.as_bytes()
                    );
                }
            } else if n["role"] == "additional" {
                assert_eq!(n["object_id"], id);
                let expected: &[u8] = if path.ends_with("Aggregates.xml") {
                    b"<OpaqueAggregate/>"
                } else if path.ends_with("Help.xml") {
                    b"<Help opaque='true'/>"
                } else {
                    b"<html>help</html>"
                };
                assert_eq!(
                    bytes(&p.data("source", n["source"].clone())["data"]),
                    expected
                );
            }
        }
        let _ = folder;
    }
    assert!(calc > 0 && recalc > 0);
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    for (path, owner) in owners {
        assert_eq!(
            db.query_row("SELECT owner_id FROM modules WHERE path=?1", [path], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            owner
        );
    }
    assert_eq!(
        db.query_row(
            "SELECT parent_id FROM objects WHERE object_id=?1",
            [recalc],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        calc
    );
    // Structurally valid wrong parent with recomputed hashes: open is not offline verify.
    db.execute(
        "UPDATE objects SET parent_id=1 WHERE object_id=?1",
        [recalc],
    )
    .unwrap();
    let hash = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [hash])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "134217728",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "300",
            "--max-source-bytes",
            "1048576",
            "--max-total-bytes",
            "4194304",
            "--max-scratch-bytes",
            "134217728",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("VerifyFacts"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

const REPORT_ROOTS: &[(&str, &str, &str)] = &[
    ("DataProcessor", "DataProcessors", "Process"),
    ("DataProcessor", "DataProcessors", "Empty"),
    ("Report", "Reports", "Overview"),
    ("Report", "Reports", "Both"),
];
const REPORT_TEMPLATES: &[(&str, &str, &[u8])] = &[
    ("Xml", "xml", b"<DCS opaque='true'/>"),
    ("Binary", "bin", BINARY),
    ("Text", "txt", b"text\r\nlast"),
];
const REPORT_ASSETS: &[(&str, &[u8])] = &[
    (
        "DataProcessors/Process/Forms/Card/Ext/Form/Items/Item/Archive.zip",
        b"PK\x00\xffopaque-not-a-valid-zip",
    ),
    (
        "DataProcessors/Process/Forms/Card/Ext/Form/Items/Item/Image.png",
        b"\x89PNG\x00\xff",
    ),
    (
        "DataProcessors/Process/Forms/Card/Ext/Form/Items/Item/Image.gif",
        b"GIF89a\x00",
    ),
    (
        "DataProcessors/Process/Forms/Card/Ext/Form/Items/Item/Image.svg",
        b"<svg opaque='true'/>",
    ),
    (
        "DataProcessors/Process/Forms/Card/Ext/Help/Assets/Picture.png",
        b"\x89PNGhelp\x00\xff",
    ),
    (
        "DataProcessors/Process/Templates/Xml/Ext/Template/ru.html",
        b"<html>template</html>",
    ),
];
fn report_modules(name: &str) -> &'static [&'static str] {
    match name {
        "Process" => &["ManagerModule"],
        "Overview" => &["ObjectModule"],
        "Both" => &["ManagerModule", "ObjectModule"],
        _ => &[],
    }
}
fn report_body(i: usize) -> String {
    if i == 1 {
        return String::new();
    }
    let n = 6000 + i as u32 * 10;
    format!("{}{}{}<Form>Card</Form><Form>NoCode</Form><Template>Xml</Template><Template>Binary</Template><Template>Text</Template>",chart_node("Attribute","Attr",n+1,""),chart_node("Command","Act",n+2,""),chart_node("TabularSection","Rows",n+3,&format!("<InternalInfo><Opaque/></InternalInfo><ChildObjects>{}</ChildObjects>",chart_node("Attribute","Cell",n+4,""))))
}

#[test]
fn reports_cli_offline_stdio_assets_commands_and_inherited_recalculation() {
    let mut f = Fixture::with_mode(&[], 14);
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let meta = p.data("metadata", json!({}));
    assert_eq!(
        meta["snapshot"]["profile"],
        bslens_core::snapshot_v5::report_writer::PROFILE
    );
    assert_eq!(
        meta["snapshot"]["coverage"]["report_semantics"],
        "not_analyzed"
    );
    let root = p.data("tree", json!({}));
    let mut owners = Vec::new();
    let mut command_links = Vec::new();
    let mut payload_owners = Vec::new();
    let mut process = 0;
    for (i, (kind, folder, name)) in REPORT_ROOTS.iter().enumerate() {
        let group = root["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == *kind)
            .unwrap();
        let mut a = group["children"].clone();
        a["name_exact"] = json!(name);
        let found = p.data("tree", a);
        assert_eq!(found["data"]["branch"]["match_status"], "matched");
        let object = &found["data"]["items"][0];
        let id = object["key"].as_str().unwrap().parse::<i64>().unwrap();
        if *name == "Process" {
            process = id;
        }
        let b = p.data("tree", object["children"].clone());
        let raw = bytes(&p.data("source", b["data"]["branch"]["source"].clone())["data"]);
        assert_eq!(
            raw,
            format!(
                "\u{feff}{}",
                register_descriptor(kind, name, 6100 + i as u32, &report_body(i))
            )
            .as_bytes()
        );
        let modules: Vec<_> = b["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["type"] == "module")
            .collect();
        assert_eq!(
            modules.len(),
            report_modules(name).len() + if *name == "Empty" { 0 } else { 1 }
        );
        for m in modules {
            let path = m["key"].as_str().unwrap();
            owners.push((path.to_string(), id));
            let refused = *name == "Overview" && path.contains("/Commands/");
            let mut a = m["children"].clone();
            a["name_exact"] = json!("япервый");
            let methods = p.data("tree", a);
            assert_eq!(
                methods["data"]["branch"]["match_status"],
                if refused { "unparsed" } else { "matched" }
            );
            if refused {
                assert!(methods["data"]["items"].as_array().unwrap().is_empty());
            } else {
                assert_eq!(
                    bytes(&p.data("source", methods["data"]["items"][0]["source"].clone())["data"]),
                    "Процедура ЯПервый()\r\nКонецПроцедуры\r\n".as_bytes()
                );
            }
            assert_eq!(
                bytes(
                    &p.data(
                        "source",
                        json!({"path":path,"start_line":1,"end_line":if refused{2}else{5}})
                    )["data"]
                ),
                if refused { BINARY } else { TEXT.as_bytes() }
            );
        }
        let mut stack = vec![(json!({"level":"inline","parent_id":id}), None::<String>)];
        let mut count = 0;
        while let Some((a, parent)) = stack.pop() {
            let list = p.data("tree", a);
            for n in list["data"]["items"].as_array().unwrap() {
                count += 1;
                assert_eq!(n["owner_id"], id);
                assert_eq!(n["parent_key"], json!(parent));
                assert_eq!(n["path"], format!("{folder}/{name}.xml"));
                let exact = bytes(&p.data("source", n["source"].clone())["data"]);
                let start = n["start_byte"].as_u64().unwrap() as usize;
                let end = n["end_byte"].as_u64().unwrap() as usize;
                assert_eq!(exact, raw[start..end]);
                if n["kind"] == "TabularSection" {
                    stack.push((
                        n["children"].clone(),
                        Some(n["key"].as_str().unwrap().into()),
                    ));
                } else if n["kind"] == "Command" {
                    let module = format!("{folder}/{name}/Commands/Act/Ext/CommandModule.bsl");
                    assert_eq!(n["module_path"], module);
                    let m = p.data("tree", n["children"].clone());
                    assert_eq!(m["data"]["branch"]["owner_declaration"], n["key"]);
                    assert_eq!(m["data"]["branch"]["container_object_id"], id);
                    command_links.push((module, n["key"].as_str().unwrap().to_string(), id));
                } else {
                    assert!(n["children"].is_null());
                }
            }
        }
        assert_eq!(count, if *name == "Empty" { 0 } else { 4 });
        let c = p.data("tree", json!({"level":"companions","parent_id":id}));
        let list = c["data"]["items"].as_array().unwrap();
        assert_eq!(
            list.iter().filter(|n| n["role"] == "descriptor").count(),
            if *name == "Empty" { 0 } else { 5 }
        );
        for n in list {
            let path = n["path"].as_str().unwrap();
            if n["role"] == "descriptor" {
                let child = n["object_id"].as_i64().unwrap();
                let branch = p.data("tree", n["children"].clone());
                let mods: Vec<_> = branch["data"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|n| n["type"] == "module")
                    .collect();
                assert_eq!(mods.len(), usize::from(path.ends_with("/Forms/Card.xml")));
                for m in mods {
                    owners.push((m["key"].as_str().unwrap().into(), child));
                    assert_eq!(
                        bytes(
                            &p.data(
                                "source",
                                json!({"path":m["key"],"start_line":1,"end_line":5})
                            )["data"]
                        ),
                        TEXT.as_bytes()
                    );
                }
                let comp = p.data("tree", json!({"level":"companions","parent_id":child}));
                for asset in comp["data"]["items"].as_array().unwrap() {
                    let path = asset["path"].as_str().unwrap();
                    if asset["role"] == "reference" {
                        continue;
                    }
                    assert_eq!(asset["object_id"], child);
                    payload_owners.push((path.to_string(), child));
                    let exact = bytes(&p.data("source", asset["source"].clone())["data"]);
                    let expected =
                        if let Some((_, v)) = REPORT_ASSETS.iter().find(|(p, _)| *p == path) {
                            *v
                        } else if path.ends_with("/Form.xml") {
                            b"<Form opaque='true'/>"
                        } else if path.ends_with("/Help.xml") {
                            b"<Help/>"
                        } else if path.ends_with("/Help/ru.html") {
                            b"<html>form</html>"
                        } else {
                            REPORT_TEMPLATES
                                .iter()
                                .find(|(_, ext, _)| path.ends_with(&format!("Template.{ext}")))
                                .unwrap()
                                .2
                        };
                    assert_eq!(exact, expected);
                }
            }
        }
    }
    // Mixed snapshot regression: new profile must retain Recalculation navigation.
    let group = root["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "CalculationRegister")
        .unwrap();
    let objects = p.data("tree", group["children"].clone());
    let calc = objects["data"]["items"][0]["key"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let c = p.data("tree", json!({"level":"companions","parent_id":calc}));
    let rec = c["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["path"] == "CalculationRegisters/Calc/Recalculations/Rebuild.xml")
        .unwrap();
    let rid = rec["object_id"].as_i64().unwrap();
    let b = p.data("tree", rec["children"].clone());
    let raw = bytes(&p.data("source", b["data"]["branch"]["source"].clone())["data"]);
    assert_eq!(
        raw,
        format!(
            "\u{feff}{}",
            register_descriptor("Recalculation", "Rebuild", 1600, &recalculation_body())
        )
        .as_bytes()
    );
    let inline = p.data("tree", json!({"level":"inline","parent_id":rid}));
    assert_eq!(inline["data"]["items"].as_array().unwrap().len(), 2);
    for n in inline["data"]["items"].as_array().unwrap() {
        assert_eq!(n["owner_id"], rid);
        let number = if n["name"] == "First" { 1801 } else { 1802 };
        assert_eq!(
            bytes(&p.data("source", n["source"].clone())["data"]),
            chart_node("Dimension", n["name"].as_str().unwrap(), number, "").as_bytes()
        );
    }
    let c = p.data("tree", json!({"level":"companions","parent_id":rid}));
    assert_eq!(
        bytes(&p.data("source", c["data"]["items"][0]["source"].clone())["data"]),
        b"<Recalculation>Rebuild</Recalculation>"
    );
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT parent_id FROM objects WHERE object_id=?1",
            [rid],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        calc
    );
    for (path, id) in owners {
        assert_eq!(
            db.query_row("SELECT owner_id FROM modules WHERE path=?1", [path], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            id
        );
    }
    for (path, key, id) in command_links {
        assert_eq!(
            db.query_row(
                "SELECT declaration_key,owner_id FROM inline_module_owners WHERE module_path=?1",
                [path],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            )
            .unwrap(),
            (key, id)
        );
    }
    for (path, id) in payload_owners {
        assert_eq!(
            db.query_row(
                "SELECT object_id FROM companion_payloads WHERE path=?1",
                [path],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            id
        );
    }
    db.execute("UPDATE companion_payloads SET object_id=?1 WHERE path='DataProcessors/Process/Forms/Card/Ext/Form/Items/Item/Archive.zip'",[process]).unwrap();
    let hash = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [hash])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "134217728",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "400",
            "--max-source-bytes",
            "1048576",
            "--max-total-bytes",
            "4194304",
            "--max-scratch-bytes",
            "134217728",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("VerifyFacts"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

const WORKFLOW_ROOTS: &[(&str, &str, &str)] = &[
    ("ExchangePlan", "ExchangePlans", "Exchange"),
    ("ExchangePlan", "ExchangePlans", "Empty"),
    ("BusinessProcess", "BusinessProcesses", "Process"),
    ("Task", "Tasks", "Work"),
];
fn workflow_body(i: usize) -> String {
    if i == 1 {
        return String::new();
    }
    let n = 40000 + i as u32 * 20;
    let mut s = chart_node("Attribute", "Attr", n + 1, "");
    if i == 3 {
        for j in 0..4 {
            s.push_str(&chart_node(
                "AddressingAttribute",
                &format!("Address{j}"),
                n + 2 + j,
                "",
            ));
            s.push_str(&chart_node("Command", &format!("Act{j}"), n + 6 + j, ""));
        }
    } else {
        s.push_str(&chart_node(
            "TabularSection",
            "Rows",
            n + 2,
            &format!(
                "<InternalInfo><Opaque/></InternalInfo><ChildObjects>{}</ChildObjects>",
                chart_node("Attribute", "Cell", n + 3, "")
            ),
        ));
    }
    s.push_str("<Form>Card</Form>");
    if i == 0 {
        s.push_str("<Form>NoCode</Form><Template>Text</Template>");
    }
    s
}
#[test]
fn workflows_cli_offline_stdio_commands_payload_and_recalculation() {
    let mut f = Fixture::with_mode(&[], 15);
    assert!(!f.repo.exists());
    let mut p = Peer::start(&f);
    let meta = p.data("metadata", json!({}));
    assert_eq!(
        meta["snapshot"]["profile"],
        bslens_core::snapshot_v5::workflow_writer::PROFILE
    );
    assert_eq!(
        meta["snapshot"]["coverage"]["workflow_semantics"],
        "not_analyzed"
    );
    let root = p.data("tree", json!({}));
    let mut owners = Vec::new();
    let mut links = Vec::new();
    let mut exchange = 0;
    let mut task = 0;
    for (i, (kind, folder, name)) in WORKFLOW_ROOTS.iter().enumerate() {
        let g = root["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == *kind)
            .unwrap();
        let mut a = g["children"].clone();
        a["name_exact"] = json!(name);
        let found = p.data("tree", a);
        assert_eq!(found["data"]["branch"]["match_status"], "matched");
        let object = &found["data"]["items"][0];
        let id = object["key"].as_str().unwrap().parse::<i64>().unwrap();
        if i == 0 {
            exchange = id;
        }
        if i == 3 {
            task = id;
        }
        let b = p.data("tree", object["children"].clone());
        let raw = bytes(&p.data("source", b["data"]["branch"]["source"].clone())["data"]);
        assert_eq!(
            raw,
            format!(
                "\u{feff}{}",
                register_descriptor(kind, name, 41000 + i as u32, &workflow_body(i))
            )
            .as_bytes()
        );
        let modules: Vec<_> = b["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["type"] == "module")
            .collect();
        assert_eq!(modules.len(), if i == 3 { 6 } else { 2 });
        for m in modules {
            let path = m["key"].as_str().unwrap();
            owners.push((path.to_string(), id));
            let refused = path.ends_with("/Commands/Act3/Ext/CommandModule.bsl");
            let mut a = m["children"].clone();
            a["name_exact"] = json!("япервый");
            let methods = p.data("tree", a);
            assert_eq!(
                methods["data"]["branch"]["match_status"],
                if refused { "unparsed" } else { "matched" }
            );
            if refused {
                assert!(methods["data"]["items"].as_array().unwrap().is_empty());
            } else {
                assert_eq!(
                    bytes(&p.data("source", methods["data"]["items"][0]["source"].clone())["data"]),
                    "Процедура ЯПервый()\r\nКонецПроцедуры\r\n".as_bytes()
                );
            }
            assert_eq!(
                bytes(
                    &p.data(
                        "source",
                        json!({"path":path,"start_line":1,"end_line":if refused{2}else{5}})
                    )["data"]
                ),
                if refused { BINARY } else { TEXT.as_bytes() }
            );
        }
        let mut stack = vec![(json!({"level":"inline","parent_id":id}), None::<String>)];
        let mut count = 0;
        while let Some((a, parent)) = stack.pop() {
            let list = p.data("tree", a);
            for n in list["data"]["items"].as_array().unwrap() {
                count += 1;
                assert_eq!(n["owner_id"], id);
                assert_eq!(n["parent_key"], json!(parent));
                assert_eq!(n["path"], format!("{folder}/{name}.xml"));
                let exact = bytes(&p.data("source", n["source"].clone())["data"]);
                let start = n["start_byte"].as_u64().unwrap() as usize;
                let end = n["end_byte"].as_u64().unwrap() as usize;
                assert_eq!(exact, raw[start..end]);
                if n["kind"] == "TabularSection" {
                    stack.push((
                        n["children"].clone(),
                        Some(n["key"].as_str().unwrap().into()),
                    ));
                } else if n["kind"] == "Command" {
                    let path = format!(
                        "{folder}/{name}/Commands/{}/Ext/CommandModule.bsl",
                        n["name"].as_str().unwrap()
                    );
                    assert_eq!(n["module_path"], path);
                    let branch = p.data("tree", n["children"].clone());
                    assert_eq!(branch["data"]["branch"]["owner_declaration"], n["key"]);
                    assert_eq!(branch["data"]["branch"]["container_object_id"], id);
                    links.push((path, n["key"].as_str().unwrap().to_string(), id));
                } else {
                    assert!(n["children"].is_null());
                }
            }
        }
        assert_eq!(
            count,
            match i {
                1 => 0,
                3 => 9,
                _ => 3,
            }
        );
        let c = p.data("tree", json!({"level":"companions","parent_id":id}));
        let list = c["data"]["items"].as_array().unwrap();
        for n in list {
            let path = n["path"].as_str().unwrap();
            if n["role"] == "descriptor" {
                let child = n["object_id"].as_i64().unwrap();
                let b = p.data("tree", n["children"].clone());
                let mods: Vec<_> = b["data"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|n| n["type"] == "module")
                    .collect();
                assert_eq!(mods.len(), usize::from(path.ends_with("/Forms/Card.xml")));
                for m in mods {
                    owners.push((m["key"].as_str().unwrap().into(), child));
                    assert_eq!(
                        bytes(
                            &p.data(
                                "source",
                                json!({"path":m["key"],"start_line":1,"end_line":5})
                            )["data"]
                        ),
                        TEXT.as_bytes()
                    );
                }
                let companions = p.data("tree", json!({"level":"companions","parent_id":child}));
                for asset in companions["data"]["items"].as_array().unwrap() {
                    let path = asset["path"].as_str().unwrap();
                    if asset["role"] == "reference" {
                        continue;
                    }
                    assert_eq!(asset["object_id"], child);
                    let expected: &[u8] = if path.ends_with("/Form.xml") {
                        b"<OpaqueForm/>"
                    } else if path.ends_with("/Template.txt") {
                        b"opaque\r\nlast"
                    } else if path.ends_with("/Help.xml") {
                        b"<Help/>"
                    } else {
                        b"<html>form</html>"
                    };
                    assert_eq!(
                        bytes(&p.data("source", asset["source"].clone())["data"]),
                        expected
                    );
                }
            } else if n["role"] == "additional" {
                assert_eq!(n["object_id"], id);
                let expected: &[u8] = if path.ends_with("/Content.xml") {
                    b"<OpaqueContent/>"
                } else if path.ends_with("/Flowchart.xml") {
                    b"<OpaqueFlowchart/>"
                } else if path.ends_with("/Help.xml") {
                    b"<Help/>"
                } else {
                    b"<html>root</html>"
                };
                assert_eq!(
                    bytes(&p.data("source", n["source"].clone())["data"]),
                    expected
                );
            }
        }
    }
    let g = root["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "CalculationRegister")
        .unwrap();
    let objects = p.data("tree", g["children"].clone());
    let calc = objects["data"]["items"][0]["key"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let c = p.data("tree", json!({"level":"companions","parent_id":calc}));
    let rec = c["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["path"] == "CalculationRegisters/Calc/Recalculations/Rebuild.xml")
        .unwrap();
    let rid = rec["object_id"].as_i64().unwrap();
    let b = p.data("tree", rec["children"].clone());
    assert_eq!(
        bytes(&p.data("source", b["data"]["branch"]["source"].clone())["data"]),
        format!(
            "\u{feff}{}",
            register_descriptor("Recalculation", "Rebuild", 1600, &recalculation_body())
        )
        .as_bytes()
    );
    let nodes = p.data("tree", json!({"level":"inline","parent_id":rid}));
    assert_eq!(nodes["data"]["items"].as_array().unwrap().len(), 2);
    for n in nodes["data"]["items"].as_array().unwrap() {
        assert_eq!(n["owner_id"], rid);
        assert_eq!(
            bytes(&p.data("source", n["source"].clone())["data"]),
            chart_node(
                "Dimension",
                n["name"].as_str().unwrap(),
                if n["name"] == "First" { 1801 } else { 1802 },
                ""
            )
            .as_bytes()
        );
    }
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT parent_id FROM objects WHERE object_id=?1",
            [rid],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        calc
    );
    for (path, id) in owners {
        assert_eq!(
            db.query_row("SELECT owner_id FROM modules WHERE path=?1", [path], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            id
        );
    }
    for (path, key, id) in links {
        assert_eq!(
            db.query_row(
                "SELECT declaration_key,owner_id FROM inline_module_owners WHERE module_path=?1",
                [path],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            )
            .unwrap(),
            (key, id)
        );
    }
    assert!(exchange > 0 && task > 0);
    db.execute("UPDATE companion_payloads SET object_id=?1 WHERE path='ExchangePlans/Exchange/Ext/Content.xml'",[task]).unwrap();
    let hash = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [hash])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "134217728",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "400",
            "--max-source-bytes",
            "1048576",
            "--max-total-bytes",
            "4194304",
            "--max-scratch-bytes",
            "134217728",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("VerifyFacts"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

const CORPUS_SUBSYSTEM_PAYLOADS: &[(&str, &[u8])] = &[
    (
        "Subsystems/Ext/Ext/CommandInterface.xml",
        b"<OpaqueCommandInterface root='true'/>",
    ),
    (
        "Subsystems/Ext/Subsystems/Alpha/Ext/CommandInterface.xml",
        b"<OpaqueCommandInterface child='true'/>",
    ),
    (
        "Subsystems/Ext/Subsystems/Alpha/Subsystems/Leaf/Ext/CommandInterface.xml",
        b"<OpaqueCommandInterface leaf='true'/>",
    ),
    ("Subsystems/Ext/Subsystems/Alpha/Ext/Help.xml", b"<Help/>"),
    (
        "Subsystems/Ext/Subsystems/Alpha/Ext/Help/ru.html",
        b"<html>subsystem</html>",
    ),
];
fn corpus_descriptors() -> Vec<(String, String, String, u32, String)> {
    vec![
        (
            "Sequences/Order.xml".into(),
            "Sequence".into(),
            "Order".into(),
            60000,
            format!(
                "{}{}",
                chart_node("Dimension", "First", 60001, ""),
                chart_node("Dimension", "Second", 60002, "")
            ),
        ),
        (
            "SettingsStorages/Ext.xml".into(),
            "SettingsStorage".into(),
            "Ext".into(),
            60003,
            (0..13).map(|i| format!("<Form>Form{i}</Form>")).collect(),
        ),
        (
            "Subsystems/Ext.xml".into(),
            "Subsystem".into(),
            "Ext".into(),
            60004,
            "<Subsystem>Alpha</Subsystem><Subsystem>Beta</Subsystem>".into(),
        ),
        (
            "Subsystems/Ext/Subsystems/Alpha.xml".into(),
            "Subsystem".into(),
            "Alpha".into(),
            60005,
            "<Subsystem>Leaf</Subsystem>".into(),
        ),
        (
            "Subsystems/Ext/Subsystems/Beta.xml".into(),
            "Subsystem".into(),
            "Beta".into(),
            60006,
            String::new(),
        ),
        (
            "Subsystems/Ext/Subsystems/Alpha/Subsystems/Leaf.xml".into(),
            "Subsystem".into(),
            "Leaf".into(),
            60007,
            String::new(),
        ),
    ]
}
#[test]
fn corpus_cli_offline_stdio_subsystem_filter_configuration_and_identity() {
    let mut f = Fixture::with_mode(&[], 16);
    assert!(!f.repo.exists());
    let boundary: Value =
        serde_json::from_slice(&fs::read(f.root.join("parameters-16384.json")).unwrap()).unwrap();
    let saved = std::mem::replace(
        &mut f.built,
        BuiltSnapshot {
            path: PathBuf::from(boundary["path"].as_str().unwrap()),
            file_sha256: boundary["file_sha256"].as_str().unwrap().into(),
            snapshot_id: boundary["snapshot_id"].as_str().unwrap().into(),
            facts_sha256: boundary["facts_sha256"].as_str().unwrap().into(),
            reused: false,
        },
    );
    let checked = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "134217728",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "600",
            "--max-source-bytes",
            "1048576",
            "--max-total-bytes",
            "4194304",
            "--max-scratch-bytes",
            "134217728",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let mut peer = Peer::start(&f);
    assert_eq!(
        peer.data("metadata", json!({}))["snapshot"]["profile"],
        bslens_core::snapshot_v5::corpus_writer::PROFILE
    );
    peer.finish();
    f.built = saved;
    let mut p = Peer::start(&f);
    let meta = p.data("metadata", json!({}));
    assert_eq!(
        meta["snapshot"]["profile"],
        bslens_core::snapshot_v5::corpus_writer::PROFILE
    );
    let root = p.data("tree", json!({}));
    let config = root["data"]["branch"]["parent_id"].as_i64().unwrap();
    let globals = p.data("tree", json!({"level":"companions","parent_id":config}));
    for (path, expected) in CORPUS_GLOBALS {
        let n = globals["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["path"] == *path)
            .unwrap();
        assert_eq!(n["object_id"], config);
        assert_eq!(
            bytes(&p.data("source", n["source"].clone())["data"]),
            *expected
        );
    }
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    for (path, kind) in [
        ("SettingsStorages/Ext.xml", "xml"),
        ("SettingsStorages/Ext/Forms/Form0.xml", "xml"),
        ("Subsystems/Ext.xml", "xml"),
        ("Subsystems/Ext/Subsystems/Alpha.xml", "xml"),
        ("Subsystems/Ext/Subsystems/Alpha/Subsystems/Leaf.xml", "xml"),
        ("SettingsStorages/Ext/Forms/Form0/Ext/Form.xml", "payload"),
        ("Subsystems/Ext/Ext/CommandInterface.xml", "payload"),
        ("Subsystems/Ext/Subsystems/Alpha/Ext/Help.xml", "payload"),
    ] {
        assert_eq!(
            db.query_row("SELECT kind FROM sources WHERE path=?1", [path], |r| r
                .get::<_, String>(
                0
            ))
            .unwrap(),
            kind,
            "{path}"
        );
    }
    let inventory: Vec<Value> =
        serde_json::from_slice(&fs::read(f.root.join("git-inventory.json")).unwrap()).unwrap();
    let manifest: String = db
        .query_row("SELECT input_manifest FROM metadata", [], |r| r.get(0))
        .unwrap();
    let manifest: Value = serde_json::from_str(&manifest).unwrap();
    let mut selected = manifest["selected"].as_array().unwrap().clone();
    selected.sort_by_key(|v| v["path"].as_str().unwrap().to_string());
    let mut expected = inventory
        .iter()
        .filter(|v| v["path"] != "outside.dat")
        .cloned()
        .collect::<Vec<_>>();
    expected.sort_by_key(|v| v["path"].as_str().unwrap().to_string());
    assert_eq!(selected, expected);
    assert_eq!(manifest["excluded"].as_array().unwrap().len(), 1);
    assert_eq!(
        manifest["excluded"][0]["input"],
        *inventory
            .iter()
            .find(|v| v["path"] == "outside.dat")
            .unwrap()
    );
    let mut q = db
        .prepare("SELECT path,mode,blob_oid FROM sources ORDER BY path COLLATE BINARY")
        .unwrap();
    let actual=q.query_map([],|r|Ok(json!({"path":r.get::<_,String>(0)?,"mode":r.get::<_,String>(1)?,"blob_oid":r.get::<_,String>(2)?}))).unwrap().collect::<Result<Vec<_>,_>>().unwrap();
    assert_eq!(actual, expected);
    drop(q);
    drop(db);
    let mut owners = Vec::new();
    for module in [
        "ManagedApplicationModule",
        "OrdinaryApplicationModule",
        "SessionModule",
        "ExternalConnectionModule",
    ] {
        let refused = module == "ExternalConnectionModule";
        assert_eq!(
            root["data"]["branch"]["configuration_modules"][module],
            if refused { "unparsed" } else { "accepted" }
        );
        let path = format!("Ext/{module}.bsl");
        let n = root["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["key"] == path)
            .unwrap();
        owners.push((path.clone(), config));
        let mut a = n["children"].clone();
        a["name_exact"] = json!("япервый");
        let methods = p.data("tree", a);
        assert_eq!(
            methods["data"]["branch"]["match_status"],
            if refused { "unparsed" } else { "matched" }
        );
        if !refused {
            assert_eq!(
                bytes(&p.data("source", methods["data"]["items"][0]["source"].clone())["data"]),
                "Процедура ЯПервый()\r\nКонецПроцедуры\r\n".as_bytes()
            );
        }
        assert_eq!(
            bytes(
                &p.data(
                    "source",
                    json!({"path":path,"start_line":1,"end_line":if refused{2}else{5}})
                )["data"]
            ),
            if refused { BINARY } else { TEXT.as_bytes() }
        );
    }
    let mut subsystem_parent = Vec::new();
    let mut seq = 0;
    for kind in ["Sequence", "SettingsStorage", "Subsystem"] {
        let group = root["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["name"] == kind)
            .unwrap();
        let objects = p.data("tree", group["children"].clone());
        let object = &objects["data"]["items"][0];
        let id = object["key"].as_str().unwrap().parse::<i64>().unwrap();
        if kind == "Sequence" {
            seq = id;
        }
        let mut queue = vec![(id, config)];
        while let Some((id, parent)) = queue.pop() {
            let b = p.data("tree", json!({"level":"object","parent_id":id}));
            let raw = bytes(&p.data("source", b["data"]["branch"]["source"].clone())["data"]);
            let path = b["data"]["branch"]["source"]["saved_path"]
                .as_str()
                .unwrap();
            let d = corpus_descriptors()
                .into_iter()
                .find(|(path0, _, _, _, _)| path0 == path)
                .unwrap();
            assert_eq!(
                raw,
                format!("\u{feff}{}", register_descriptor(&d.1, &d.2, d.3, &d.4)).as_bytes()
            );
            if kind == "Subsystem" {
                subsystem_parent.push((id, parent));
            }
            if kind == "Sequence" {
                let inline = p.data("tree", json!({"level":"inline","parent_id":id}));
                assert_eq!(inline["data"]["items"].as_array().unwrap().len(), 2);
                for n in inline["data"]["items"].as_array().unwrap() {
                    assert_eq!(n["owner_id"], id);
                    assert_eq!(
                        bytes(&p.data("source", n["source"].clone())["data"]),
                        chart_node(
                            "Dimension",
                            n["name"].as_str().unwrap(),
                            if n["name"] == "First" { 60001 } else { 60002 },
                            ""
                        )
                        .as_bytes()
                    );
                }
            }
            if kind == "SettingsStorage" {
                let module = b["data"]["items"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|n| n["type"] == "module")
                    .unwrap();
                owners.push((module["key"].as_str().unwrap().into(), id));
            }
            let c = p.data("tree", json!({"level":"companions","parent_id":id}));
            let list = c["data"]["items"].as_array().unwrap();
            if kind == "SettingsStorage" {
                assert_eq!(
                    list.iter().filter(|n| n["role"] == "descriptor").count(),
                    13
                );
            }
            for n in list {
                if n["role"] == "descriptor" {
                    let child = n["object_id"].as_i64().unwrap();
                    if kind == "Subsystem" {
                        queue.push((child, id));
                    } else {
                        let b = p.data("tree", n["children"].clone());
                        let module = b["data"]["items"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .find(|n| n["type"] == "module")
                            .unwrap();
                        owners.push((module["key"].as_str().unwrap().into(), child));
                        assert_eq!(
                            bytes(
                                &p.data(
                                    "source",
                                    json!({"path":module["key"],"start_line":1,"end_line":5})
                                )["data"]
                            ),
                            TEXT.as_bytes()
                        );
                    }
                } else if n["role"] == "additional" {
                    let path = n["path"].as_str().unwrap();
                    assert_eq!(n["object_id"], id);
                    assert_eq!(
                        bytes(&p.data("source", n["source"].clone())["data"]),
                        CORPUS_SUBSYSTEM_PAYLOADS
                            .iter()
                            .find(|(p, _)| *p == path)
                            .unwrap()
                            .1
                    );
                } else if kind == "Subsystem" && parent != config {
                    assert_eq!(
                        bytes(&p.data("source", n["source"].clone())["data"]),
                        format!("<Subsystem>{}</Subsystem>", d.2).as_bytes()
                    );
                }
            }
        }
    }
    // Recalculation remains visible in the integrated profile.
    let group = root["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "CalculationRegister")
        .unwrap();
    let objects = p.data("tree", group["children"].clone());
    let id = objects["data"]["items"][0]["key"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let c = p.data("tree", json!({"level":"companions","parent_id":id}));
    let rec = c["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["path"] == "CalculationRegisters/Calc/Recalculations/Rebuild.xml")
        .unwrap();
    let b = p.data("tree", rec["children"].clone());
    assert_eq!(
        bytes(&p.data("source", b["data"]["branch"]["source"].clone())["data"]),
        format!(
            "\u{feff}{}",
            register_descriptor("Recalculation", "Rebuild", 1600, &recalculation_body())
        )
        .as_bytes()
    );
    p.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let optional: Value =
        serde_json::from_slice(&fs::read(f.root.join("config-absent.json")).unwrap()).unwrap();
    let saved = std::mem::replace(
        &mut f.built,
        BuiltSnapshot {
            path: PathBuf::from(optional["path"].as_str().unwrap()),
            snapshot_id: optional["snapshot_id"].as_str().unwrap().into(),
            facts_sha256: optional["facts_sha256"].as_str().unwrap().into(),
            file_sha256: optional["file_sha256"].as_str().unwrap().into(),
            reused: false,
        },
    );
    let mut peer = Peer::start(&f);
    let root = peer.data("tree", json!({}));
    assert_eq!(
        root["data"]["branch"]["configuration_modules"]["OrdinaryApplicationModule"],
        "absent"
    );
    peer.finish();
    f.built = saved;
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    assert_eq!(
        db.query_row(
            "SELECT count(DISTINCT kind) FROM objects WHERE parent_id=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        42
    );
    for (path, id) in owners {
        assert_eq!(
            db.query_row("SELECT owner_id FROM modules WHERE path=?1", [path], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap(),
            id
        );
    }
    assert_eq!(subsystem_parent.len(), 4);
    for (id, parent) in subsystem_parent {
        assert_eq!(
            db.query_row(
                "SELECT parent_id FROM objects WHERE object_id=?1",
                [id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            parent
        );
    }
    db.execute("UPDATE companion_payloads SET object_id=?1 WHERE path='Subsystems/Ext/Subsystems/Alpha/Ext/CommandInterface.xml'",[seq]).unwrap();
    let hash = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [hash])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let out = Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        .env("PATH", "")
        .args([
            "verify",
            f.built.path.to_str().unwrap(),
            "--staging-dir",
            f.stage.to_str().unwrap(),
            "--max-sqlite-bytes",
            "134217728",
            "--expected-sha256",
            &f.built.file_sha256,
            "--max-sources",
            "600",
            "--max-source-bytes",
            "1048576",
            "--max-total-bytes",
            "4194304",
            "--max-scratch-bytes",
            "134217728",
            "--max-facts",
            "100000",
            "--max-coverage-bytes",
            "1048576",
            "--facts-time-ms",
            "30000",
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("VerifyFacts"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn corpus_bin_cli_large_owned_payload_caps_offline_verify_stdio() {
    let mut f = Fixture::new(&[]);
    fs::create_dir(&f.repo).unwrap();
    let git = |args: &[&str]| {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&f.repo)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let xml = |kind: &str, name: &str, id: u32, children: Option<&str>| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{kind} uuid=\"80000000-0000-0000-0000-{id:012}\"><Properties><Name>{name}</Name></Properties>{}</{kind}></MetaDataObject>",children.map_or(String::new(),|c|format!("<ChildObjects>{c}</ChildObjects>")))
    };
    for (path,body) in [
        ("Configuration.xml",xml("Configuration","Synthetic",1,Some("<Catalog>C</Catalog><CommonTemplate>Shared</CommonTemplate><XDTOPackage>Package</XDTOPackage>"))),
        ("Catalogs/C.xml",xml("Catalog","C",2,Some("<Template>Large</Template>"))),
        ("Catalogs/C/Templates/Large.xml",xml("Template","Large",3,None)),
        ("CommonTemplates/Shared.xml",xml("CommonTemplate","Shared",4,None)),
        ("XDTOPackages/Package.xml",xml("XDTOPackage","Package",5,None)),
        ("Ext/SessionModule.bsl",TEXT.into()),
    ] {let p=f.repo.join(path);fs::create_dir_all(p.parent().unwrap()).unwrap();fs::write(p,body).unwrap();}
    let payloads = [
        (
            "Catalogs/C/Templates/Large/Ext/Template.bin",
            21_561_550usize,
        ),
        ("CommonTemplates/Shared/Ext/Template.bin", 16_405_794),
        ("Ext/Data.bin", 9_538_006),
        ("XDTOPackages/Package/Ext/Package.bin", 31),
    ];
    let pattern = |a: usize, n: usize| (a..a + n).map(|i| (i % 251) as u8).collect::<Vec<_>>();
    for (path, n) in payloads {
        let p = f.repo.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        let mut file = fs::File::create(p).unwrap();
        let mut offset = 0;
        while offset < n {
            let count = (n - offset).min(65536);
            file.write_all(&pattern(offset, count)).unwrap();
            offset += count;
        }
    }
    let commit = || {
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@invalid",
            "commit",
            "-qm",
            "BIN fixture",
        ]);
    };
    commit();
    let args = |output: &str| {
        vec![
            "build-corpus-bin".into(),
            f.repo.to_str().unwrap().into(),
            git(&["rev-parse", "HEAD"]),
            git(&["rev-parse", "HEAD^{tree}"]),
            "synthetic:bin".into(),
            f.root.join(output).to_str().unwrap().into(),
            "100".into(),
            "8388608".into(),
            "67108864".into(),
            "134217728".into(),
            "1000".into(),
            "1000".into(),
            "1000".into(),
            "8388608".into(),
            "134217728".into(),
            "30000".into(),
            "10000".into(),
            "1048576".into(),
            "30000".into(),
            "[\"Ext/Data.bin\"]".into(),
            "25165824".into(),
        ]
    };
    let measured = |label: &str| {
        if let Some(wrapper) = std::env::var_os("BSLENS_TEST_RSS_WRAPPER") {
            let mut c = Command::new(wrapper);
            c.arg(label).arg(env!("CARGO_BIN_EXE_bslens-v5"));
            c
        } else {
            Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
        }
    };
    let run = |a: &[String], label: &str| {
        let out = measured(label).args(a).output().unwrap();
        if std::env::var_os("BSLENS_TEST_RSS_WRAPPER").is_some() {
            eprintln!("{}", String::from_utf8_lossy(&out.stderr));
        }
        out
    };
    let out = run(&args("bin-ok"), "build");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let built: Value = serde_json::from_slice(&out.stdout).unwrap();
    let again = run(&args("bin-ok"), "reuse");
    assert!(again.status.success());
    let again: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(again["snapshot_id"], built["snapshot_id"]);
    assert_eq!(again["reused"], true);
    let reject = |a: Vec<String>, label: &str, code: &str| {
        let out = run(&a, label);
        assert!(!out.status.success(), "{label}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains(code),
            "{label}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!std::path::Path::new(&a[5]).exists() || fs::read_dir(&a[5]).unwrap().count() == 0);
    };
    let mut old = args("bin-old");
    old[0] = "build-corpus".into();
    old.pop();
    reject(old, "old", "object_bytes");
    let mut commit_cap = args("bin-commit-cap");
    commit_cap[13] = "1".into();
    reject(commit_cap, "commit-cap", "object_bytes");
    let mut ordinary = args("bin-ordinary-oversize");
    ordinary[13] = "25165824".into();
    reject(ordinary, "ordinary-oversize", "ingest_limits");
    let mut ordinary = args("bin-source-oversize");
    ordinary[7] = "25165824".into();
    reject(ordinary, "source-oversize", "source_bytes");
    let mut policy = args("bin-policy-oversize");
    policy[20] = "25165825".into();
    reject(policy, "policy-oversize", "opaque_bin_bytes");
    let mut low = args("bin-low");
    low[20] = "8388608".into();
    reject(low, "low", "opaque_bin_bytes");
    let mut total = args("bin-total");
    total[8] = "33554432".into();
    reject(total, "source-total", "total_source_bytes");
    let mut total = args("bin-git-total");
    total[14] = "33554432".into();
    reject(total, "git-total", "total_bytes");
    let mut limit = args("bin-facts");
    limit[16] = "1".into();
    reject(limit, "late-facts", "facts");
    let mut limit = args("bin-sqlite");
    limit[9] = "8388608".into();
    reject(limit, "sqlite", "sqlite_bytes=8388608");
    let p = f.repo.join(payloads[0].0);
    fs::OpenOptions::new()
        .write(true)
        .open(&p)
        .unwrap()
        .set_len(25_165_824)
        .unwrap();
    commit();
    let exact = run(&args("bin-exact-cap"), "exact-cap");
    assert!(
        exact.status.success(),
        "{}",
        String::from_utf8_lossy(&exact.stderr)
    );
    let exact: Value = serde_json::from_slice(&exact.stdout).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&p)
        .unwrap()
        .set_len(25_165_825)
        .unwrap();
    commit();
    reject(args("bin-cap-plus-one"), "cap-plus-one", "opaque_bin_bytes");
    fs::OpenOptions::new()
        .write(true)
        .open(&p)
        .unwrap()
        .set_len(payloads[0].1 as u64)
        .unwrap();
    commit();
    for (name, code) in [
        ("Other.bin", "object_bytes"),
        ("Template.xml", "object_bytes"),
    ] {
        let wrong = p.with_file_name(name);
        fs::rename(&p, &wrong).unwrap();
        commit();
        reject(args(&format!("bin-{name}")), name, code);
        fs::rename(wrong, &p).unwrap();
        commit();
    }
    // Small unsupported path still fails structural scope, rather than being silently excluded.
    let wrong = p.with_file_name("Other.bin");
    fs::write(&wrong, BINARY).unwrap();
    commit();
    reject(
        args("bin-small-unknown"),
        "small-unknown",
        "opaque_bin_layout_unsupported",
    );
    fs::remove_file(wrong).unwrap();
    commit();
    let descriptor = f.repo.join("Catalogs/C/Templates/Large.xml");
    let saved = fs::read(&descriptor).unwrap();
    fs::write(&descriptor, xml("Form", "Large", 3, None)).unwrap();
    commit();
    reject(args("bin-wrong-type"), "wrong-type", "Xml");
    fs::write(descriptor, saved).unwrap();
    commit();
    let mut alternate = args("bin-alt-budget");
    alternate[20] = "23068672".into();
    let alt = run(&alternate, "alternate-budget");
    assert!(alt.status.success());
    let alt: Value = serde_json::from_slice(&alt.stdout).unwrap();
    assert_ne!(alt["snapshot_id"], built["snapshot_id"]);
    assert_eq!(alt["facts_sha256"], built["facts_sha256"]);
    fs::remove_dir_all(&f.repo).unwrap();
    f.built = BuiltSnapshot {
        path: PathBuf::from(built["path"].as_str().unwrap()),
        snapshot_id: built["snapshot_id"].as_str().unwrap().into(),
        facts_sha256: built["facts_sha256"].as_str().unwrap().into(),
        file_sha256: built["file_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.read_limit = "134217728";
    let verify = vec![
        "verify".into(),
        f.built.path.to_str().unwrap().into(),
        "--staging-dir".into(),
        f.stage.to_str().unwrap().into(),
        "--max-sqlite-bytes".into(),
        "134217728".into(),
        "--expected-sha256".into(),
        f.built.file_sha256.clone(),
        "--max-sources".into(),
        "100".into(),
        "--max-source-bytes".into(),
        "8388608".into(),
        "--max-total-bytes".into(),
        "67108864".into(),
        "--max-scratch-bytes".into(),
        "134217728".into(),
        "--max-facts".into(),
        "10000".into(),
        "--max-coverage-bytes".into(),
        "1048576".into(),
        "--facts-time-ms".into(),
        "30000".into(),
        "--max-opaque-bin-bytes".into(),
        "25165824".into(),
    ];
    let verify_run = |a: &[String]| {
        let out = measured("verify").env("PATH", "").args(a).output().unwrap();
        if std::env::var_os("BSLENS_TEST_RSS_WRAPPER").is_some() {
            eprintln!("{}", String::from_utf8_lossy(&out.stderr));
        }
        out
    };
    let mut exact_verify = verify.clone();
    exact_verify[1] = exact["path"].as_str().unwrap().into();
    exact_verify[7] = exact["file_sha256"].as_str().unwrap().into();
    let exact_out = verify_run(&exact_verify);
    assert!(
        exact_out.status.success(),
        "{}",
        String::from_utf8_lossy(&exact_out.stderr)
    );
    let out = verify_run(&verify);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut missing = verify.clone();
    missing.truncate(missing.len() - 2);
    let out = verify_run(&missing);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("opaque_bin_verify_limit_required"));
    let mut low = verify.clone();
    *low.last_mut().unwrap() = "8388608".into();
    assert!(!verify_run(&low).status.success());
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    let params: String = db
        .query_row("SELECT build_parameters FROM metadata", [], |r| r.get(0))
        .unwrap();
    let params: Value = serde_json::from_str(&params).unwrap();
    assert_eq!(params["opaque_bin"]["bytes"], 25165824);
    assert_eq!(params["git_limits"]["object_bytes"], 8388608);
    assert_eq!(params["limits"]["source_bytes"], 8388608);
    for (path, n) in payloads {
        assert_eq!(
            db.query_row(
                "SELECT length(content) FROM sources WHERE path=?1",
                [path],
                |r| r.get::<_, usize>(0)
            )
            .unwrap(),
            n
        );
    }
    drop(db);
    let mut peer = Peer::start(&f);
    let meta = peer.data("metadata", json!({}));
    assert_eq!(
        meta["snapshot"]["profile"],
        bslens_core::snapshot_v5::corpus_bin_writer::PROFILE
    );
    let root = peer.data("tree", json!({}));
    let cat = root["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "Catalog")
        .unwrap();
    let branch = peer.data("tree", cat["children"].clone());
    let owner = branch["data"]["items"][0]["key"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let saved = peer.data("tree", json!({"level":"companions","parent_id":owner}));
    let tpl = saved["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["role"] == "descriptor")
        .unwrap();
    let tpl_id = tpl["object_id"].as_i64().unwrap();
    let saved = peer.data("tree", json!({"level":"companions","parent_id":tpl_id}));
    let payload = saved["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["path"] == payloads[0].0)
        .unwrap();
    assert_eq!(payload["object_id"], tpl_id);
    for (path, n) in payloads {
        for start in [0, n - 31] {
            let got = peer.data(
                "source",
                json!({"saved_path":path,"start_byte":start,"end_byte":start+31}),
            );
            assert_eq!(got["data"]["kind"], "payload");
            assert_eq!(bytes(&got["data"]), pattern(start, 31));
        }
    }
    assert_eq!(
        bytes(
            &peer.data(
                "source",
                json!({"path":"Ext/SessionModule.bsl","start_line":1,"end_line":5})
            )["data"]
        ),
        TEXT.as_bytes()
    );
    let oversized = peer.call(
        "source",
        json!({"saved_path":payloads[0].0,"start_byte":0,"end_byte":50000}),
    );
    assert_eq!(oversized["error"]["code"], -32001);
    assert!(oversized.get("result").is_none());
    let oversized = peer.call(
        "source",
        json!({"saved_path":payloads[0].0,"start_byte":0,"end_byte":65537}),
    );
    assert_eq!(
        oversized["result"]["structuredContent"]["code"],
        "budget_exceeded"
    );
    peer.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}

#[test]
fn root_help_cli_offline_stdio_exact_owner_pair_and_forged_facts() {
    let mut f = Fixture::new(&[]);
    fs::create_dir(&f.repo).unwrap();
    let git = |args: &[&str]| {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&f.repo)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let xml = |kind: &str, name: &str, id: u32, children: &str| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{kind} uuid=\"90000000-0000-0000-0000-{id:012}\"><Properties><Name>{name}</Name></Properties><ChildObjects>{children}</ChildObjects></{kind}></MetaDataObject>")
    };
    let asset = "DataProcessors/Ext/Ext/Help/Images/icon.png";
    let payload = b"\x89PNG\r\n\x1a\n\x00\xffopaque\r\nlast";
    for (path, bytes) in [
        (
            "Configuration.xml",
            xml(
                "Configuration",
                "Synthetic",
                1,
                "<DataProcessor>Ext</DataProcessor><Report>Other</Report>",
            )
            .into_bytes(),
        ),
        (
            "DataProcessors/Ext.xml",
            xml("DataProcessor", "Ext", 2, "").into_bytes(),
        ),
        (
            "Reports/Other.xml",
            xml("Report", "Other", 3, "").into_bytes(),
        ),
        ("DataProcessors/Ext/Ext/Help.xml", b"<opaque/>".to_vec()),
        (
            "DataProcessors/Ext/Ext/Help/ru.html",
            b"<html>opaque</html>".to_vec(),
        ),
        ("Reports/Other/Ext/Help.xml", b"<opaque/>".to_vec()),
        (
            "Reports/Other/Ext/Help/ru.html",
            b"<html>opaque</html>".to_vec(),
        ),
        ("Ext/SessionModule.bsl", TEXT.as_bytes().to_vec()),
        (asset, payload.to_vec()),
    ] {
        let p = f.repo.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
    let commit = || {
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@invalid",
            "commit",
            "-qm",
            "Root help fixture",
        ]);
    };
    commit();
    let args = |output: &str| {
        vec![
            "build-corpus-help".into(),
            f.repo.to_str().unwrap().into(),
            git(&["rev-parse", "HEAD"]),
            git(&["rev-parse", "HEAD^{tree}"]),
            "synthetic:root-help".into(),
            f.root.join(output).to_str().unwrap().into(),
            "100".into(),
            "8388608".into(),
            "67108864".into(),
            "134217728".into(),
            "1000".into(),
            "1000".into(),
            "1000".into(),
            "8388608".into(),
            "134217728".into(),
            "30000".into(),
            "10000".into(),
            "1048576".into(),
            "30000".into(),
            "[]".into(),
            "25165824".into(),
        ]
    };
    let run = |a: &[String]| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .args(a)
            .output()
            .unwrap()
    };
    let out = run(&args("help-ok"));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let built: Value = serde_json::from_slice(&out.stdout).unwrap();
    let again = run(&args("help-ok"));
    assert!(again.status.success());
    let again: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(again["snapshot_id"], built["snapshot_id"]);
    assert_eq!(again["reused"], true);
    let reject = |a: Vec<String>, code: &str| {
        let o = run(&a);
        assert!(!o.status.success());
        assert!(
            String::from_utf8_lossy(&o.stderr).contains(code),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert!(!std::path::Path::new(&a[5]).exists() || fs::read_dir(&a[5]).unwrap().count() == 0);
    };
    let mut old = args("old-bin-refuses");
    old[0] = "build-corpus-bin".into();
    reject(old, "report_layout_unsupported");
    fs::OpenOptions::new()
        .write(true)
        .open(f.repo.join(asset))
        .unwrap()
        .set_len(8_388_609)
        .unwrap();
    commit();
    reject(args("png-no-bin-cap"), "object_bytes");
    fs::write(f.repo.join(asset), payload).unwrap();
    commit();
    for (n, missing) in [
        vec!["DataProcessors/Ext/Ext/Help.xml"],
        vec!["DataProcessors/Ext/Ext/Help/ru.html"],
        vec![
            "DataProcessors/Ext/Ext/Help.xml",
            "DataProcessors/Ext/Ext/Help/ru.html",
        ],
    ]
    .iter()
    .enumerate()
    {
        let saved: Vec<_> = missing
            .iter()
            .map(|p| (*p, fs::read(f.repo.join(p)).unwrap()))
            .collect();
        for p in missing {
            fs::remove_file(f.repo.join(p)).unwrap();
        }
        commit();
        reject(
            args(&format!("missing-{n}")),
            if n == 2 {
                "root_help_pair_required"
            } else {
                "report_help_incomplete"
            },
        );
        for (p, b) in saved {
            fs::write(f.repo.join(p), b).unwrap();
        }
        commit();
    }
    for (n, wrong) in [
        "Reports/Other/Ext/Help/Images/icon.png",
        "DataProcessors/Ext/Ext/Help/icon.png",
        "DataProcessors/Ext/Ext/Help/Images/Nested/icon.png",
        "DataProcessors/Ext/Ext/Help/Images/icon.PNG",
        "DataProcessors/Ext/Ext/Help/Images/icon.bin",
        "DataProcessors/Ext/Ext/Help/Images/.png",
    ]
    .iter()
    .enumerate()
    {
        let path = f.repo.join(wrong);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::rename(f.repo.join(asset), &path).unwrap();
        commit();
        reject(args(&format!("wrong-{n}")), "report_layout_unsupported");
        fs::rename(path, f.repo.join(asset)).unwrap();
        commit();
    }
    let mut low = args("png-ordinary-cap");
    low[7] = "1".into();
    reject(low, "source_bytes");
    let mut late = args("late-facts");
    late[16] = "1".into();
    reject(late, "facts");
    fs::remove_dir_all(&f.repo).unwrap();
    f.built = BuiltSnapshot {
        path: PathBuf::from(built["path"].as_str().unwrap()),
        snapshot_id: built["snapshot_id"].as_str().unwrap().into(),
        file_sha256: built["file_sha256"].as_str().unwrap().into(),
        facts_sha256: built["facts_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.read_limit = "134217728";
    let verify = |f: &Fixture| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .env("PATH", "")
            .args([
                "verify",
                f.built.path.to_str().unwrap(),
                "--staging-dir",
                f.stage.to_str().unwrap(),
                "--max-sqlite-bytes",
                "134217728",
                "--expected-sha256",
                &f.built.file_sha256,
                "--max-sources",
                "100",
                "--max-source-bytes",
                "8388608",
                "--max-total-bytes",
                "67108864",
                "--max-scratch-bytes",
                "134217728",
                "--max-facts",
                "10000",
                "--max-coverage-bytes",
                "1048576",
                "--facts-time-ms",
                "30000",
                "--max-opaque-bin-bytes",
                "25165824",
            ])
            .output()
            .unwrap()
    };
    let out = verify(&f);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let mut peer = Peer::start(&f);
    assert_eq!(
        peer.data("metadata", json!({}))["snapshot"]["profile"],
        bslens_core::snapshot_v5::corpus_help_writer::PROFILE
    );
    let root = peer.data("tree", json!({}));
    let group = root["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "DataProcessor")
        .unwrap();
    let mut exact = group["children"].clone();
    exact["name_exact"] = json!("Ext");
    let objects = peer.data("tree", exact);
    assert_eq!(objects["data"]["items"].as_array().unwrap().len(), 1);
    let owner = objects["data"]["items"][0]["key"]
        .as_str()
        .unwrap()
        .parse::<i64>()
        .unwrap();
    let saved = peer.data("tree", json!({"level":"companions","parent_id":owner}));
    let node = saved["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["path"] == asset)
        .unwrap();
    assert_eq!(node["object_id"], owner);
    assert_eq!(node["role"], "additional");
    for (a, z) in [(0, 8), (8, payload.len())] {
        let result = peer.data(
            "source",
            json!({"saved_path":asset,"start_byte":a,"end_byte":z}),
        );
        assert_eq!(result["data"]["kind"], "payload");
        assert_eq!(bytes(&result["data"]), payload[a..z]);
    }
    peer.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    let params: String = db
        .query_row("SELECT build_parameters FROM metadata", [], |r| r.get(0))
        .unwrap();
    let params: Value = serde_json::from_str(&params).unwrap();
    assert_eq!(
        params["root_help_guard"],
        "dataprocessor-root-help-png-pair-v1"
    );
    assert_eq!(params["opaque_bin"]["bytes"], 25165824);
    db.execute("UPDATE companion_payloads SET object_id=(SELECT object_id FROM objects WHERE xml_path='Reports/Other.xml') WHERE path=?1",[asset]).unwrap();
    let hash = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [hash])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let rejected = verify(&f);
    assert!(!rejected.status.success());
    assert!(
        String::from_utf8_lossy(&rejected.stderr).contains("VerifyFacts"),
        "{}",
        String::from_utf8_lossy(&rejected.stderr)
    );
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}

#[test]
fn configuration_dom_cli_3607_refs_offline_verify_stdio_and_limits() {
    let mut f = Fixture::new(&[]);
    fs::create_dir(&f.repo).unwrap();
    let git = |args: &[&str]| {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&f.repo)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let xml = |kind: &str, name: &str, id: usize, props: &str, children: &str| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{kind} uuid=\"a0000000-0000-0000-0000-{id:012}\"><Properties><Name>{name}</Name>{props}</Properties>{children}</{kind}></MetaDataObject>")
    };
    let refs = (0..3607)
        .map(|i| format!("<Language>N{i:04}</Language>\n"))
        .collect::<String>();
    let config = xml(
        "Configuration",
        "Synthetic",
        1,
        "",
        &format!("<ChildObjects>\n{refs}</ChildObjects>"),
    );
    let probe = |text: &str| {
        let dom = roxmltree::Document::parse(text)
            .unwrap()
            .descendants()
            .count();
        let mut r = quick_xml::Reader::from_str(text);
        let (mut n, mut d, mut depth) = (0, 0usize, 0usize);
        loop {
            n += 1;
            match r.read_event().unwrap() {
                quick_xml::events::Event::Start(_) => {
                    d += 1;
                    depth = depth.max(d);
                }
                quick_xml::events::Event::Empty(_) => depth = depth.max(d + 1),
                quick_xml::events::Event::End(_) => d -= 1,
                quick_xml::events::Event::Eof => break,
                _ => {}
            }
        }
        (dom, n, depth)
    };
    let (nodes, events, depth) = probe(&config);
    assert!(nodes > 10000 && nodes < 16384 && events < 20000 && depth < 64);
    eprintln!(
        "CONFIG_PROBE bytes={} nodes={nodes} events={events} depth={depth} refs=3607",
        config.len()
    );
    fs::write(f.repo.join("Configuration.xml"), &config).unwrap();
    fs::create_dir(f.repo.join("Languages")).unwrap();
    fs::create_dir(f.repo.join("Ext")).unwrap();
    fs::write(f.repo.join("Ext/SessionModule.bsl"), TEXT).unwrap();
    for i in 0..3607 {
        fs::write(
            f.repo.join(format!("Languages/N{i:04}.xml")),
            xml("Language", &format!("N{i:04}"), i + 2, "", ""),
        )
        .unwrap();
    }
    let commit = || {
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@invalid",
            "commit",
            "-qm",
            "Configuration DOM fixture",
        ]);
    };
    commit();
    let args = |output: &str| {
        vec![
            "build-corpus-config".into(),
            f.repo.to_str().unwrap().into(),
            git(&["rev-parse", "HEAD"]),
            git(&["rev-parse", "HEAD^{tree}"]),
            "synthetic:configuration".into(),
            f.root.join(output).to_str().unwrap().into(),
            "4000".into(),
            "8388608".into(),
            "16777216".into(),
            "134217728".into(),
            "10000".into(),
            "1000".into(),
            "10000".into(),
            "8388608".into(),
            "33554432".into(),
            "60000".into(),
            "100000".into(),
            "1048576".into(),
            "60000".into(),
            "[]".into(),
            "25165824".into(),
        ]
    };
    let run = |a: &[String]| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .args(a)
            .output()
            .unwrap()
    };
    let out = run(&args("config-ok"));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let built: Value = serde_json::from_slice(&out.stdout).unwrap();
    let again = run(&args("config-ok"));
    assert!(again.status.success());
    let again: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(again["snapshot_id"], built["snapshot_id"]);
    assert_eq!(again["reused"], true);
    let reject = |a: Vec<String>, path: &str| {
        let o = run(&a);
        assert!(!o.status.success());
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.contains("Limit") && err.contains(path), "{err}");
        assert_eq!(fs::read_dir(&a[5]).unwrap().count(), 0);
    };
    let mut old = args("old-dom");
    old[0] = "build-corpus-help".into();
    reject(old, "Configuration.xml");
    let with_nodes = |n: usize| {
        config.replace(
            "</Properties>",
            &format!("{}</Properties>", "<!--n-->".repeat(n - nodes)),
        )
    };
    let exact = with_nodes(16384);
    assert_eq!(probe(&exact).0, 16384);
    assert!(probe(&exact).1 < 20000);
    fs::write(f.repo.join("Configuration.xml"), &exact).unwrap();
    commit();
    let out = run(&args("config-exact"));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let boundary: Value = serde_json::from_slice(&out.stdout).unwrap();
    let over = with_nodes(16385);
    assert_eq!(probe(&over).0, 16385);
    assert!(probe(&over).1 < 20000);
    fs::write(f.repo.join("Configuration.xml"), over).unwrap();
    commit();
    reject(args("config-cap-plus-one"), "Configuration.xml");
    let event_over = xml(
        "Configuration",
        "Synthetic",
        1,
        &format!("<Opaque>{}</Opaque>", "<n></n>".repeat(10000)),
        "<ChildObjects/>",
    );
    assert!(probe(&event_over).0 < 16384 && probe(&event_over).1 > 20000);
    fs::write(f.repo.join("Configuration.xml"), event_over).unwrap();
    commit();
    reject(args("config-events"), "Configuration.xml");
    fs::write(f.repo.join("Configuration.xml"), &config).unwrap();
    let small = xml("Language", "N3606", 3608, "", "");
    let other = small.replace(
        "</Properties>",
        &format!(
            "<Opaque>{}</Opaque></Properties>",
            "<n/>".repeat(10001 - probe(&small).0 - 1)
        ),
    );
    assert_eq!(probe(&other).0, 10001);
    assert!(probe(&other).1 < 20000);
    fs::write(f.repo.join("Languages/N3606.xml"), other).unwrap();
    commit();
    reject(args("other-dom"), "Languages/N3606.xml");
    fs::write(f.repo.join("Languages/N3606.xml"), &small).unwrap();
    commit();
    fs::remove_dir_all(&f.repo).unwrap();
    let to_built = |v: &Value| BuiltSnapshot {
        path: PathBuf::from(v["path"].as_str().unwrap()),
        snapshot_id: v["snapshot_id"].as_str().unwrap().into(),
        file_sha256: v["file_sha256"].as_str().unwrap().into(),
        facts_sha256: v["facts_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.built = to_built(&built);
    f.read_limit = "134217728";
    let verify = |f: &Fixture| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .env("PATH", "")
            .args([
                "verify",
                f.built.path.to_str().unwrap(),
                "--staging-dir",
                f.stage.to_str().unwrap(),
                "--max-sqlite-bytes",
                "134217728",
                "--expected-sha256",
                &f.built.file_sha256,
                "--max-sources",
                "4000",
                "--max-source-bytes",
                "8388608",
                "--max-total-bytes",
                "16777216",
                "--max-scratch-bytes",
                "134217728",
                "--max-facts",
                "100000",
                "--max-coverage-bytes",
                "1048576",
                "--facts-time-ms",
                "60000",
                "--max-opaque-bin-bytes",
                "25165824",
            ])
            .output()
            .unwrap()
    };
    let out = verify(&f);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    f.built = to_built(&boundary);
    let out = verify(&f);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    f.built = to_built(&built);
    let mut peer = Peer::start(&f);
    assert_eq!(
        peer.data("metadata", json!({}))["snapshot"]["profile"],
        bslens_core::snapshot_v5::corpus_config_writer::PROFILE
    );
    let root = peer.data("tree", json!({}));
    let group = root["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "Language")
        .unwrap();
    assert_eq!(
        peer.call("tree", group["children"].clone())["result"]["structuredContent"]["code"],
        "budget_exceeded"
    );
    let mut query = group["children"].clone();
    query["name_exact"] = json!("N3606");
    let selected = peer.data("tree", query);
    assert_eq!(selected["data"]["items"].as_array().unwrap().len(), 1);
    let object = peer.data("tree", selected["data"]["items"][0]["children"].clone());
    assert_eq!(
        bytes(&peer.data("source", object["data"]["branch"]["source"].clone())["data"]),
        small.as_bytes()
    );
    assert_eq!(
        bytes(
            &peer.data(
                "source",
                json!({"path":"Ext/SessionModule.bsl","start_line":1,"end_line":5})
            )["data"]
        ),
        TEXT.as_bytes()
    );
    peer.finish();
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    let p: String = db
        .query_row("SELECT build_parameters FROM metadata", [], |r| r.get(0))
        .unwrap();
    let p: Value = serde_json::from_str(&p).unwrap();
    assert_eq!(p["configuration_xml"]["nodes"], 16384);
    assert_eq!(p["xml_limits"]["nodes"], 10000);
    assert_eq!(p["xml_limits"]["events"], 20000);
    assert_eq!(p["xml_limits"]["depth"], 64);
    assert_eq!(
        db.query_row(
            "SELECT count(*) FROM objects WHERE kind='Language' AND parent_id=1",
            [],
            |r| r.get::<_, usize>(0)
        )
        .unwrap(),
        3607
    );
}

#[test]
fn diagnostics_cli_dense_offline_stdio_limits_and_forged_evidence() {
    let mut f = Fixture::new(&[]);
    fs::create_dir(&f.repo).unwrap();
    let git = |a: &[&str]| {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&f.repo)
            .args(a)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let xml = |kind: &str, name: &str, id: usize, props: &str, children: &str| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{kind} uuid=\"b0000000-0000-0000-0000-{id:012}\"><Properties><Name>{name}</Name>{props}</Properties>{children}</{kind}></MetaDataObject>")
    };
    let refs = (0..600)
        .map(|i| format!("<Language>N{i:04}</Language>"))
        .collect::<String>();
    let config = xml(
        "Configuration",
        "Synthetic",
        1,
        "",
        &format!("<ChildObjects>{refs}</ChildObjects>"),
    );
    fs::write(f.repo.join("Configuration.xml"), &config).unwrap();
    fs::create_dir(f.repo.join("Languages")).unwrap();
    fs::create_dir(f.repo.join("Ext")).unwrap();
    fs::write(f.repo.join(ROOT), TEXT).unwrap();
    let props = (0..120)
        .map(|i| format!("<UnverifiedProperty{i:04}/>"))
        .collect::<String>();
    let last = xml("Language", "N0599", 601, &props, "");
    for i in 0..600 {
        fs::write(
            f.repo.join(format!("Languages/N{i:04}.xml")),
            xml("Language", &format!("N{i:04}"), i + 2, &props, ""),
        )
        .unwrap();
    }
    let commit = || {
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@invalid",
            "commit",
            "-qm",
            "dense diagnostics",
        ]);
    };
    commit();
    let args = |out: &str| {
        vec![
            "build-diagnostics".into(),
            f.repo.to_str().unwrap().into(),
            git(&["rev-parse", "HEAD"]),
            git(&["rev-parse", "HEAD^{tree}"]),
            "synthetic:diagnostics".into(),
            f.root.join(out).to_str().unwrap().into(),
            "4000".into(),
            "8388608".into(),
            "33554432".into(),
            "134217728".into(),
            "10000".into(),
            "1000".into(),
            "10000".into(),
            "8388608".into(),
            "67108864".into(),
            "60000".into(),
            "100000".into(),
            "1048576".into(),
            "60000".into(),
            "[]".into(),
            "25165824".into(),
            "67108864".into(),
        ]
    };
    let run = |a: &[String]| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .args(a)
            .output()
            .unwrap()
    };
    let out = run(&args("diag-ok"));
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let built: Value = serde_json::from_slice(&out.stdout).unwrap();
    let again = run(&args("diag-ok"));
    assert!(again.status.success());
    let again: Value = serde_json::from_slice(&again.stdout).unwrap();
    assert_eq!(again["snapshot_id"], built["snapshot_id"]);
    assert_eq!(again["reused"], true);
    // Independent streaming canonical evidence calculation, including multiplicity.
    let summary = |db: &rusqlite::Connection| {
        let mut h = Sha256::new();
        h.update(b"BSLens-v5/xml-diagnostics/json1\0");
        let (mut count, mut n) = (0u64, 0u64);
        let mut q=db.prepare("SELECT path,selector FROM xml_diagnostics ORDER BY path COLLATE BINARY,selector COLLATE BINARY").unwrap();
        let mut rows = q.query([]).unwrap();
        while let Some(r) = rows.next().unwrap() {
            let p: String = r.get(0).unwrap();
            let s: String = r.get(1).unwrap();
            let row = json!({"path":p,"selector":s}).to_string();
            count += 1;
            n += row.len() as u64;
            h.update((row.len() as u64).to_be_bytes());
            h.update(row.as_bytes());
        }
        json!({"format":"xml-diagnostics-json-v1","domain":"BSLens-v5/xml-diagnostics/json1","count":count,"bytes":n,"sha256":format!("{:x}",h.finalize()),"details":"xml_diagnostics","semantics":"not_analyzed"})
    };
    let db = rusqlite::Connection::open(built["path"].as_str().unwrap()).unwrap();
    let expected = summary(&db);
    assert_eq!(expected["count"], 72000);
    let total = expected["bytes"].as_u64().unwrap();
    assert!(total >= 8 * 1024 * 1024);
    eprintln!("DIAGNOSTICS_PROBE count=72000 exact_bytes={total}");
    let raw: String = db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    assert!(raw.len() < 8192);
    let coverage: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(coverage["xml_diagnostics"], expected);
    assert!(coverage.get("xml_unverified").is_none());
    drop(db);
    let reject = |a: Vec<String>, error: &str| {
        let o = run(&a);
        assert!(!o.status.success());
        assert!(
            String::from_utf8_lossy(&o.stderr).contains(error),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert_eq!(fs::read_dir(&a[5]).unwrap().count(), 0);
    };
    let mut a = args("diag-over");
    a[21] = (total - 1).to_string();
    reject(a, "diagnostics_bytes");
    let mut a = args("diag-facts");
    a[16] = "72000".into();
    reject(a, "facts");
    let mut a = args("diag-sqlite");
    a[9] = "4194304".into();
    reject(a, "лимит страниц SQLite");
    let mut a = args("diag-old");
    a[0] = "build-corpus-config".into();
    a.pop();
    reject(a, "coverage_bytes");
    let mut a = args("diag-exact");
    a[21] = total.to_string();
    let boundary = run(&a);
    assert!(
        boundary.status.success(),
        "{}",
        String::from_utf8_lossy(&boundary.stderr)
    );
    let huge = format!("<{} />", "X".repeat(16384));
    fs::write(
        f.repo.join("Languages/N0599.xml"),
        xml("Language", "N0599", 601, &huge, ""),
    )
    .unwrap();
    commit();
    reject(args("diag-row"), "diagnostic_row_bytes");
    fs::write(f.repo.join("Languages/N0599.xml"), &last).unwrap();
    // Shared admissible fixture: old array equals new persisted evidence, no dropped fields.
    for i in 0..599 {
        fs::remove_file(f.repo.join(format!("Languages/N{i:04}.xml"))).unwrap();
    }
    fs::write(
        f.repo.join("Configuration.xml"),
        xml(
            "Configuration",
            "Synthetic",
            1,
            "",
            "<ChildObjects><Language>N0599</Language></ChildObjects>",
        ),
    )
    .unwrap();
    commit();
    let new_small = run(&args("diag-small"));
    assert!(
        new_small.status.success(),
        "{}",
        String::from_utf8_lossy(&new_small.stderr)
    );
    let mut old_args = args("diag-small-old");
    old_args[0] = "build-corpus-config".into();
    old_args.pop();
    let old_small = run(&old_args);
    assert!(
        old_small.status.success(),
        "{}",
        String::from_utf8_lossy(&old_small.stderr)
    );
    let new_v: Value = serde_json::from_slice(&new_small.stdout).unwrap();
    let old_v: Value = serde_json::from_slice(&old_small.stdout).unwrap();
    let new_db = rusqlite::Connection::open(new_v["path"].as_str().unwrap()).unwrap();
    let old_db = rusqlite::Connection::open(old_v["path"].as_str().unwrap()).unwrap();
    let old_cov: String = old_db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    let mut old_cov: Value = serde_json::from_str(&old_cov).unwrap();
    let new_cov: String = new_db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    let mut new_cov: Value = serde_json::from_str(&new_cov).unwrap();
    let rows:Vec<Value>=new_db.prepare("SELECT path,selector FROM xml_diagnostics ORDER BY path COLLATE BINARY,selector COLLATE BINARY").unwrap().query_map([],|r|Ok(json!({"path":r.get::<_,String>(0)?,"selector":r.get::<_,String>(1)?}))).unwrap().map(Result::unwrap).collect();
    assert_eq!(old_cov["xml_unverified"], json!(rows));
    old_cov.as_object_mut().unwrap().remove("xml_unverified");
    new_cov.as_object_mut().unwrap().remove("xml_diagnostics");
    assert_eq!(old_cov, new_cov);
    drop(old_db);
    drop(new_db);
    fs::remove_dir_all(&f.repo).unwrap();
    f.built = BuiltSnapshot {
        path: PathBuf::from(built["path"].as_str().unwrap()),
        snapshot_id: built["snapshot_id"].as_str().unwrap().into(),
        file_sha256: built["file_sha256"].as_str().unwrap().into(),
        facts_sha256: built["facts_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.read_limit = "134217728";
    let verify = |f: &Fixture, cap: u64| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .env("PATH", "")
            .args([
                "verify",
                f.built.path.to_str().unwrap(),
                "--staging-dir",
                f.stage.to_str().unwrap(),
                "--max-sqlite-bytes",
                "134217728",
                "--expected-sha256",
                &f.built.file_sha256,
                "--max-sources",
                "4000",
                "--max-source-bytes",
                "8388608",
                "--max-total-bytes",
                "33554432",
                "--max-scratch-bytes",
                "134217728",
                "--max-facts",
                "100000",
                "--max-coverage-bytes",
                "1048576",
                "--facts-time-ms",
                "60000",
                "--max-opaque-bin-bytes",
                "25165824",
                "--max-diagnostics-bytes",
                &cap.to_string(),
            ])
            .output()
            .unwrap()
    };
    let o = verify(&f, total);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let o = verify(&f, total - 1);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("diagnostics_bytes"));
    let mut peer = Peer::start(&f);
    let meta = peer.data("metadata", json!({}));
    assert_eq!(meta["snapshot"]["coverage"]["xml_unverified_count"], 72000);
    assert_eq!(
        meta["snapshot"]["coverage"]["xml_diagnostics_sha256"],
        expected["sha256"]
    );
    assert!(meta.to_string().len() < 8192);
    let root = peer.data("tree", json!({}));
    let kind = root["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["name"] == "Language")
        .unwrap();
    let mut a = kind["children"].clone();
    a["name_exact"] = json!("N0599");
    let selected = peer.data("tree", a);
    let branch = peer.data("tree", selected["data"]["items"][0]["children"].clone());
    assert_eq!(
        bytes(&peer.data("source", branch["data"]["branch"]["source"].clone())["data"]),
        last.as_bytes()
    );
    assert_eq!(
        bytes(&peer.data("source", json!({"path":ROOT,"start_line":1,"end_line":5}))["data"]),
        TEXT.as_bytes()
    );
    peer.finish();
    let backup = f.root.join("good-backup.sqlite");
    fs::copy(&f.built.path, &backup).unwrap();
    for mutation in ["DELETE FROM xml_diagnostics WHERE diagnostic_id=(SELECT max(diagnostic_id) FROM xml_diagnostics)","UPDATE xml_diagnostics SET selector='/forged' WHERE diagnostic_id=1"] {
        fs::copy(&backup,&f.built.path).unwrap();let db=rusqlite::Connection::open(&f.built.path).unwrap();db.execute(mutation,[]).unwrap();let mut v=coverage.clone();v["xml_diagnostics"]=summary(&db);db.execute("UPDATE metadata SET coverage_draft=?1",[v.to_string()]).unwrap();let digest=bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();db.execute("UPDATE metadata SET facts_sha256=?1",[digest]).unwrap();drop(db);f.alter("");assert!(f.command().output().unwrap().status.success());let o=verify(&f,67108864);assert!(!o.status.success());assert!(String::from_utf8_lossy(&o.stderr).contains("VerifyFacts"),"{}",String::from_utf8_lossy(&o.stderr));assert_eq!(fs::read_dir(&f.stage).unwrap().count(),0);
    }
}

#[test]
fn optional_forms_cli_offline_stdio_absent_unparsed_owners_and_strict_families() {
    let mut f = Fixture::new(&[]);
    fs::create_dir(&f.repo).unwrap();
    let git = |a: &[&str]| {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&f.repo)
            .args(a)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let write = |p: &str, b: &[u8]| {
        let p = f.repo.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b).unwrap();
    };
    let xml = |kind: &str, name: &str, id: usize, children: &str| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{kind} uuid=\"c0000000-0000-0000-0000-{id:012}\"><Properties><Name>{name}</Name><Unverified/></Properties>{children}</{kind}></MetaDataObject>")
    };
    let roots = [
        ("AccumulationRegister", "AccumulationRegisters", "Accum"),
        (
            "ChartOfCharacteristicTypes",
            "ChartsOfCharacteristicTypes",
            "Characteristic",
        ),
        ("AccountingRegister", "AccountingRegisters", "Accounting"),
        ("CalculationRegister", "CalculationRegisters", "Calculation"),
        ("ChartOfAccounts", "ChartsOfAccounts", "Accounts"),
        (
            "ChartOfCalculationTypes",
            "ChartsOfCalculationTypes",
            "CalcTypes",
        ),
    ];
    let refs = roots
        .iter()
        .map(|(k, _, n)| format!("<{k}>{n}</{k}>"))
        .collect::<String>();
    write(
        "Configuration.xml",
        xml(
            "Configuration",
            "Synthetic",
            1,
            &format!("<ChildObjects>{refs}</ChildObjects>"),
        )
        .as_bytes(),
    );
    let mut descriptors = Vec::new();
    for (i, (kind, folder, name)) in roots.iter().enumerate() {
        let forms: Vec<&str> = if i < 2 {
            vec!["Absent", "WithHelp", "Coded", "Refused"]
        } else {
            vec!["Strict"]
        };
        let mut children = forms
            .iter()
            .map(|n| format!("<Form>{n}</Form>"))
            .collect::<String>();
        if *kind == "CalculationRegister" {
            children.push_str("<Recalculation>Again</Recalculation>");
            write(
                &format!("{folder}/{name}/Recalculations/Again.xml"),
                register_descriptor("Recalculation", "Again", 900, &recalculation_body())
                    .as_bytes(),
            );
        }
        write(
            &format!("{folder}/{name}.xml"),
            xml(
                kind,
                name,
                i + 2,
                &format!("<InternalInfo/><ChildObjects>{children}</ChildObjects>"),
            )
            .as_bytes(),
        );
        if *kind == "AccountingRegister" {
            write(
                &format!("{folder}/{name}/Ext/RecordSetModule.bsl"),
                TEXT.as_bytes(),
            );
        }
        if kind.starts_with("ChartOf") {
            write(
                &format!("{folder}/{name}/Ext/ManagerModule.bsl"),
                TEXT.as_bytes(),
            );
        }
        for (j, form) in forms.iter().enumerate() {
            let base = format!("{folder}/{name}/Forms/{form}");
            let raw = xml("Form", form, 100 + i * 10 + j, "");
            write(&format!("{base}.xml"), raw.as_bytes());
            write(&format!("{base}/Ext/Form.xml"), b"<OpaqueLayout/>");
            if ["Coded", "Strict"].contains(form) {
                write(&format!("{base}/Ext/Form/Module.bsl"), TEXT.as_bytes());
            }
            if *form == "Refused" {
                write(&format!("{base}/Ext/Form/Module.bsl"), BINARY);
            }
            if *form == "WithHelp" {
                write(&format!("{base}/Ext/Help.xml"), b"<OpaqueHelp/>");
                write(&format!("{base}/Ext/Help/ru.html"), b"<html>opaque</html>");
            }
            descriptors.push((i, form.to_string(), base, raw));
        }
    }
    let commit = || {
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@invalid",
            "commit",
            "-qm",
            "optional form fixture",
        ]);
    };
    commit();
    let args = |out: &str| {
        vec![
            "build-optional-form".into(),
            f.repo.to_str().unwrap().into(),
            git(&["rev-parse", "HEAD"]),
            git(&["rev-parse", "HEAD^{tree}"]),
            "synthetic:optional-form".into(),
            f.root.join(out).to_str().unwrap().into(),
            "4000".into(),
            "8388608".into(),
            "33554432".into(),
            "134217728".into(),
            "10000".into(),
            "1000".into(),
            "10000".into(),
            "8388608".into(),
            "67108864".into(),
            "60000".into(),
            "100000".into(),
            "1048576".into(),
            "60000".into(),
            "[]".into(),
            "25165824".into(),
            "67108864".into(),
        ]
    };
    let run = |a: &[String]| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .args(a)
            .output()
            .unwrap()
    };
    let o = run(&args("forms-ok"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let built: Value = serde_json::from_slice(&o.stdout).unwrap();
    let repeat = run(&args("forms-repeat"));
    assert!(repeat.status.success());
    let repeat: Value = serde_json::from_slice(&repeat.stdout).unwrap();
    assert_eq!(repeat["snapshot_id"], built["snapshot_id"]);
    assert_eq!(repeat["facts_sha256"], built["facts_sha256"]);
    let reject = |a: Vec<String>, err: &str| {
        let o = run(&a);
        assert!(!o.status.success());
        assert!(
            String::from_utf8_lossy(&o.stderr).contains(err),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert_eq!(fs::read_dir(&a[5]).unwrap().count(), 0);
    };
    let mut limited = args("forms-diagnostics-limit");
    limited[21] = "1".into();
    reject(limited, "diagnostics_bytes");
    let mut old = args("forms-old");
    old[0] = "build-diagnostics".into();
    reject(old, "register_form_incomplete");
    // Missing layout, each half-help and an unexpected BSL never become valid absence.
    for (i, folder, name) in [
        (0, "AccumulationRegisters", "Accum"),
        (1, "ChartsOfCharacteristicTypes", "Characteristic"),
    ] {
        let prefix = format!("{folder}/{name}/Forms");
        let err = if i == 0 {
            "register_form_incomplete"
        } else {
            "chart_form_incomplete"
        };
        fs::remove_file(f.repo.join(format!("{prefix}/Absent/Ext/Form.xml"))).unwrap();
        commit();
        reject(args(&format!("no-layout-{i}")), err);
        write(&format!("{prefix}/Absent/Ext/Form.xml"), b"<OpaqueLayout/>");
        commit();
        for suffix in ["Help.xml", "Help/ru.html"] {
            let p = format!("{prefix}/WithHelp/Ext/{suffix}");
            let old = fs::read(f.repo.join(&p)).unwrap();
            fs::remove_file(f.repo.join(&p)).unwrap();
            commit();
            reject(
                args(&format!("half-help-{i}-{}", suffix.len())),
                if i == 0 {
                    "register_help_incomplete"
                } else {
                    "chart_help_incomplete"
                },
            );
            write(&p, &old);
            commit();
        }
        let p = format!("{prefix}/Absent/Ext/OtherModule.bsl");
        write(&p, TEXT.as_bytes());
        commit();
        reject(args(&format!("other-bsl-{i}")), "layout_unsupported");
        fs::remove_file(f.repo.join(&p)).unwrap();
        commit();
        let p = format!("{prefix}/Absent.xml");
        let good = fs::read(f.repo.join(&p)).unwrap();
        write(&p, xml("Template", "Absent", 777, "").as_bytes());
        commit();
        reject(args(&format!("wrong-type-{i}")), "Xml");
        write(&p, &good);
        commit();
    }
    for (i, (_, folder, name)) in roots.iter().enumerate().skip(2) {
        let p = format!("{folder}/{name}/Forms/Strict/Ext/Form/Module.bsl");
        fs::remove_file(f.repo.join(&p)).unwrap();
        commit();
        reject(
            args(&format!("strict-{i}")),
            if i < 4 {
                "register_form_incomplete"
            } else {
                "chart_form_incomplete"
            },
        );
        write(&p, TEXT.as_bytes());
        commit();
    }
    // Legacy chart rule remains strict even when register forms all have code.
    for form in ["Absent", "WithHelp"] {
        write(
            &format!("AccumulationRegisters/Accum/Forms/{form}/Ext/Form/Module.bsl"),
            TEXT.as_bytes(),
        );
    }
    commit();
    let mut old = args("forms-old-chart");
    old[0] = "build-diagnostics".into();
    reject(old, "chart_form_incomplete");
    fs::remove_dir_all(&f.repo).unwrap();
    f.built = BuiltSnapshot {
        path: PathBuf::from(built["path"].as_str().unwrap()),
        snapshot_id: built["snapshot_id"].as_str().unwrap().into(),
        file_sha256: built["file_sha256"].as_str().unwrap().into(),
        facts_sha256: built["facts_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.read_limit = "134217728";
    let verify = |f: &Fixture| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .env("PATH", "")
            .args([
                "verify",
                f.built.path.to_str().unwrap(),
                "--staging-dir",
                f.stage.to_str().unwrap(),
                "--max-sqlite-bytes",
                "134217728",
                "--expected-sha256",
                &f.built.file_sha256,
                "--max-sources",
                "4000",
                "--max-source-bytes",
                "8388608",
                "--max-total-bytes",
                "33554432",
                "--max-scratch-bytes",
                "134217728",
                "--max-facts",
                "100000",
                "--max-coverage-bytes",
                "1048576",
                "--facts-time-ms",
                "60000",
                "--max-opaque-bin-bytes",
                "25165824",
                "--max-diagnostics-bytes",
                "67108864",
            ])
            .output()
            .unwrap()
    };
    let o = verify(&f);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    let raw: String = db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    assert!(raw.len() < 8192);
    let cov: Value = serde_json::from_str(&raw).unwrap();
    assert!(cov["xml_diagnostics"]["count"].as_u64().unwrap() > 0);
    assert!(cov.get("xml_unverified").is_none());
    let mut peer = Peer::start(&f);
    let calc: i64 = db
        .query_row(
            "SELECT object_id FROM objects WHERE xml_path='CalculationRegisters/Calculation.xml'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let refs = peer.data("tree", json!({"level":"companions","parent_id":calc}));
    let rec = refs["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["path"] == "CalculationRegisters/Calculation/Recalculations/Again.xml")
        .unwrap();
    assert_eq!(
        bytes(&peer.data("source", rec["source"].clone())["data"]),
        register_descriptor("Recalculation", "Again", 900, &recalculation_body()).as_bytes()
    );
    let mut absent_id = 0;
    for (i, form, base, raw) in &descriptors {
        let path = format!("{base}.xml");
        let (id, parent): (i64, i64) = db
            .query_row(
                "SELECT object_id,parent_id FROM objects WHERE xml_path=?1",
                [&path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let parent_path: String = db
            .query_row(
                "SELECT xml_path FROM objects WHERE object_id=?1",
                [parent],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(parent_path, format!("{}/{}.xml", roots[*i].1, roots[*i].2));
        let b = peer.data("tree", json!({"level":"object","parent_id":id}));
        if *i < 2 {
            assert_eq!(
                b["data"]["branch"]["form_module_presence"],
                match form.as_str() {
                    "Coded" => "accepted",
                    "Refused" => "unparsed",
                    _ => "absent",
                }
            );
        }
        assert_eq!(
            bytes(&peer.data("source", b["data"]["branch"]["source"].clone())["data"]),
            raw.as_bytes()
        );
        let module = format!("{base}/Ext/Form/Module.bsl");
        let count: i64 = db
            .query_row(
                "SELECT count(*) FROM modules WHERE path=?1",
                [&module],
                |r| r.get(0),
            )
            .unwrap();
        if ["Absent", "WithHelp"].contains(&form.as_str()) {
            assert_eq!(count, 0);
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM sources WHERE path=?1",
                    [&module],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                0
            );
            let missing = peer.call("source", json!({"path":module,"start_line":1,"end_line":1}));
            assert!(missing["result"]["isError"].as_bool().unwrap());
            if *i == 0 && form == "Absent" {
                absent_id = id;
            }
        } else {
            assert_eq!(count, 1);
            let owner: i64 = db
                .query_row(
                    "SELECT owner_id FROM modules WHERE path=?1",
                    [&module],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(owner, id);
            let result=peer.data("source",json!({"saved_path":module,"start_byte":0,"end_byte":if form=="Refused"{BINARY.len()}else{TEXT.len()}}));
            assert_eq!(
                bytes(&result["data"]),
                if form == "Refused" {
                    BINARY
                } else {
                    TEXT.as_bytes()
                }
            );
        }
    }
    peer.finish();
    assert!(absent_id > 0);
    drop(db);
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    db.execute(
        "UPDATE objects SET parent_id=1 WHERE object_id=?1",
        [absent_id],
    )
    .unwrap();
    let digest = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [digest])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let o = verify(&f);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("VerifyFacts"));
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}

#[test]
fn final_parent_cli_homonyms_offline_stdio_and_real_duplicate_rollback() {
    let mut f = Fixture::new(&[]);
    fs::create_dir(&f.repo).unwrap();
    let git = |a: &[&str]| {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&f.repo)
            .args(a)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let write = |p: &str, b: &[u8]| {
        let p = f.repo.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b).unwrap();
    };
    let xml = |kind: &str, name: &str, id: usize, children: &str| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{kind} uuid=\"d0000000-0000-0000-0000-{id:012}\"><Properties><Name>{name}</Name><Unverified/></Properties>{children}</{kind}></MetaDataObject>")
    };
    let roots = [
        ("Subsystem", "Subsystems", "Same"),
        ("Subsystem", "Subsystems", "Other"),
        ("AccumulationRegister", "AccumulationRegisters", "One"),
        ("AccumulationRegister", "AccumulationRegisters", "Two"),
        ("Catalog", "Catalogs", "One"),
        ("Catalog", "Catalogs", "Two"),
        ("CalculationRegister", "CalculationRegisters", "One"),
        ("CalculationRegister", "CalculationRegisters", "Two"),
    ];
    let refs = roots
        .iter()
        .map(|(k, _, n)| format!("<{k}>{n}</{k}>"))
        .collect::<String>();
    let config = xml(
        "Configuration",
        "Synthetic",
        1,
        &format!("<ChildObjects>{refs}</ChildObjects>"),
    );
    write("Configuration.xml", config.as_bytes());
    let mut evidence = Vec::<(String, String, String, String)>::new(); // path,parent,exact XML,reference XML
    let mut next = 100;
    for (kind, folder, name) in roots {
        let (child_kind, child_folder) = match kind {
            "Subsystem" => ("Subsystem", "Subsystems"),
            "Catalog" => ("Template", "Templates"),
            "CalculationRegister" => ("Recalculation", "Recalculations"),
            _ => ("Form", "Forms"),
        };
        let reference = format!("<{child_kind}>Same</{child_kind}>");
        let root_path = format!("{folder}/{name}.xml");
        next += 1;
        let root = xml(
            kind,
            name,
            next,
            &format!("<InternalInfo/><ChildObjects>{reference}</ChildObjects>"),
        );
        write(&root_path, root.as_bytes());
        let child_path = format!("{folder}/{name}/{child_folder}/Same.xml");
        next += 1;
        let child = if child_kind == "Recalculation" {
            register_descriptor(
                "Recalculation",
                "Same",
                next as u32,
                &chart_node("Dimension", "D", next as u32 + 1000, ""),
            )
        } else {
            xml(
                child_kind,
                "Same",
                next,
                if child_kind == "Subsystem" {
                    "<ChildObjects><Subsystem>Same</Subsystem></ChildObjects>"
                } else {
                    ""
                },
            )
        };
        write(&child_path, child.as_bytes());
        evidence.push((child_path.clone(), root_path, child, reference));
        if child_kind == "Subsystem" {
            next += 1;
            let p = format!("{folder}/{name}/{child_folder}/Same/Subsystems/Same.xml");
            let raw = xml("Subsystem", "Same", next, "");
            write(&p, raw.as_bytes());
            evidence.push((p, child_path, raw, "<Subsystem>Same</Subsystem>".into()));
        }
        if child_kind == "Form" {
            write(
                &format!("{folder}/{name}/Forms/Same/Ext/Form.xml"),
                b"<OpaqueLayout/>",
            );
            if name == "Two" {
                write(
                    &format!("{folder}/{name}/Forms/Same/Ext/Form/Module.bsl"),
                    TEXT.as_bytes(),
                );
            }
        }
        if child_kind == "Template" {
            write(
                &format!("{folder}/{name}/Templates/Same/Ext/Template.txt"),
                b"\x00\xffopaque",
            );
        }
    }
    let commit = || {
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@invalid",
            "commit",
            "-qm",
            "final parents",
        ]);
    };
    commit();
    let args = |out: &str| {
        vec![
            "build-optional-form".into(),
            f.repo.to_str().unwrap().into(),
            git(&["rev-parse", "HEAD"]),
            git(&["rev-parse", "HEAD^{tree}"]),
            "synthetic:parents".into(),
            f.root.join(out).to_str().unwrap().into(),
            "4000".into(),
            "8388608".into(),
            "33554432".into(),
            "134217728".into(),
            "10000".into(),
            "1000".into(),
            "10000".into(),
            "8388608".into(),
            "67108864".into(),
            "60000".into(),
            "100000".into(),
            "1048576".into(),
            "60000".into(),
            "[]".into(),
            "25165824".into(),
            "67108864".into(),
        ]
    };
    let run = |a: &[String]| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .args(a)
            .output()
            .unwrap()
    };
    let o = run(&args("parents-ok"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let built: Value = serde_json::from_slice(&o.stdout).unwrap();
    let o = run(&args("parents-repeat"));
    assert!(o.status.success());
    let again: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(built["snapshot_id"], again["snapshot_id"]);
    assert_eq!(built["facts_sha256"], again["facts_sha256"]);
    // Genuine duplicate declaration under the same parent must still fail closed.
    let p = "Subsystems/Other.xml";
    let good = fs::read(f.repo.join(p)).unwrap();
    let duplicate = String::from_utf8(good.clone()).unwrap().replace(
        "<Subsystem>Same</Subsystem>",
        "<Subsystem>Same</Subsystem><Subsystem>Same</Subsystem>",
    );
    write(p, duplicate.as_bytes());
    commit();
    let a = args("parents-duplicate");
    let o = run(&a);
    assert!(!o.status.success());
    assert_eq!(fs::read_dir(&a[5]).unwrap().count(), 0);
    write(p, &good);
    commit();
    fs::remove_dir_all(&f.repo).unwrap();
    f.built = BuiltSnapshot {
        path: PathBuf::from(built["path"].as_str().unwrap()),
        snapshot_id: built["snapshot_id"].as_str().unwrap().into(),
        file_sha256: built["file_sha256"].as_str().unwrap().into(),
        facts_sha256: built["facts_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.read_limit = "134217728";
    let verify = |f: &Fixture| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .env("PATH", "")
            .args([
                "verify",
                f.built.path.to_str().unwrap(),
                "--staging-dir",
                f.stage.to_str().unwrap(),
                "--max-sqlite-bytes",
                "134217728",
                "--expected-sha256",
                &f.built.file_sha256,
                "--max-sources",
                "4000",
                "--max-source-bytes",
                "8388608",
                "--max-total-bytes",
                "33554432",
                "--max-scratch-bytes",
                "134217728",
                "--max-facts",
                "100000",
                "--max-coverage-bytes",
                "1048576",
                "--facts-time-ms",
                "60000",
                "--max-opaque-bin-bytes",
                "25165824",
                "--max-diagnostics-bytes",
                "67108864",
            ])
            .output()
            .unwrap()
    };
    let o = verify(&f);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    assert!(db
        .prepare("PRAGMA foreign_key_check")
        .unwrap()
        .query([])
        .unwrap()
        .next()
        .unwrap()
        .is_none());
    assert_eq!(
        db.query_row(
            "SELECT parent_id IS NULL FROM objects WHERE object_id=1 AND kind='Configuration'",
            [],
            |r| r.get::<_, bool>(0)
        )
        .unwrap(),
        true
    );
    let mut peer = Peer::start(&f);
    for (path, parent, raw, reference) in &evidence {
        let (id, owner): (i64, i64) = db
            .query_row(
                "SELECT object_id,parent_id FROM objects WHERE xml_path=?1",
                [path],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        let actual: String = db
            .query_row(
                "SELECT xml_path FROM objects WHERE object_id=?1",
                [owner],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(&actual, parent);
        let branch = peer.data("tree", json!({"level":"object","parent_id":id}));
        assert_eq!(
            bytes(&peer.data("source", branch["data"]["branch"]["source"].clone())["data"]),
            raw.as_bytes()
        );
        let parent_branch = peer.data("tree", json!({"level":"companions","parent_id":owner}));
        let node = parent_branch["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["path"] == *path && n["role"] == "descriptor")
            .unwrap();
        assert_eq!(node["object_id"], id);
        assert_eq!(
            bytes(&peer.data("source", node["source"].clone())["data"]),
            raw.as_bytes()
        );
        let own = peer.data("tree", json!({"level":"companions","parent_id":id}));
        let r = own["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["role"] == "reference")
            .unwrap();
        assert_eq!(r["path"], *parent);
        assert_eq!(
            bytes(&peer.data("source", r["source"].clone())["data"]),
            reference.as_bytes()
        );
        if path.starts_with("AccumulationRegisters/") {
            assert_eq!(
                branch["data"]["branch"]["form_module_presence"],
                if path.contains("/One/") {
                    "absent"
                } else {
                    "accepted"
                }
            );
        }
    }
    let source = "AccumulationRegisters/Two/Forms/Same/Ext/Form/Module.bsl";
    assert_eq!(
        bytes(
            &peer.data(
                "source",
                json!({"saved_path":source,"start_byte":0,"end_byte":TEXT.len()})
            )["data"]
        ),
        TEXT.as_bytes()
    );
    peer.finish();
    // UNIQUE remains a real DB invariant, including rollback of an attempted true duplicate.
    db.execute_batch("SAVEPOINT duplicate_probe").unwrap();
    let before: i64 = db
        .query_row("SELECT count(*) FROM objects", [], |r| r.get(0))
        .unwrap();
    let err=db.execute("INSERT INTO objects(object_id,parent_id,kind,name,xml_path) SELECT 99999,parent_id,kind,name,'synthetic-duplicate.xml' FROM objects WHERE xml_path='Subsystems/Same.xml'",[]).unwrap_err();
    assert!(matches!(err,rusqlite::Error::SqliteFailure(ref e,_) if e.extended_code==2067));
    db.execute_batch("ROLLBACK TO duplicate_probe; RELEASE duplicate_probe")
        .unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM objects", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        before
    );
    drop(db);
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}

#[test]
fn root_sidecars_cli_offline_stdio_owner_inventory_html_scope_and_tamper() {
    let mut f = Fixture::new(&[]);
    fs::create_dir(&f.repo).unwrap();
    let git = |a: &[&str]| {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&f.repo)
            .args(a)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let write = |p: &str, b: &[u8]| {
        let p = f.repo.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b).unwrap();
    };
    let xml = |kind: &str, name: &str, id: usize, children: &str| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{kind} uuid=\"f0000000-0000-0000-0000-{id:012}\"><Properties><Name>{name}</Name><Unverified/></Properties>{children}</{kind}></MetaDataObject>")
    };
    let roots = [
        ("Catalog", "Catalogs", "Ext"),
        ("Catalog", "Catalogs", "Other"),
        ("Document", "Documents", "Doc"),
        ("InformationRegister", "InformationRegisters", "Info"),
        ("AccumulationRegister", "AccumulationRegisters", "Accum"),
    ];
    let refs = roots
        .iter()
        .map(|(k, _, n)| format!("<{k}>{n}</{k}>"))
        .collect::<String>();
    write(
        "Configuration.xml",
        xml(
            "Configuration",
            "Synthetic",
            1,
            &format!("<ChildObjects>{refs}</ChildObjects>"),
        )
        .as_bytes(),
    );
    for (i, (kind, folder, name)) in roots.iter().enumerate() {
        write(
            &format!("{folder}/{name}.xml"),
            xml(
                kind,
                name,
                i + 2,
                if *kind == "AccumulationRegister" {
                    "<InternalInfo/><ChildObjects><Form>Card</Form></ChildObjects>"
                } else {
                    ""
                },
            )
            .as_bytes(),
        );
    }
    write(
        "AccumulationRegisters/Accum/Forms/Card.xml",
        xml("Form", "Card", 100, "").as_bytes(),
    );
    write(
        "AccumulationRegisters/Accum/Forms/Card/Ext/Form.xml",
        b"<OpaqueLayout/>",
    );
    let sidecars = [
        ("Catalogs/Ext/Ext/Help.xml", "Catalogs/Ext.xml"),
        ("Catalogs/Ext/Ext/Predefined.xml", "Catalogs/Ext.xml"),
        ("Documents/Doc/Ext/Help.xml", "Documents/Doc.xml"),
        (
            "InformationRegisters/Info/Ext/Help.xml",
            "InformationRegisters/Info.xml",
        ),
    ];
    let payload =
        b"\xef\xbb\xbf<Opaque xmlns='urn:synthetic' version='2.20'>\r\n&unknown;</Opaque>";
    for (p, _) in sidecars {
        write(p, payload);
    }
    let html = [
        "Catalogs/Ext/Ext/Help/ru.html",
        "Documents/Doc/Ext/Help/ru.html",
        "InformationRegisters/Info/Ext/Help/ru.html",
    ];
    for p in html {
        write(p, b"excluded synthetic HTML - not read");
    }
    let commit = || {
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@invalid",
            "commit",
            "-qm",
            "root sidecars",
        ]);
    };
    commit();
    let args = |out: &str| {
        vec![
            "build-root-sidecars".into(),
            f.repo.to_str().unwrap().into(),
            git(&["rev-parse", "HEAD"]),
            git(&["rev-parse", "HEAD^{tree}"]),
            "synthetic:sidecars".into(),
            f.root.join(out).to_str().unwrap().into(),
            "4000".into(),
            "8388608".into(),
            "33554432".into(),
            "134217728".into(),
            "10000".into(),
            "1000".into(),
            "10000".into(),
            "8388608".into(),
            "67108864".into(),
            "60000".into(),
            "100000".into(),
            "1048576".into(),
            "60000".into(),
            "[]".into(),
            "25165824".into(),
            "67108864".into(),
        ]
    };
    let run = |a: &[String]| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .args(a)
            .output()
            .unwrap()
    };
    let o = run(&args("sidecars-ok"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let built: Value = serde_json::from_slice(&o.stdout).unwrap();
    let o = run(&args("sidecars-repeat"));
    assert!(o.status.success());
    let repeat: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(repeat["snapshot_id"], built["snapshot_id"]);
    assert_eq!(repeat["facts_sha256"], built["facts_sha256"]);
    let reject = |a: Vec<String>, err: &str| {
        let o = run(&a);
        assert!(!o.status.success());
        assert!(
            String::from_utf8_lossy(&o.stderr).contains(err),
            "{}",
            String::from_utf8_lossy(&o.stderr)
        );
        assert_eq!(fs::read_dir(&a[5]).unwrap().count(), 0);
    };
    let mut old = args("sidecars-legacy");
    old[0] = "build-optional-form".into();
    reject(old, "companion_descriptor_inventory");
    for (i, p) in [
        "Documents/Doc/Ext/Predefined.xml",
        "InformationRegisters/Info/Ext/Predefined.xml",
        "Catalogs/Ext/Ext/Other.xml",
        "Catalogs/Ext/Ext/More/Help.xml",
        "Catalogs/Ext/Help.xml",
    ]
    .iter()
    .enumerate()
    {
        write(p, payload);
        commit();
        reject(
            args(&format!("wrong-path-{i}")),
            "companion_descriptor_inventory",
        );
        fs::remove_file(f.repo.join(p)).unwrap();
        commit();
    }
    write("Catalogs/Missing/Ext/Help.xml", payload);
    commit();
    reject(args("sidecars-orphan"), "companion_payload_orphan");
    fs::remove_file(f.repo.join("Catalogs/Missing/Ext/Help.xml")).unwrap();
    commit();
    let root = "Catalogs/Ext.xml";
    let good = fs::read(f.repo.join(root)).unwrap();
    write(root, xml("Document", "Ext", 2, "").as_bytes());
    commit();
    reject(args("wrong-owner-kind"), "Xml");
    write(root, &good);
    commit();
    let mut a = args("sidecars-facts");
    a[16] = "1".into();
    reject(a, "facts");
    fs::remove_dir_all(&f.repo).unwrap();
    f.built = BuiltSnapshot {
        path: PathBuf::from(built["path"].as_str().unwrap()),
        snapshot_id: built["snapshot_id"].as_str().unwrap().into(),
        file_sha256: built["file_sha256"].as_str().unwrap().into(),
        facts_sha256: built["facts_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.read_limit = "134217728";
    let verify = |f: &Fixture| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .env("PATH", "")
            .args([
                "verify",
                f.built.path.to_str().unwrap(),
                "--staging-dir",
                f.stage.to_str().unwrap(),
                "--max-sqlite-bytes",
                "134217728",
                "--expected-sha256",
                &f.built.file_sha256,
                "--max-sources",
                "4000",
                "--max-source-bytes",
                "8388608",
                "--max-total-bytes",
                "33554432",
                "--max-scratch-bytes",
                "134217728",
                "--max-facts",
                "100000",
                "--max-coverage-bytes",
                "1048576",
                "--facts-time-ms",
                "60000",
                "--max-opaque-bin-bytes",
                "25165824",
                "--max-diagnostics-bytes",
                "67108864",
            ])
            .output()
            .unwrap()
    };
    let o = verify(&f);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    assert_eq!(
        db.query_row("SELECT count(*) FROM sources", [], |r| r.get::<_, usize>(0))
            .unwrap(),
        12
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM sources WHERE kind='xml'", [], |r| {
            r.get::<_, usize>(0)
        })
        .unwrap(),
        7
    );
    assert_eq!(
        db.query_row("SELECT count(*) FROM objects", [], |r| r.get::<_, usize>(0))
            .unwrap(),
        7
    );
    let selection: String = db
        .query_row("SELECT input_manifest FROM metadata", [], |r| r.get(0))
        .unwrap();
    let selection: Value = serde_json::from_str(&selection).unwrap();
    assert_eq!(selection["selected"].as_array().unwrap().len(), 12);
    assert_eq!(selection["excluded"].as_array().unwrap().len(), 3);
    for p in html {
        assert_eq!(
            db.query_row("SELECT count(*) FROM sources WHERE path=?1", [p], |r| r
                .get::<_, usize>(
                0
            ))
            .unwrap(),
            0
        );
        assert!(selection["excluded"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["input"]["path"] == p && e["reason"] == "OutsideSelectionNotRead"));
    }
    let mut peer = Peer::start(&f);
    let metadata = peer.data("metadata", json!({}));
    assert_eq!(
        metadata["snapshot"]["coverage"]["root_xml_sidecars_count"],
        4
    );
    assert_eq!(
        metadata["snapshot"]["coverage"]["sidecar_help_html"],
        "outside_scope_not_read"
    );
    for (path, parent) in sidecars {
        let owner: i64 = db
            .query_row(
                "SELECT object_id FROM objects WHERE xml_path=?1",
                [parent],
                |r| r.get(0),
            )
            .unwrap();
        let b = peer.data("tree", json!({"level":"companions","parent_id":owner}));
        assert_eq!(b["data"]["payload_presence"], "present_unparsed");
        let n = b["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["path"] == path)
            .unwrap();
        assert_eq!(n["object_id"], owner);
        assert_eq!(n["role"], "additional");
        for (a, z) in [(0, 3), (3, payload.len())] {
            let source = peer.data(
                "source",
                json!({"saved_path":path,"start_byte":a,"end_byte":z}),
            );
            assert_eq!(source["data"]["kind"], "payload");
            assert_eq!(bytes(&source["data"]), payload[a..z]);
        }
    }
    let form: i64 = db
        .query_row("SELECT object_id FROM objects WHERE kind='Form'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        peer.data("tree", json!({"level":"object","parent_id":form}))["data"]["branch"]
            ["form_module_presence"],
        "absent"
    );
    peer.finish();
    db.execute("UPDATE companion_payloads SET object_id=(SELECT object_id FROM objects WHERE xml_path='Catalogs/Other.xml') WHERE path='Catalogs/Ext/Ext/Help.xml'",[]).unwrap();
    let digest = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [digest])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    let o = verify(&f);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("VerifyFacts"));
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}

#[test]
fn context_annotations_cli_offline_stdio_exact_ranges_unparsed_and_ambiguity() {
    let mut f = Fixture::new(&[]);
    fs::create_dir(&f.repo).unwrap();
    let git = |a: &[&str]| {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&f.repo)
            .args(a)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success());
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let write = |p: &str, b: &[u8]| {
        let p = f.repo.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b).unwrap();
    };
    let xml = |k: &str, n: &str, id: usize, c: &str| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{k} uuid=\"a1000000-0000-0000-0000-{id:012}\"><Properties><Name>{n}</Name><Unverified/></Properties>{c}</{k}></MetaDataObject>")
    };
    let accepted="\u{feff}#Область R\r\n&НаКлиенте\r\nПроцедура Первый()\r\nКонецПроцедуры\r\n&НаСервере\r\nФункция Второй() Экспорт\r\nКонецФункции\r\n&НаСервереБезКонтекста\r\nПроцедура Первый()\r\nКонецПроцедуры\r\n#КонецОбласти";
    let bad = [
        "&AtClient\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n&НаСервере\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n#Область R\nПроцедура P()\nКонецПроцедуры\n#КонецОбласти",
        "Процедура P()\n&НаСервере\nКонецПроцедуры",
        "&НаКлиенте\nПроцедура P(Arg)\nКонецПроцедуры",
        "#Если Клиент Тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли",
        "&НаКлиенте",
        "&НаКлиенте Процедура P()\nКонецПроцедуры",
        "&НаКлиенте\n// gap\nПроцедура P()\nКонецПроцедуры",
    ];
    let mut modules = vec![
        ("Accepted".to_string(), accepted.to_string()),
        ("Plain".to_string(), TEXT.to_string()),
    ];
    for (i, b) in bad.iter().enumerate() {
        modules.push((
            format!("Bad{i:02}"),
            format!("Процедура Before()\nКонецПроцедуры\n{b}"),
        ));
    }
    let refs = modules
        .iter()
        .map(|(n, _)| format!("<CommonModule>{n}</CommonModule>"))
        .collect::<String>();
    write(
        "Configuration.xml",
        xml(
            "Configuration",
            "Synthetic",
            1,
            &format!("<ChildObjects>{refs}<Catalog>Owner</Catalog></ChildObjects>"),
        )
        .as_bytes(),
    );
    write(
        "Catalogs/Owner.xml",
        xml("Catalog", "Owner", 900, "").as_bytes(),
    );
    write(
        "Catalogs/Owner/Ext/Help.xml",
        b"<opaque>&notParsed;</opaque>",
    );
    write("Catalogs/Owner/Ext/Help/ru.html", b"excluded");
    for (i, (n, b)) in modules.iter().enumerate() {
        write(
            &format!("CommonModules/{n}.xml"),
            xml("CommonModule", n, i + 2, "").as_bytes(),
        );
        write(&format!("CommonModules/{n}/Ext/Module.bsl"), b.as_bytes());
    }
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@invalid",
        "commit",
        "-qm",
        "context annotations",
    ]);
    let args = |cmd: &str, out: &str| {
        vec![
            cmd.into(),
            f.repo.to_str().unwrap().into(),
            git(&["rev-parse", "HEAD"]),
            git(&["rev-parse", "HEAD^{tree}"]),
            "synthetic:context".into(),
            f.root.join(out).to_str().unwrap().into(),
            "4000".into(),
            "8388608".into(),
            "33554432".into(),
            "134217728".into(),
            "10000".into(),
            "1000".into(),
            "10000".into(),
            "8388608".into(),
            "67108864".into(),
            "60000".into(),
            "100000".into(),
            "1048576".into(),
            "60000".into(),
            "[]".into(),
            "25165824".into(),
            "67108864".into(),
        ]
    };
    let run = |a: &[String]| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .args(a)
            .output()
            .unwrap()
    };
    let o = run(&args("build-context", "context-ok"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let built: Value = serde_json::from_slice(&o.stdout).unwrap();
    let o = run(&args("build-root-sidecars", "context-old"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let old: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_ne!(old["snapshot_id"], built["snapshot_id"]);
    let old_db = rusqlite::Connection::open(old["path"].as_str().unwrap()).unwrap();
    assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/Accepted/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"unparsed");
    assert_eq!(old_db.query_row("SELECT count(*) FROM methods WHERE module_path='CommonModules/Accepted/Ext/Module.bsl'",[],|r|r.get::<_,usize>(0)).unwrap(),0);
    drop(old_db);
    let o = run(&args("build-context", "context-repeat"));
    assert!(o.status.success());
    let repeat: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(repeat["snapshot_id"], built["snapshot_id"]);
    assert_eq!(repeat["facts_sha256"], built["facts_sha256"]);
    fs::remove_dir_all(&f.repo).unwrap();
    f.built = BuiltSnapshot {
        path: PathBuf::from(built["path"].as_str().unwrap()),
        snapshot_id: built["snapshot_id"].as_str().unwrap().into(),
        file_sha256: built["file_sha256"].as_str().unwrap().into(),
        facts_sha256: built["facts_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.read_limit = "134217728";
    let verify = |f: &Fixture| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .env("PATH", "")
            .args([
                "verify",
                f.built.path.to_str().unwrap(),
                "--staging-dir",
                f.stage.to_str().unwrap(),
                "--max-sqlite-bytes",
                "134217728",
                "--expected-sha256",
                &f.built.file_sha256,
                "--max-sources",
                "4000",
                "--max-source-bytes",
                "8388608",
                "--max-total-bytes",
                "33554432",
                "--max-scratch-bytes",
                "134217728",
                "--max-facts",
                "100000",
                "--max-coverage-bytes",
                "1048576",
                "--facts-time-ms",
                "60000",
                "--max-opaque-bin-bytes",
                "25165824",
                "--max-diagnostics-bytes",
                "67108864",
            ])
            .output()
            .unwrap()
    };
    let o = verify(&f);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    assert_eq!(
        db.query_row("SELECT lexical_version FROM metadata", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "r09-context-ru-v1"
    );
    let cov: String = db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    let cov: Value = serde_json::from_str(&cov).unwrap();
    assert_eq!(cov["parse_counts"]["accepted"], 2);
    assert!(cov["xml_diagnostics"]["count"].as_u64().unwrap() > 0);
    let mut peer = Peer::start(&f);
    assert_eq!(
        peer.data("metadata", json!({}))["snapshot"]["lexical_guard"],
        "r09-context-ru-v1"
    );
    for (n, body) in &modules {
        let path = format!("CommonModules/{n}/Ext/Module.bsl");
        let id: i64 = db
            .query_row(
                "SELECT object_id FROM objects WHERE xml_path=?1",
                [format!("CommonModules/{n}.xml")],
                |r| r.get(0),
            )
            .unwrap();
        let tree = peer.data("tree", json!({"level":"object","parent_id":id}));
        let m = tree["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["type"] == "module")
            .unwrap();
        let mut query = m["children"].clone();
        if n == "Accepted" {
            let all = peer.data("tree", query.clone());
            assert_eq!(all["data"]["items"].as_array().unwrap().len(), 3);
            query["name_exact"] = json!("первый");
            let ambiguous = peer.data("tree", query.clone());
            assert_eq!(ambiguous["data"]["branch"]["match_status"], "ambiguous");
            assert_eq!(ambiguous["data"]["items"].as_array().unwrap().len(), 2);
            query["name_exact"] = json!("второй");
            let one = peer.data("tree", query);
            let method = &one["data"]["items"][0];
            assert_eq!(
                bytes(&peer.data("source", method["source"].clone())["data"]),
                "Функция Второй() Экспорт\r\nКонецФункции\r\n".as_bytes()
            );
            assert_eq!(
                db.query_row(
                    "SELECT start_line FROM methods WHERE name='второй'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                6
            );
        }
        if n.starts_with("Bad") {
            assert_eq!(
                db.query_row(
                    "SELECT parse_status FROM modules WHERE path=?1",
                    [&path],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "unparsed"
            );
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM methods WHERE module_path=?1",
                    [&path],
                    |r| r.get::<_, usize>(0)
                )
                .unwrap(),
                0
            );
        }
        assert_eq!(
            bytes(
                &peer.data(
                    "source",
                    json!({"saved_path":path,"start_byte":0,"end_byte":body.len()})
                )["data"]
            ),
            body.as_bytes()
        );
    }
    peer.finish();
    db.execute("UPDATE methods SET name='forged' WHERE name='второй'", [])
        .unwrap();
    let digest = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [digest])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    assert!(String::from_utf8_lossy(&verify(&f).stderr).contains("VerifyFacts"));
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}

#[test]
fn parameter_headers_cli_offline_stdio_exact_ranges_unparsed_and_rollback() {
    let mut f = Fixture::new(&[]);
    fs::create_dir(&f.repo).unwrap();
    let git = |a: &[&str]| {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&f.repo)
            .args(a)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success());
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let write = |p: &str, b: &[u8]| {
        let p = f.repo.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b).unwrap();
    };
    let xml = |k: &str, n: &str, id: usize, c: &str| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{k} uuid=\"a1000000-0000-0000-0000-{id:012}\"><Properties><Name>{n}</Name><Unverified/></Properties>{c}</{k}></MetaDataObject>")
    };
    let accepted="\u{feff}#Область R\r\n&НаКлиенте\r\nПроцедура Первый(Знач A, B)\r\nКонецПроцедуры\r\n&НаСервере\r\nФункция Второй(A, Val Б, В, Г) Экспорт\r\nКонецФункции\r\n&НаСервереБезКонтекста\r\nПроцедура Первый(Знач A, B)\r\nКонецПроцедуры\r\n#КонецОбласти";
    let bad = [
        "&AtClient\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n&НаСервере\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n\nПроцедура P()\nКонецПроцедуры",
        "&НаКлиенте\n#Область R\nПроцедура P()\nКонецПроцедуры\n#КонецОбласти",
        "Процедура P()\n&НаСервере\nКонецПроцедуры",
        "&НаКлиенте\nПроцедура P(Arg=1)\nКонецПроцедуры",
        "#Если Клиент Тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли",
        "&НаКлиенте",
        "&НаКлиенте Процедура P()\nКонецПроцедуры",
        "&НаКлиенте\n// gap\nПроцедура P()\nКонецПроцедуры",
    ];
    let mut modules = vec![
        ("Accepted".to_string(), accepted.to_string()),
        ("Plain".to_string(), TEXT.to_string()),
    ];
    for (i, b) in bad.iter().enumerate() {
        modules.push((
            format!("Bad{i:02}"),
            format!("Процедура Before()\nКонецПроцедуры\n{b}"),
        ));
    }
    let refs = modules
        .iter()
        .map(|(n, _)| format!("<CommonModule>{n}</CommonModule>"))
        .collect::<String>();
    write(
        "Configuration.xml",
        xml(
            "Configuration",
            "Synthetic",
            1,
            &format!("<ChildObjects>{refs}<Catalog>Owner</Catalog></ChildObjects>"),
        )
        .as_bytes(),
    );
    write(
        "Catalogs/Owner.xml",
        xml("Catalog", "Owner", 900, "").as_bytes(),
    );
    write(
        "Catalogs/Owner/Ext/Help.xml",
        b"<opaque>&notParsed;</opaque>",
    );
    write("Catalogs/Owner/Ext/Help/ru.html", b"excluded");
    for (i, (n, b)) in modules.iter().enumerate() {
        write(
            &format!("CommonModules/{n}.xml"),
            xml("CommonModule", n, i + 2, "").as_bytes(),
        );
        write(&format!("CommonModules/{n}/Ext/Module.bsl"), b.as_bytes());
    }
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@invalid",
        "commit",
        "-qm",
        "context annotations",
    ]);
    let args = |cmd: &str, out: &str| {
        vec![
            cmd.into(),
            f.repo.to_str().unwrap().into(),
            git(&["rev-parse", "HEAD"]),
            git(&["rev-parse", "HEAD^{tree}"]),
            "synthetic:context".into(),
            f.root.join(out).to_str().unwrap().into(),
            "4000".into(),
            "8388608".into(),
            "33554432".into(),
            "134217728".into(),
            "10000".into(),
            "1000".into(),
            "10000".into(),
            "8388608".into(),
            "67108864".into(),
            "60000".into(),
            "100000".into(),
            "1048576".into(),
            "60000".into(),
            "[]".into(),
            "25165824".into(),
            "67108864".into(),
        ]
    };
    let run = |a: &[String]| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .args(a)
            .output()
            .unwrap()
    };
    let o = run(&args("build-parameters", "context-ok"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let built: Value = serde_json::from_slice(&o.stdout).unwrap();
    let o = run(&args("build-context", "context-old"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let old: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_ne!(old["snapshot_id"], built["snapshot_id"]);
    let old_db = rusqlite::Connection::open(old["path"].as_str().unwrap()).unwrap();
    assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/Accepted/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"unparsed");
    assert_eq!(old_db.query_row("SELECT count(*) FROM methods WHERE module_path='CommonModules/Accepted/Ext/Module.bsl'",[],|r|r.get::<_,usize>(0)).unwrap(),0);
    drop(old_db);
    let o = run(&args("build-parameters", "context-repeat"));
    assert!(o.status.success());
    let repeat: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(repeat["snapshot_id"], built["snapshot_id"]);
    assert_eq!(repeat["facts_sha256"], built["facts_sha256"]);
    let mut refused = args("build-parameters", "parameters-rollback");
    refused[16] = "1".into();
    let failed = run(&refused);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    assert_eq!(
        fs::read_dir(f.root.join("parameters-rollback"))
            .unwrap()
            .count(),
        0
    );
    fs::remove_dir_all(&f.repo).unwrap();
    f.built = BuiltSnapshot {
        path: PathBuf::from(built["path"].as_str().unwrap()),
        snapshot_id: built["snapshot_id"].as_str().unwrap().into(),
        file_sha256: built["file_sha256"].as_str().unwrap().into(),
        facts_sha256: built["facts_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.read_limit = "134217728";
    let verify = |f: &Fixture| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .env("PATH", "")
            .args([
                "verify",
                f.built.path.to_str().unwrap(),
                "--staging-dir",
                f.stage.to_str().unwrap(),
                "--max-sqlite-bytes",
                "134217728",
                "--expected-sha256",
                &f.built.file_sha256,
                "--max-sources",
                "4000",
                "--max-source-bytes",
                "8388608",
                "--max-total-bytes",
                "33554432",
                "--max-scratch-bytes",
                "134217728",
                "--max-facts",
                "100000",
                "--max-coverage-bytes",
                "1048576",
                "--facts-time-ms",
                "60000",
                "--max-opaque-bin-bytes",
                "25165824",
                "--max-diagnostics-bytes",
                "67108864",
            ])
            .output()
            .unwrap()
    };
    let o = verify(&f);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    assert_eq!(
        db.query_row("SELECT lexical_version FROM metadata", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "r09-parameters-v1"
    );
    let cov: String = db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    let cov: Value = serde_json::from_str(&cov).unwrap();
    assert_eq!(cov["parse_counts"]["accepted"], 2);
    assert!(cov["xml_diagnostics"]["count"].as_u64().unwrap() > 0);
    let mut peer = Peer::start(&f);
    assert_eq!(
        peer.data("metadata", json!({}))["snapshot"]["lexical_guard"],
        "r09-parameters-v1"
    );
    for (n, body) in &modules {
        let path = format!("CommonModules/{n}/Ext/Module.bsl");
        let id: i64 = db
            .query_row(
                "SELECT object_id FROM objects WHERE xml_path=?1",
                [format!("CommonModules/{n}.xml")],
                |r| r.get(0),
            )
            .unwrap();
        let tree = peer.data("tree", json!({"level":"object","parent_id":id}));
        let m = tree["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["type"] == "module")
            .unwrap();
        let mut query = m["children"].clone();
        if n == "Accepted" {
            let all = peer.data("tree", query.clone());
            assert_eq!(all["data"]["items"].as_array().unwrap().len(), 3);
            query["name_exact"] = json!("первый");
            let ambiguous = peer.data("tree", query.clone());
            assert_eq!(ambiguous["data"]["branch"]["match_status"], "ambiguous");
            assert_eq!(ambiguous["data"]["items"].as_array().unwrap().len(), 2);
            query["name_exact"] = json!("второй");
            let one = peer.data("tree", query.clone());
            let method = &one["data"]["items"][0];
            assert_eq!(
                bytes(&peer.data("source", method["source"].clone())["data"]),
                "Функция Второй(A, Val Б, В, Г) Экспорт\r\nКонецФункции\r\n".as_bytes()
            );
            assert_eq!(
                db.query_row(
                    "SELECT start_line FROM methods WHERE name='второй'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                6
            );
        }
        if n.starts_with("Bad") {
            let refused = peer.data("tree", query.clone());
            assert!(refused.to_string().contains("unparsed"));
            assert_eq!(
                db.query_row(
                    "SELECT parse_status FROM modules WHERE path=?1",
                    [&path],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "unparsed"
            );
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM methods WHERE module_path=?1",
                    [&path],
                    |r| r.get::<_, usize>(0)
                )
                .unwrap(),
                0
            );
        }
        assert_eq!(
            bytes(
                &peer.data(
                    "source",
                    json!({"saved_path":path,"start_byte":0,"end_byte":body.len()})
                )["data"]
            ),
            body.as_bytes()
        );
    }
    peer.finish();
    db.execute("UPDATE methods SET name='forged' WHERE name='второй'", [])
        .unwrap();
    let digest = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [digest])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    assert!(String::from_utf8_lossy(&verify(&f).stderr).contains("VerifyFacts"));
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}

#[test]
fn conditional_branches_cli_offline_stdio_evidence_ambiguity_and_rollback() {
    let mut f = Fixture::new(&[]);
    fs::create_dir(&f.repo).unwrap();
    let git = |a: &[&str]| {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&f.repo)
            .args(a)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success());
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let write = |p: &str, b: &[u8]| {
        let p = f.repo.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b).unwrap();
    };
    let xml = |k: &str, n: &str, id: usize, c: &str| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{k} uuid=\"a1000000-0000-0000-0000-{id:012}\"><Properties><Name>{n}</Name><Unverified/></Properties>{c}</{k}></MetaDataObject>")
    };
    let accepted="﻿#Если Сервер Тогда\r\n&НаКлиенте\r\nПроцедура Первый(Знач A, B)\r\n#Если ВнешнееСоединение Тогда\r\nX=(1);\r\n#Иначе\r\nX=\"#Если ,) //\";\r\n#КонецЕсли\r\nКонецПроцедуры\r\n#ИначеЕсли Не Сервер Или ТолстыйКлиентОбычноеПриложение Тогда\r\n&НаСервере\r\nФункция Второй(A, Val Б, В, Г) Экспорт\r\nКонецФункции\r\n#Иначе\r\n#Если Сервер Тогда\r\n&НаСервереБезКонтекста\r\nПроцедура Первый(Знач A, B)\r\nКонецПроцедуры\r\n#КонецЕсли\r\n#КонецЕсли";
    let long_name = "Я".repeat(76);
    let accepted = format!("{accepted}\r\nПроцедура {long_name}()\r\nКонецПроцедуры");
    let bad = ["#Если Клиент Тогда\n#КонецЕсли","#Если Сервер Тогда\nПроцедура P()\n#Иначе\nКонецПроцедуры\n#КонецЕсли","Процедура P()\n#Если Сервер Тогда\nКонецПроцедуры\nПроцедура P()\n#КонецЕсли\nКонецПроцедуры","#Область R\n#Если Сервер Тогда\n#КонецОбласти\n#Область R\n#КонецЕсли\n#КонецОбласти","#Если Сервер Тогда\n&НаКлиенте\n#КонецЕсли","#Если Сервер Тогда\nX=(\n#Иначе\n);\n#КонецЕсли","#Если Сервер И Тогда\n#КонецЕсли","#Если Сервер Тогда","#Если Сервер Тогда\n#Иначе\n#ИначеЕсли Сервер Тогда\n#КонецЕсли","#Если Сервер Тогда\nПроцедура P(A=1)\nКонецПроцедуры\n#КонецЕсли"];
    let mut modules = vec![
        ("Accepted".to_string(), accepted.to_string()),
        ("Plain".to_string(), TEXT.to_string()),
    ];
    for (i, b) in bad.iter().enumerate() {
        modules.push((
            format!("Bad{i:02}"),
            format!("Процедура Before()\nКонецПроцедуры\n{b}"),
        ));
    }
    let refs = modules
        .iter()
        .map(|(n, _)| format!("<CommonModule>{n}</CommonModule>"))
        .collect::<String>();
    write(
        "Configuration.xml",
        xml(
            "Configuration",
            "Synthetic",
            1,
            &format!("<ChildObjects>{refs}<Catalog>Owner</Catalog></ChildObjects>"),
        )
        .as_bytes(),
    );
    write(
        "Catalogs/Owner.xml",
        xml("Catalog", "Owner", 900, "").as_bytes(),
    );
    write(
        "Catalogs/Owner/Ext/Help.xml",
        b"<opaque>&notParsed;</opaque>",
    );
    write("Catalogs/Owner/Ext/Help/ru.html", b"excluded");
    for (i, (n, b)) in modules.iter().enumerate() {
        write(
            &format!("CommonModules/{n}.xml"),
            xml("CommonModule", n, i + 2, "").as_bytes(),
        );
        write(&format!("CommonModules/{n}/Ext/Module.bsl"), b.as_bytes());
    }
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@invalid",
        "commit",
        "-qm",
        "context annotations",
    ]);
    let args = |cmd: &str, out: &str| {
        vec![
            cmd.into(),
            f.repo.to_str().unwrap().into(),
            git(&["rev-parse", "HEAD"]),
            git(&["rev-parse", "HEAD^{tree}"]),
            "synthetic:context".into(),
            f.root.join(out).to_str().unwrap().into(),
            "4000".into(),
            "8388608".into(),
            "33554432".into(),
            "134217728".into(),
            "10000".into(),
            "1000".into(),
            "10000".into(),
            "8388608".into(),
            "67108864".into(),
            "60000".into(),
            "100000".into(),
            "1048576".into(),
            "60000".into(),
            "[]".into(),
            "25165824".into(),
            "67108864".into(),
        ]
    };
    let run = |a: &[String]| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .args(a)
            .output()
            .unwrap()
    };
    let o = run(&args("build-conditionals", "context-ok"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let built: Value = serde_json::from_slice(&o.stdout).unwrap();
    let o = run(&args("build-parameters", "context-old"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let old: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_ne!(old["snapshot_id"], built["snapshot_id"]);
    let old_db = rusqlite::Connection::open(old["path"].as_str().unwrap()).unwrap();
    assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/Accepted/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"unparsed");
    assert_eq!(old_db.query_row("SELECT count(*) FROM methods WHERE module_path='CommonModules/Accepted/Ext/Module.bsl'",[],|r|r.get::<_,usize>(0)).unwrap(),0);
    drop(old_db);
    let o = run(&args("build-conditionals", "context-repeat"));
    assert!(o.status.success());
    let repeat: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(repeat["snapshot_id"], built["snapshot_id"]);
    assert_eq!(repeat["facts_sha256"], built["facts_sha256"]);
    let mut refused = args("build-conditionals", "parameters-rollback");
    refused[16] = "1".into();
    let failed = run(&refused);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    assert_eq!(
        fs::read_dir(f.root.join("parameters-rollback"))
            .unwrap()
            .count(),
        0
    );
    fs::remove_dir_all(&f.repo).unwrap();
    f.built = BuiltSnapshot {
        path: PathBuf::from(built["path"].as_str().unwrap()),
        snapshot_id: built["snapshot_id"].as_str().unwrap().into(),
        file_sha256: built["file_sha256"].as_str().unwrap().into(),
        facts_sha256: built["facts_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.read_limit = "134217728";
    let verify = |f: &Fixture| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .env("PATH", "")
            .args([
                "verify",
                f.built.path.to_str().unwrap(),
                "--staging-dir",
                f.stage.to_str().unwrap(),
                "--max-sqlite-bytes",
                "134217728",
                "--expected-sha256",
                &f.built.file_sha256,
                "--max-sources",
                "4000",
                "--max-source-bytes",
                "8388608",
                "--max-total-bytes",
                "33554432",
                "--max-scratch-bytes",
                "134217728",
                "--max-facts",
                "100000",
                "--max-coverage-bytes",
                "1048576",
                "--facts-time-ms",
                "60000",
                "--max-opaque-bin-bytes",
                "25165824",
                "--max-diagnostics-bytes",
                "67108864",
            ])
            .output()
            .unwrap()
    };
    let o = verify(&f);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    assert_eq!(
        db.query_row("SELECT lexical_version FROM metadata", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "r09-conditionals-v1"
    );
    let cov: String = db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    let cov: Value = serde_json::from_str(&cov).unwrap();
    assert_eq!(cov["parse_counts"]["accepted"], 2);
    assert!(cov["xml_diagnostics"]["count"].as_u64().unwrap() > 0);
    let mut peer = Peer::start(&f);
    assert_eq!(
        peer.data("metadata", json!({}))["snapshot"]["lexical_guard"],
        "r09-conditionals-v1"
    );
    for (n, body) in &modules {
        let path = format!("CommonModules/{n}/Ext/Module.bsl");
        let id: i64 = db
            .query_row(
                "SELECT object_id FROM objects WHERE xml_path=?1",
                [format!("CommonModules/{n}.xml")],
                |r| r.get(0),
            )
            .unwrap();
        let tree = peer.data("tree", json!({"level":"object","parent_id":id}));
        let m = tree["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["type"] == "module")
            .unwrap();
        let mut query = m["children"].clone();
        if n == "Accepted" {
            let all = peer.data("tree", query.clone());
            assert_eq!(all["data"]["items"].as_array().unwrap().len(), 4);
            let mut long_query = query.clone();
            long_query["name_exact"] = json!(long_name.to_lowercase());
            let long_method = peer.data("tree", long_query);
            assert_eq!(long_method["data"]["items"].as_array().unwrap().len(), 1);
            assert_eq!(
                bytes(
                    &peer.data("source", long_method["data"]["items"][0]["source"].clone())["data"]
                ),
                format!("Процедура {long_name}()\r\nКонецПроцедуры").as_bytes()
            );
            query["name_exact"] = json!("первый");
            let ambiguous = peer.data("tree", query.clone());
            assert_eq!(ambiguous["data"]["branch"]["match_status"], "ambiguous");
            assert_eq!(ambiguous["data"]["items"].as_array().unwrap().len(), 2);
            query["name_exact"] = json!("второй");
            let one = peer.data("tree", query.clone());
            let method = &one["data"]["items"][0];
            assert_eq!(
                method["location"]["conditional_evaluation"],
                "not_evaluated"
            );
            assert_eq!(
                method["location"]["declaration_branch_path"][0]["block_line"],
                1
            );
            assert_eq!(method["location"]["declaration_branch_path"][0]["arm"], 1);
            assert_eq!(
                method["location"]["declaration_branch_path"][0]["kind"],
                "elseif"
            );
            assert_eq!(
                bytes(&peer.data("source", method["source"].clone())["data"]),
                "Функция Второй(A, Val Б, В, Г) Экспорт\r\nКонецФункции\r\n".as_bytes()
            );
            assert_eq!(
                db.query_row(
                    "SELECT start_line FROM methods WHERE name='второй'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                12
            );
        }
        if n.starts_with("Bad") {
            let refused = peer.data("tree", query.clone());
            assert!(refused.to_string().contains("unparsed"));
            assert_eq!(
                db.query_row(
                    "SELECT parse_status FROM modules WHERE path=?1",
                    [&path],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "unparsed"
            );
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM methods WHERE module_path=?1",
                    [&path],
                    |r| r.get::<_, usize>(0)
                )
                .unwrap(),
                0
            );
        }
        assert_eq!(
            bytes(
                &peer.data(
                    "source",
                    json!({"saved_path":path,"start_byte":0,"end_byte":body.len()})
                )["data"]
            ),
            body.as_bytes()
        );
    }
    peer.finish();
    db.execute("UPDATE method_branches SET branch_path='[]' WHERE method_id=(SELECT method_id FROM methods WHERE name='второй')", [])
        .unwrap();
    let digest = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [digest])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    assert!(String::from_utf8_lossy(&verify(&f).stderr).contains("VerifyFacts"));
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}

#[test]
fn pipe_strings_cli_offline_stdio_exact_opaque_evidence_and_rollback() {
    let mut f = Fixture::new(&[]);
    fs::create_dir(&f.repo).unwrap();
    let git = |a: &[&str]| {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&f.repo)
            .args(a)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success());
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let write = |p: &str, b: &[u8]| {
        let p = f.repo.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b).unwrap();
    };
    let xml = |k: &str, n: &str, id: usize, c: &str| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{k} uuid=\"a1000000-0000-0000-0000-{id:012}\"><Properties><Name>{n}</Name><Unverified/></Properties>{c}</{k}></MetaDataObject>")
    };
    let accepted="﻿#Если Сервер Тогда\r\n&НаКлиенте\r\nПроцедура Первый(Знач A, B)\r\n#Если ВнешнееСоединение Тогда\r\nX=(\"open\r\n \t|Процедура Fake()\r\n |#Если Unknown Тогда ([ &НаКлиенте\r\n // quote \" and #КонецЕсли ignored\r\n |КонецПроцедуры\r\n |doubled \"\"quote\"\" // literal\r\n |close\");\r\n#Иначе\r\nX=\"#Если ,) //\";\r\n#КонецЕсли\r\nКонецПроцедуры\r\n#ИначеЕсли Не Сервер Или ТолстыйКлиентОбычноеПриложение Тогда\r\n&НаСервере\r\nФункция Второй(A, Val Б, В, Г) Экспорт\r\nКонецФункции\r\n#Иначе\r\n#Если Сервер Тогда\r\n&НаСервереБезКонтекста\r\nПроцедура Первый(Знач A, B)\r\nКонецПроцедуры\r\n#КонецЕсли\r\n#КонецЕсли";
    let long_name = "Я".repeat(76);
    let accepted = format!("{accepted}\r\nПроцедура {long_name}()\r\nКонецПроцедуры");
    let bad = [
        "Процедура P()\nX=\"open\nnot pipe\";\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n\n|end\";\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n// comment \"",
        "Процедура P()\nX=\"open\n|end\"; #Если Сервер Тогда\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n|end\"; &НаКлиенте\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n|end\");\nКонецПроцедуры",
        "Процедура P(A=\"open\n|end\")\nКонецПроцедуры",
        "Процедура P(A,\nB)\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n|КонецПроцедуры",
        "Процедура P()\nX=\"open\n|end\"; Процедура Fake()\nКонецПроцедуры",
    ];
    let mut modules = vec![
        ("Accepted".to_string(), accepted.to_string()),
        ("Plain".to_string(), TEXT.to_string()),
    ];
    for (i, b) in bad.iter().enumerate() {
        modules.push((
            format!("Bad{i:02}"),
            format!("Процедура Before()\nКонецПроцедуры\n{b}"),
        ));
    }
    let refs = modules
        .iter()
        .map(|(n, _)| format!("<CommonModule>{n}</CommonModule>"))
        .collect::<String>();
    write(
        "Configuration.xml",
        xml(
            "Configuration",
            "Synthetic",
            1,
            &format!("<ChildObjects>{refs}<Catalog>Owner</Catalog></ChildObjects>"),
        )
        .as_bytes(),
    );
    write(
        "Catalogs/Owner.xml",
        xml("Catalog", "Owner", 900, "").as_bytes(),
    );
    write(
        "Catalogs/Owner/Ext/Help.xml",
        b"<opaque>&notParsed;</opaque>",
    );
    write("Catalogs/Owner/Ext/Help/ru.html", b"excluded");
    for (i, (n, b)) in modules.iter().enumerate() {
        write(
            &format!("CommonModules/{n}.xml"),
            xml("CommonModule", n, i + 2, "").as_bytes(),
        );
        write(&format!("CommonModules/{n}/Ext/Module.bsl"), b.as_bytes());
    }
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@invalid",
        "commit",
        "-qm",
        "context annotations",
    ]);
    let args = |cmd: &str, out: &str| {
        vec![
            cmd.into(),
            f.repo.to_str().unwrap().into(),
            git(&["rev-parse", "HEAD"]),
            git(&["rev-parse", "HEAD^{tree}"]),
            "synthetic:context".into(),
            f.root.join(out).to_str().unwrap().into(),
            "4000".into(),
            "8388608".into(),
            "33554432".into(),
            "134217728".into(),
            "10000".into(),
            "1000".into(),
            "10000".into(),
            "8388608".into(),
            "67108864".into(),
            "60000".into(),
            "100000".into(),
            "1048576".into(),
            "60000".into(),
            "[]".into(),
            "25165824".into(),
            "67108864".into(),
        ]
    };
    let run = |a: &[String]| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .args(a)
            .output()
            .unwrap()
    };
    let o = run(&args("build-strings", "context-ok"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let built: Value = serde_json::from_slice(&o.stdout).unwrap();
    let o = run(&args("build-conditionals", "context-old"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let old: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_ne!(old["snapshot_id"], built["snapshot_id"]);
    let old_db = rusqlite::Connection::open(old["path"].as_str().unwrap()).unwrap();
    assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/Accepted/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"unparsed");
    assert_eq!(old_db.query_row("SELECT count(*) FROM methods WHERE module_path='CommonModules/Accepted/Ext/Module.bsl'",[],|r|r.get::<_,usize>(0)).unwrap(),0);
    drop(old_db);
    let o = run(&args("build-strings", "context-repeat"));
    assert!(o.status.success());
    let repeat: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(repeat["snapshot_id"], built["snapshot_id"]);
    assert_eq!(repeat["facts_sha256"], built["facts_sha256"]);
    let mut refused = args("build-strings", "parameters-rollback");
    refused[16] = "1".into();
    let failed = run(&refused);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    assert_eq!(
        fs::read_dir(f.root.join("parameters-rollback"))
            .unwrap()
            .count(),
        0
    );
    fs::remove_dir_all(&f.repo).unwrap();
    f.built = BuiltSnapshot {
        path: PathBuf::from(built["path"].as_str().unwrap()),
        snapshot_id: built["snapshot_id"].as_str().unwrap().into(),
        file_sha256: built["file_sha256"].as_str().unwrap().into(),
        facts_sha256: built["facts_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.read_limit = "134217728";
    let verify = |f: &Fixture| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .env("PATH", "")
            .args([
                "verify",
                f.built.path.to_str().unwrap(),
                "--staging-dir",
                f.stage.to_str().unwrap(),
                "--max-sqlite-bytes",
                "134217728",
                "--expected-sha256",
                &f.built.file_sha256,
                "--max-sources",
                "4000",
                "--max-source-bytes",
                "8388608",
                "--max-total-bytes",
                "33554432",
                "--max-scratch-bytes",
                "134217728",
                "--max-facts",
                "100000",
                "--max-coverage-bytes",
                "1048576",
                "--facts-time-ms",
                "60000",
                "--max-opaque-bin-bytes",
                "25165824",
                "--max-diagnostics-bytes",
                "67108864",
            ])
            .output()
            .unwrap()
    };
    let o = verify(&f);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    assert_eq!(
        db.query_row("SELECT lexical_version FROM metadata", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "r09-pipe-strings-v1"
    );
    let cov: String = db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    let cov: Value = serde_json::from_str(&cov).unwrap();
    assert_eq!(cov["parse_counts"]["accepted"], 2);
    assert!(cov["xml_diagnostics"]["count"].as_u64().unwrap() > 0);
    let mut peer = Peer::start(&f);
    assert_eq!(
        peer.data("metadata", json!({}))["snapshot"]["lexical_guard"],
        "r09-pipe-strings-v1"
    );
    for (n, body) in &modules {
        let path = format!("CommonModules/{n}/Ext/Module.bsl");
        let id: i64 = db
            .query_row(
                "SELECT object_id FROM objects WHERE xml_path=?1",
                [format!("CommonModules/{n}.xml")],
                |r| r.get(0),
            )
            .unwrap();
        let tree = peer.data("tree", json!({"level":"object","parent_id":id}));
        let m = tree["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["type"] == "module")
            .unwrap();
        let mut query = m["children"].clone();
        if n == "Accepted" {
            let all = peer.data("tree", query.clone());
            assert_eq!(all["data"]["items"].as_array().unwrap().len(), 4);
            let mut long_query = query.clone();
            long_query["name_exact"] = json!(long_name.to_lowercase());
            let long_method = peer.data("tree", long_query);
            assert_eq!(long_method["data"]["items"].as_array().unwrap().len(), 1);
            assert_eq!(
                bytes(
                    &peer.data("source", long_method["data"]["items"][0]["source"].clone())["data"]
                ),
                format!("Процедура {long_name}()\r\nКонецПроцедуры").as_bytes()
            );
            query["name_exact"] = json!("первый");
            let ambiguous = peer.data("tree", query.clone());
            assert_eq!(ambiguous["data"]["branch"]["match_status"], "ambiguous");
            assert_eq!(ambiguous["data"]["items"].as_array().unwrap().len(), 2);
            query["name_exact"] = json!("второй");
            let one = peer.data("tree", query.clone());
            let method = &one["data"]["items"][0];
            assert_eq!(
                method["location"]["conditional_evaluation"],
                "not_evaluated"
            );
            assert_eq!(
                method["location"]["declaration_branch_path"][0]["block_line"],
                1
            );
            assert_eq!(method["location"]["declaration_branch_path"][0]["arm"], 1);
            assert_eq!(
                method["location"]["declaration_branch_path"][0]["kind"],
                "elseif"
            );
            assert_eq!(
                bytes(&peer.data("source", method["source"].clone())["data"]),
                "Функция Второй(A, Val Б, В, Г) Экспорт\r\nКонецФункции\r\n".as_bytes()
            );
            assert_eq!(
                db.query_row(
                    "SELECT start_line FROM methods WHERE name='второй'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                18
            );
        }
        if n.starts_with("Bad") {
            let refused = peer.data("tree", query.clone());
            assert!(refused.to_string().contains("unparsed"));
            assert_eq!(
                db.query_row(
                    "SELECT parse_status FROM modules WHERE path=?1",
                    [&path],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "unparsed"
            );
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM methods WHERE module_path=?1",
                    [&path],
                    |r| r.get::<_, usize>(0)
                )
                .unwrap(),
                0
            );
        }
        assert_eq!(
            bytes(
                &peer.data(
                    "source",
                    json!({"saved_path":path,"start_byte":0,"end_byte":body.len()})
                )["data"]
            ),
            body.as_bytes()
        );
    }
    peer.finish();
    db.execute("UPDATE method_branches SET branch_path='[]' WHERE method_id=(SELECT method_id FROM methods WHERE name='второй')", [])
        .unwrap();
    let digest = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [digest])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    assert!(String::from_utf8_lossy(&verify(&f).stderr).contains("VerifyFacts"));
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}

#[test]
fn literal_defaults_cli_offline_stdio_exact_opaque_evidence_and_rollback() {
    exercise_literals_and_wrapped_headers(
        false, false, false, false, false, false, false, false, false, false, false, false, false,
    );
}

#[test]
fn wrapped_headers_cli_offline_stdio_exact_ranges_and_rollback() {
    exercise_literals_and_wrapped_headers(
        true, false, false, false, false, false, false, false, false, false, false, false, false,
    );
}

#[test]
fn corpus_refinements_cli_offline_stdio_exact_ranges_and_rollback() {
    exercise_literals_and_wrapped_headers(
        true, true, false, false, false, false, false, false, false, false, false, false, false,
    );
}

#[test]
fn annotated_module_variables_cli_offline_stdio_no_phantom_methods() {
    exercise_literals_and_wrapped_headers(
        true, true, true, false, false, false, false, false, false, false, false, false, false,
    );
}

#[test]
fn dates_members_cli_offline_stdio_exact_ranges_and_rollback() {
    exercise_literals_and_wrapped_headers(
        true, true, true, true, false, false, false, false, false, false, false, false, false,
    );
}

#[test]
fn async_casing_cli_offline_stdio_preserves_modifier_and_predecessor() {
    exercise_literals_and_wrapped_headers(
        true, true, true, true, true, false, false, false, false, false, false, false, false,
    );
}

#[test]
fn mobile_vars_cli_offline_stdio_exact_branch_and_no_phantom_methods() {
    exercise_literals_and_wrapped_headers(
        true, true, true, true, true, true, false, false, false, false, false, false, false,
    );
}

#[test]
fn method_regions_cli_offline_stdio_exact_quote_without_region_facts() {
    exercise_literals_and_wrapped_headers(
        true, true, true, true, true, true, true, false, false, false, false, false, false,
    );
}

#[test]
fn header_forms_cli_offline_stdio_exact_wrapped_async_and_null() {
    exercise_literals_and_wrapped_headers(
        true, true, true, true, true, true, true, true, false, false, false, false, false,
    );
}

#[test]
fn source_limits_cli_offline_stdio_large_body_and_exact_exceptions() {
    exercise_literals_and_wrapped_headers(
        true, true, true, true, true, true, true, true, true, false, false, false, false,
    );
}

#[test]
fn long_headers_cli_offline_stdio_exact_ordinary_and_no_relaxation() {
    exercise_literals_and_wrapped_headers(
        true, true, true, true, true, true, true, true, true, true, false, false, false,
    );
}

#[test]
fn lexical_tail_cli_offline_stdio_large_tokens_and_exact_chained_method() {
    exercise_literals_and_wrapped_headers(
        true, true, true, true, true, true, true, true, true, true, true, false, false,
    );
}

#[test]
fn source_anomalies_cli_offline_stdio_qualified_and_tamper() {
    exercise_literals_and_wrapped_headers(
        true, true, true, true, true, true, true, true, true, false, false, true, false,
    );
}

#[test]
fn navigator_agreement_cli_offline_stdio_combines_tail_and_qualification() {
    exercise_literals_and_wrapped_headers(
        true, true, true, true, true, true, true, true, true, true, true, true, false,
    );
}

#[test]
fn navigator_owners_cli_offline_stdio_binds_only_exact_root_commands() {
    exercise_literals_and_wrapped_headers(
        true, true, true, true, true, true, true, true, true, true, true, true, true,
    );
}

fn exercise_literals_and_wrapped_headers(
    wrapped: bool,
    refined: bool,
    annotated_vars: bool,
    dates_members: bool,
    async_casing: bool,
    mobile_vars: bool,
    method_regions: bool,
    header_forms: bool,
    source_limits: bool,
    long_headers: bool,
    lexical_tail: bool,
    source_anomalies: bool,
    command_owners: bool,
) {
    let (build_cmd, guard) = if command_owners {
        ("build-navigator-owners", "r09-navigator-agreement-v1")
    } else if source_anomalies && lexical_tail {
        ("build-navigator-agreement", "r09-navigator-agreement-v1")
    } else if source_anomalies {
        ("build-source-anomalies", "r09-source-anomalies-v1")
    } else if lexical_tail {
        ("build-lexical-tail", "r09-lexical-tail-v1")
    } else if long_headers {
        ("build-long-headers", "r09-long-headers-v1")
    } else if source_limits {
        ("build-source-limits", "r09-source-limits-v1")
    } else if header_forms {
        ("build-header-forms", "r09-header-forms-v1")
    } else if method_regions {
        ("build-method-regions", "r09-method-regions-v1")
    } else if mobile_vars {
        ("build-mobile-vars", "r09-mobile-vars-v1")
    } else if async_casing {
        ("build-async-casing", "r09-async-casing-v1")
    } else if dates_members {
        ("build-dates-members", "r09-dates-members-v1")
    } else if annotated_vars {
        ("build-annotated-vars", "r09-annotated-vars-v1")
    } else if refined {
        ("build-corpus-refinements", "r09-corpus-refinements-v1")
    } else if wrapped {
        ("build-wrapped-headers", "r09-wrapped-headers-v1")
    } else {
        ("build-literals", "r09-literal-defaults-v1")
    };
    let wrapped_method = "Процедура Перенос(A, B, C,\r\n  Г, Д)\r\nКонецПроцедуры\r\n";
    let wrapped_bsl = format!("\u{feff}#Если Сервер Тогда\r\n{wrapped_method}#КонецЕсли");
    let refined_method = "Функция Длинный(\r\n  Знач A=1,\r\n  Б=\"https://example.invalid/\"\"quoted\"\"/tail\") Экспорт ;\r\nКонецФункции\r\n";
    let refined_source =
        format!("\u{feff}#Если ВебКлиент И Не МобильныйКлиент Тогда\r\n{refined_method}#КонецЕсли");
    let after_variables = "Процедура ПослеПеременных()\r\nКонецПроцедуры\r\n";
    let variables_source = format!("\u{feff}#Если Сервер Тогда\r\n&НаКлиенте\r\nПерем A, B; // comment\r\n&НаСервере\r\nПерем C Экспорт;\r\n{after_variables}#КонецЕсли");
    let date_method = "Процедура Срок(A='20240229',\r\n B='202402291010') Экспорт\r\nObj.Процедура = '20240229010203'; // ignore '\r\nКонецПроцедуры";
    let date_source = format!("\u{feff}#Если Сервер Тогда\r\n{date_method}\r\n#КонецЕсли");
    let member_method = "Процедура Member()\nX = Obj.Процедура;\nКонецПроцедуры";
    let async_method = "Асинх Процедура Действие(Знач Ввод='20240101') Экспорт\r\nЖдать SomeCall(Ввод);\r\nКонецПроцедуры";
    let async_source = format!("\u{feff}&НаКлиенте\r\n{async_method}");
    let folded_method = "Процедура MixedCase()\r\nКонецПроцедуры\r\n";
    let folded_source =
        format!("#Если нЕ (вЕбКлИеНт иЛи Сервер) Тогда\r\n{folded_method}#КонецЕсли");
    let mobile_method = "Процедура MobileBranch()\r\nКонецПроцедуры\r\n";
    let mobile_source = format!("#Если Сервер Тогда\r\n#ИначеЕсли нЕ моБильнЫйАвтономныйСервер Или МобильноеПриложениеКлиент Тогда\r\n{mobile_method}#КонецЕсли");
    let wrapped_var_method = "Процедура AfterList()\r\nКонецПроцедуры";
    let wrapped_var_source =
        format!("\u{feff}&НаКлиенте\r\nПерем A, B,\r\n C, D,\r\n E, F;\r\n{wrapped_var_method}");
    let method_region_quote = "Процедура OpenAndClose()\r\n#Область Local\r\nX = 1;\r\n#КонецОбласти\r\nКонецПроцедуры\r\n";
    let method_region_source = format!("\u{feff}#Область Outer\r\n#Если Сервер Тогда\r\n{method_region_quote}#КонецЕсли\r\n#КонецОбласти");
    let null_header_method = "Процедура NullDefault(A=NuLl)\r\nКонецПроцедуры";
    let wrapped_async_quote = "Асинх Процедура WrappedAsync(A, // intro\r\n B=\"literal // value\", // comment\r\n C) Экспорт // last\r\nКонецПроцедуры";
    let wrapped_async_source = format!("&НаКлиенте\r\n{wrapped_async_quote}");
    let dashed_method = "Процедура Dashed()\r\nX = '2024-02-29';\r\nКонецПроцедуры";
    let comment_method = "Процедура AfterComments()\r\nКонецПроцедуры";
    let comment_source = format!("&НаКлиенте\r\n// comment only\r\n{comment_method}");
    let large_method = "Процедура LargeMethod()\nКонецПроцедуры";
    let anomaly_cr = "Процедура CrSource()\r\n// before\r// after\r\nКонецПроцедуры";
    let anomaly_blank = "Процедура BlankSource()\r\nX=\"open\r\n\r\n|finish\";\r\nКонецПроцедуры";
    let anomaly_semicolon = "Процедура SemicolonSource()\r\nКонецПроцедуры;";
    let compound_agreement =
        "Процедура Compound()\r\nA.B.Процедура.Field = \"value // quote\";\r\nКонецПроцедуры;";
    let long_ordinary_params = (1..=17).map(|i| format!(" A{i},\r\n")).collect::<String>();
    let long_id = "Я".repeat(65);
    let chained_method = "Процедура Chain()\r\nA.B.Процедура.Field = \"Процедура \"\"Байт\"\"; //\";\r\nКонецПроцедуры";
    let token_method = "Процедура AfterHeavyTokens()\nКонецПроцедуры";
    let long_ordinary = format!("Функция LongOrdinary(\r\n{long_ordinary_params} {long_id}=0,\r\n Z) Экспорт\r\nКонецФункции");
    let mut f = Fixture::new(&[]);
    fs::create_dir(&f.repo).unwrap();
    let git = |a: &[&str]| {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&f.repo)
            .args(a)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(o.status.success());
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    };
    git(&["init", "-q"]);
    let write = |p: &str, b: &[u8]| {
        let p = f.repo.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b).unwrap();
    };
    let xml = |k: &str, n: &str, id: usize, c: &str| {
        format!("<MetaDataObject xmlns=\"http://v8.1c.ru/8.3/MDClasses\" version=\"2.21\"><{k} uuid=\"a1000000-0000-0000-0000-{id:012}\"><Properties><Name>{n}</Name><Unverified/></Properties>{c}</{k}></MetaDataObject>")
    };
    let accepted="﻿#Если Сервер Тогда\r\n&НаКлиенте\r\nПроцедура Первый(Знач A, B); // separator\r\n#Если ВнешнееСоединение Тогда\r\nX=(\"open\r\n \t|Процедура Fake()\r\n |#Если Unknown Тогда ([ &НаКлиенте\r\n // quote \" and #КонецЕсли ignored\r\n |КонецПроцедуры\r\n |doubled \"\"quote\"\" // literal\r\n |close\");\r\n#Иначе\r\nX=\"#Если ,) //\";\r\n#КонецЕсли\r\nКонецПроцедуры\r\n#ИначеЕсли Не Сервер Или ТолстыйКлиентОбычноеПриложение Тогда\r\n&НаКлиентеНаСервереБезКонтекста\r\nФункция Второй(A=Неопределено, Val Б=-0.125, В=Истина, Г=\"), \"\"quotes\"\" // #Если\") Экспорт ;\r\nКонецФункции\r\n#Иначе\r\n#Если Сервер Тогда\r\n&НаСервереБезКонтекста\r\nПроцедура Первый(Знач A, B)\r\nКонецПроцедуры\r\n#КонецЕсли\r\n#КонецЕсли";
    let long_name = "Я".repeat(76);
    let accepted = format!("{accepted}\r\nПроцедура {long_name}() Export\r\nКонецПроцедуры");
    let bad = [
        "Процедура P(A=Call())\nКонецПроцедуры",
        "Процедура P(A=1+2)\nКонецПроцедуры",
        "Процедура P(A=- 1)\nКонецПроцедуры",
        "Процедура P(A=1.2.3)\nКонецПроцедуры",
        "Процедура P(A=1e3)\nКонецПроцедуры",
        "Процедура P(A=1);;\nКонецПроцедуры",
        "Процедура P(A=1); X=1\nКонецПроцедуры",
        "Процедура P(A=1); Экспорт\nКонецПроцедуры",
        "&НаКлиентеНаСервереБезКонтекста // not bare\nПроцедура P()\nКонецПроцедуры",
        "Процедура P()\nX=\"open\nnot pipe\";\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n\n|end\";\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n// comment \"",
        "Процедура P()\nX=\"open\n|end\"; #Если Сервер Тогда\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n|end\"; &НаКлиенте\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n|end\");\nКонецПроцедуры",
        "Процедура P(A=\"open\n|end\")\nКонецПроцедуры",
        "Процедура P(A,\nB)\nКонецПроцедуры",
        "Процедура P()\nX=\"open\n|КонецПроцедуры",
        "Процедура P()\nX=\"open\n|end\"; Процедура Fake()\nКонецПроцедуры",
    ];
    let mut modules = vec![
        ("Accepted".to_string(), accepted.to_string()),
        ("Plain".to_string(), TEXT.to_string()),
    ];
    for (i, b) in bad.iter().enumerate() {
        // This valid continuation is a negative only in the predecessor profile.
        // The Wrapped fixture below exercises exact ranges in the successor.
        if wrapped && *b == "Процедура P(A,\nB)\nКонецПроцедуры" {
            continue;
        }
        // The exact one-blank fixture is still negative in every predecessor.
        // This profile qualifies it, without changing its bytes or the
        // other negative fixtures. Its raw ranges are asserted below.
        let name = if source_anomalies && *b == "Процедура P()\nX=\"open\n\n|end\";\nКонецПроцедуры"
        {
            "InheritedBlank".into()
        } else {
            format!("Bad{i:02}")
        };
        modules.push((name, format!("Процедура Before()\nКонецПроцедуры\n{b}")));
    }
    if wrapped {
        modules.push(("Wrapped".into(), wrapped_bsl));
        // The inherited wrapped profile refuses defaults on a continuation,
        // while the refined grammar deliberately admits bounded scalar defaults.
        modules.push((
            if refined {
                "WrappedDefault"
            } else {
                "BadWrapped"
            }
            .into(),
            "Процедура Before()\nКонецПроцедуры\nПроцедура Bad(A=0,\n B)\nКонецПроцедуры".into(),
        ));
        if refined {
            modules.push((
                "BadWrappedExpression".into(),
                "Процедура Before()\nКонецПроцедуры\nПроцедура Bad(A=Call(),\n B)\nКонецПроцедуры"
                    .into(),
            ));
        }
    }
    if refined {
        modules.push(("Refined".into(), refined_source));
        modules.push((
            "BadRefined".into(),
            "Процедура Before()\nКонецПроцедуры\nПроцедура Bad(\n#Если ВебКлиент Тогда\n B)\nКонецПроцедуры\n#КонецЕсли".into(),
        ));
    }
    if annotated_vars {
        modules.push(("AnnotatedVars".into(), variables_source));
        modules.push((
            "BadAnnotatedVars".into(),
            "Процедура Before()\nКонецПроцедуры\n&НаКлиенте\nПерем A=1;\nПроцедура After()\nКонецПроцедуры".into(),
        ));
    }

    if dates_members {
        modules.push(("Dates".into(), date_source));
        modules.push(("Member".into(), member_method.into()));
        modules.push((
            "BadMember".into(),
            "Процедура P()\nX=Obj..Процедура;\nКонецПроцедуры".into(),
        ));
        modules.push((
            "BadDate".into(),
            "Процедура P(A='2024-02-29')\nКонецПроцедуры".into(),
        ));
    }

    if async_casing {
        modules.push(("AsyncCase".into(), async_source));
        modules.push(("Folded".into(), folded_source));
        modules.push((
            "BadAsyncServer".into(),
            "&НаСервере\nАсинх Процедура P()\nКонецПроцедуры".into(),
        ));
        // The mobile atom remains unsupported in the async predecessor.
        let unknown = if mobile_vars {
            "НеизвестныйМобильныйКлиент"
        } else {
            "МобильноеПриложениеКлиент"
        };
        modules.push((
            "BadAsyncFoldedUnknown".into(),
            format!("#Если нЕ {unknown} Тогда\nАсинх Процедура P()\nКонецПроцедуры\n#КонецЕсли"),
        ));
    }

    if mobile_vars {
        modules.push(("MobileBranch".into(), mobile_source));
        modules.push(("WrappedVarList".into(), wrapped_var_source));
        modules.push((
            "BadWrappedVarDirective".into(),
            "&НаКлиенте\nПерем A,\n#Если Сервер Тогда\n B;\n#КонецЕсли".into(),
        ));
        modules.push((
            "BadMobileUnknown".into(),
            "#Если МобильноеПриложениеФон Тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли".into(),
        ));
    }

    if method_regions {
        modules.push(("MethodRegion".into(), method_region_source));
        modules.push((
            "BadRegionLeak".into(),
            "Процедура P()\n#Область Local\nКонецПроцедуры\n#КонецОбласти".into(),
        ));
        modules.push(("BadRegionArm".into(), "Процедура P()\n#Если Сервер Тогда\n#Область Local\n#Иначе\n#КонецОбласти\n#КонецЕсли\nКонецПроцедуры".into()));
    }

    if header_forms {
        modules.push(("NullHeader".into(), null_header_method.into()));
        modules.push(("WrappedAsync".into(), wrapped_async_source));
        modules.push((
            "BadWrappedRealDirective".into(),
            "Асинх Процедура P(A, // comment\n#Если Сервер Тогда\nB)\nКонецПроцедуры\n#КонецЕсли"
                .into(),
        ));
        modules.push((
            "BadNullExpression".into(),
            "Процедура P(A=Null+1)\nКонецПроцедуры".into(),
        ));
    }

    if lexical_tail {
        let token_lines = format!("{}\n", "X=1;".repeat(25)).repeat(1100);
        let heavy_module = format!(
            "//{}\nПроцедура Filler()\n{token_lines}КонецПроцедуры\n{token_method}",
            "x".repeat(2 * 1024 * 1024)
        );
        modules.push(("ChainedMember".into(), chained_method.into()));
        modules.push(("LargeTokens".into(), heavy_module));
        modules.push((
            "BadChainedTail".into(),
            "Процедура P()\nA.B.Процедура.Field.More = V;\nКонецПроцедуры".into(),
        ));
    }
    if long_headers {
        modules.push(("LongOrdinary".into(), long_ordinary.clone()));
        modules.push((
            "BadLongAsync".into(),
            format!("Асинх Процедура P(\r\n{long_ordinary_params} Z)\r\nКонецПроцедуры"),
        ));
        modules.push((
            "BadLongDefault".into(),
            format!(
                "Процедура P(\r\n{long_ordinary_params} Z=\"{}\")\r\nКонецПроцедуры",
                "x".repeat(128)
            ),
        ));
    }
    if source_limits {
        modules.push(("DashedBody".into(), dashed_method.into()));
        modules.push(("CommentedClient".into(), comment_source));
        modules.push((
            "LargeModule".into(),
            format!("//{}\n{large_method}", "x".repeat(1024 * 1024)),
        ));
        modules.push((
            "BadCommentedBranch".into(),
            "&НаКлиенте\n// comment\n#Если Сервер Тогда\nПроцедура P()\nКонецПроцедуры\n#КонецЕсли"
                .into(),
        ));
        modules.push((
            "BadDashedBody".into(),
            "Процедура P()\nX='2024/02/29';\nКонецПроцедуры".into(),
        ));
    }

    if source_anomalies {
        modules.push(("AnomalyCr".into(), anomaly_cr.into()));
        modules.push(("AnomalyBlank".into(), anomaly_blank.into()));
        modules.push(("AnomalySemicolon".into(), anomaly_semicolon.into()));
        modules.push((
            "BadAnomalyDivergence".into(),
            "Процедура P()\nX=1;\r\tКонецПроцедуры".into(),
        ));
        modules.push((
            "BadAnomalyDoubleGap".into(),
            "Процедура P()\nX=\"open\n\n\n|end\";\nКонецПроцедуры".into(),
        ));
    }
    if source_anomalies && lexical_tail {
        modules.push(("CompoundAgreement".into(), compound_agreement.into()));
    }
    let refs = modules
        .iter()
        .map(|(n, _)| format!("<CommonModule>{n}</CommonModule>"))
        .collect::<String>();
    let command_parents = [
        ("Catalog", "Catalogs", "Shared"),
        ("Document", "Documents", "Shared"),
        ("InformationRegister", "InformationRegisters", "Shared"),
        ("DocumentJournal", "DocumentJournals", "Legacy"),
    ];
    let command_xml = |kind: &str, name: &str, id: usize| {
        format!("<{kind} uuid=\"a2000000-0000-0000-0000-{id:012}\"><Properties><Name>{name}</Name></Properties></{kind}>")
    };
    let command_body = "Процедура Run()\r\nКонецПроцедуры";
    let orphan_names = ["Undeclared", "caseonly", "OnlyAttribute"];
    let mut command_refs = String::new();
    if command_owners {
        for (i, (kind, folder, name)) in command_parents.iter().copied().enumerate() {
            command_refs.push_str(&format!("<{kind}>{name}</{kind}>"));
            let mut children = command_xml("Command", "Exact", 910 + i);
            if kind == "Catalog" {
                children.push_str(&command_xml("Command", "CaseOnly", 920));
                children.push_str(&command_xml("Command", "Absent", 921));
                children.push_str(&command_xml("Attribute", "OnlyAttribute", 922));
            }
            write(
                &format!("{folder}/{name}.xml"),
                xml(
                    kind,
                    name,
                    930 + i,
                    &format!("<ChildObjects>{children}</ChildObjects>"),
                )
                .as_bytes(),
            );
            write(
                &format!("{folder}/{name}/Commands/Exact/Ext/CommandModule.bsl"),
                command_body.as_bytes(),
            );
        }
        for orphan in orphan_names {
            write(
                &format!("Catalogs/Shared/Commands/{orphan}/Ext/CommandModule.bsl"),
                command_body.as_bytes(),
            );
        }
    }
    write(
        "Configuration.xml",
        xml(
            "Configuration",
            "Synthetic",
            1,
            &format!("<ChildObjects>{refs}{command_refs}<Catalog>Owner</Catalog></ChildObjects>"),
        )
        .as_bytes(),
    );
    write(
        "Catalogs/Owner.xml",
        xml("Catalog", "Owner", 900, "").as_bytes(),
    );
    write(
        "Catalogs/Owner/Ext/Help.xml",
        b"<opaque>&notParsed;</opaque>",
    );
    write("Catalogs/Owner/Ext/Help/ru.html", b"excluded");
    for (i, (n, b)) in modules.iter().enumerate() {
        write(
            &format!("CommonModules/{n}.xml"),
            xml("CommonModule", n, i + 2, "").as_bytes(),
        );
        write(&format!("CommonModules/{n}/Ext/Module.bsl"), b.as_bytes());
    }
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@invalid",
        "commit",
        "-qm",
        "context annotations",
    ]);
    let args = |cmd: &str, out: &str| {
        vec![
            cmd.into(),
            f.repo.to_str().unwrap().into(),
            git(&["rev-parse", "HEAD"]),
            git(&["rev-parse", "HEAD^{tree}"]),
            "synthetic:context".into(),
            f.root.join(out).to_str().unwrap().into(),
            "4000".into(),
            "8388608".into(),
            "33554432".into(),
            "134217728".into(),
            "10000".into(),
            "1000".into(),
            "10000".into(),
            "8388608".into(),
            "67108864".into(),
            "60000".into(),
            "100000".into(),
            "1048576".into(),
            "60000".into(),
            "[]".into(),
            "25165824".into(),
            "67108864".into(),
        ]
    };
    let run = |a: &[String]| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .args(a)
            .output()
            .unwrap()
    };
    let o = run(&args(build_cmd, "context-ok"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let built: Value = serde_json::from_slice(&o.stdout).unwrap();
    let previous_cmd = if command_owners {
        "build-navigator-agreement"
    } else if source_anomalies && lexical_tail {
        "build-lexical-tail"
    } else if source_anomalies {
        "build-source-limits"
    } else if lexical_tail {
        "build-long-headers"
    } else if long_headers {
        "build-source-limits"
    } else if source_limits {
        "build-header-forms"
    } else if header_forms {
        "build-method-regions"
    } else if method_regions {
        "build-mobile-vars"
    } else if mobile_vars {
        "build-async-casing"
    } else if async_casing {
        "build-dates-members"
    } else if dates_members {
        "build-annotated-vars"
    } else if annotated_vars {
        "build-corpus-refinements"
    } else if refined {
        "build-wrapped-headers"
    } else if wrapped {
        "build-literals"
    } else {
        "build-strings"
    };
    let o = run(&args(previous_cmd, "context-old"));
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let old: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_ne!(old["snapshot_id"], built["snapshot_id"]);
    let old_db = rusqlite::Connection::open(old["path"].as_str().unwrap()).unwrap();
    let refused_path = if command_owners {
        "CommonModules/BadAnomalyDivergence/Ext/Module.bsl"
    } else if source_anomalies {
        "CommonModules/AnomalyCr/Ext/Module.bsl"
    } else if lexical_tail {
        "CommonModules/ChainedMember/Ext/Module.bsl"
    } else if long_headers {
        "CommonModules/LongOrdinary/Ext/Module.bsl"
    } else if source_limits {
        "CommonModules/LargeModule/Ext/Module.bsl"
    } else if header_forms {
        "CommonModules/WrappedAsync/Ext/Module.bsl"
    } else if method_regions {
        "CommonModules/MethodRegion/Ext/Module.bsl"
    } else if mobile_vars {
        "CommonModules/WrappedVarList/Ext/Module.bsl"
    } else if async_casing {
        "CommonModules/AsyncCase/Ext/Module.bsl"
    } else if dates_members {
        "CommonModules/Dates/Ext/Module.bsl"
    } else if annotated_vars {
        "CommonModules/AnnotatedVars/Ext/Module.bsl"
    } else if refined {
        "CommonModules/Refined/Ext/Module.bsl"
    } else if wrapped {
        "CommonModules/Wrapped/Ext/Module.bsl"
    } else {
        "CommonModules/Accepted/Ext/Module.bsl"
    };
    assert_eq!(
        old_db
            .query_row(
                "SELECT parse_status FROM modules WHERE path=?1",
                [refused_path],
                |r| r.get::<_, String>(0)
            )
            .unwrap(),
        "unparsed"
    );
    assert_eq!(
        old_db
            .query_row(
                "SELECT count(*) FROM methods WHERE module_path=?1",
                [refused_path],
                |r| r.get::<_, usize>(0)
            )
            .unwrap(),
        0
    );
    if refined {
        let expected = if annotated_vars {
            "accepted"
        } else {
            "unparsed"
        };
        assert_eq!(
            old_db.query_row(
                "SELECT parse_status FROM modules WHERE path='CommonModules/WrappedDefault/Ext/Module.bsl'",
                [],
                |r| r.get::<_, String>(0)
            ).unwrap(),
            expected
        );
    }
    if command_owners {
        let previous_owner = |path: &str| {
            old_db
                .query_row("SELECT owner_id FROM modules WHERE path=?1", [path], |r| {
                    r.get::<_, Option<i64>>(0)
                })
                .unwrap()
        };
        for (_, folder, name) in command_parents {
            assert_eq!(
                previous_owner(&format!(
                    "{folder}/{name}/Commands/Exact/Ext/CommandModule.bsl"
                ))
                .is_some(),
                folder == "DocumentJournals"
            );
        }
        assert_eq!(
            old_db
                .query_row("SELECT count(*) FROM bsl_source_anomalies", [], |r| r
                    .get::<_, u64>(0))
                .unwrap(),
            5
        );
    } else if source_anomalies {
        for another in [
            "CommonModules/AnomalyBlank/Ext/Module.bsl",
            "CommonModules/AnomalySemicolon/Ext/Module.bsl",
            "CommonModules/InheritedBlank/Ext/Module.bsl",
        ] {
            assert_eq!(
                old_db
                    .query_row(
                        "SELECT parse_status FROM modules WHERE path=?1",
                        [another],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                "unparsed"
            );
        }
    } else if source_limits {
        let previous_status = if long_headers { "accepted" } else { "unparsed" };
        for earlier_refusal in [
            "CommonModules/DashedBody/Ext/Module.bsl",
            "CommonModules/CommentedClient/Ext/Module.bsl",
        ] {
            assert_eq!(
                old_db
                    .query_row(
                        "SELECT parse_status FROM modules WHERE path=?1",
                        [earlier_refusal],
                        |r| r.get::<_, String>(0)
                    )
                    .unwrap(),
                previous_status
            );
        }
    } else if header_forms {
        assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/NullHeader/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"unparsed");
        assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/MethodRegion/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"accepted");
    } else if method_regions {
        assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/MobileBranch/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"accepted");
        assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/WrappedVarList/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"accepted");
    } else if mobile_vars {
        assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/MobileBranch/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"unparsed");
        assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/AsyncCase/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"accepted");
    } else if async_casing {
        assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/Folded/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"unparsed");
        assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/Member/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"accepted");
    } else if dates_members {
        assert_eq!(old_db.query_row("SELECT parse_status FROM modules WHERE path='CommonModules/Member/Ext/Module.bsl'",[],|r|r.get::<_,String>(0)).unwrap(),"unparsed");
    }
    drop(old_db);
    if source_anomalies && lexical_tail {
        let pure = run(&args("build-source-anomalies", "context-anomaly-only"));
        assert!(
            pure.status.success(),
            "{}",
            String::from_utf8_lossy(&pure.stderr)
        );
        let info: Value = serde_json::from_slice(&pure.stdout).unwrap();
        assert_ne!(info["snapshot_id"], built["snapshot_id"]);
        let db = rusqlite::Connection::open(info["path"].as_str().unwrap()).unwrap();
        let status = |name: &str| {
            db.query_row(
                "SELECT parse_status FROM modules WHERE path=?1",
                [format!("CommonModules/{name}/Ext/Module.bsl")],
                |r| r.get::<_, String>(0),
            )
            .unwrap()
        };
        assert_eq!(status("CompoundAgreement"), "unparsed");
        assert_eq!(status("AnomalySemicolon"), "accepted");
    }
    let o = run(&args(build_cmd, "context-repeat"));
    assert!(o.status.success());
    let repeat: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(repeat["snapshot_id"], built["snapshot_id"]);
    assert_eq!(repeat["facts_sha256"], built["facts_sha256"]);
    let mut refused = args(build_cmd, "parameters-rollback");
    refused[16] = "1".into();
    let failed = run(&refused);
    assert!(!failed.status.success());
    assert!(failed.stdout.is_empty());
    assert_eq!(
        fs::read_dir(f.root.join("parameters-rollback"))
            .unwrap()
            .count(),
        0
    );
    if command_owners {
        let path = f.repo.join("Catalogs/Shared.xml");
        let original = fs::read_to_string(&path).unwrap();
        let duplicate = command_xml("Command", "Exact", 9999);
        fs::write(
            &path,
            original.replace("</ChildObjects>", &format!("{duplicate}</ChildObjects>")),
        )
        .unwrap();
        git(&["add", "-A"]);
        git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@invalid",
            "commit",
            "-qm",
            "duplicate inline declaration",
        ]);
        let rejected = args(build_cmd, "duplicate-command");
        let failed = run(&rejected);
        assert!(!failed.status.success());
        assert!(String::from_utf8_lossy(&failed.stderr).contains("Duplicate"));
        assert_eq!(fs::read_dir(&rejected[5]).unwrap().count(), 0);
    }
    fs::remove_dir_all(&f.repo).unwrap();
    f.built = BuiltSnapshot {
        path: PathBuf::from(built["path"].as_str().unwrap()),
        snapshot_id: built["snapshot_id"].as_str().unwrap().into(),
        file_sha256: built["file_sha256"].as_str().unwrap().into(),
        facts_sha256: built["facts_sha256"].as_str().unwrap().into(),
        reused: false,
    };
    f.read_limit = "134217728";
    let new_snapshot = std::mem::replace(
        &mut f.built,
        BuiltSnapshot {
            path: PathBuf::from(old["path"].as_str().unwrap()),
            snapshot_id: old["snapshot_id"].as_str().unwrap().into(),
            file_sha256: old["file_sha256"].as_str().unwrap().into(),
            facts_sha256: old["facts_sha256"].as_str().unwrap().into(),
            reused: false,
        },
    );
    let mut prior_peer = Peer::start(&f);
    let previous = prior_peer.data(
        "tree",
        json!({"level":"module","module_path":"CommonModules/Plain/Ext/Module.bsl"}),
    );
    assert_eq!(previous["data"]["branch"]["status"], "accepted");
    assert_eq!(previous["data"]["items"].as_array().unwrap().len(), 2);
    let refused = prior_peer.data("tree", json!({"level":"module","module_path":refused_path}));
    assert_eq!(refused["data"]["branch"]["status"], "unparsed");
    assert!(refused["data"]["items"].as_array().unwrap().is_empty());
    prior_peer.finish();
    let verify = |f: &Fixture| {
        Command::new(env!("CARGO_BIN_EXE_bslens-v5"))
            .env("PATH", "")
            .args([
                "verify",
                f.built.path.to_str().unwrap(),
                "--staging-dir",
                f.stage.to_str().unwrap(),
                "--max-sqlite-bytes",
                "134217728",
                "--expected-sha256",
                &f.built.file_sha256,
                "--max-sources",
                "4000",
                "--max-source-bytes",
                "8388608",
                "--max-total-bytes",
                "33554432",
                "--max-scratch-bytes",
                "134217728",
                "--max-facts",
                "100000",
                "--max-coverage-bytes",
                "1048576",
                "--facts-time-ms",
                "60000",
                "--max-opaque-bin-bytes",
                "25165824",
                "--max-diagnostics-bytes",
                "67108864",
            ])
            .output()
            .unwrap()
    };
    if command_owners {
        let previous = verify(&f);
        assert!(
            previous.status.success(),
            "{}",
            String::from_utf8_lossy(&previous.stderr)
        );
    }
    f.built = new_snapshot;
    let o = verify(&f);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    let db = rusqlite::Connection::open(&f.built.path).unwrap();
    assert_eq!(
        db.query_row("SELECT lexical_version FROM metadata", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        guard
    );
    let cov: String = db
        .query_row("SELECT coverage_draft FROM metadata", [], |r| r.get(0))
        .unwrap();
    let cov: Value = serde_json::from_str(&cov).unwrap();
    assert_eq!(
        cov["parse_counts"]["accepted"],
        if command_owners {
            28
        } else if lexical_tail {
            21
        } else if long_headers {
            19
        } else if source_limits {
            18
        } else if header_forms {
            15
        } else if method_regions {
            13
        } else if mobile_vars {
            12
        } else if async_casing {
            10
        } else if dates_members {
            8
        } else if annotated_vars {
            6
        } else if refined {
            5
        } else if wrapped {
            3
        } else {
            2
        }
    );
    assert!(cov["xml_diagnostics"]["count"].as_u64().unwrap() > 0);
    let mut peer = Peer::start(&f);
    assert_eq!(
        peer.data("metadata", json!({}))["snapshot"]["lexical_guard"],
        guard
    );

    if source_anomalies {
        let expected_qualified = if lexical_tail { 5 } else { 4 };
        assert_eq!(
            cov["parse_counts"]["parsed_with_source_anomalies"],
            expected_qualified
        );
        let summary = peer.data("metadata", json!({}));
        assert_eq!(
            summary["snapshot"]["coverage"]["accepted_modules"],
            if command_owners {
                28
            } else if lexical_tail {
                21
            } else {
                18
            }
        );
        assert_eq!(
            summary["snapshot"]["coverage"]["parsed_with_source_anomalies_modules"],
            expected_qualified
        );
        assert_eq!(summary["snapshot"]["compiler_validation"], "not_performed");
    }
    if command_owners {
        let config = peer.data("tree", json!({"level":"configuration"}));
        for (i, (kind, folder, name)) in command_parents.iter().copied().enumerate() {
            let group = config["data"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["key"] == kind)
                .unwrap();
            let objects = peer.data("tree", group["children"].clone());
            let object = objects["data"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["name"] == name)
                .unwrap();
            let id = object["key"].as_str().unwrap().parse::<i64>().unwrap();
            let object_tree = peer.data("tree", object["children"].clone());
            let inline_group = object_tree["data"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["type"] == "inline_group")
                .unwrap();
            let inline = peer.data("tree", inline_group["children"].clone());
            let items = inline["data"]["items"].as_array().unwrap();
            let exact = items
                .iter()
                .find(|v| v["kind"] == "Command" && v["name"] == "Exact")
                .unwrap();
            let module = format!("{folder}/{name}/Commands/Exact/Ext/CommandModule.bsl");
            assert_eq!(exact["owner_id"], id);
            assert!(exact["parent_key"].is_null());
            assert_eq!(exact["path"], format!("{folder}/{name}.xml"));
            assert_eq!(exact["module_path"], module);
            assert_eq!(
                bytes(&peer.data("source", exact["source"].clone())["data"]),
                command_xml("Command", "Exact", 910 + i).as_bytes()
            );
            let listed = object_tree["data"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|v| v["key"] == module)
                .unwrap();
            assert_eq!(listed["owner_declaration"], exact["key"]);
            let methods = peer.data("tree", exact["children"].clone());
            assert_eq!(methods["data"]["branch"]["owner_declaration"], exact["key"]);
            assert_eq!(methods["data"]["branch"]["container_object_id"], id);
            let method = &methods["data"]["items"][0];
            assert_eq!(method["name"], "run");
            assert_eq!(
                bytes(&peer.data("source", method["source"].clone())["data"]),
                command_body.as_bytes()
            );
            if kind == "Catalog" {
                for name in ["CaseOnly", "Absent", "OnlyAttribute"] {
                    let no_link = items.iter().find(|v| v["name"] == name).unwrap();
                    assert!(no_link["children"].is_null());
                    assert!(no_link["module_path"].is_null());
                }
            }
        }
        let unknown = peer.data("tree", json!({"level":"unconfirmed"}));
        for orphan in orphan_names {
            let path = format!("Catalogs/Shared/Commands/{orphan}/Ext/CommandModule.bsl");
            assert!(unknown["data"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v["key"] == path));
        }
        assert_eq!(
            db.query_row("SELECT count(*) FROM inline_module_owners", [], |r| r
                .get::<_, u64>(0))
                .unwrap(),
            4
        );
    }
    for (n, body) in &modules {
        let path = format!("CommonModules/{n}/Ext/Module.bsl");
        let id: i64 = db
            .query_row(
                "SELECT object_id FROM objects WHERE xml_path=?1",
                [format!("CommonModules/{n}.xml")],
                |r| r.get(0),
            )
            .unwrap();
        let tree = peer.data("tree", json!({"level":"object","parent_id":id}));
        let m = tree["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["type"] == "module")
            .unwrap();
        let mut query = m["children"].clone();
        if n == "Accepted" {
            let all = peer.data("tree", query.clone());
            assert_eq!(all["data"]["items"].as_array().unwrap().len(), 4);
            let mut long_query = query.clone();
            long_query["name_exact"] = json!(long_name.to_lowercase());
            let long_method = peer.data("tree", long_query);
            assert_eq!(long_method["data"]["items"].as_array().unwrap().len(), 1);
            assert_eq!(
                long_method["data"]["items"][0]["location"]["exported"],
                true
            );
            assert_eq!(
                bytes(
                    &peer.data("source", long_method["data"]["items"][0]["source"].clone())["data"]
                ),
                format!("Процедура {long_name}() Export\r\nКонецПроцедуры").as_bytes()
            );
            query["name_exact"] = json!("первый");
            let ambiguous = peer.data("tree", query.clone());
            assert_eq!(ambiguous["data"]["branch"]["match_status"], "ambiguous");
            assert_eq!(ambiguous["data"]["items"].as_array().unwrap().len(), 2);
            for item in ambiguous["data"]["items"].as_array().unwrap() {
                assert_eq!(item["location"]["exported"], false);
            }
            query["name_exact"] = json!("второй");
            let one = peer.data("tree", query.clone());
            let method = &one["data"]["items"][0];
            assert_eq!(method["location"]["exported"], true);
            let exported: bool = db
                .query_row(
                    "SELECT exported FROM methods WHERE name='второй'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert!(exported);
            assert_eq!(
                method["location"]["conditional_evaluation"],
                "not_evaluated"
            );
            assert_eq!(
                method["location"]["declaration_branch_path"][0]["block_line"],
                1
            );
            assert_eq!(method["location"]["declaration_branch_path"][0]["arm"], 1);
            assert_eq!(
                method["location"]["declaration_branch_path"][0]["kind"],
                "elseif"
            );
            assert_eq!(
                bytes(&peer.data("source", method["source"].clone())["data"]),
                "Функция Второй(A=Неопределено, Val Б=-0.125, В=Истина, Г=\"), \"\"quotes\"\" // #Если\") Экспорт ;\r\nКонецФункции\r\n".as_bytes()
            );
            assert_eq!(
                db.query_row(
                    "SELECT start_line FROM methods WHERE name='второй'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
                18
            );
        }
        if n == "Wrapped" {
            query["name_exact"] = json!("перенос");
            let found = peer.data("tree", query.clone());
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item["location"]["start_line"], 2);
            assert_eq!(item["location"]["end_line"], 4);
            assert_eq!(item["location"]["exported"], false);
            assert_eq!(
                item["location"]["declaration_branch_path"][0]["block_line"],
                1
            );
            assert_eq!(item["location"]["declaration_branch_path"][0]["arm"], 0);
            assert_eq!(
                bytes(&peer.data("source", item["source"].clone())["data"]),
                wrapped_method.as_bytes()
            );
        }
        if n == "Refined" {
            query["name_exact"] = json!("длинный");
            let found = peer.data("tree", query.clone());
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item["location"]["start_line"], 2);
            assert_eq!(item["location"]["end_line"], 5);
            assert_eq!(item["location"]["exported"], true);
            assert_eq!(
                item["location"]["declaration_branch_path"][0]["block_line"],
                1
            );
            assert_eq!(item["location"]["declaration_branch_path"][0]["arm"], 0);
            assert_eq!(
                bytes(&peer.data("source", item["source"].clone())["data"]),
                refined_method.as_bytes()
            );
        }
        if n == "AnnotatedVars" {
            let found = peer.data("tree", query.clone());
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item["location"]["start_line"], 6);
            assert_eq!(item["location"]["end_line"], 7);
            assert_eq!(
                item["location"]["declaration_branch_path"][0]["block_line"],
                1
            );
            assert_eq!(
                bytes(&peer.data("source", item["source"].clone())["data"]),
                after_variables.as_bytes()
            );
        }

        if n == "Dates" || n == "Member" {
            let found = peer.data("tree", query.clone());
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            if n == "Dates" {
                assert_eq!(item["location"]["start_line"], 2);
                assert_eq!(item["location"]["end_line"], 5);
                assert_eq!(item["location"]["exported"], true);
                assert_eq!(
                    item["location"]["declaration_branch_path"][0]["block_line"],
                    1
                );
            } else {
                assert_eq!(item["location"]["start_line"], 1);
                assert_eq!(item["location"]["end_line"], 3);
                assert_eq!(item["location"]["exported"], false);
            }
            // MCP quotes full physical lines, unlike the lexical token range.
            // Dates has a CRLF before #КонецЕсли; Member ends at EOF.
            let expected = if n == "Dates" {
                format!("{date_method}\r\n").into_bytes()
            } else {
                member_method.as_bytes().to_vec()
            };
            let quoted = peer.data("source", item["source"].clone());
            assert_eq!(bytes(&quoted["data"]), expected);
        }

        if n == "AsyncCase" || n == "Folded" {
            let found = peer.data("tree", query.clone());
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item["location"]["start_line"], 2);
            let expected = if n == "AsyncCase" {
                assert_eq!(item["location"]["end_line"], 4);
                assert_eq!(item["location"]["exported"], true);
                async_method
            } else {
                assert_eq!(item["location"]["end_line"], 3);
                assert_eq!(
                    item["location"]["declaration_branch_path"][0]["block_line"],
                    1
                );
                folded_method
            };
            assert_eq!(
                bytes(&peer.data("source", item["source"].clone())["data"]),
                expected.as_bytes()
            );
        }

        if n == "WrappedVarList" || n == "MobileBranch" {
            let found = peer.data("tree", query.clone());
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            let expected = if n == "WrappedVarList" {
                assert_eq!(item["location"]["start_line"], 5);
                assert_eq!(item["location"]["end_line"], 6);
                wrapped_var_method
            } else {
                assert_eq!(item["location"]["start_line"], 3);
                assert_eq!(item["location"]["end_line"], 4);
                let evidence = &item["location"]["declaration_branch_path"][0];
                assert_eq!(evidence["block_line"], 1);
                assert_eq!(evidence["arm_line"], 2);
                assert_eq!(evidence["arm"], 1);
                mobile_method
            };
            assert_eq!(
                bytes(&peer.data("source", item["source"].clone())["data"]),
                expected.as_bytes()
            );
        }

        if n == "MethodRegion" {
            let found = peer.data("tree", query.clone());
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item["location"]["start_line"], 3);
            assert_eq!(item["location"]["end_line"], 7);
            assert_eq!(
                item["location"]["declaration_branch_path"][0]["block_line"],
                2
            );
            assert_eq!(
                bytes(&peer.data("source", item["source"].clone())["data"]),
                method_region_quote.as_bytes()
            );
        }

        if n == "NullHeader" || n == "WrappedAsync" {
            let found = peer.data("tree", query.clone());
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            let expected = if n == "WrappedAsync" {
                assert_eq!(item["location"]["start_line"], 2);
                assert_eq!(item["location"]["end_line"], 5);
                assert_eq!(item["location"]["exported"], true);
                wrapped_async_quote
            } else {
                assert_eq!(item["location"]["start_line"], 1);
                assert_eq!(item["location"]["end_line"], 2);
                assert_eq!(item["location"]["exported"], false);
                null_header_method
            };
            assert_eq!(
                bytes(&peer.data("source", item["source"].clone())["data"]),
                expected.as_bytes()
            );
        }

        if n == "ChainedMember" || n == "LargeTokens" {
            let mut named = query.clone();
            let (name, first, last, expected) = if n == "ChainedMember" {
                ("chain", 1, 3, chained_method)
            } else {
                ("afterheavytokens", 1104, 1105, token_method)
            };
            named["name_exact"] = json!(name);
            let found = peer.data("tree", named);
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item["location"]["start_line"], first);
            assert_eq!(item["location"]["end_line"], last);
            assert_eq!(
                bytes(&peer.data("source", item["source"].clone())["data"]),
                expected.as_bytes()
            );
        }

        if n == "LongOrdinary" {
            let mut named = query.clone();
            named["name_exact"] = json!("longordinary");
            let found = peer.data("tree", named);
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item["location"]["start_line"], 1);
            assert_eq!(item["location"]["end_line"], 21);
            assert_eq!(item["location"]["exported"], true);
            assert_eq!(
                bytes(&peer.data("source", item["source"].clone())["data"]),
                long_ordinary.as_bytes()
            );
        }

        if n == "DashedBody" || n == "CommentedClient" || n == "LargeModule" {
            let found = peer.data("tree", query.clone());
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            let (first, last, expected) = match n.as_str() {
                "DashedBody" => (1, 3, dashed_method),
                "CommentedClient" => (3, 4, comment_method),
                _ => (2, 3, large_method),
            };
            assert_eq!(item["location"]["start_line"], first);
            assert_eq!(item["location"]["end_line"], last);
            assert_eq!(
                bytes(&peer.data("source", item["source"].clone())["data"]),
                expected.as_bytes()
            );
        }

        if n == "CompoundAgreement" {
            assert_eq!(m["structural_status"], "parsed_with_source_anomalies");
            query["name_exact"] = json!("compound");
            let found = peer.data("tree", query.clone());
            assert_eq!(found["data"]["branch"]["match_status"], "matched");
            assert_eq!(
                found["data"]["branch"]["source_anomaly"]["code"],
                "method_terminal_semicolon"
            );
            let item = &found["data"]["items"][0];
            assert_eq!(item["location"]["start_line"], 1);
            assert_eq!(item["location"]["end_line"], 3);
            assert_eq!(
                bytes(&peer.data("source", item["source"].clone())["data"]),
                compound_agreement.as_bytes()
            );
        }
        if n == "InheritedBlank" {
            assert_eq!(m["structural_status"], "parsed_with_source_anomalies");
            let all = peer.data("tree", query.clone());
            let names = all["data"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v["name"].as_str().unwrap())
                .collect::<Vec<_>>();
            assert_eq!(names, ["before", "p"]);
            assert_eq!(
                all["data"]["branch"]["source_anomaly"]["code"],
                "empty_opaque_physical_line"
            );
            assert_eq!(all["data"]["branch"]["source_anomaly"]["physical_line"], 5);
            query["name_exact"] = json!("p");
            let found = peer.data("tree", query.clone());
            let item = &found["data"]["items"][0];
            assert_eq!(item["location"]["start_line"], 3);
            assert_eq!(item["location"]["end_line"], 7);
            let expected = &body[body.find("Процедура P()").unwrap()..];
            let quote = peer.data("source", item["source"].clone());
            assert_eq!(
                quote["data"]["structural_status"],
                "parsed_with_source_anomalies"
            );
            assert_eq!(bytes(&quote["data"]), expected.as_bytes());
        }
        if n.starts_with("Anomaly") {
            assert_eq!(m["structural_status"], "parsed_with_source_anomalies");
            let (needle, code, line, raw_site) = match n.as_str() {
                "AnomalyCr" => ("crsource", "lone_cr_structural_agreement", 2, "\r"),
                "AnomalyBlank" => ("blanksource", "empty_opaque_physical_line", 3, "\r\n"),
                _ => ("semicolonsource", "method_terminal_semicolon", 2, ";"),
            };
            query["name_exact"] = json!(needle);
            let found = peer.data("tree", query.clone());
            let branch = &found["data"]["branch"];
            assert_eq!(branch["status"], "accepted");
            assert_eq!(branch["structural_status"], "parsed_with_source_anomalies");
            assert_eq!(branch["match_status"], "matched");
            assert_eq!(branch["compiler_validation"], "not_performed");
            let site = &branch["source_anomaly"];
            assert_eq!(site["code"], code);
            assert_eq!(site["physical_line"], line);
            let a = site["start_byte"].as_u64().unwrap() as usize;
            let b = site["end_byte"].as_u64().unwrap() as usize;
            assert_eq!(&body.as_bytes()[a..b], raw_site.as_bytes());
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item["structural_status"], "parsed_with_source_anomalies");
            let quote = peer.data("source", item["source"].clone());
            assert_eq!(
                quote["data"]["structural_status"],
                "parsed_with_source_anomalies"
            );
            assert_eq!(quote["data"]["source_anomaly"], *site);
            assert_eq!(bytes(&quote["data"]), body.as_bytes());
            if n == "AnomalySemicolon" {
                assert_eq!(item["location"]["end_byte"], site["start_byte"]);
            }
            query["name_exact"] = json!("missing_name");
            let miss = peer.data("tree", query.clone());
            assert_eq!(miss["data"]["branch"]["match_status"], "no_match");
            assert_eq!(
                miss["data"]["branch"]["structural_status"],
                "parsed_with_source_anomalies"
            );
            assert!(miss["data"]["items"].as_array().unwrap().is_empty());
        }
        if n == "WrappedDefault" {
            let all = peer.data("tree", query.clone());
            assert_eq!(all["data"]["items"].as_array().unwrap().len(), 2);
            query["name_exact"] = json!("bad");
            let found = peer.data("tree", query.clone());
            let items = found["data"]["items"].as_array().unwrap();
            assert_eq!(items.len(), 1);
            let item = &items[0];
            assert_eq!(item["location"]["start_line"], 3);
            assert_eq!(item["location"]["end_line"], 5);
            assert_eq!(item["location"]["exported"], false);
            assert_eq!(
                bytes(&peer.data("source", item["source"].clone())["data"]),
                "Процедура Bad(A=0,\n B)\nКонецПроцедуры".as_bytes()
            );
        }
        if n.starts_with("Bad") {
            let refused = peer.data("tree", query.clone());
            assert!(refused.to_string().contains("unparsed"));
            assert_eq!(
                db.query_row(
                    "SELECT parse_status FROM modules WHERE path=?1",
                    [&path],
                    |r| r.get::<_, String>(0)
                )
                .unwrap(),
                "unparsed"
            );
            assert_eq!(
                db.query_row(
                    "SELECT count(*) FROM methods WHERE module_path=?1",
                    [&path],
                    |r| r.get::<_, usize>(0)
                )
                .unwrap(),
                0
            );
        }
        // A full module can exceed the independently unchanged MCP quote cap.
        // Its named method was checked above; here check a bounded raw slice.
        let raw_end = if n == "LargeModule" || n == "LargeTokens" {
            32
        } else {
            body.len()
        };
        assert_eq!(
            bytes(
                &peer.data(
                    "source",
                    json!({"saved_path":path,"start_byte":0,"end_byte":raw_end})
                )["data"]
            ),
            &body.as_bytes()[..raw_end]
        );
    }
    peer.finish();
    if command_owners {
        let wrong_key: String = db.query_row(
            "SELECT declaration_key FROM inline_declarations WHERE xml_path='Catalogs/Shared.xml' AND kind='Attribute'",
            [], |r|r.get(0),
        ).unwrap();
        assert_eq!(db.execute("UPDATE inline_module_owners SET declaration_key=?1 WHERE module_path='Catalogs/Shared/Commands/Exact/Ext/CommandModule.bsl'",[wrong_key]).unwrap(), 1);
    } else if source_anomalies {
        assert_eq!(db.execute("UPDATE bsl_source_anomalies SET code='lone_cr_structural_agreement' WHERE module_path='CommonModules/AnomalySemicolon/Ext/Module.bsl'",[]).unwrap(), 1);
    } else {
        db.execute("UPDATE method_branches SET branch_path='[]' WHERE method_id=(SELECT method_id FROM methods WHERE name='второй')", [])
            .unwrap();
    }
    let digest = bslens_core::snapshot_v5::contract::facts_sha256(&db).unwrap();
    db.execute("UPDATE metadata SET facts_sha256=?1", [digest])
        .unwrap();
    drop(db);
    f.alter("");
    assert!(f.command().output().unwrap().status.success());
    assert!(String::from_utf8_lossy(&verify(&f).stderr).contains("VerifyFacts"));
    assert_eq!(fs::read_dir(&f.stage).unwrap().count(), 0);
}
