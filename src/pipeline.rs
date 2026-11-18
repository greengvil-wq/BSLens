//! Experimental composition of checked Git objects and the limited XML structure.
//! This type cannot create a ValidatedSnapshot.
use crate::{
    git_manifest::File,
    lexical,
    structure::{Node, NodeKind},
};
use crate::{
    git_manifest::{self, Limits, Manifest},
    structure::{self, Page, Structure, MAX_TEXT_BYTES},
    xml221::{self, Xml221Subset},
};
use std::{collections::BTreeMap, path::Path};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    pub commit: String,
    pub tree: String,
    pub selected_files: usize,
    pub other_files: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundPage {
    pub context: Context,
    pub page: Page,
    pub methods: Vec<Method>,
}
#[derive(Debug)]
pub enum Error {
    Git(git_manifest::Code),
    Xml(xml221::XmlError),
    Structure(structure::Error),
    ContextMismatch,
    Lexical(lexical::Refusal),
    SourceRange,
    SourceIdentity,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}

pub struct GitStructure {
    context: Context,
    structure: Structure,
    sources: BTreeMap<String, File>,
}
impl GitStructure {
    pub fn read(repo: &Path, commit: &str, limits: Limits) -> Result<Self, Error> {
        let manifest = Manifest::read(repo, commit, limits).map_err(|e| Error::Git(e.code))?;
        Self::from_manifest(&manifest)
    }
    pub fn from_manifest(manifest: &Manifest) -> Result<Self, Error> {
        let blobs = manifest.xml_inputs();
        let subset = Xml221Subset::inspect(&blobs).map_err(Error::Xml)?;
        let structure = Structure::from_subset(&subset).map_err(Error::Structure)?;
        Ok(Self {
            context: Context {
                commit: manifest.commit().to_owned(),
                tree: manifest.tree().to_owned(),
                selected_files: blobs.len(),
                other_files: manifest.files().len() - blobs.len(),
            },
            structure,
            sources: manifest
                .files()
                .iter()
                .filter(|f| f.path.ends_with(".bsl"))
                .map(|f| (f.path.clone(), f.clone()))
                .collect(),
        })
    }
    #[cfg(feature = "experimental-cache")]
    pub(crate) fn from_cached(context: Context, files: Vec<File>) -> Result<Self, Error> {
        let blobs = files
            .iter()
            .map(|f| (f.path.clone(), f.bytes().to_vec()))
            .collect();
        let subset = Xml221Subset::inspect(&blobs).map_err(Error::Xml)?;
        let structure = Structure::from_subset(&subset).map_err(Error::Structure)?;
        Ok(Self {
            context,
            structure,
            sources: files
                .into_iter()
                .filter(|f| f.path.ends_with(".bsl"))
                .map(|f| (f.path.clone(), f))
                .collect(),
        })
    }

    pub fn context(&self) -> &Context {
        &self.context
    }
    pub fn page(
        &self,
        node_id: Option<&str>,
        offset: usize,
        limit: usize,
    ) -> Result<BoundPage, Error> {
        let prefix = format!("g1:{}:", self.context.commit);
        let projection_id = node_id
            .map(|id| id.strip_prefix(&prefix).ok_or(Error::ContextMismatch))
            .transpose()?;
        let (base_id, ordinal) = if let Some(id) = projection_id {
            if let Some((base, n)) = id.rsplit_once(":m:") {
                if n.len() > 10 || n.is_empty() || !n.bytes().all(|c| c.is_ascii_digit()) {
                    return Err(Error::ContextMismatch);
                }
                let ordinal = n.parse::<usize>().map_err(|_| Error::ContextMismatch)?;
                if ordinal.to_string() != n {
                    return Err(Error::ContextMismatch);
                }
                (Some(base), Some(ordinal))
            } else {
                (Some(id), None)
            }
        } else {
            (None, None)
        };
        let mut page = self
            .structure
            .page(base_id, offset, limit)
            .map_err(Error::Structure)?;
        page.coverage.methods =
            "available on module expansion; narrow lexical subset; failures explicit";
        page.coverage.provenance =
            "exact Git commit/tree and blob bytes; not a fully validated snapshot";
        let mut methods = vec![];
        if page.node.kind == NodeKind::Module {
            let path = page.node.address.as_deref().ok_or(Error::SourceIdentity)?;
            let file = self.sources.get(path).ok_or(Error::SourceIdentity)?;
            let facts = lexical::analyze(file.bytes())
                .map_err(Error::Lexical)?
                .boundaries;
            page.coverage.methods =
                "lexical zero-argument subset; no body/call validation; absence unknown";
            if let Some(n) = ordinal {
                let fact = facts.get(n).ok_or(Error::ContextMismatch)?;
                let (node, detail) = self.method(&page.node, file, n, fact);
                page.node = node;
                page.items.clear();
                page.total = 0;
                page.next_offset = None;
                methods.push(detail);
            } else {
                page.total = facts.len();
                page.items.clear();
                for (n, fact) in facts.iter().enumerate().skip(offset).take(limit) {
                    let (node, detail) = self.method(&page.node, file, n, fact);
                    page.items.push(node);
                    methods.push(detail);
                }
                page.next_offset = if page.items.is_empty()
                    || page.items.len() == facts.len().saturating_sub(offset)
                {
                    None
                } else {
                    Some(offset + page.items.len())
                };
            }
        } else if ordinal.is_some() {
            return Err(Error::ContextMismatch);
        }
        // Keep the accepted projection untouched. Bind only this API's returned IDs.
        for node in std::iter::once(&mut page.node).chain(page.items.iter_mut()) {
            node.id.insert_str(0, &prefix);
        }
        let bytes: usize = std::iter::once(&page.node)
            .chain(page.items.iter())
            .map(|n| {
                n.id.len()
                    + n.label.len()
                    + n.address.as_deref().map_or(0, str::len)
                    + n.ownership.map_or(0, str::len)
            })
            .sum::<usize>()
            + self.context.commit.len()
            + self.context.tree.len()
            + methods.iter().map(Method::text_bytes).sum::<usize>();
        if bytes > MAX_TEXT_BYTES {
            return Err(Error::Structure(structure::Error::TextBudget));
        }
        Ok(BoundPage {
            context: self.context.clone(),
            page,
            methods,
        })
    }
    fn method(
        &self,
        module: &Node,
        file: &File,
        ordinal: usize,
        fact: &lexical::Boundary,
    ) -> (Node, Method) {
        let positions = lines(file.bytes());
        let first = fact.start_line - 1;
        let mut end = None;
        for (i, (_, b)) in positions
            .iter()
            .enumerate()
            .take(fact.end_line)
            .skip(first)
            .take(100)
        {
            if b - positions[first].0 + source_overhead(&self.context, file) <= MAX_TEXT_BYTES {
                end = Some(i + 1);
            } else {
                break;
            }
        }
        let source = end.map(|end_line| SourceRef {
            commit: self.context.commit.clone(),
            blob_oid: file.oid.clone(),
            path: file.path.clone(),
            start_line: fact.start_line,
            end_line,
        });
        let detail = Method {
            ordinal,
            boundary: fact.clone(),
            blob_oid: file.oid.clone(),
            sha256: file.sha256.clone(),
            source,
            source_next_line: end.filter(|e| *e < fact.end_line).map(|e| e + 1),
            source_unavailable: if end.is_none() {
                Some("physical line exceeds source text budget")
            } else {
                None
            },
        };
        (
            Node {
                id: format!("{}:m:{ordinal}", module.id),
                kind: NodeKind::Method,
                label: fact.name.clone(),
                address: Some(file.path.clone()),
                ownership: module.ownership,
            },
            detail,
        )
    }
    /// Exact committed physical lines, not a lexical token slice. No working-tree reads.
    pub fn source(&self, reference: &SourceRef) -> Result<Source, Error> {
        if reference.commit != self.context.commit {
            return Err(Error::ContextMismatch);
        }
        let file = self
            .sources
            .get(&reference.path)
            .ok_or(Error::SourceIdentity)?;
        if reference.blob_oid != file.oid {
            return Err(Error::SourceIdentity);
        }
        lexical::analyze(file.bytes()).map_err(Error::Lexical)?;
        let (start, end) = (reference.start_line, reference.end_line);
        if start == 0 || start > end || end - start >= 100 {
            return Err(Error::SourceRange);
        }
        let positions = lines(file.bytes());
        let rows = positions.get(start - 1..end).ok_or(Error::SourceRange)?;
        let (a, b) = (rows[0].0, rows.last().unwrap().1);
        if b - a + source_overhead(&self.context, file) > MAX_TEXT_BYTES {
            return Err(Error::Structure(structure::Error::TextBudget));
        }
        let text = std::str::from_utf8(&file.bytes()[a..b])
            .map_err(|_| Error::SourceRange)?
            .to_owned();
        Ok(Source {
            context: self.context.clone(),
            reference: reference.clone(),
            sha256: file.sha256.clone(),
            text,
            start_byte: a,
            end_byte: b,
            positions: rows
                .iter()
                .enumerate()
                .map(|(i, (a, b))| [start + i, *a, *b])
                .collect(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRef {
    pub commit: String,
    pub blob_oid: String,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub context: Context,
    pub reference: SourceRef,
    pub sha256: String,
    pub text: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub positions: Vec<[usize; 3]>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Method {
    pub ordinal: usize,
    pub boundary: lexical::Boundary,
    pub blob_oid: String,
    pub sha256: String,
    pub source: Option<SourceRef>,
    pub source_next_line: Option<usize>,
    pub source_unavailable: Option<&'static str>,
}
impl Method {
    fn text_bytes(&self) -> usize {
        self.boundary.name.len()
            + self.boundary.kind.len()
            + self.blob_oid.len()
            + self.sha256.len()
            + self.source_unavailable.map_or(0, str::len)
            + self
                .source
                .as_ref()
                .map_or(0, |s| s.commit.len() + s.blob_oid.len() + s.path.len())
    }
}
fn lines(bytes: &[u8]) -> Vec<(usize, usize)> {
    let mut offset = 0;
    bytes
        .split_inclusive(|c| *c == b'\n')
        .map(|line| {
            let start = offset;
            offset += line.len();
            (start, offset)
        })
        .collect()
}
fn source_overhead(c: &Context, f: &File) -> usize {
    c.commit.len() * 2 + c.tree.len() + f.oid.len() + f.path.len() + f.sha256.len()
}
