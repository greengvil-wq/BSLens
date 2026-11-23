#![cfg(feature = "experimental-stdio")]
use bslens_core::mcp_transport::{
    Exit, Session, State, MAX_BODY, MAX_MESSAGES, MAX_SESSION_BYTES, PROTOCOL,
};
use serde_json::{json, Value};
use std::io::{self, Cursor, Write};
fn call(s: &mut Session, v: Value) -> Value {
    serde_json::from_slice(&s.handle_body(&serde_json::to_vec(&v).unwrap()).unwrap()).unwrap()
}
fn request(id: Value, method: &str) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method})
}
fn initialize(id: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":"initialize","params":{"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"authored","version":"1","title":"Allowed optional field"}}})
}
fn ready() -> Session {
    let mut s = Session::new();
    call(&mut s, initialize(json!(1)));
    assert!(s
        .handle_body(br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
        .is_none());
    assert_eq!(s.state(), State::Ready);
    s
}
#[test]
fn lifecycle_negotiation_empty_tools_and_no_router() {
    let mut s = Session::new();
    assert_eq!(call(&mut s, request(json!(0), "ping"))["result"], json!({}));
    assert_eq!(
        call(&mut s, request(json!(1), "tools/list"))["error"]["code"],
        -32600
    );
    let mut init = initialize(json!(2));
    init["params"]["protocolVersion"] = json!("future-version");
    let v = call(&mut s, init);
    assert_eq!(v["result"]["protocolVersion"], PROTOCOL);
    assert_eq!(v["result"]["capabilities"], json!({"tools":{}}));
    assert_eq!(s.state(), State::AwaitInitialized);
    assert_eq!(
        call(&mut s, request(json!(3), "tools/list"))["error"]["code"],
        -32600
    );
    s.handle_body(
        br#"{"jsonrpc":"2.0","method":"notifications/initialized","params":{"_meta":{}}}"#,
    );
    assert_eq!(s.state(), State::Ready);
    assert_eq!(
        call(&mut s, request(json!(4), "tools/list"))["result"],
        json!({"tools":[]})
    );
    assert_eq!(
        call(&mut s, request(json!(5), "tools/call"))["error"]["code"],
        -32601
    );
    assert!(call(&mut s, initialize(json!(6))).get("error").is_some());
}
#[test]
fn notifications_never_reply_or_initialize_early() {
    let mut s = Session::new();
    for raw in [
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
        r#"{"jsonrpc":"2.0","method":"initialize","params":{}}"#,
        r#"{"jsonrpc":"bad","method":"unknown","params":null}"#,
        r#"{"jsonrpc":"2.0","method":"unknown"}"#,
    ] {
        assert!(s.handle_body(raw.as_bytes()).is_none());
        assert_eq!(s.state(), State::New);
    }
    call(&mut s, initialize(json!(1)));
    assert!(s
        .handle_body(br#"{"jsonrpc":"2.0","method":"notifications/initialized","params":null}"#)
        .is_none());
    assert_eq!(s.state(), State::AwaitInitialized);
}
#[test]
fn canonical_ids_and_errors_never_null() {
    let mut s = ready();
    assert!(call(&mut s, request(json!(2), "ping"))
        .get("result")
        .is_some());
    assert!(call(&mut s, request(json!("2"), "ping"))
        .get("result")
        .is_some());
    assert_eq!(
        call(&mut s, request(json!(2), "ping"))["error"]["code"],
        -32600
    );
    for id in [
        Value::Null,
        json!(true),
        json!(1.0),
        json!(u64::MAX),
        json!("x".repeat(129)),
    ] {
        let v = call(&mut s, request(id, "ping"));
        assert!(v.get("id").is_none());
        assert_eq!(v["error"]["code"], -32600);
    }
    for raw in [
        br#"{"jsonrpc":"2.0","id":2e0,"method":"ping"}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":-0,"method":"ping"}"#,
    ] {
        let v: Value = serde_json::from_slice(&s.handle_body(raw).unwrap()).unwrap();
        assert!(v.get("id").is_none());
    }
    assert!(s
        .handle_body(br#"{"jsonrpc":"2.0","id":"\u0061","method":"ping"}"#)
        .is_some());
    assert_eq!(
        call(&mut s, request(json!("a"), "ping"))["error"]["code"],
        -32600
    );
    assert_eq!(
        call(&mut s, request(json!(i64::MIN), "ping"))["id"],
        json!(i64::MIN)
    );
}
#[test]
fn duplicate_keys_utf8_depth_and_shapes_fail_closed() {
    for raw in [
        br#"{"jsonrpc":"2.0","id":1,"id":2,"method":"ping"}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"x":1,"\u0078":2}}"#,
        b"\xff",
        b"NaN",
        b"{\n}",
        br#"{"a":1} trailing"#,
    ] {
        let mut s = Session::new();
        let v: Value = serde_json::from_slice(&s.handle_body(raw).unwrap()).unwrap();
        assert_eq!(v["error"]["code"], -32700);
        assert!(v.get("id").is_none());
        assert_eq!(s.state(), State::New);
    }
    let mut s = Session::new();
    let nested = format!("{}0{}", "[".repeat(40), "]".repeat(40));
    let v: Value = serde_json::from_slice(&s.handle_body(nested.as_bytes()).unwrap()).unwrap();
    assert_eq!(v["error"]["code"], -32700);
    for v in [
        json!([]),
        json!([request(json!(1), "ping")]),
        json!({}),
        json!({"jsonrpc":"2.0","id":1,"result":{}}),
    ] {
        assert_eq!(call(&mut s, v)["error"]["code"], -32600);
    }
}
#[test]
fn argument_validation_does_not_change_lifecycle() {
    let mut s = Session::new();
    let mut init = initialize(json!(1));
    init["params"]["capabilities"] = json!(false);
    assert_eq!(call(&mut s, init)["error"]["code"], -32602);
    assert_eq!(s.state(), State::New);
    let mut bad = request(json!(2), "ping");
    bad["params"] = json!({"_meta":null});
    assert_eq!(call(&mut s, bad)["error"]["code"], -32602);
    let mut s = ready();
    let mut list = request(json!(2), "tools/list");
    list["params"] = json!({"cursor":"unissued"});
    assert_eq!(call(&mut s, list)["error"]["code"], -32602);
}
#[test]
fn framing_exact_limit_crlf_multiframe_eof_and_no_partial_output() {
    let base = serde_json::to_string(&request(json!(1), "ping")).unwrap();
    let raw = format!("{}{}\n", base, " ".repeat(MAX_BODY - base.len()));
    let mut output = vec![];
    let mut s = Session::new();
    assert_eq!(
        s.serve(&mut Cursor::new(raw), &mut output).unwrap(),
        Exit::Eof
    );
    assert_eq!(output.iter().filter(|&&b| b == b'\n').count(), 1);
    assert!(serde_json::from_slice::<Value>(&output)
        .unwrap()
        .get("result")
        .is_some());
    let mut s = Session::new();
    let mut out = vec![];
    let body=b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\r\n{\"jsonrpc\":\"2.0\",\"method\":\"unknown\"}\n";
    s.serve(&mut Cursor::new(body), &mut out).unwrap();
    assert_eq!(out.iter().filter(|&&b| b == b'\n').count(), 1);
    for bytes in [vec![b'x'; MAX_BODY + 2], base.as_bytes().to_vec()] {
        let mut s = Session::new();
        let mut input = Cursor::new(bytes);
        let mut out = vec![];
        assert_eq!(s.serve(&mut input, &mut out).unwrap(), Exit::FrameRejected);
        assert!(input.position() <= MAX_BODY as u64 + 1);
        let v: Value = serde_json::from_slice(&out).unwrap();
        assert!(v.get("id").is_none());
        assert!(out.len() <= MAX_BODY + 1);
        assert_eq!(s.state(), State::Closed);
    }
}
#[test]
fn total_message_and_byte_budgets_include_notifications() {
    let raw = br#"{"jsonrpc":"2.0","method":"unknown"}"#;
    let mut s = Session::new();
    for _ in 0..MAX_MESSAGES {
        assert!(s.handle_body(raw).is_none());
    }
    assert!(s.handle_body(raw).is_none());
    assert_eq!(s.state(), State::Closed);
    assert!(s.handle_body(b"broken").is_none());
    let mut body = raw.to_vec();
    body.resize(MAX_BODY, b' ');
    let mut s = Session::new();
    for _ in 0..MAX_SESSION_BYTES / MAX_BODY {
        assert!(s.handle_body(&body).is_none());
    }
    assert!(s.handle_body(&body).is_none());
    assert_eq!(s.state(), State::Closed);
}
struct BrokenWriter;
impl Write for BrokenWriter {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
#[test]
fn broken_pipe_is_io_error_not_panic() {
    let mut s = Session::new();
    let raw = format!("{}\n", request(json!(1), "ping"));
    let e = s
        .serve(&mut Cursor::new(raw), &mut BrokenWriter)
        .unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::BrokenPipe);
    assert_eq!(s.state(), State::Closed);
}

#[test]
fn read_and_flush_errors_close_session() {
    struct BrokenRead;
    impl io::Read for BrokenRead {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            Err(io::ErrorKind::Other.into())
        }
    }
    impl io::BufRead for BrokenRead {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            Err(io::ErrorKind::Other.into())
        }
        fn consume(&mut self, _: usize) {}
    }
    let mut s = Session::new();
    let mut out = vec![];
    assert!(s.serve(&mut BrokenRead, &mut out).is_err());
    assert!(out.is_empty());
    assert_eq!(s.state(), State::Closed);
    struct BrokenFlush;
    impl Write for BrokenFlush {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }
    let mut s = Session::new();
    let raw = format!("{}\n", request(json!(1), "ping"));
    assert!(s.serve(&mut Cursor::new(raw), &mut BrokenFlush).is_err());
    assert_eq!(s.state(), State::Closed);
}
