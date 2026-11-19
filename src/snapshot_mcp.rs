//! One immutable v5 reader per stdio session; no Git, parsing or dynamic open tools.
use crate::{
    linux_v5_reader::{Error, LinuxSnapshotReader, StagingError, TreeLimits},
    mcp_transport::{Exit, RouteError, Session, ToolRouter},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{self, BufRead, Write};

pub const MAX_SOURCE_BYTES: u64 = 65536;
pub const MAX_TREE_NODES: usize = 128;
pub const MAX_TREE_TEXT_BYTES: usize = 32768;

pub struct SnapshotSession {
    transport: Session,
    router: Router,
}
impl SnapshotSession {
    pub fn new(reader: LinuxSnapshotReader) -> Result<Self, Error> {
        let metadata = reader.info()?;
        Ok(Self {
            transport: Session::new(),
            router: Router { reader, metadata },
        })
    }
    pub fn serve<R: BufRead, W: Write>(
        &mut self,
        input: &mut R,
        output: &mut W,
    ) -> io::Result<Exit> {
        self.transport.serve_with_tools(input, output, &self.router)
    }
}

/// Stable, non-sensitive startup refusals. No caller paths or source content in stderr.
pub fn startup_code(error: &Error) -> &'static str {
    match error {
        Error::Version => "snapshot_version",
        Error::Profile => "snapshot_profile",
        Error::Identity => "snapshot_identity",
        Error::Limit | Error::Staging(StagingError::Limit) => "snapshot_budget_exceeded",
        Error::Staging(StagingError::Digest) => "snapshot_checksum",
        Error::Staging(StagingError::NotStandalone) => "snapshot_not_standalone",
        Error::Staging(_) => "snapshot_staging",
        Error::Facts | Error::ForeignKey | Error::Schema => "snapshot_integrity",
        _ => "snapshot_rejected",
    }
}
struct Router {
    reader: LinuxSnapshotReader,
    metadata: Value,
}
fn keys(v: &Value, allowed: &[&str]) -> Result<(), RouteError> {
    if v.as_object()
        .is_none_or(|o| o.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        return Err(RouteError::InvalidArgs);
    }
    Ok(())
}
fn text<'a>(v: &'a Value, key: &str, max: usize) -> Result<&'a str, RouteError> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= max)
        .ok_or(RouteError::InvalidArgs)
}
fn number(v: &Value, key: &str) -> Result<i64, RouteError> {
    v.get(key)
        .and_then(Value::as_i64)
        .filter(|n| *n > 0)
        .ok_or(RouteError::InvalidArgs)
}
fn failure(code: &'static str) -> Value {
    let value = json!({"status":"error","code":code});
    json!({"isError":true,"structuredContent":value,"content":[{"type":"text","text":value.to_string()}]})
}
fn refusal(error: Error) -> Value {
    failure(match error {
        Error::Limit => "budget_exceeded",
        Error::Sqlite(rusqlite::Error::QueryReturnedNoRows) => "not_found",
        Error::Range => "source_range",
        _ => "snapshot_read_refused",
    })
}
impl Router {
    fn success(&self, data: Value) -> Value {
        let value = json!({"snapshot":self.metadata,"data":data});
        // The existing transport checks the entire envelope AFTER both representations
        // are serialized, including escaping, request ID and base64 expansion.
        json!({"isError":false,"structuredContent":value,"content":[{"type":"text","text":value.to_string()}]})
    }
    fn tree(&self, args: &Value) -> Result<Value, RouteError> {
        keys(
            args,
            &["level", "parent_id", "kind", "module_path", "max_nodes"],
        )?;
        let level = if args.get("level").is_some() {
            text(args, "level", 32)?
        } else {
            "configuration"
        };
        if !["configuration", "kind", "object", "unconfirmed", "module"].contains(&level) {
            return Err(RouteError::InvalidArgs);
        }
        let parent = args
            .get("parent_id")
            .map(|_| number(args, "parent_id"))
            .transpose()?;
        let kind = args
            .get("kind")
            .map(|_| text(args, "kind", 128))
            .transpose()?;
        let path = args
            .get("module_path")
            .map(|_| text(args, "module_path", 1024))
            .transpose()?;
        let shape = match level {
            "configuration" => kind.is_none() && path.is_none(),
            "object" => parent.is_some() && kind.is_none() && path.is_none(),
            "kind" => parent.is_some() && kind.is_some() && path.is_none(),
            "module" => parent.is_none() && kind.is_none() && path.is_some(),
            _ => parent.is_none() && kind.is_none() && path.is_none(),
        };
        if !shape {
            return Err(RouteError::InvalidArgs);
        }
        let max = args
            .get("max_nodes")
            .map(|_| number(args, "max_nodes"))
            .transpose()?
            .unwrap_or(MAX_TREE_NODES as i64);
        if max > MAX_TREE_NODES as i64 {
            return Err(RouteError::InvalidArgs);
        }
        let (parent, branch) = match self.reader.branch_info(level, parent, kind, path) {
            Ok(v) => v,
            Err(e) => return Ok(refusal(e)),
        };
        let nodes = match self.reader.tree(
            level,
            parent,
            kind,
            path,
            TreeLimits {
                rows: max as usize,
                bytes: MAX_TREE_TEXT_BYTES,
            },
        ) {
            Ok(v) => v,
            Err(e) => return Ok(refusal(e)),
        };
        let mut items = Vec::new();
        for n in nodes {
            let mut item = json!({"type":n.node_type,"key":n.key,"name":n.name,"parse_status":n.status,"reason":n.reason});
            let children = match n.node_type.as_str() {
                "kind" => Some(json!({"level":"kind","parent_id":parent,"kind":n.key})),
                "object" => Some(
                    json!({"level":"object","parent_id":n.key.parse::<i64>().map_err(|_|RouteError::InvalidArgs)?}),
                ),
                "module" => Some(json!({"level":"module","module_path":n.key})),
                "unconfirmed" => Some(json!({"level":"unconfirmed"})),
                "method" => {
                    let id = n.key.parse::<i64>().map_err(|_| RouteError::InvalidArgs)?;
                    let location = match self.reader.method_location(id) {
                        Ok(v) => v,
                        Err(e) => return Ok(refusal(e)),
                    };
                    item["source"] = json!({"path":path,"start_line":location["start_line"],"end_line":location["end_line"]});
                    item["location"] = location;
                    None
                }
                _ => None,
            };
            item["children"] = json!(children);
            items.push(item);
        }
        Ok(self.success(json!({"branch":branch,"items":items,"complete_branch":true})))
    }
    fn source(&self, args: &Value) -> Result<Value, RouteError> {
        keys(args, &["path", "start_line", "end_line"])?;
        let path = text(args, "path", 1024)?;
        if path.starts_with('/')
            || path.contains(['\\', '\0'])
            || path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
        {
            return Err(RouteError::InvalidArgs);
        }
        let start = number(args, "start_line")?;
        let end = number(args, "end_line")?;
        if end < start {
            return Err(RouteError::InvalidArgs);
        }
        let (_, module) = match self.reader.branch_info("module", None, None, Some(path)) {
            Ok(v) => v,
            Err(e) => return Ok(refusal(e)),
        };
        let (bytes, a, b) = match self.reader.source_span(path, start, end, MAX_SOURCE_BYTES) {
            Ok(v) => v,
            Err(e) => return Ok(refusal(e)),
        };
        let (encoding, data) = match std::str::from_utf8(&bytes) {
            Ok(s) => ("utf-8", s.to_owned()),
            Err(_) => ("base64", base64(&bytes)),
        };
        Ok(self.success(json!({"status":"ok","path":path,"parse_status":module["status"],"reason":module["reason"],"start_line":start,"end_line":end,"line_units":"physical_1_based_inclusive","start_byte":a,"end_byte":b,"byte_units":"0_based_end_exclusive","byte_length":bytes.len(),"sha256":format!("{:x}",Sha256::digest(&bytes)),"encoding":encoding,"data":data})))
    }
}
impl ToolRouter for Router {
    fn list(&self) -> Value {
        let annotations = json!({"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false});
        json!({"tools":[
            {"name":"metadata","description":"Fixed autonomous v5 profile, scope and temporary coverage; relations not analyzed, Git provenance not verified","inputSchema":{"type":"object","properties":{},"additionalProperties":false},"annotations":annotations},
            {"name":"tree","description":"One bounded stored branch; use children arguments, accepted and unparsed are distinct","inputSchema":{"type":"object","additionalProperties":false,"properties":{"level":{"enum":["configuration","kind","object","module","unconfirmed"]},"parent_id":{"type":"integer","minimum":1},"kind":{"type":"string","maxLength":128},"module_path":{"type":"string","maxLength":1024},"max_nodes":{"type":"integer","minimum":1,"maximum":MAX_TREE_NODES}}},"annotations":annotations},
            {"name":"source","description":"Exact physical line bytes; encoding is utf-8 or padded RFC4648 base64, never lossy","inputSchema":{"type":"object","additionalProperties":false,"required":["path","start_line","end_line"],"properties":{"path":{"type":"string","maxLength":1024},"start_line":{"type":"integer","minimum":1},"end_line":{"type":"integer","minimum":1}}},"annotations":annotations}
        ]})
    }
    fn call(&self, params: &Value) -> Result<Value, RouteError> {
        keys(params, &["name", "arguments", "_meta"])?;
        let args = params
            .get("arguments")
            .filter(|v| v.is_object())
            .ok_or(RouteError::InvalidArgs)?;
        match text(params, "name", 64)? {
            "metadata" => {
                keys(args, &[])?;
                Ok(self.success(json!({"status":"ok","source_budget_bytes":MAX_SOURCE_BYTES,"tree_nodes":MAX_TREE_NODES,"tree_text_budget_bytes":MAX_TREE_TEXT_BYTES})))
            }
            "tree" => self.tree(args),
            "source" => self.source(args),
            _ => Err(RouteError::InvalidArgs),
        }
    }
    fn instructions(&self) -> &'static str {
        "Experimental Linux x86_64 v5 snapshot, one immutable reader per session. Read-only metadata/tree/source; no Git or BSL reparse. Relations not analyzed; Git provenance not verified."
    }
}
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = chunk.get(1).copied().unwrap_or(0);
        let c = chunk.get(2).copied().unwrap_or(0);
        result.push(ALPHABET[(a >> 2) as usize] as char);
        result.push(ALPHABET[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        result.push(if chunk.len() > 1 {
            ALPHABET[(((b & 15) << 2) | (c >> 6)) as usize] as char
        } else {
            '='
        });
        result.push(if chunk.len() > 2 {
            ALPHABET[(c & 63) as usize] as char
        } else {
            '='
        });
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn base64_rfc4648_vectors_and_binary() {
        for (input, expected) in [
            (b"".as_slice(), ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
            (&[0xff, 0, 0x80], "/wCA"),
        ] {
            assert_eq!(base64(input), expected);
        }
    }
}
