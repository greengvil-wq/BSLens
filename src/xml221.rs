//! Bounded structural subset, not a validated Git snapshot or platform model.
use quick_xml::{events::Event, Reader};
use roxmltree::{Document, Node, ParsingOptions};
use std::collections::{BTreeMap, BTreeSet};

pub const NS: &str = "http://v8.1c.ru/8.3/MDClasses";
pub const MAX_FILES: usize = 4096;
pub const MAX_TOTAL_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_XML_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_DEPTH: usize = 64;
pub const MAX_EVENTS: usize = 20000;
pub const MAX_NODES: u32 = 10000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    Limit,
    Path,
    Utf8,
    Encoding,
    Dtd,
    ProcessingInstruction,
    Malformed,
    Namespace,
    Version,
    Shape,
    Identifier,
    Uuid,
    Duplicate,
    Missing,
    UnsupportedType,
    NestedUnsupported,
    NameMismatch,
    OrphanXml,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlError {
    pub code: Code,
    pub path: String,
}
impl std::fmt::Display for XmlError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.path)
    }
}
impl std::error::Error for XmlError {}
type Result<T> = std::result::Result<T, XmlError>;
fn error(code: Code, path: &str) -> XmlError {
    XmlError {
        code,
        path: path.into(),
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    Configuration,
    Catalog,
    Document,
    InformationRegister,
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Configuration => "Configuration",
            Self::Catalog => "Catalog",
            Self::Document => "Document",
            Self::InformationRegister => "InformationRegister",
        }
    }
    fn folder(self) -> &'static str {
        match self {
            Self::Catalog => "Catalogs",
            Self::Document => "Documents",
            Self::InformationRegister => "InformationRegisters",
            Self::Configuration => "",
        }
    }
    fn modules(self) -> &'static [&'static str] {
        match self {
            Self::Configuration => &[
                "ManagedApplicationModule",
                "OrdinaryApplicationModule",
                "ExternalConnectionModule",
                "SessionModule",
            ],
            Self::Catalog | Self::Document => &["ObjectModule", "ManagerModule"],
            Self::InformationRegister => &["RecordSetModule", "ManagerModule"],
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    pub kind: String,
    pub path: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub kind: Kind,
    pub name: String,
    pub uuid: Option<String>,
    pub path: String,
    pub modules: Vec<Module>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unverified {
    pub path: String,
    pub selector: String,
}
#[derive(Debug)]
pub struct Xml221Subset {
    objects: Vec<Object>,
    unverified: Vec<Unverified>,
    unassigned: Vec<String>,
}
impl Xml221Subset {
    pub fn objects(&self) -> &[Object] {
        &self.objects
    }
    pub fn unverified(&self) -> &[Unverified] {
        &self.unverified
    }
    pub fn unassigned_modules(&self) -> &[String] {
        &self.unassigned
    }
    pub fn inspect(blobs: &BTreeMap<String, Vec<u8>>) -> Result<Self> {
        if blobs.len() > MAX_FILES
            || blobs
                .values()
                .try_fold(0usize, |n, b| n.checked_add(b.len()))
                .is_none_or(|n| n > MAX_TOTAL_BYTES)
        {
            return Err(error(Code::Limit, ""));
        }
        for path in blobs.keys() {
            path_check(path)?;
        }
        if !blobs.contains_key("Configuration.xml") {
            return Err(error(Code::Missing, "Configuration.xml"));
        }
        let mut docs = BTreeMap::new();
        for (path, raw) in blobs {
            if path.ends_with(".xml") {
                docs.insert(path.as_str(), parse(raw, path)?);
            } else if !path.ends_with(".bsl") {
                return Err(error(Code::Path, path));
            }
        }
        let mut state = State {
            blobs,
            docs: &docs,
            seen: BTreeSet::new(),
            uuids: BTreeSet::new(),
            objects: vec![],
            unverified: vec![],
            assigned: BTreeSet::new(),
        };
        state.read("Configuration.xml", Kind::Configuration, None)?;
        for path in docs.keys() {
            if !state.seen.contains(*path) {
                return Err(error(Code::OrphanXml, path));
            }
        }
        state.objects.sort_by(|a, b| a.path.cmp(&b.path));
        state
            .unverified
            .sort_by(|a, b| (&a.path, &a.selector).cmp(&(&b.path, &b.selector)));
        let unassigned = blobs
            .keys()
            .filter(|p| p.ends_with(".bsl") && !state.assigned.contains(*p))
            .cloned()
            .collect();
        Ok(Self {
            objects: state.objects,
            unverified: state.unverified,
            unassigned,
        })
    }
}
fn path_check(path: &str) -> Result<()> {
    if path.len() > 1024 {
        return Err(error(Code::Limit, path));
    }
    if path.is_empty()
        || path.contains(['\\', ':'])
        || path.chars().any(|c| c < ' ')
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(error(Code::Path, path));
    }
    Ok(())
}
fn parse<'a>(raw: &'a [u8], path: &str) -> Result<Document<'a>> {
    if raw.len() > MAX_XML_BYTES {
        return Err(error(Code::Limit, path));
    }
    let text = std::str::from_utf8(raw).map_err(|_| error(Code::Utf8, path))?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.starts_with('\u{feff}') {
        return Err(error(Code::Malformed, path));
    }
    let mut reader = Reader::from_str(text);
    let mut depth = 0usize;
    let mut count = 0usize;
    loop {
        let event = reader
            .read_event()
            .map_err(|_| error(Code::Malformed, path))?;
        count += 1;
        if count > MAX_EVENTS {
            return Err(error(Code::Limit, path));
        }
        match event {
            Event::DocType(_) => return Err(error(Code::Dtd, path)),
            Event::PI(_) => return Err(error(Code::ProcessingInstruction, path)),
            Event::Decl(d) => {
                if let Some(encoding) = d.encoding() {
                    let encoding = encoding.map_err(|_| error(Code::Malformed, path))?;
                    if !encoding.eq_ignore_ascii_case(b"utf-8") {
                        return Err(error(Code::Encoding, path));
                    }
                }
            }
            Event::Start(_) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(error(Code::Limit, path));
                }
            }
            Event::Empty(_) => {
                if depth + 1 > MAX_DEPTH {
                    return Err(error(Code::Limit, path));
                }
            }
            Event::End(_) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| error(Code::Malformed, path))?;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Document::parse_with_options(
        text,
        ParsingOptions {
            allow_dtd: false,
            nodes_limit: MAX_NODES,
        },
    )
    .map_err(|e| {
        error(
            if matches!(e, roxmltree::Error::NodesLimitReached) {
                Code::Limit
            } else {
                Code::Malformed
            },
            path,
        )
    })
}
fn matches(node: Node<'_, '_>, name: &str) -> bool {
    node.is_element() && node.tag_name().namespace() == Some(NS) && node.tag_name().name() == name
}
fn elements<'a, 'i>(n: Node<'a, 'i>) -> Vec<Node<'a, 'i>> {
    n.children().filter(|c| c.is_element()).collect()
}
fn no_text(n: Node<'_, '_>) -> bool {
    n.children()
        .filter(|c| c.is_text())
        .all(|c| c.text().unwrap_or("").trim().is_empty())
}
fn scalar(n: Node<'_, '_>) -> String {
    n.children()
        .filter(|c| c.is_text())
        .map(|c| c.text().unwrap_or(""))
        .collect()
}
fn identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_alphabetic())
        && chars.all(|c| c == '_' || c.is_alphanumeric())
}
fn uuid(s: &str) -> bool {
    s.len() == 36
        && s.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}
struct State<'a, 'b> {
    blobs: &'a BTreeMap<String, Vec<u8>>,
    docs: &'b BTreeMap<&'a str, Document<'a>>,
    seen: BTreeSet<String>,
    uuids: BTreeSet<String>,
    objects: Vec<Object>,
    unverified: Vec<Unverified>,
    assigned: BTreeSet<String>,
}
impl State<'_, '_> {
    fn read(&mut self, path: &str, kind: Kind, expected: Option<&str>) -> Result<()> {
        if !self.seen.insert(path.into()) {
            return Err(error(Code::Duplicate, path));
        }
        let doc = self
            .docs
            .get(path)
            .ok_or_else(|| error(Code::Missing, path))?;
        let root = doc.root_element();
        if !matches(root, "MetaDataObject") {
            return Err(error(Code::Namespace, path));
        }
        if root.attribute("version") != Some("2.21") {
            return Err(error(Code::Version, path));
        }
        let children = elements(root);
        if children.len() != 1 || !matches(children[0], kind.name()) || !no_text(root) {
            return Err(error(Code::Shape, path));
        }
        let obj = children[0];
        if !no_text(obj) {
            return Err(error(Code::Shape, path));
        }
        let props: Vec<_> = elements(obj)
            .into_iter()
            .filter(|n| matches(*n, "Properties"))
            .collect();
        if props.len() != 1 {
            return Err(error(Code::Shape, path));
        }
        if !no_text(props[0]) {
            return Err(error(Code::Shape, path));
        }
        for attr in props[0].attributes() {
            self.unverified.push(Unverified {
                path: path.into(),
                selector: format!(
                    "/MetaDataObject/{}/Properties/@{{{}}}{}",
                    kind.name(),
                    attr.namespace().unwrap_or(""),
                    attr.name()
                ),
            });
        }
        let names: Vec<_> = elements(props[0])
            .into_iter()
            .filter(|n| matches(*n, "Name"))
            .collect();
        if names.len() != 1 || !elements(names[0]).is_empty() || names[0].attributes().len() != 0 {
            return Err(error(Code::Shape, path));
        }
        let name = scalar(names[0]);
        if !identifier(&name) {
            return Err(error(Code::Identifier, path));
        }
        if expected.is_some_and(|e| e != name) {
            return Err(error(Code::NameMismatch, path));
        }
        let uid = obj.attribute("uuid");
        if let Some(uid) = uid {
            if !uuid(uid) {
                return Err(error(Code::Uuid, path));
            }
            if !self.uuids.insert(uid.to_lowercase()) {
                return Err(error(Code::Duplicate, path));
            }
        }
        let base = if kind == Kind::Configuration {
            "Ext/".to_string()
        } else {
            format!("{}/Ext/", path.strip_suffix(".xml").unwrap())
        };
        let modules = kind
            .modules()
            .iter()
            .filter_map(|m| {
                let p = format!("{base}{m}.bsl");
                if self.blobs.contains_key(&p) {
                    self.assigned.insert(p.clone());
                    Some(Module {
                        kind: (*m).into(),
                        path: p,
                    })
                } else {
                    None
                }
            })
            .collect();
        self.objects.push(Object {
            kind,
            name: name.clone(),
            uuid: uid.map(str::to_string),
            path: path.into(),
            modules,
        });
        let selector = format!("/MetaDataObject/{}", kind.name());
        for (node, allowed, prefix) in [
            (root, "version", "/MetaDataObject"),
            (obj, "uuid", selector.as_str()),
        ] {
            for attr in node.attributes() {
                if attr.name() != allowed || attr.namespace().is_some() {
                    self.unverified.push(Unverified {
                        path: path.into(),
                        selector: format!(
                            "{prefix}/@{{{}}}{}",
                            attr.namespace().unwrap_or(""),
                            attr.name()
                        ),
                    });
                }
            }
        }
        for prop in elements(props[0]) {
            if !matches(prop, "Name") {
                self.unverified.push(Unverified {
                    path: path.into(),
                    selector: format!(
                        "{selector}/Properties/{{{}}}{}",
                        prop.tag_name().namespace().unwrap_or(""),
                        prop.tag_name().name()
                    ),
                });
            }
        }
        let containers: Vec<_> = elements(obj)
            .into_iter()
            .filter(|n| matches(*n, "ChildObjects"))
            .collect();
        if containers.len() > 1 {
            return Err(error(Code::Duplicate, path));
        }
        for other in elements(obj) {
            if !matches(other, "Properties") && !matches(other, "ChildObjects") {
                self.unverified.push(Unverified {
                    path: path.into(),
                    selector: format!(
                        "{selector}/{{{}}}{}",
                        other.tag_name().namespace().unwrap_or(""),
                        other.tag_name().name()
                    ),
                });
            }
        }
        if let Some(container) = containers.first() {
            if container.attributes().len() != 0 || !no_text(*container) {
                return Err(error(Code::Shape, path));
            }
            let refs = elements(*container);
            if kind != Kind::Configuration && !refs.is_empty() {
                return Err(error(Code::NestedUnsupported, path));
            }
            let mut seen = BTreeSet::new();
            for child in refs {
                let child_kind = if matches(child, "Catalog") {
                    Kind::Catalog
                } else if matches(child, "Document") {
                    Kind::Document
                } else if matches(child, "InformationRegister") {
                    Kind::InformationRegister
                } else {
                    return Err(error(Code::UnsupportedType, path));
                };
                if child.attributes().len() != 0 || !elements(child).is_empty() {
                    return Err(error(Code::Shape, path));
                }
                let child_name = scalar(child);
                if !identifier(&child_name) {
                    return Err(error(Code::Identifier, path));
                }
                let target = format!("{}/{}.xml", child_kind.folder(), child_name);
                if !seen.insert(target.clone()) {
                    return Err(error(Code::Duplicate, path));
                }
                self.read(&target, child_kind, Some(&child_name))?;
            }
        }
        Ok(())
    }
}
