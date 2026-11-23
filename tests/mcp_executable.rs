#![cfg(all(feature = "experimental-router", target_os = "linux"))]
use bslens_core::mcp_transport::{MAX_BODY, PROTOCOL};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    process::{Child, ChildStdin, Command, ExitStatus, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    path: PathBuf,
    sha: String,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "r06c-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        let mut f = Self {
            path,
            sha: String::new(),
        };
        f.git(&["init", "-q"]);
        fs::write(f.path.join("Configuration.xml"),"<MetaDataObject xmlns='http://v8.1c.ru/8.3/MDClasses' version='2.21'><Configuration><Properties><Name>Root</Name></Properties><ChildObjects/></Configuration></MetaDataObject>").unwrap();
        fs::create_dir(f.path.join("Ext")).unwrap();
        fs::write(
            f.path.join("Ext/SessionModule.bsl"),
            "\u{feff}// 😀\r\nПроцедура Тест()\r\nКонецПроцедуры\r\n",
        )
        .unwrap();
        f.git(&["add", "."]);
        f.git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@invalid",
            "commit",
            "-qm",
            "fixture",
        ]);
        f.sha = f.git(&["rev-parse", "HEAD"]);
        f
    }
    fn git(&self, args: &[&str]) -> String {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&self.path)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(o.status.success());
        String::from_utf8(o.stdout).unwrap().trim().into()
    }
    fn command(&self) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_bslens-mcp"));
        c.args(["--repo", self.path.to_str().unwrap(), "--commit", &self.sha]);
        c
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
struct Peer {
    child: Child,
    input: Option<ChildStdin>,
    rx: mpsc::Receiver<Vec<u8>>,
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
        Self { child, input, rx }
    }
    fn raw(&mut self, data: &[u8]) {
        self.input.as_mut().unwrap().write_all(data).unwrap();
        self.input.as_mut().unwrap().flush().unwrap();
    }
    fn send(&mut self, v: Value) {
        self.raw(format!("{v}\n").as_bytes());
    }
    fn response(&self) -> Value {
        let line = self
            .rx
            .recv_timeout(Duration::from_secs(5))
            .expect("response timeout");
        assert!(line.len() <= MAX_BODY + 1);
        serde_json::from_slice(&line).unwrap()
    }
    fn finish(&mut self) -> (ExitStatus, String) {
        self.input.take();
        let deadline = Instant::now() + Duration::from_secs(5);
        let status = loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                break s;
            }
            assert!(Instant::now() < deadline, "exit timeout");
            thread::sleep(Duration::from_millis(10));
        };
        let mut err = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut err)
            .unwrap();
        (status, err)
    }
}
impl Drop for Peer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn request(id: u32, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn call(id: u32, name: &str, args: Value) -> Value {
    request(id, "tools/call", json!({"name":name,"arguments":args}))
}
#[test]
fn actual_process_tree_method_source_and_eof() {
    let f = Fixture::new();
    let before = fs::read(f.path.join("Ext/SessionModule.bsl")).unwrap();
    let mut p = Peer::start(&f);
    p.send(request(1,"initialize",json!({"protocolVersion":PROTOCOL,"capabilities":{},"clientInfo":{"name":"subprocess","version":"1"}})));
    assert!(p.response()["result"].is_object());
    p.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    p.send(request(2, "tools/list", json!({})));
    let listed = p.response()["result"].clone();
    assert_eq!(listed["tools"].as_array().unwrap().len(), 2);
    let base = json!({"commit":f.sha,"scope_id":listed["scope_id"]});
    p.send(call(3, "tree", base.clone()));
    let root = p.response()["result"]["structuredContent"].clone();
    assert_eq!(root["context"]["commit"], f.sha);
    let mut args = base;
    args["node_id"] = root["data"]["items"][0]["id"].clone();
    p.send(call(4, "tree", args));
    let methods = p.response()["result"]["structuredContent"].clone();
    assert_eq!(methods["data"]["items"][0]["kind"], "method");
    p.send(call(
        5,
        "source",
        methods["data"]["methods"][0]["source"].clone(),
    ));
    let source = p.response()["result"]["structuredContent"].clone();
    assert_eq!(
        source["data"]["text"],
        "Процедура Тест()\r\nКонецПроцедуры\r\n"
    );
    assert_eq!(source["scope_id"], listed["scope_id"]);
    p.send(request(6, "quit", json!({})));
    assert_eq!(p.response()["error"]["code"], -32601);
    let (status, err) = p.finish();
    assert!(status.success());
    assert!(err.is_empty());
    assert_eq!(
        fs::read(f.path.join("Ext/SessionModule.bsl")).unwrap(),
        before
    );
    println!("VERIFIED real subprocess initialize→tools/list→tree→method→source→EOF, UTF-8/CRLF exact, no startup stdout noise");
}
#[test]
fn actual_process_partial_and_oversize_close_nonzero() {
    let f = Fixture::new();
    for body in [b"{\"jsonrpc\"".to_vec(), vec![b'x'; MAX_BODY + 1]] {
        let mut p = Peer::start(&f);
        p.raw(&body);
        p.input.take();
        let v = p.response();
        assert_eq!(v["error"]["code"], -32600);
        let (status, err) = p.finish();
        assert!(!status.success());
        assert_eq!(err, "MCP frame rejected; session closed\n");
    }
}
#[test]
fn startup_errors_have_no_protocol_or_sensitive_output() {
    let f = Fixture::new();
    for args in [
        vec![],
        vec!["--repo", f.path.to_str().unwrap(), "--commit", "HEAD"],
        vec![
            "--repo",
            f.path.to_str().unwrap(),
            "--commit",
            &f.sha,
            "--repo",
            "other",
        ],
        vec!["--unknown"],
    ] {
        let o = Command::new(env!("CARGO_BIN_EXE_bslens-mcp"))
            .args(args)
            .output()
            .unwrap();
        assert!(!o.status.success());
        assert!(o.stdout.is_empty());
        let e = String::from_utf8(o.stderr).unwrap();
        assert!(!e.contains(f.path.to_str().unwrap()));
        assert!(!e.contains(&f.sha));
        assert!(!e.contains("Процедура"));
        assert!(e.len() < 100);
    }
}

#[test]
fn relative_repository_is_refused_before_opening_valid_context() {
    let f = Fixture::new();
    let o = Command::new(env!("CARGO_BIN_EXE_bslens-mcp"))
        .current_dir(&f.path)
        .args(["--repo", ".", "--commit", &f.sha])
        .output()
        .unwrap();
    assert!(!o.status.success());
    assert!(o.stdout.is_empty());
    assert_eq!(
        String::from_utf8(o.stderr).unwrap(),
        "Expected --repo ABSOLUTE_PATH --commit FULL_SHA\n"
    );
}

#[cfg(feature = "experimental-cache")]
fn cache_run(f: &Fixture, args: &[&str]) -> std::process::Output {
    let mut child = f
        .command()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input=concat!("{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":\"2025-11-25\",\"capabilities\":{},\"clientInfo\":{\"name\":\"cache-test\",\"version\":\"1\"}}}\n", "{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n", "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/list\"}\n");
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("cache CLI timeout")
        }
        thread::sleep(Duration::from_millis(2));
    }
    child.wait_with_output().unwrap()
}
#[cfg(feature = "experimental-cache")]
#[test]
fn opt_in_cache_miss_warm_corruption_rebuild_and_default_unchanged() {
    let f = Fixture::new();
    let dir = f.path.join("cache");
    let path = dir.to_str().unwrap();
    let plain = cache_run(&f, &[]);
    assert!(plain.status.success());
    assert!(!dir.exists());
    let cold = cache_run(&f, &["--cache-dir", path]);
    assert!(
        cold.status.success(),
        "{}",
        String::from_utf8_lossy(&cold.stderr)
    );
    assert_eq!(cold.stdout, plain.stdout);
    assert!(cold.stderr.is_empty());
    let cached = dir.join(format!("v1-{}.bslcache", f.sha));
    let bytes = fs::read(&cached).unwrap();
    let warm = cache_run(&f, &["--cache-dir", path]);
    assert!(warm.status.success());
    assert_eq!(warm.stdout, plain.stdout);
    assert_eq!(fs::read(&cached).unwrap(), bytes);
    fs::write(&cached, b"corrupt").unwrap();
    let bad = cache_run(&f, &["--cache-dir", path]);
    assert!(!bad.status.success());
    assert!(bad.stdout.is_empty());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("--rebuild-cache"));
    assert_eq!(fs::read(&cached).unwrap(), b"corrupt");
    let rebuilt = cache_run(&f, &["--cache-dir", path, "--rebuild-cache"]);
    assert!(rebuilt.status.success());
    assert_eq!(rebuilt.stdout, plain.stdout);
    assert_eq!(fs::read(&cached).unwrap(), bytes);
    for args in [
        vec!["--rebuild-cache"],
        vec!["--cache-dir", "relative"],
        vec!["--cache-dir", path, "--cache-dir", path],
        vec!["--cache-dir", path, "--rebuild-cache", "--rebuild-cache"],
    ] {
        let denied = cache_run(&f, &args);
        assert!(!denied.status.success());
        assert!(denied.stdout.is_empty());
    }
}

#[test]
fn region_fixture_actual_stdio_matches_direct_source_and_scope() {
    use bslens_core::{git_manifest::Limits, pipeline::GitStructure};
    let mut f = Fixture::new();
    let body="\u{feff}#Область Наружная\r\n#rEgIoN Inner // region\r\n  Процедура Тест()\r\nКонецПроцедуры\r\n#EndRegion\r\n#КонецОбласти\r\n";
    fs::write(f.path.join("Ext/SessionModule.bsl"), body).unwrap();
    f.git(&["add", "."]);
    f.git(&[
        "-c",
        "user.name=Synthetic",
        "-c",
        "user.email=synthetic@invalid",
        "commit",
        "-qm",
        "regions",
    ]);
    f.sha = f.git(&["rev-parse", "HEAD"]);
    let core = GitStructure::read(&f.path, &f.sha, Limits::default()).unwrap();
    let module = core.page(None, 0, 20).unwrap().page.items[0].id.clone();
    let page = core.page(Some(&module), 0, 20).unwrap();
    let hint = page.methods[0].source.as_ref().unwrap();
    let expected = core.source(hint).unwrap();
    #[cfg(feature = "experimental-cache")]
    {
        let cache = f.path.join("cache");
        bslens_core::cache::build(&f.path, &f.sha, &cache).unwrap();
        let warm = bslens_core::cache::open(&f.path, &f.sha, &cache).unwrap();
        assert_eq!(warm.page(Some(&module), 0, 20).unwrap(), page);
        assert_eq!(warm.source(hint).unwrap(), expected);
    }
    let mut p = Peer::start(&f);
    p.send(request(1,"initialize",json!({"protocolVersion":PROTOCOL,"capabilities":{},"clientInfo":{"name":"region-test","version":"1"}})));
    assert!(p.response()["result"].is_object());
    p.send(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    p.send(request(2, "tools/list", json!({})));
    let list = p.response()["result"].clone();
    assert_eq!(
        list["scope"]["lexical"],
        "r09-zero-args-regions-outside-methods"
    );
    p.send(call(
        3,
        "tree",
        json!({"commit":f.sha,"scope_id":list["scope_id"],"node_id":module}),
    ));
    let wire_page = p.response()["result"]["structuredContent"].clone();
    let reference = wire_page["data"]["methods"][0]["source"].clone();
    assert_eq!(reference["start_line"], 3);
    assert_eq!(reference["end_line"], 4);
    p.send(call(4, "source", reference));
    let wire = p.response()["result"]["structuredContent"].clone();
    let source = &wire["data"];
    assert_eq!(source["text"], expected.text);
    assert_eq!(source["sha256"], expected.sha256);
    assert_eq!(source["start_byte"], expected.start_byte);
    assert_eq!(source["end_byte"], expected.end_byte);
    assert_eq!(source["positions"], json!(expected.positions));
    assert_eq!(wire["scope_id"], list["scope_id"]);
    assert_eq!(source["text"], "  Процедура Тест()\r\nКонецПроцедуры\r\n");
    let (status, err) = p.finish();
    assert!(status.success());
    assert!(err.is_empty());
}
