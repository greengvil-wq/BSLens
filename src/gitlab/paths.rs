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
    let mut result = Finding::new(reason);
    result.evidence.rule_id = Some("R1".into());
    result
}
// Lossy UTF-8 makes otherwise unrepresentable path bytes readable without exposing blob content.
fn bad_path(reason: ReasonCode, path: &[u8]) -> Finding {
    let mut result = bad(reason);
    result
        .evidence
        .paths
        .push(String::from_utf8_lossy(path).into_owned());
    result
}
fn path_git_finding(error: super::process::GitError) -> Finding {
    let mut result = git_finding(error);
    result.evidence.rule_id = Some("R1".into());
    result
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
    let before_bytes = git.tree(&input.base, root).map_err(path_git_finding)?;
    let after_bytes = git.tree(&input.head, root).map_err(path_git_finding)?;
    if before_bytes.len() > limits.max_output_bytes || after_bytes.len() > limits.max_output_bytes {
        return Err(bad(ReasonCode::LimitExceeded));
    }
    let before = tree(&before_bytes)?;
    let after = tree(&after_bytes)?;
    let mut scope = BTreeSet::new();
    for (path, mode) in before.iter().chain(after.iter()) {
        if !supported(mode) {
            return Err(bad_path(ReasonCode::UnsupportedEntry, path));
        }
        std::str::from_utf8(path).map_err(|_| bad_path(ReasonCode::NonUtf8Path, path))?;
        scope.insert(path.clone());
    }
    let output = git
        .diff(&input.base, &input.head)
        .map_err(path_git_finding)?;
    if output.len() > limits.max_output_bytes {
        return Err(bad(ReasonCode::LimitExceeded));
    }
    let fields = records(&output)?;
    let mut i = 0;
    let mut changed = BTreeMap::new();
    while i < fields.len() {
        let header = fields[i];
        i += 1;
        let metadata = header
            .strip_prefix(b":")
            .ok_or_else(|| bad(ReasonCode::GitFailure))?;
        let parts: Vec<_> = metadata.split(|b| *b == b' ').collect();
        if parts.len() != 5
            || parts[2].len() != input.base.len()
            || parts[3].len() != input.head.len()
        {
            return Err(bad(ReasonCode::GitFailure));
        }
        let status = parts[4];
        if !(matches!(status, b"A" | b"M" | b"D" | b"T")
            || ((status.starts_with(b"R") || status.starts_with(b"C"))
                && status.len() > 1
                && status[1..].iter().all(u8::is_ascii_digit)))
        {
            return Err(bad(ReasonCode::UnsupportedEntry));
        }
        let kind = status[0];
        let count = if kind == b'R' || kind == b'C' { 2 } else { 1 };
        if i + count > fields.len() {
            return Err(bad(ReasonCode::GitFailure));
        }
        for n in 0..count {
            let path = fields[i + n];
            std::str::from_utf8(path).map_err(|_| bad_path(ReasonCode::NonUtf8Path, path))?;
            if [parts[0], parts[1]]
                .iter()
                .any(|m| *m != b"000000" && !supported(m))
            {
                return Err(bad_path(ReasonCode::UnsupportedEntry, path));
            }
            let change = match (kind, n) {
                (b'A', _) => Change::Added,
                (b'M' | b'T', _) => Change::Modified,
                (b'D', _) => Change::Deleted,
                (b'R' | b'C', 0) => Change::Deleted,
                (b'R' | b'C', 1) => Change::Added,
                _ => return Err(bad(ReasonCode::UnsupportedEntry)),
            };
            if changed.insert(path.to_vec(), change).is_some() {
                return Err(bad_path(ReasonCode::UnsupportedEntry, path));
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
