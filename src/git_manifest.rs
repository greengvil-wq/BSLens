//! Exact bounded local Git objects. No revision expressions or caller-defined commands.
use sha1::Sha1;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
mod process;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    InvalidSha,
    Path,
    Layout,
    Limit,
    Process,
    Timeout,
    Object,
    Identity,
    UnsupportedType,
    UnsupportedPlatform,
}
#[derive(Debug)]
pub struct GitError {
    pub code: Code,
}
impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Git manifest: {:?}", self.code)
    }
}
impl std::error::Error for GitError {}
type Result<T> = std::result::Result<T, GitError>;
fn err(code: Code) -> GitError {
    GitError { code }
}
fn io<T>(r: std::io::Result<T>) -> Result<T> {
    r.map_err(|_| err(Code::Layout))
}
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub files: usize,
    pub bytes: usize,
    pub object_bytes: usize,
    pub objects: usize,
    pub deadline: Duration,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            files: 4096,
            bytes: 32 * 1024 * 1024,
            object_bytes: 8 * 1024 * 1024,
            objects: 8192,
            deadline: Duration::from_secs(30),
        }
    }
}
impl Limits {
    fn validate(self) -> Result<Self> {
        let max = Self::default();
        if self.files == 0
            || self.files > max.files
            || self.bytes == 0
            || self.bytes > max.bytes
            || self.object_bytes == 0
            || self.object_bytes > max.object_bytes
            || self.objects == 0
            || self.objects > max.objects
            || self.deadline.is_zero()
            || self.deadline > max.deadline
        {
            return Err(err(Code::Limit));
        }
        Ok(self)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    pub path: String,
    pub mode: u32,
    pub oid: String,
    pub sha256: String,
    bytes: Vec<u8>,
}
impl File {
    #[cfg(feature = "experimental-cache")]
    pub(crate) fn cached(path: String, mode: u32, oid: String, bytes: Vec<u8>) -> Result<Self> {
        if !valid_oid(&oid) || object_id("blob", &bytes, oid.len()) != oid {
            return Err(err(Code::Identity));
        }
        Ok(Self {
            path,
            mode,
            oid,
            sha256: hex(&Sha256::digest(&bytes)),
            bytes,
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}
#[derive(Debug)]
pub struct Manifest {
    #[cfg(feature = "experimental-cache")]
    checked_bytes: usize,
    commit: String,
    tree: String,
    files: Vec<File>,
}
impl Manifest {
    #[cfg(feature = "experimental-cache")]
    pub(crate) fn checked_bytes(&self) -> usize {
        self.checked_bytes
    }
    pub fn commit(&self) -> &str {
        &self.commit
    }
    pub fn tree(&self) -> &str {
        &self.tree
    }
    pub fn files(&self) -> &[File] {
        &self.files
    }
    /// Exact selected bytes; this does not turn XML facts into a ValidatedSnapshot.
    pub fn xml_inputs(&self) -> BTreeMap<String, Vec<u8>> {
        self.files
            .iter()
            .filter(|f| f.path.ends_with(".xml") || f.path.ends_with(".bsl"))
            .map(|f| (f.path.clone(), f.bytes.clone()))
            .collect()
    }
    #[cfg(feature = "experimental-cache")]
    pub(crate) fn inventory(repo: &Path, commit: &str) -> Result<Self> {
        Self::read_mode(repo, commit, Limits::default(), true)
    }

    pub fn read(repo: &Path, commit: &str, limits: Limits) -> Result<Self> {
        Self::read_mode(repo, commit, limits, false)
    }
    fn read_mode(repo: &Path, commit: &str, limits: Limits, inventory: bool) -> Result<Self> {
        if !valid_oid(commit) {
            return Err(err(Code::InvalidSha));
        }
        let limits = limits.validate()?;
        let deadline = Instant::now() + limits.deadline;
        let objects = object_store(repo, deadline)?;
        let shell = process::Shell::new(&objects, commit.len())?;
        let mut reader = ObjectReader {
            shell,
            inventory,
            limits,
            deadline,
            count: 0,
            bytes: 0,
            files: vec![],
            paths: BTreeSet::new(),
        };
        let raw = reader.object(commit, "commit")?;
        let first = raw
            .split(|&c| c == b'\n')
            .next()
            .ok_or_else(|| err(Code::Object))?;
        let first = std::str::from_utf8(first).map_err(|_| err(Code::Object))?;
        let tree = first
            .strip_prefix("tree ")
            .filter(|s| valid_oid(s) && s.len() == commit.len())
            .ok_or_else(|| err(Code::Object))?
            .to_string();
        // Require a complete header/body boundary. Other commit headers are opaque, not identity facts.
        if !raw.windows(2).any(|w| w == b"\n\n") {
            return Err(err(Code::Object));
        }
        reader.walk(&tree, "", 0)?;
        reader.shell.finish(deadline)?;
        reader.files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self {
            #[cfg(feature = "experimental-cache")]
            checked_bytes: reader.bytes,
            commit: commit.into(),
            tree,
            files: reader.files,
        })
    }
}
fn valid_oid(s: &str) -> bool {
    [40, 64].contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn hex(raw: &[u8]) -> String {
    raw.iter().map(|b| format!("{b:02x}")).collect()
}
fn object_id(kind: &str, raw: &[u8], len: usize) -> String {
    let mut v = format!("{kind} {}\0", raw.len()).into_bytes();
    v.extend(raw);
    if len == 40 {
        hex(&Sha1::digest(v))
    } else {
        hex(&Sha256::digest(v))
    }
}
fn safe_path(path: &Path) -> Result<PathBuf> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        io(std::env::current_dir())?.join(path)
    };
    let mut current = PathBuf::new();
    for part in abs.components() {
        if matches!(part, std::path::Component::ParentDir) {
            return Err(err(Code::Path));
        }
        current.push(part);
        if io(fs::symlink_metadata(&current))?.file_type().is_symlink() {
            return Err(err(Code::Layout));
        }
    }
    io(fs::canonicalize(abs))
}
fn object_store(repo: &Path, deadline: Instant) -> Result<PathBuf> {
    let repo = safe_path(repo)?;
    let git = repo.join(".git");
    if !io(fs::symlink_metadata(&git))?.is_dir() {
        return Err(err(Code::Layout));
    }
    let objects = safe_path(&git.join("objects"))?;
    if !objects.is_dir() {
        return Err(err(Code::Layout));
    }
    let mut stack = vec![(objects.clone(), 0)];
    let mut entries = 0;
    while let Some((dir, depth)) = stack.pop() {
        if depth > 64 {
            return Err(err(Code::Limit));
        }
        for entry in io(fs::read_dir(dir))? {
            if Instant::now() >= deadline {
                return Err(err(Code::Timeout));
            }
            entries += 1;
            if entries > 100_000 {
                return Err(err(Code::Limit));
            }
            let entry = io(entry)?;
            let kind = io(entry.file_type())?;
            let path = entry.path();
            if kind.is_symlink()
                || (!kind.is_file() && !kind.is_dir())
                || path
                    .file_name()
                    .is_some_and(|n| n == "alternates" || n == "http-alternates")
                || path.extension().is_some_and(|e| e == "promisor")
            {
                return Err(err(Code::Layout));
            }
            if kind.is_dir() {
                stack.push((path, depth + 1));
            }
        }
    }
    Ok(objects)
}
struct ObjectReader {
    inventory: bool,
    shell: process::Shell,
    limits: Limits,
    deadline: Instant,
    count: usize,
    bytes: usize,
    files: Vec<File>,
    paths: BTreeSet<String>,
}
impl ObjectReader {
    fn object(&mut self, oid: &str, kind: &str) -> Result<Vec<u8>> {
        if Instant::now() >= self.deadline {
            return Err(err(Code::Timeout));
        }
        self.count += 1;
        if self.count > self.limits.objects {
            return Err(err(Code::Limit));
        }
        let remaining = self
            .limits
            .bytes
            .checked_sub(self.bytes)
            .ok_or_else(|| err(Code::Limit))?;
        let cap = remaining.min(self.limits.object_bytes);
        let raw = self.shell.read(oid, kind, cap, self.deadline)?;
        let header_end = raw
            .iter()
            .position(|&b| b == b'\n')
            .ok_or_else(|| err(Code::Object))?;
        let header = std::str::from_utf8(&raw[..header_end]).map_err(|_| err(Code::Object))?;
        let parts: Vec<_> = header.split(' ').collect();
        if parts.len() != 3 || parts[0] != oid || parts[1] != kind {
            return Err(err(Code::Object));
        }
        let size: usize = parts[2].parse().map_err(|_| err(Code::Object))?;
        if size > cap || parts[2] != size.to_string() {
            return Err(err(Code::Limit));
        }
        let body = &raw[header_end + 1..];
        if body.len() != size + 1 || body.last() != Some(&b'\n') {
            return Err(err(Code::Object));
        }
        let body = &body[..size];
        if object_id(kind, body, oid.len()) != oid {
            return Err(err(Code::Identity));
        }
        self.bytes += body.len();
        Ok(body.to_vec())
    }
    fn walk(&mut self, oid: &str, parent: &str, depth: usize) -> Result<()> {
        if depth > 64 {
            return Err(err(Code::Limit));
        }
        let raw = self.object(oid, "tree")?;
        let mut offset = 0;
        let mut names = BTreeSet::new();
        while offset < raw.len() {
            let space = raw[offset..]
                .iter()
                .position(|&b| b == b' ')
                .ok_or_else(|| err(Code::Object))?
                + offset;
            let nul = raw[space + 1..]
                .iter()
                .position(|&b| b == 0)
                .ok_or_else(|| err(Code::Object))?
                + space
                + 1;
            let mode = &raw[offset..space];
            let name = std::str::from_utf8(&raw[space + 1..nul]).map_err(|_| err(Code::Path))?;
            if name.is_empty()
                || name == "."
                || name == ".."
                || name.contains(['/', '\\', ':'])
                || name.chars().any(|c| c < ' ')
                || !names.insert(name)
            {
                return Err(err(Code::Path));
            }
            let end = nul + 1 + oid.len() / 2;
            if end > raw.len() {
                return Err(err(Code::Object));
            }
            let child = hex(&raw[nul + 1..end]);
            offset = end;
            let path = if parent.is_empty() {
                name.to_string()
            } else {
                format!("{parent}/{name}")
            };
            if path.len() > 1024 {
                return Err(err(Code::Limit));
            }
            match mode {
                b"40000" => self.walk(&child, &path, depth + 1)?,
                b"100644" | b"100755" => {
                    if self.files.len() >= self.limits.files || !self.paths.insert(path.clone()) {
                        return Err(err(Code::Limit));
                    }
                    let bytes = if self.inventory {
                        self.count += 1;
                        if self.count > self.limits.objects {
                            return Err(err(Code::Limit));
                        }
                        Vec::new()
                    } else {
                        self.object(&child, "blob")?
                    };
                    self.files.push(File {
                        path,
                        mode: if mode == b"100644" {
                            0o100644
                        } else {
                            0o100755
                        },
                        oid: child,
                        sha256: hex(&Sha256::digest(&bytes)),
                        bytes,
                    });
                }
                _ => return Err(err(Code::UnsupportedType)),
            }
        }
        Ok(())
    }
}

/// Bounded opt-in ingest; legacy Manifest remains unchanged.
#[cfg(feature = "experimental-v5-ingest")]
pub mod ingest;
