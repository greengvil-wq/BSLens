//! Fixed read-only core binding. Caller supplies one already checked GitStructure.
use crate::{
    mcp_transport::{Exit, RouteError, Session, State, ToolRouter},
    pipeline::{self, GitStructure, SourceRef},
    structure::{Node, NodeKind},
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{self, BufRead, Write};

/// No repository path or replaceable core is accepted after construction.
pub struct CoreSession {
    transport: Session,
    router: Router,
}
impl CoreSession {
    pub fn new(core: GitStructure) -> Self {
        Self {
            transport: Session::new(),
            router: Router::new(core),
        }
    }
    pub fn state(&self) -> State {
        self.transport.state()
    }
    pub fn handle_body(&mut self, body: &[u8]) -> Option<Vec<u8>> {
        self.transport.handle_with_tools(body, &self.router)
    }
    pub fn serve<R: BufRead, W: Write>(
        &mut self,
        input: &mut R,
        output: &mut W,
    ) -> io::Result<Exit> {
        self.transport.serve_with_tools(input, output, &self.router)
    }
}
struct Router {
    core: GitStructure,
    context: Value,
    scope: Value,
    scope_id: String,
}
impl Router {
    fn new(core: GitStructure) -> Self {
        let c = core.context();
        let context = json!({"commit":c.commit,"tree":c.tree,"selected_files":c.selected_files,"other_files":c.other_files});
        let scope = json!({"router":"r06b-read-only-v1","xml":"r02-xml221-subset","structure":"r05b-git-methods","lexical":"r04b-zero-args-control-checked","source":"physical-utf8-lines-100-text-budget-v1","production":false});
        let scope_id = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&json!({"context":context,"scope":scope})).unwrap())
        );
        Self {
            core,
            context,
            scope,
            scope_id,
        }
    }
    fn wrapped(&self, data: Value) -> Value {
        json!({"context":self.context,"scope":self.scope,"scope_id":self.scope_id,"data":data})
    }
    fn success(&self, data: Value) -> Value {
        let value = self.wrapped(data);
        json!({"isError":false,"content":[{"type":"text","text":value.to_string()}],"structuredContent":value})
    }
    fn source_ref(&self, r: &SourceRef) -> Value {
        json!({"commit":r.commit,"scope_id":self.scope_id,"blob_oid":r.blob_oid,"path":r.path,"start_line":r.start_line,"end_line":r.end_line})
    }
    fn schema(&self, name: &str) -> Value {
        let mut properties = json!({"commit":{"type":"string","const":self.core.context().commit},"scope_id":{"type":"string","const":self.scope_id}});
        let mut required = vec!["commit", "scope_id"];
        if name == "tree" {
            properties["node_id"] = json!({"type":"string","minLength":1,"maxLength":256});
            properties["offset"] =
                json!({"type":"integer","minimum":0,"maximum":4294967295u64,"default":0});
            properties["limit"] = json!({"type":"integer","minimum":1,"maximum":20,"default":20});
        } else {
            properties["blob_oid"] =
                json!({"type":"string","pattern":"^([0-9a-f]{40}|[0-9a-f]{64})$"});
            properties["path"] = json!({"type":"string","minLength":1,"maxLength":1024,"description":"Exact UTF-8 path, also limited to 1024 bytes"});
            properties["start_line"] = json!({"type":"integer","minimum":1,"maximum":65536});
            properties["end_line"] = json!({"type":"integer","minimum":1,"maximum":65536});
            required.extend(["blob_oid", "path", "start_line", "end_line"]);
        }
        let data = if name == "tree" {
            json!({"type":"object","required":["node","items","total","offset","next_offset","coverage","methods"]})
        } else {
            json!({"type":"object","required":["reference","sha256","text","start_byte","end_byte","positions"]})
        };
        json!({"name":name,"description":if name=="tree"{"Read bounded children from the fixed exact Git commit; lexical refusals are explicit"}else{"Read exact committed physical lines using a source hint; not a working-tree file"},
            "inputSchema":{"type":"object","additionalProperties":false,"required":required,"properties":properties},
            "outputSchema":{"type":"object","additionalProperties":false,"required":["context","scope","scope_id","data"],"properties":{"context":{"const":self.context},"scope":{"const":self.scope},"scope_id":{"const":self.scope_id},"data":data}},
            "annotations":{"readOnlyHint":true,"destructiveHint":false,"idempotentHint":true,"openWorldHint":false}})
    }
    fn tree(&self, args: &Value) -> Result<Value, RouteError> {
        exact_keys(args, &["commit", "scope_id", "node_id", "offset", "limit"])?;
        let node = args.get("node_id").map(|v| string(v, 256)).transpose()?;
        let offset = args
            .get("offset")
            .map(|v| integer(v, 0, 4294967295))
            .transpose()?
            .unwrap_or(0);
        let limit = args
            .get("limit")
            .map(|v| integer(v, 1, 20))
            .transpose()?
            .unwrap_or(20);
        if let Some(error) = self.identity(args)? {
            return Ok(error);
        }
        match self.core.page(node, offset, limit) {
            Err(e) => Ok(tool_error(e)),
            Ok(p) => {
                let coverage = &p.page.coverage;
                let methods:Vec<_>=p.methods.iter().map(|m|{
                    let b=&m.boundary;
                    json!({"ordinal":m.ordinal,"blob_oid":m.blob_oid,"sha256":m.sha256,"source":m.source.as_ref().map(|r|self.source_ref(r)),"source_next_line":m.source_next_line,"source_unavailable":m.source_unavailable,
                        "boundary":{"name":b.name,"kind":b.kind,"start_line":b.start_line,"start_byte":b.start_byte,"header_start_byte":b.header_start_byte,"header_end_byte":b.header_end_byte,"header_end_line":b.header_end_line,"end_line":b.end_line,"end_byte":b.end_byte,"occurrences":b.occurrences,"duplicate":b.duplicate}})
                }).collect();
                Ok(self.success(json!({"node":node_json(&p.page.node),"items":p.page.items.iter().map(node_json).collect::<Vec<_>>(),"total":p.page.total,"offset":p.page.offset,"next_offset":p.page.next_offset,"methods":methods,
                    "coverage":{"completeness":coverage.completeness,"unverified_entries":coverage.unverified_entries,"unassigned_modules":coverage.unassigned_modules,"methods":coverage.methods,"provenance":coverage.provenance}})))
            }
        }
    }
    fn source(&self, args: &Value) -> Result<Value, RouteError> {
        exact_keys(
            args,
            &[
                "commit",
                "scope_id",
                "blob_oid",
                "path",
                "start_line",
                "end_line",
            ],
        )?;
        let oid = string(required(args, "blob_oid")?, 64)?;
        if !valid_oid(oid) {
            return Err(RouteError::InvalidArgs);
        }
        let path = string(required(args, "path")?, 1024)?;
        if path.contains(['\\', ':'])
            || path.chars().any(char::is_control)
            || path
                .split('/')
                .any(|p| p.is_empty() || p == "." || p == "..")
        {
            return Err(RouteError::InvalidArgs);
        }
        let start = integer(required(args, "start_line")?, 1, 65536)?;
        let end = integer(required(args, "end_line")?, 1, 65536)?;
        if end < start || end - start >= 100 {
            return Err(RouteError::InvalidArgs);
        }
        if let Some(error) = self.identity(args)? {
            return Ok(error);
        }
        let reference = SourceRef {
            commit: self.core.context().commit.clone(),
            blob_oid: oid.into(),
            path: path.into(),
            start_line: start,
            end_line: end,
        };
        match self.core.source(&reference){
            Err(e)=>Ok(tool_error(e)),
            Ok(s)=>Ok(self.success(json!({"reference":self.source_ref(&s.reference),"sha256":s.sha256,"text":s.text,"start_byte":s.start_byte,"end_byte":s.end_byte,"positions":s.positions})))
        }
    }
    fn identity(&self, args: &Value) -> Result<Option<Value>, RouteError> {
        let commit = string(required(args, "commit")?, 64)?;
        let scope = string(required(args, "scope_id")?, 64)?;
        if !valid_oid(commit)
            || scope.len() != 64
            || !scope
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(RouteError::InvalidArgs);
        }
        Ok(
            if commit != self.core.context().commit || scope != self.scope_id {
                Some(failure("context_mismatch", None))
            } else {
                None
            },
        )
    }
}
impl ToolRouter for Router {
    fn list(&self) -> Value {
        json!({"tools":[self.schema("tree"),self.schema("source")],"context":self.context,"scope":self.scope,"scope_id":self.scope_id})
    }
    fn call(&self, params: &Value) -> Result<Value, RouteError> {
        exact_keys(params, &["name", "arguments", "_meta"])?;
        let name = string(required(params, "name")?, 64)?;
        let args = required(params, "arguments")?;
        if !args.is_object() {
            return Err(RouteError::InvalidArgs);
        }
        match name {
            "tree" => self.tree(args),
            "source" => self.source(args),
            _ => Err(RouteError::InvalidArgs),
        }
    }
    fn instructions(&self) -> &'static str {
        "Experimental read-only tree/source bound to one Git commit. Obtain commit and scope_id from tools/list schemas. No production snapshot validation or working-tree access."
    }
}
fn required<'a>(v: &'a Value, k: &str) -> Result<&'a Value, RouteError> {
    v.get(k).ok_or(RouteError::InvalidArgs)
}
fn exact_keys(v: &Value, allowed: &[&str]) -> Result<(), RouteError> {
    let obj = v.as_object().ok_or(RouteError::InvalidArgs)?;
    if obj.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Err(RouteError::InvalidArgs);
    }
    Ok(())
}
fn string(v: &Value, max: usize) -> Result<&str, RouteError> {
    v.as_str()
        .filter(|s| !s.is_empty() && s.len() <= max)
        .ok_or(RouteError::InvalidArgs)
}
fn integer(v: &Value, min: usize, max: usize) -> Result<usize, RouteError> {
    v.as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .filter(|n| *n >= min && *n <= max)
        .ok_or(RouteError::InvalidArgs)
}
fn valid_oid(s: &str) -> bool {
    [40, 64].contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn failure(code: &str, line: Option<usize>) -> Value {
    json!({"isError":true,"content":[{"type":"text","text":json!({"error":code,"line":line}).to_string()}]})
}
fn tool_error(e: pipeline::Error) -> Value {
    match e {
        pipeline::Error::Lexical(r) => failure(&format!("lexical_{:?}", r.reason), Some(r.line)),
        pipeline::Error::ContextMismatch => failure("context_mismatch", None),
        pipeline::Error::SourceIdentity => failure("source_identity", None),
        pipeline::Error::SourceRange => failure("source_range", None),
        pipeline::Error::Structure(crate::structure::Error::TextBudget) => {
            failure("text_budget", None)
        }
        pipeline::Error::Structure(_) => failure("invalid_node", None),
        _ => failure("core_refusal", None),
    }
}
fn node_json(n: &Node) -> Value {
    json!({"id":n.id,"kind":match n.kind{NodeKind::Configuration=>"configuration",NodeKind::ObjectType=>"object_type",NodeKind::Object=>"object",NodeKind::Module=>"module",NodeKind::Method=>"method",NodeKind::UnassignedModules=>"unassigned_modules"},"label":n.label,"address":n.address,"ownership":n.ownership})
}
