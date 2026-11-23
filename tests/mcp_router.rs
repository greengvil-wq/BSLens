#![cfg(all(feature = "experimental-router", target_os = "linux"))]
use bslens_core::{
    git_manifest::Limits,
    mcp_router::CoreSession,
    mcp_transport::{Exit, MAX_BODY, PROTOCOL},
    pipeline::GitStructure,
};
use serde_json::{json, Value};
use std::{
    fs,
    io::Cursor,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture {
    path: PathBuf,
    sha: String,
}
impl Fixture {
    fn new(body: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "r06b-{}-{}",
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
        fs::write(f.path.join("Ext/SessionModule.bsl"), body).unwrap();
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
    fn core(&self) -> GitStructure {
        GitStructure::read(&self.path, &self.sha, Limits::default()).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn request(id: u32, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn init() -> Value {
    request(
        1,
        "initialize",
        json!({"protocolVersion":PROTOCOL,"capabilities":{},"clientInfo":{"name":"fixture","version":"1"}}),
    )
}
fn ready() -> Value {
    json!({"jsonrpc":"2.0","method":"notifications/initialized"})
}
fn send(s: &mut CoreSession, v: Value) -> Value {
    let bytes = s.handle_body(&serde_json::to_vec(&v).unwrap()).unwrap();
    assert!(bytes.len() <= MAX_BODY);
    serde_json::from_slice(&bytes).unwrap()
}
fn setup(s: &mut CoreSession) -> Value {
    assert!(send(s, init())["result"].is_object());
    assert!(s
        .handle_body(&serde_json::to_vec(&ready()).unwrap())
        .is_none());
    send(s, request(2, "tools/list", json!({})))["result"].clone()
}
fn call(s: &mut CoreSession, id: u32, name: &str, args: Value) -> Value {
    send(
        s,
        request(id, "tools/call", json!({"name":name,"arguments":args})),
    )
}
fn check_success(v: &Value, list: &Value) -> Value {
    assert_eq!(v["result"]["isError"], false);
    let result = &v["result"]["structuredContent"];
    assert_eq!(result["context"], list["context"]);
    assert_eq!(result["scope"], list["scope"]);
    assert_eq!(result["scope_id"], list["scope_id"]);
    assert_eq!(
        serde_json::from_str::<Value>(v["result"]["content"][0]["text"].as_str().unwrap()).unwrap(),
        *result
    );
    result["data"].clone()
}
#[test]
fn byte_requests_tree_then_source_fixed_scope_and_exact_physical_text() {
    let body="\u{feff}// 😀\r\n Процедура ЁЖ()\r\nКонецПроцедуры\r\nПроцедура ёж()\r\nКонецПроцедуры\r\n";
    let f = Fixture::new(body);
    let mut s = CoreSession::new(f.core());
    fs::write(f.path.join("Ext/SessionModule.bsl"), "dirty").unwrap();
    let list = setup(&mut s);
    assert_eq!(list["tools"].as_array().unwrap().len(), 2);
    let base = json!({"commit":f.sha,"scope_id":list["scope_id"]});
    let root = check_success(&call(&mut s, 3, "tree", base.clone()), &list);
    let module = &root["items"][0]["id"];
    let mut args = base.clone();
    args["node_id"] = module.clone();
    args["limit"] = json!(1);
    let page = check_success(&call(&mut s, 4, "tree", args.clone()), &list);
    assert_eq!(page["total"], 2);
    assert_eq!(page["methods"][0]["boundary"]["duplicate"], true);
    args["offset"] = json!(1);
    let second = check_success(&call(&mut s, 5, "tree", args), &list);
    assert_ne!(page["items"][0]["id"], second["items"][0]["id"]);
    let hint = page["methods"][0]["source"].clone();
    let source = check_success(&call(&mut s, 6, "source", hint), &list);
    assert_eq!(source["text"], " Процедура ЁЖ()\r\nКонецПроцедуры\r\n");
    let a = source["start_byte"].as_u64().unwrap() as usize;
    let b = source["end_byte"].as_u64().unwrap() as usize;
    assert_eq!(
        source["text"].as_str().unwrap().as_bytes(),
        &body.as_bytes()[a..b]
    );
}
#[test]
fn in_memory_stdio_full_session_and_unknown_argument_failures() {
    let f = Fixture::new("Procedure X()\nEndProcedure\n");
    let core = f.core();
    let module = core.page(None, 0, 1).unwrap().page.items.remove(0);
    let method = core.page(Some(&module.id), 0, 1).unwrap();
    let hint = method.methods[0].source.clone().unwrap();
    let mut probe = CoreSession::new(f.core());
    let list = setup(&mut probe);
    let scope = list["scope_id"].clone();
    let requests = [
        init(),
        ready(),
        request(2, "tools/list", json!({})),
        request(
            3,
            "tools/call",
            json!({"name":"tree","arguments":{"commit":f.sha,"scope_id":scope,"node_id":module.id}}),
        ),
        request(
            4,
            "tools/call",
            json!({"name":"source","arguments":{"commit":f.sha,"scope_id":scope,"blob_oid":hint.blob_oid,"path":hint.path,"start_line":hint.start_line,"end_line":hint.end_line}}),
        ),
    ];
    let input = requests
        .iter()
        .map(|v| format!("{v}\n"))
        .collect::<String>();
    let mut out = vec![];
    let mut s = CoreSession::new(core);
    assert_eq!(
        s.serve(&mut Cursor::new(input.as_bytes()), &mut out)
            .unwrap(),
        Exit::Eof
    );
    let responses: Vec<Value> = out
        .split(|b| *b == b'\n')
        .filter(|b| !b.is_empty())
        .map(|b| {
            assert!(b.len() <= MAX_BODY);
            serde_json::from_slice(b).unwrap()
        })
        .collect();
    assert_eq!(responses.len(), 4);
    check_success(&responses[2], &list);
    check_success(&responses[3], &list);
    let mut s = CoreSession::new(f.core());
    let early = call(&mut s, 90, "tree", json!({}));
    assert_eq!(early["error"]["code"], -32600);
    let list = setup(&mut s);
    let base = json!({"commit":f.sha,"scope_id":list["scope_id"]});
    for (i, patch) in [
        json!({"limit":true}),
        json!({"offset":-1}),
        json!({"limit":1.0}),
        json!({"extra":"ignored?"}),
        json!({"node_id":null}),
    ]
    .into_iter()
    .enumerate()
    {
        let mut args = base.clone();
        args.as_object_mut()
            .unwrap()
            .extend(patch.as_object().unwrap().clone());
        assert_eq!(
            call(&mut s, 10 + i as u32, "tree", args)["error"]["code"],
            -32602
        );
    }
    assert_eq!(
        call(&mut s, 20, "tree", json!({"commit":f.sha}))["error"]["code"],
        -32602
    );
    let mut wrong = base.clone();
    wrong["scope_id"] = json!("0".repeat(64));
    let result = call(&mut s, 21, "tree", wrong);
    assert_eq!(result["result"]["isError"], true);
    assert!(result["result"].get("structuredContent").is_none());
    let mut wrong = base;
    wrong["commit"] = json!("0".repeat(40));
    assert_eq!(call(&mut s, 22, "tree", wrong)["result"]["isError"], true);
}
#[test]
fn lexical_refusal_and_full_encoded_response_budget_are_explicit() {
    let f = Fixture::new("Procedure X()\n// hidden\0\nEndProcedure");
    let core = f.core();
    let module = core.page(None, 0, 1).unwrap().page.items.remove(0);
    let mut s = CoreSession::new(core);
    let list = setup(&mut s);
    let refused = call(
        &mut s,
        3,
        "tree",
        json!({"commit":f.sha,"scope_id":list["scope_id"],"node_id":module.id}),
    );
    assert_eq!(refused["result"]["isError"], true);
    assert!(refused["result"].get("structuredContent").is_none());
    assert!(refused["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("InvalidControl"));
    let f = Fixture::new(&format!(
        "Procedure X()\n//{}\nEndProcedure\n",
        "\\".repeat(12000)
    ));
    let core = f.core();
    let module = core.page(None, 0, 1).unwrap().page.items.remove(0);
    let mut s = CoreSession::new(core);
    let list = setup(&mut s);
    let page = check_success(
        &call(
            &mut s,
            3,
            "tree",
            json!({"commit":f.sha,"scope_id":list["scope_id"],"node_id":module.id}),
        ),
        &list,
    );
    let hint = page["methods"][0]["source"].clone();
    let overflow = call(&mut s, 4, "source", hint);
    assert_eq!(overflow["error"]["code"], -32001);
    assert!(overflow.get("result").is_none());
}
