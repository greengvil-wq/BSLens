//! Experimental read-only projection; identity is not snapshot provenance.
use crate::xml221::{Kind, Xml221Subset};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub const MAX_PAGE: usize = 20;
pub const MAX_TEXT_BYTES: usize = 16384;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    Configuration,
    ObjectType,
    Object,
    Module,
    UnassignedModules,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
    pub address: Option<String>,
    pub ownership: Option<&'static str>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    pub completeness: &'static str,
    pub unverified_entries: usize,
    pub unassigned_modules: usize,
    pub methods: &'static str,
    pub provenance: &'static str,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub node: Node,
    pub items: Vec<Node>,
    pub total: usize,
    pub offset: usize,
    pub next_offset: Option<usize>,
    pub coverage: Coverage,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    InvalidProjection,
    UnknownNode,
    InvalidLimit,
    TextBudget,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for Error {}
pub struct Structure {
    nodes: Vec<Node>,
    children: Vec<Vec<usize>>,
    lookup: BTreeMap<String, usize>,
    coverage: Coverage,
}
impl Structure {
    pub fn from_subset(subset: &Xml221Subset) -> Result<Self, Error> {
        let configs: Vec<_> = subset
            .objects()
            .iter()
            .filter(|o| o.kind == Kind::Configuration)
            .collect();
        if configs.len() != 1 {
            return Err(Error::InvalidProjection);
        }
        let mut s = Self {
            nodes: vec![],
            children: vec![],
            lookup: BTreeMap::new(),
            coverage: Coverage {
                completeness: "XML 2.21 declared subset only; absence is not proof",
                unverified_entries: subset.unverified().len(),
                unassigned_modules: subset.unassigned_modules().len(),
                methods: "not implemented; no validated lexical facts",
                provenance: "not checked; projection identity is not snapshot identity",
            },
        };
        let config = configs[0];
        s.add(
            NodeKind::Configuration,
            &config.name,
            Some(&config.path),
            None,
            None,
        )?;
        let mut root_modules = config.modules.clone();
        root_modules.sort_by(|a, b| a.path.cmp(&b.path));
        for m in root_modules {
            s.add(
                NodeKind::Module,
                &m.kind,
                Some(&m.path),
                Some("xml-subset-exact-path"),
                Some(0),
            )?;
        }
        let mut grouped = BTreeMap::<&str, Vec<_>>::new();
        for o in subset
            .objects()
            .iter()
            .filter(|o| o.kind != Kind::Configuration)
        {
            grouped.entry(o.kind.name()).or_default().push(o);
        }
        for (kind, mut objects) in grouped {
            let group = s.add(NodeKind::ObjectType, kind, None, None, Some(0))?;
            objects.sort_by(|a, b| a.path.cmp(&b.path));
            for o in objects {
                let parent = s.add(NodeKind::Object, &o.name, Some(&o.path), None, Some(group))?;
                let mut modules = o.modules.clone();
                modules.sort_by(|a, b| a.path.cmp(&b.path));
                for m in modules {
                    s.add(
                        NodeKind::Module,
                        &m.kind,
                        Some(&m.path),
                        Some("xml-subset-exact-path"),
                        Some(parent),
                    )?;
                }
            }
        }
        if !subset.unassigned_modules().is_empty() {
            let group = s.add(
                NodeKind::UnassignedModules,
                "Modules with unverified owner",
                None,
                None,
                Some(0),
            )?;
            let mut paths = subset.unassigned_modules().to_vec();
            paths.sort();
            for p in paths {
                s.add(
                    NodeKind::Module,
                    &p,
                    Some(&p),
                    Some("unverified"),
                    Some(group),
                )?;
            }
        }
        // Length-prefixed fields avoid separator collisions. Scope covers this projection
        // plus UUID/unverified evidence, but deliberately does not attest code bytes or Git.
        let mut h = Sha256::new();
        field(&mut h, "bslens-structural-projection-v1");
        for (i, n) in s.nodes.iter().enumerate() {
            field(&mut h, &format!("{:?}", n.kind));
            field(&mut h, &n.label);
            field(&mut h, n.address.as_deref().unwrap_or(""));
            field(&mut h, n.ownership.unwrap_or(""));
            field(&mut h, &format!("{:?}", s.children[i]));
        }
        let mut objects = subset.objects().to_vec();
        objects.sort_by(|a, b| a.path.cmp(&b.path));
        for o in objects {
            field(&mut h, &o.path);
            field(&mut h, o.uuid.as_deref().unwrap_or(""));
        }
        let mut unverified = subset.unverified().to_vec();
        unverified.sort_by(|a, b| (&a.path, &a.selector).cmp(&(&b.path, &b.selector)));
        for u in unverified {
            field(&mut h, &u.path);
            field(&mut h, &u.selector);
        }
        let scope = format!("{:x}", h.finalize());
        for (i, n) in s.nodes.iter_mut().enumerate() {
            n.id = format!("p1:{scope}:{i}");
            s.lookup.insert(n.id.clone(), i);
        }
        Ok(s)
    }
    fn add(
        &mut self,
        kind: NodeKind,
        label: &str,
        address: Option<&str>,
        ownership: Option<&'static str>,
        parent: Option<usize>,
    ) -> Result<usize, Error> {
        if label.len() > 1024 || address.is_some_and(|p| p.len() > 1024) {
            return Err(Error::TextBudget);
        }
        let i = self.nodes.len();
        self.nodes.push(Node {
            id: String::new(),
            kind,
            label: label.into(),
            address: address.map(str::to_owned),
            ownership,
        });
        self.children.push(vec![]);
        if let Some(p) = parent {
            self.children[p].push(i);
        }
        Ok(i)
    }
    pub fn page(&self, node_id: Option<&str>, offset: usize, limit: usize) -> Result<Page, Error> {
        if !(1..=MAX_PAGE).contains(&limit) {
            return Err(Error::InvalidLimit);
        }
        let i = match node_id {
            None => 0,
            Some(id) => *self.lookup.get(id).ok_or(Error::UnknownNode)?,
        };
        let children = &self.children[i];
        let selected: Vec<_> = children
            .get(offset..)
            .unwrap_or(&[])
            .iter()
            .take(limit)
            .copied()
            .collect();
        let bytes = std::iter::once(i)
            .chain(selected.iter().copied())
            .map(|j| text_bytes(&self.nodes[j]))
            .sum::<usize>();
        if bytes > MAX_TEXT_BYTES {
            return Err(Error::TextBudget);
        }
        let next_offset =
            if selected.is_empty() || selected.len() == children.len().saturating_sub(offset) {
                None
            } else {
                Some(offset + selected.len())
            };
        Ok(Page {
            node: self.nodes[i].clone(),
            items: selected
                .into_iter()
                .map(|j| self.nodes[j].clone())
                .collect(),
            total: children.len(),
            offset,
            next_offset,
            coverage: self.coverage.clone(),
        })
    }
}
fn field(h: &mut Sha256, s: &str) {
    h.update((s.len() as u64).to_le_bytes());
    h.update(s.as_bytes());
}
fn text_bytes(n: &Node) -> usize {
    n.id.len()
        + n.label.len()
        + n.address.as_deref().map_or(0, str::len)
        + n.ownership.map_or(0, str::len)
}
