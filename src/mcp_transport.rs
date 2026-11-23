//! Experimental library framing/lifecycle with an internal tool hook. No stdio binary.
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    io::{self, BufRead, Write},
};
mod strict_json;
pub const MAX_BODY: usize = 65536;
pub const MAX_MESSAGES: usize = 1024;
pub const MAX_SESSION_BYTES: usize = 4 * 1024 * 1024;
pub const PROTOCOL: &str = "2025-11-25";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    New,
    AwaitInitialized,
    Ready,
    Closed,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exit {
    Eof,
    FrameRejected,
    SessionLimit,
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Id {
    String(String),
    Integer(i64),
}
impl Id {
    fn parse(v: &Value) -> Option<Self> {
        match v {
            Value::String(s) if s.len() <= 128 => Some(Self::String(s.clone())),
            Value::Number(n) => n.as_i64().map(Self::Integer),
            _ => None,
        }
    }
    fn value(&self) -> Value {
        match self {
            Self::String(s) => s.clone().into(),
            Self::Integer(n) => (*n).into(),
        }
    }
}
pub(crate) enum RouteError {
    InvalidArgs,
    NotImplemented,
}
pub(crate) trait ToolRouter {
    fn list(&self) -> Value;
    fn call(&self, params: &Value) -> Result<Value, RouteError>;
    fn instructions(&self) -> &'static str;
}
struct EmptyTools;
impl ToolRouter for EmptyTools {
    fn list(&self) -> Value {
        json!({"tools":[]})
    }
    fn call(&self, _: &Value) -> Result<Value, RouteError> {
        Err(RouteError::NotImplemented)
    }
    fn instructions(&self) -> &'static str {
        "Experimental lifecycle harness; no core tools available"
    }
}
pub struct Session {
    state: State,
    ids: BTreeSet<Id>,
    messages: usize,
    bytes: usize,
}
impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}
impl Session {
    pub fn new() -> Self {
        Self {
            state: State::New,
            ids: BTreeSet::new(),
            messages: 0,
            bytes: 0,
        }
    }
    pub fn state(&self) -> State {
        self.state
    }
    /// Input excludes LF. No filesystem, stdout, core, subprocess or tool access.
    pub fn handle_body(&mut self, raw: &[u8]) -> Option<Vec<u8>> {
        self.handle_with_tools(raw, &EmptyTools)
    }
    pub(crate) fn handle_with_tools(
        &mut self,
        raw: &[u8],
        tools: &impl ToolRouter,
    ) -> Option<Vec<u8>> {
        if self.state == State::Closed
            || self.messages >= MAX_MESSAGES
            || self.bytes >= MAX_SESSION_BYTES
        {
            self.state = State::Closed;
            return None;
        }
        if raw.len() > MAX_BODY {
            self.state = State::Closed;
            return Some(error(None, -32600, "Frame exceeds byte limit"));
        }
        if self.messages >= MAX_MESSAGES || raw.len() > MAX_SESSION_BYTES.saturating_sub(self.bytes)
        {
            self.state = State::Closed;
            return None;
        }
        self.messages += 1;
        self.bytes += raw.len();
        if raw.contains(&b'\n') {
            return Some(error(None, -32700, "Raw newline inside JSON body"));
        }
        let request = match strict_json::parse(raw) {
            Ok(v) => v,
            Err(_) => return Some(error(None, -32700, "Invalid bounded UTF-8 JSON")),
        };
        let Some(obj) = request.as_object() else {
            return Some(error(None, -32600, "Expected request object"));
        };
        // A parsed notification never gets a reply, including invalid/unknown notifications.
        let notification =
            !obj.contains_key("id") && obj.get("method").is_some_and(Value::is_string);
        let id = obj.get("id").and_then(Id::parse);
        let envelope = obj.get("jsonrpc").and_then(Value::as_str) == Some("2.0")
            && obj
                .get("method")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty() && s.len() <= 128)
            && obj
                .keys()
                .all(|k| ["jsonrpc", "id", "method", "params"].contains(&k.as_str()))
            && (!obj.contains_key("id") || id.is_some());
        if !envelope {
            return if notification {
                None
            } else {
                Some(error(id.as_ref(), -32600, "Invalid request envelope"))
            };
        }
        let empty = json!({});
        let params = obj.get("params").unwrap_or(&empty);
        let valid_params = params.is_object() && params.get("_meta").is_none_or(Value::is_object);
        let method = obj["method"].as_str().unwrap();
        if notification {
            if method == "notifications/initialized"
                && self.state == State::AwaitInitialized
                && valid_params
                && params.as_object().unwrap().keys().all(|k| k == "_meta")
            {
                self.state = State::Ready;
            }
            return None;
        }
        let Some(id) = id else {
            return Some(error(None, -32600, "Missing request ID"));
        };
        if !self.ids.insert(id.clone()) {
            return Some(error(Some(&id), -32600, "Request ID already used"));
        }
        if !valid_params {
            return Some(error(
                Some(&id),
                -32602,
                "Expected object params and metadata",
            ));
        }
        let result = match method {
            "initialize" => {
                if self.state != State::New {
                    return Some(error(Some(&id), -32600, "Already initialized"));
                }
                let valid = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .is_some_and(|s| !s.is_empty() && s.len() <= 64)
                    && params.get("capabilities").is_some_and(Value::is_object)
                    && params.get("clientInfo").is_some_and(|c| {
                        c.is_object()
                            && ["name", "version"].iter().all(|k| {
                                c.get(k)
                                    .and_then(Value::as_str)
                                    .is_some_and(|s| !s.is_empty() && s.len() <= 128)
                            })
                    });
                if !valid {
                    return Some(error(Some(&id), -32602, "Invalid initialization"));
                }
                self.state = State::AwaitInitialized;
                json!({"protocolVersion":PROTOCOL,"capabilities":{"tools":{}},"serverInfo":{"name":"bslens-framing-experiment","version":"0.1.0"},"instructions":tools.instructions()})
            }
            "ping" => {
                if params.as_object().unwrap().keys().any(|k| k != "_meta") {
                    return Some(error(Some(&id), -32602, "Invalid ping params"));
                }
                json!({})
            }
            _ if self.state != State::Ready => {
                return Some(error(Some(&id), -32600, "Initialization required"))
            }
            "tools/list" => {
                if params.as_object().unwrap().keys().any(|k| k != "_meta") {
                    return Some(error(Some(&id), -32602, "No cursor for fixed tool list"));
                }
                tools.list()
            }
            "tools/call" => match tools.call(params) {
                Ok(v) => v,
                Err(RouteError::InvalidArgs) => {
                    return Some(error(Some(&id), -32602, "Invalid tool name or arguments"))
                }
                Err(RouteError::NotImplemented) => {
                    return Some(error(Some(&id), -32601, "Method not implemented"))
                }
            },
            _ => return Some(error(Some(&id), -32601, "Method not implemented")),
        };
        Some(encode(
            json!({"jsonrpc":"2.0","id":id.value(),"result":result}),
            Some(&id),
        ))
    }
    pub fn serve<R: BufRead, W: Write>(
        &mut self,
        input: &mut R,
        output: &mut W,
    ) -> io::Result<Exit> {
        self.serve_with_tools(input, output, &EmptyTools)
    }
    pub(crate) fn serve_with_tools<R: BufRead, W: Write>(
        &mut self,
        input: &mut R,
        output: &mut W,
        tools: &impl ToolRouter,
    ) -> io::Result<Exit> {
        let result = self.serve_inner(input, output, tools);
        if result.is_err() {
            self.state = State::Closed;
        }
        result
    }
    fn serve_inner<R: BufRead, W: Write>(
        &mut self,
        input: &mut R,
        output: &mut W,
        tools: &impl ToolRouter,
    ) -> io::Result<Exit> {
        loop {
            if self.state == State::Closed
                || self.messages >= MAX_MESSAGES
                || self.bytes >= MAX_SESSION_BYTES
            {
                self.state = State::Closed;
                return Ok(Exit::SessionLimit);
            }
            let frame = read_frame(input, MAX_SESSION_BYTES - self.bytes)?;
            let body = match frame {
                Frame::Eof => {
                    self.state = State::Closed;
                    return Ok(Exit::Eof);
                }
                Frame::Rejected => {
                    self.state = State::Closed;
                    write_frame(
                        output,
                        &error(
                            None,
                            -32600,
                            "Oversized or unterminated frame; session closed",
                        ),
                    )?;
                    return Ok(Exit::FrameRejected);
                }
                Frame::SessionLimit => {
                    self.state = State::Closed;
                    return Ok(Exit::SessionLimit);
                }
                Frame::Body(body) => body,
            };
            if let Some(response) = self.handle_with_tools(&body, tools) {
                write_frame(output, &response)?;
            }
        }
    }
}
fn encode(value: Value, id: Option<&Id>) -> Vec<u8> {
    // Bound the full locally constructed envelope, including routed source text and JSON escaping.
    let raw = serde_json::to_vec(&value).expect("JSON Value serialization cannot fail");
    if raw.len() > MAX_BODY {
        return error(id, -32001, "Response exceeds byte limit");
    }
    raw
}
fn error(id: Option<&Id>, code: i32, message: &str) -> Vec<u8> {
    let mut v = json!({"jsonrpc":"2.0","error":{"code":code,"message":message}});
    if let Some(id) = id {
        v["id"] = id.value();
    }
    serde_json::to_vec(&v).expect("bounded error serialization")
}
fn write_frame<W: Write>(out: &mut W, body: &[u8]) -> io::Result<()> {
    out.write_all(body)?;
    out.write_all(b"\n")?;
    out.flush()
}
enum Frame {
    Eof,
    Rejected,
    SessionLimit,
    Body(Vec<u8>),
}
fn read_frame<R: BufRead>(input: &mut R, remaining_bytes: usize) -> io::Result<Frame> {
    let body_limit = MAX_BODY.min(remaining_bytes);
    let mut raw = Vec::new();
    loop {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            return Ok(if raw.is_empty() {
                Frame::Eof
            } else {
                Frame::Rejected
            });
        }
        let available = &buf[..buf.len().min(body_limit + 1 - raw.len())];
        let take = available
            .iter()
            .position(|&b| b == b'\n')
            .map_or(available.len(), |n| n + 1);
        raw.extend_from_slice(&buf[..take]);
        input.consume(take);
        if raw.last() == Some(&b'\n') {
            raw.pop();
            return Ok(Frame::Body(raw));
        }
        if raw.len() > body_limit {
            return Ok(if remaining_bytes <= MAX_BODY {
                Frame::SessionLimit
            } else {
                Frame::Rejected
            });
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn serializer_bounds_whole_envelope() {
        let id = Id::String("id".into());
        let raw = encode(json!({"data":"x".repeat(MAX_BODY+1)}), Some(&id));
        assert!(raw.len() <= MAX_BODY);
        let v: Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(v["id"], "id");
        assert_eq!(v["error"]["code"], -32001);
    }
}
