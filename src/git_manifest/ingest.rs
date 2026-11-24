//! Opt-in bounded inventory + file-at-a-time ingest. No snapshot capability.
use super::{hex, object_id, object_store, process, valid_oid, Code};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    path::Path,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub entries: usize,
    pub trees: usize,
    pub reads: usize,
    pub object_bytes: usize,
    /// All commit/tree/selected blob bodies, including repeated OIDs.
    pub total_bytes: usize,
    pub deadline: Duration,
}
impl Limits {
    /// Hard ceilings for this experimental profile; callers must supply limits.
    pub const MAX: Self = Self {
        entries: 100_000,
        trees: 20_000,
        reads: 120_001,
        object_bytes: 16 * 1024 * 1024,
        total_bytes: 1024 * 1024 * 1024,
        deadline: Duration::from_secs(300),
    };
    pub(crate) fn validate(self) -> Result<()> {
        for (name, value, ceiling) in [
            ("entries", self.entries, Self::MAX.entries),
            ("trees", self.trees, Self::MAX.trees),
            ("reads", self.reads, Self::MAX.reads),
            ("object_bytes", self.object_bytes, Self::MAX.object_bytes),
            ("total_bytes", self.total_bytes, Self::MAX.total_bytes),
        ] {
            if value == 0 || value > ceiling {
                return Err(Error::InvalidLimit { name });
            }
        }
        if self.deadline.is_zero() || self.deadline > Self::MAX.deadline {
            return Err(Error::InvalidLimit { name: "deadline" });
        }
        Ok(())
    }
}
#[derive(Debug)]
pub enum Error {
    InvalidLimit {
        name: &'static str,
    },
    Limit {
        name: &'static str,
        maximum: usize,
        actual: usize,
    },
    Git(Code),
    /// Backend refuses before allocating a body larger than cap. Code preserves
    /// malformed protocol vs identity vs deadline; Limit does not imply a size.
    Read {
        oid: String,
        code: Code,
        cap: usize,
        budget: &'static str,
    },
    Consumer(String),
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v5 ingest: {self:?}")
    }
}
impl std::error::Error for Error {}
impl From<super::GitError> for Error {
    fn from(e: super::GitError) -> Self {
        Self::Git(e.code)
    }
}
pub type Result<T> = std::result::Result<T, Error>;
fn check(name: &'static str, maximum: usize, actual: usize) -> Result<()> {
    if actual > maximum {
        Err(Error::Limit {
            name,
            maximum,
            actual,
        })
    } else {
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub mode: u32,
    pub oid: String,
}
impl Entry {
    pub fn selected(&self) -> bool {
        self.path.ends_with(".xml") || self.path.ends_with(".bsl")
    }
}
#[derive(Debug)]
pub struct Manifest {
    commit: String,
    tree: String,
    entries: Vec<Entry>,
}
impl Manifest {
    pub fn commit(&self) -> &str {
        &self.commit
    }
    pub fn tree(&self) -> &str {
        &self.tree
    }
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}
/// Successful ingest receipt. Unselected blobs are inventoried, not read or hashed.
#[derive(Debug)]
pub struct Receipt {
    pub manifest: Manifest,
    pub reads: usize,
    pub bytes: usize,
    pub selected: usize,
}
struct Reader {
    shell: process::Shell,
    limits: Limits,
    deadline: Instant,
    reads: usize,
    bytes: usize,
    trees: usize,
    entry_count: usize,
    entries: Vec<Entry>,
}
impl Reader {
    fn time(&self) -> Result<()> {
        if Instant::now() >= self.deadline {
            Err(Error::Git(Code::Timeout))
        } else {
            Ok(())
        }
    }
    fn object(&mut self, oid: &str, kind: &str) -> Result<Vec<u8>> {
        self.time()?;
        check("reads", self.limits.reads, self.reads + 1)?;
        self.reads += 1;
        let remaining = self.limits.total_bytes - self.bytes;
        let cap = remaining.min(self.limits.object_bytes);
        let budget = if remaining < self.limits.object_bytes {
            "total_bytes"
        } else {
            "object_bytes"
        };
        let mut raw = self
            .shell
            .read(oid, kind, cap, self.deadline)
            .map_err(|e| Error::Read {
                oid: oid.into(),
                code: e.code,
                cap,
                budget,
            })?;
        // Shell has validated exact header/type/size/framing before allocation.
        let start = raw
            .iter()
            .position(|&b| b == b'\n')
            .ok_or(Error::Git(Code::Object))?
            + 1;
        raw.pop();
        raw.drain(..start);
        if object_id(kind, &raw, oid.len()) != oid {
            return Err(Error::Git(Code::Identity));
        }
        self.bytes += raw.len();
        self.time()?;
        Ok(raw)
    }
    fn walk(&mut self, oid: &str, parent: &str, depth: usize) -> Result<()> {
        check("depth", 64, depth)?;
        check("trees", self.limits.trees, self.trees + 1)?;
        self.trees += 1;
        let raw = self.object(oid, "tree")?;
        let mut offset = 0;
        let mut names = BTreeSet::new();
        // Parse one bounded tree before descending, releasing its raw body.
        let mut children = Vec::new();
        while offset < raw.len() {
            self.time()?;
            check("entries", self.limits.entries, self.entry_count + 1)?;
            self.entry_count += 1;
            let space = raw[offset..]
                .iter()
                .position(|&b| b == b' ')
                .ok_or(Error::Git(Code::Object))?
                + offset;
            let nul = raw[space + 1..]
                .iter()
                .position(|&b| b == 0)
                .ok_or(Error::Git(Code::Object))?
                + space
                + 1;
            let mode = &raw[offset..space];
            let name =
                std::str::from_utf8(&raw[space + 1..nul]).map_err(|_| Error::Git(Code::Path))?;
            if name.is_empty()
                || name == "."
                || name == ".."
                || name.contains(['/', '\\', ':'])
                || name.chars().any(|c| c < ' ')
                || !names.insert(name)
            {
                return Err(Error::Git(Code::Path));
            }
            let end = nul + 1 + oid.len() / 2;
            if end > raw.len() {
                return Err(Error::Git(Code::Object));
            }
            let child = hex(&raw[nul + 1..end]);
            offset = end;
            let path = if parent.is_empty() {
                name.to_string()
            } else {
                format!("{parent}/{name}")
            };
            check("path_bytes", 1024, path.len())?;
            match mode {
                b"40000" => children.push((child, path)),
                b"100644" | b"100755" => self.entries.push(Entry {
                    path,
                    mode: if mode == b"100644" {
                        0o100644
                    } else {
                        0o100755
                    },
                    oid: child,
                }),
                _ => return Err(Error::Git(Code::UnsupportedType)),
            }
        }
        drop(names);
        drop(raw);
        for (oid, path) in children {
            self.walk(&oid, &path, depth + 1)?;
        }
        Ok(())
    }
}
/// Verifies the full commit/tree inventory before invoking consumer in sorted path
/// order. Only exact lowercase .xml/.bsl blobs are read. SHA-256 describes verified
/// original bytes, not a second externally supplied expected hash. Consumer must
/// stage/rollback its own writes: an error can follow successful earlier callbacks.
/// Deadline includes callbacks but cannot preempt arbitrary caller code.
pub fn ingest<F>(
    repo: &Path,
    commit: &str,
    expected_tree: &str,
    limits: Limits,
    mut consumer: F,
) -> Result<Receipt>
where
    F: FnMut(&Entry, &str, &[u8]) -> std::result::Result<(), String>,
{
    if !valid_oid(commit) || !valid_oid(expected_tree) || commit.len() != expected_tree.len() {
        return Err(Error::Git(Code::InvalidSha));
    }
    limits.validate()?;
    let deadline = Instant::now() + limits.deadline;
    let objects = object_store(repo, deadline)?;
    let mut reader = Reader {
        shell: process::Shell::new(&objects, commit.len())?,
        limits,
        deadline,
        reads: 0,
        bytes: 0,
        trees: 0,
        entry_count: 0,
        entries: Vec::new(),
    };
    let raw = reader.object(commit, "commit")?;
    let first = raw
        .split(|&b| b == b'\n')
        .next()
        .ok_or(Error::Git(Code::Object))?;
    let tree = std::str::from_utf8(first)
        .ok()
        .and_then(|s| s.strip_prefix("tree "))
        .filter(|s| valid_oid(s) && s.len() == commit.len())
        .ok_or(Error::Git(Code::Object))?;
    if !raw.windows(2).any(|w| w == b"\n\n") {
        return Err(Error::Git(Code::Object));
    }
    if tree != expected_tree {
        return Err(Error::Git(Code::Identity));
    }
    drop(raw);
    reader.walk(expected_tree, "", 0)?;
    let mut entries = std::mem::take(&mut reader.entries);
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    let selected = entries.iter().filter(|e| e.selected()).count();
    check("reads", limits.reads, reader.reads + selected)?;
    for entry in entries.iter().filter(|e| e.selected()) {
        let raw = reader.object(&entry.oid, "blob")?;
        let sha256 = hex(&Sha256::digest(&raw));
        reader.time()?;
        consumer(entry, &sha256, &raw).map_err(Error::Consumer)?;
        reader.time()?;
    }
    reader.shell.finish(deadline)?;
    Ok(Receipt {
        manifest: Manifest {
            commit: commit.into(),
            tree: expected_tree.into(),
            entries,
        },
        reads: reader.reads,
        bytes: reader.bytes,
        selected,
    })
}
