//! Experimental composition of checked Git objects and the limited XML structure.
//! This type cannot create a ValidatedSnapshot.
use crate::{
    git_manifest::{self, Limits, Manifest},
    structure::{self, Page, Structure, MAX_TEXT_BYTES},
    xml221::{self, Xml221Subset},
};
use std::path::Path;

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
}
#[derive(Debug)]
pub enum Error {
    Git(git_manifest::Code),
    Xml(xml221::XmlError),
    Structure(structure::Error),
    ContextMismatch,
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
        let mut page = self
            .structure
            .page(projection_id, offset, limit)
            .map_err(Error::Structure)?;
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
            .sum();
        if bytes > MAX_TEXT_BYTES {
            return Err(Error::Structure(structure::Error::TextBudget));
        }
        Ok(BoundPage {
            context: self.context.clone(),
            page,
        })
    }
}
