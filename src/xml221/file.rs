//! Bounded identity-only XML 2.21 frontend. No full-schema or snapshot capability.
use super::{
    elements, identifier, matches, no_text, parse_bounded, path_check, scalar, uuid, Code, Kind,
    ParseLimits, XmlError, MAX_DEPTH, MAX_EVENTS, MAX_NODES, MAX_XML_BYTES, NS,
};
use crate::git_manifest::ingest::Entry;
use sha1::Sha1;
use sha2::{Digest, Sha256};

pub const ADAPTER: &str = "xml221-file-identity-v1";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub bytes: usize,
    pub nodes: u32,
    pub depth: usize,
    pub events: usize,
}
impl Limits {
    pub const MAX: Self = Self {
        bytes: MAX_XML_BYTES,
        nodes: MAX_NODES,
        depth: MAX_DEPTH,
        events: MAX_EVENTS,
    };
    fn validate(self) -> Result<()> {
        for (name, value, maximum) in [
            ("bytes", self.bytes as u64, Self::MAX.bytes as u64),
            ("nodes", self.nodes as u64, Self::MAX.nodes as u64),
            ("depth", self.depth as u64, Self::MAX.depth as u64),
            ("events", self.events as u64, Self::MAX.events as u64),
        ] {
            if value == 0 || value > maximum {
                return Err(Error::InvalidLimit { name });
            }
        }
        Ok(())
    }
    /// Stable input for a FUTURE profile's build parameters, not a snapshot_id.
    pub fn normalized_parameters(self) -> Result<String> {
        self.validate()?;
        Ok(
            serde_json::json!({"adapter":ADAPTER, "bytes":self.bytes, "nodes":self.nodes,
            "depth":self.depth, "events":self.events, "metadata_version":"2.21", "metadata_namespace":NS,
                "known_types":["Configuration","Catalog","Document","InformationRegister"],
                "path_policy":"hierarchical_identity", "nonmetadata_version":"observed_only",
            "coverage":"identity_only_or_unanalyzed", "service_path":"ConfigDumpInfo.xml",
            "path_bytes":1024, "xml_suffix":".xml", "suffix_case":"sensitive"})
            .to_string(),
        )
    }
}
#[derive(Debug)]
pub enum Error {
    InvalidLimit { name: &'static str },
    Identity,
    Xml(XmlError),
}
impl From<XmlError> for Error {
    fn from(e: XmlError) -> Self {
        Self::Xml(e)
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "XML file: {self:?}")
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub path: String,
    pub mode: u32,
    pub oid: String,
    pub sha256: String,
    pub bytes: usize,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Envelope {
    pub namespace: Option<String>,
    pub root: String,
    pub declared_version: Option<String>,
}
/// Only these three properties have been interpreted. No module/child/relationship
/// list exists: absence of analysis must not look like an empty collection of facts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectIdentity {
    pub kind: Kind,
    pub name: String,
    pub uuid: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Analysis {
    ObjectIdentity(ObjectIdentity),
    /// MDClasses MetaDataObject, 2.21, one child in MDClasses; body not interpreted.
    UnsupportedType {
        type_name: String,
    },
    /// Recognized by documented reserved layout path, NOT by an invented schema.
    ServiceDocument,
    /// Well-formed safe XML; root QName/schema is not implemented by this adapter.
    UnknownEnvelope,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspection {
    pub source: Source,
    pub envelope: Envelope,
    pub analysis: Analysis,
}
impl Inspection {
    pub fn coverage(&self) -> &'static str {
        match self.analysis {
            Analysis::ObjectIdentity(_) => "identity_only",
            Analysis::UnsupportedType { .. } => "unsupported_type",
            Analysis::ServiceDocument => "service_document_unanalyzed",
            Analysis::UnknownEnvelope => "unknown_envelope",
        }
    }
}
fn xml(code: Code, path: &str) -> Error {
    super::error(code, path).into()
}
fn full_hex(s: &str, lengths: &[usize]) -> bool {
    lengths.contains(&s.len())
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn blob_oid(raw: &[u8], len: usize) -> String {
    let header = format!("blob {}\0", raw.len());
    if len == 40 {
        let mut h = Sha1::new();
        h.update(header);
        h.update(raw);
        format!("{:x}", h.finalize())
    } else {
        let mut h = Sha256::new();
        h.update(header);
        h.update(raw);
        format!("{:x}", h.finalize())
    }
}
/// Use from ingest's callback or from saved BLOB bytes during offline verification.
/// Rechecks byte identity against supplied Git OID AND SHA-256 before XML parsing.
/// This proves byte identity, not repository/commit provenance without ingest's tree.
/// Input and DOM live for this call only; returned DTO contains no source/DOM copy.
/// Known metadata envelopes must declare 2.21. Service/unknown XML is never analyzed
/// as metadata; its version string is merely recorded, not interpreted as 1C format.
pub fn inspect(
    entry: &Entry,
    expected_sha256: &str,
    raw: &[u8],
    limits: Limits,
) -> Result<Inspection> {
    limits.validate()?;
    path_check(&entry.path)?;
    if !entry.path.ends_with(".xml") || ![0o100644, 0o100755].contains(&entry.mode) {
        return Err(xml(Code::Path, &entry.path));
    }
    // Check before hashing/allocation, including unknown/service inputs.
    if raw.len() > limits.bytes {
        return Err(xml(Code::Limit, &entry.path));
    }
    if !full_hex(&entry.oid, &[40, 64])
        || !full_hex(expected_sha256, &[64])
        || blob_oid(raw, entry.oid.len()) != entry.oid
        || format!("{:x}", Sha256::digest(raw)) != expected_sha256
    {
        return Err(Error::Identity);
    }
    let doc = parse_bounded(
        raw,
        &entry.path,
        ParseLimits {
            bytes: limits.bytes,
            nodes: limits.nodes,
            depth: limits.depth,
            events: limits.events,
        },
    )?;
    let root = doc.root_element();
    let envelope = Envelope {
        namespace: root.tag_name().namespace().map(str::to_owned),
        root: root.tag_name().name().into(),
        declared_version: root.attribute("version").map(str::to_owned),
    };
    let analysis = if matches(root, "MetaDataObject") {
        if root.attribute("version") != Some("2.21") {
            return Err(xml(Code::Version, &entry.path));
        }
        let children = elements(root);
        if children.len() != 1 || !no_text(root) {
            return Err(xml(Code::Shape, &entry.path));
        }
        let obj = children[0];
        if obj.tag_name().namespace() != Some(NS) {
            return Err(xml(Code::Namespace, &entry.path));
        }
        let kind = match obj.tag_name().name() {
            "Configuration" => Some(Kind::Configuration),
            "Catalog" => Some(Kind::Catalog),
            "Document" => Some(Kind::Document),
            "InformationRegister" => Some(Kind::InformationRegister),
            _ => None,
        };
        if let Some(kind) = kind {
            Analysis::ObjectIdentity(read_identity(obj, kind, &entry.path)?)
        } else {
            Analysis::UnsupportedType {
                type_name: obj.tag_name().name().into(),
            }
        }
    } else if entry.path == "ConfigDumpInfo.xml" {
        // Only path role is known. No claim about root/fields/version schema.
        Analysis::ServiceDocument
    } else {
        Analysis::UnknownEnvelope
    };
    Ok(Inspection {
        source: Source {
            path: entry.path.clone(),
            mode: entry.mode,
            oid: entry.oid.clone(),
            sha256: expected_sha256.into(),
            bytes: raw.len(),
        },
        envelope,
        analysis,
    })
}
fn read_identity(obj: roxmltree::Node<'_, '_>, kind: Kind, path: &str) -> Result<ObjectIdentity> {
    if !no_text(obj) {
        return Err(xml(Code::Shape, path));
    }
    let props: Vec<_> = elements(obj)
        .into_iter()
        .filter(|n| matches(*n, "Properties"))
        .collect();
    if props.len() != 1 || !no_text(props[0]) {
        return Err(xml(Code::Shape, path));
    }
    let names: Vec<_> = elements(props[0])
        .into_iter()
        .filter(|n| matches(*n, "Name"))
        .collect();
    if names.len() != 1 || !elements(names[0]).is_empty() || names[0].attributes().len() != 0 {
        return Err(xml(Code::Shape, path));
    }
    let name = scalar(names[0]);
    if !identifier(&name) {
        return Err(xml(Code::Identifier, path));
    }
    let expected = if kind == Kind::Configuration {
        "Configuration.xml".into()
    } else {
        format!("{}/{}.xml", kind.folder(), name)
    };
    if path != expected {
        return Err(xml(Code::NameMismatch, path));
    }
    let uid = obj.attribute("uuid");
    if uid.is_some_and(|s| !uuid(s)) {
        return Err(xml(Code::Uuid, path));
    }
    Ok(ObjectIdentity {
        kind,
        name,
        uuid: uid.map(str::to_owned),
    })
}
