//! Local changed paths and the configuration-scope denominator.
use super::graph::git_finding;
use super::process::GitReader;
use super::types::{Change, ChangedPath, Finding, Limits, LocalInput, ReasonCode, VendorPolicy};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathSet {
    pub paths: Vec<ChangedPath>,
    pub scope_count: usize,
}
fn bad(reason: ReasonCode) -> Finding {
    Finding::new(reason)
}
fn records(bytes: &[u8]) -> Result<Vec<&[u8]>, Finding> {
    if !bytes.is_empty() && !bytes.ends_with(&[0]) {
        return Err(bad(ReasonCode::GitFailure));
    }
    Ok(bytes.split(|b| *b == 0).filter(|x| !x.is_empty()).collect())
}
fn tree(bytes: &[u8]) -> Result<BTreeMap<Vec<u8>, Vec<u8>>, Finding> {
    let mut result = BTreeMap::new();
    for entry in records(bytes)? {
        let tab = entry
            .iter()
            .position(|b| *b == b'\t')
            .ok_or_else(|| bad(ReasonCode::GitFailure))?;
        let mode = entry[..tab]
            .split(|b| *b == b' ')
            .next()
            .ok_or_else(|| bad(ReasonCode::GitFailure))?;
        result.insert(entry[tab + 1..].to_vec(), mode.to_vec());
    }
    Ok(result)
}
fn supported(mode: &[u8]) -> bool {
    mode == b"100644" || mode == b"100755"
}
fn in_root(path: &[u8], root: &str) -> bool {
    root == "."
        || path == root.as_bytes()
        || path
            .strip_prefix(root.as_bytes())
            .is_some_and(|p| p.starts_with(b"/"))
}
pub fn changed_paths(
    input: &LocalInput,
    policy: &VendorPolicy,
    limits: &Limits,
    git: &dyn GitReader,
) -> Result<PathSet, Finding> {
    let root = policy.config_root.trim_end_matches('/');
    if root.is_empty()
        || (root != "."
            && (root.starts_with('/')
                || root
                    .split('/')
                    .any(|p| p.is_empty() || p == "." || p == "..")))
    {
        return Err(bad(ReasonCode::GitFailure));
    }
    let before = tree(&git.tree(&input.base, ".").map_err(git_finding)?)?;
    let after = tree(&git.tree(&input.head, ".").map_err(git_finding)?)?;
    let mut scope = BTreeSet::new();
    for (path, mode) in before.iter().chain(after.iter()) {
        if in_root(path, root) {
            if !supported(mode) {
                return Err(bad(ReasonCode::UnsupportedEntry));
            }
            std::str::from_utf8(path).map_err(|_| bad(ReasonCode::NonUtf8Path))?;
            scope.insert(path.clone());
        }
    }
    let output = git.diff(&input.base, &input.head).map_err(git_finding)?;
    let fields = records(&output)?;
    let mut i = 0;
    let mut changed = BTreeMap::new();
    while i < fields.len() {
        let status = fields[i];
        i += 1;
        let kind = *status.first().ok_or_else(|| bad(ReasonCode::GitFailure))?;
        let count = if kind == b'R' || kind == b'C' { 2 } else { 1 };
        if i + count > fields.len() {
            return Err(bad(ReasonCode::GitFailure));
        }
        for n in 0..count {
            let path = fields[i + n];
            std::str::from_utf8(path).map_err(|_| bad(ReasonCode::NonUtf8Path))?;
            if before
                .get(path)
                .into_iter()
                .chain(after.get(path))
                .any(|m| !supported(m))
            {
                return Err(bad(ReasonCode::UnsupportedEntry));
            }
            let change = match (kind, n) {
                (b'A', _) => Change::Added,
                (b'M', _) => Change::Modified,
                (b'D', _) => Change::Deleted,
                (b'R' | b'C', 0) => Change::Deleted,
                (b'R' | b'C', 1) => Change::Added,
                _ => return Err(bad(ReasonCode::UnsupportedEntry)),
            };
            if changed.insert(path.to_vec(), change).is_some() {
                return Err(bad(ReasonCode::UnsupportedEntry));
            }
            if changed.len() > limits.max_paths {
                return Err(bad(ReasonCode::LimitExceeded));
            }
        }
        i += count;
    }
    Ok(PathSet {
        paths: changed
            .into_iter()
            .map(|(path, change)| ChangedPath {
                path: String::from_utf8(path).expect("validated UTF-8"),
                change,
            })
            .collect(),
        scope_count: scope.len(),
    })
}
