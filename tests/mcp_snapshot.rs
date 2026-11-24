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
struct Fixture {
    root: PathBuf,
    repo: PathBuf,
    stage: PathBuf,
    built: BuiltSnapshot,
    old: GitStructure,
}
impl Fixture {
    fn new(extra: &[(&str, Vec<u8>)]) -> Self {
        Self::with_ingest(extra, false)
    }
    fn with_ingest(extra: &[(&str, Vec<u8>)], ingest_mode: bool) -> Self {
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

        Self {
            root,
            repo,
            stage,
            built,
            old,
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
            "4000000",
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
