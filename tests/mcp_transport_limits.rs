#![cfg(feature = "experimental-stdio")]
use bslens_core::mcp_transport::{Exit, Session, State, MAX_BODY, MAX_MESSAGES, MAX_SESSION_BYTES};
use serde_json::{json, Value};
use std::io::{self, BufRead, Read, Write};

/// A further read would block a real stream: fail immediately if the server tries it.
struct Sentinel {
    bytes: Vec<u8>,
    position: usize,
}
impl Read for Sentinel {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        panic!("unexpected Read; test uses BufRead");
    }
}
impl BufRead for Sentinel {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        assert!(
            self.position < self.bytes.len(),
            "read past last permitted frame"
        );
        Ok(&self.bytes[self.position..])
    }
    fn consume(&mut self, n: usize) {
        self.position += n;
        assert!(self.position <= self.bytes.len());
    }
}
#[derive(Default)]
struct Output {
    bytes: Vec<u8>,
    flushes: usize,
}
impl Write for Output {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.bytes.extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}
fn ping(id: usize) -> Vec<u8> {
    serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":"ping"})).unwrap()
}
fn verify_output(out: &Output, count: usize) {
    assert_eq!(out.flushes, count);
    let lines: Vec<_> = out.bytes.split(|&b| b == b'\n').collect();
    assert_eq!(lines.len(), count + 1);
    assert!(lines.last().unwrap().is_empty());
    for (id, line) in lines[..count].iter().enumerate() {
        let response: Value = serde_json::from_slice(line).unwrap();
        assert_eq!(response["id"], id);
        assert_eq!(response["result"], json!({}));
    }
}
#[test]
fn exact_message_quota_writes_last_response_then_never_reads_again() {
    let mut bytes = vec![];
    for id in 0..MAX_MESSAGES {
        bytes.extend(ping(id));
        bytes.push(b'\n');
    }
    let mut input = Sentinel { bytes, position: 0 };
    let mut out = Output::default();
    let mut session = Session::new();
    assert_eq!(
        session.serve(&mut input, &mut out).unwrap(),
        Exit::SessionLimit
    );
    assert_eq!(input.position, input.bytes.len());
    assert_eq!(session.state(), State::Closed);
    verify_output(&out, MAX_MESSAGES);
}
#[test]
fn exact_four_mib_quota_writes_last_response_then_never_reads_again() {
    assert_eq!(MAX_SESSION_BYTES % MAX_BODY, 0);
    let count = MAX_SESSION_BYTES / MAX_BODY;
    let mut bytes = vec![];
    for id in 0..count {
        let mut body = ping(id);
        body.resize(MAX_BODY, b' ');
        bytes.extend(body);
        bytes.push(b'\n');
    }
    let mut input = Sentinel { bytes, position: 0 };
    let mut out = Output::default();
    let mut session = Session::new();
    assert_eq!(
        session.serve(&mut input, &mut out).unwrap(),
        Exit::SessionLimit
    );
    assert_eq!(input.position, input.bytes.len());
    verify_output(&out, count);
}
#[test]
fn remaining_session_bytes_bound_next_frame_without_drain_or_dispatch() {
    let mut session = Session::new();
    let mut notification = br#"{"jsonrpc":"2.0","method":"unknown"}"#.to_vec();
    notification.resize(MAX_BODY, b' ');
    for _ in 0..MAX_SESSION_BYTES / MAX_BODY - 1 {
        assert!(session.handle_body(&notification).is_none());
    }
    // Seven body bytes left: reader may consume one extra byte solely to detect non-LF.
    notification.truncate(MAX_BODY - 7);
    assert!(session.handle_body(&notification).is_none());
    let mut input = Sentinel {
        bytes: vec![b'x'; 10000],
        position: 0,
    };
    let mut out = Output::default();
    assert_eq!(
        session.serve(&mut input, &mut out).unwrap(),
        Exit::SessionLimit
    );
    assert_eq!(input.position, 8);
    assert!(out.bytes.is_empty());
    assert_eq!(out.flushes, 0);
    assert_eq!(session.state(), State::Closed);
}
#[test]
fn next_queued_message_is_not_consumed_at_exact_quota() {
    let mut session = Session::new();
    let notification = br#"{"jsonrpc":"2.0","method":"unknown"}"#;
    for _ in 0..MAX_MESSAGES {
        assert!(session.handle_body(notification).is_none());
    }
    let mut input = Sentinel {
        bytes: b"do not read\n".to_vec(),
        position: 0,
    };
    let mut out = Output::default();
    assert_eq!(
        session.serve(&mut input, &mut out).unwrap(),
        Exit::SessionLimit
    );
    assert_eq!(input.position, 0);
    assert!(out.bytes.is_empty());
}

#[test]
fn exhausted_direct_session_does_not_emit_oversized_error() {
    let oversized = vec![b'x'; MAX_BODY + 1];
    let mut by_count = Session::new();
    let notification = br#"{"jsonrpc":"2.0","method":"unknown"}"#;
    for _ in 0..MAX_MESSAGES {
        assert!(by_count.handle_body(notification).is_none());
    }
    assert!(by_count.handle_body(&oversized).is_none());
    assert_eq!(by_count.state(), State::Closed);
    let mut by_bytes = Session::new();
    let mut body = notification.to_vec();
    body.resize(MAX_BODY, b' ');
    for _ in 0..MAX_SESSION_BYTES / MAX_BODY {
        assert!(by_bytes.handle_body(&body).is_none());
    }
    assert!(by_bytes.handle_body(&oversized).is_none());
    assert_eq!(by_bytes.state(), State::Closed);
}
