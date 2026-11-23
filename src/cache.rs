//! Opt-in Linux cache of checked selected bytes; never a ValidatedSnapshot.
use crate::{
    git_manifest::{File, Limits, Manifest},
    pipeline::{Context, GitStructure},
};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const MAGIC: &[u8; 16] = b"BSLens-cache-v1\0";
const MAX_CACHE: usize = 40 * 1024 * 1024;
const NOFOLLOW: i32 = 0x20000; // Linux O_NOFOLLOW, this API explicitly refuses other platforms.
static NEXT: AtomicU64 = AtomicU64::new(0);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Platform,
    Path,
    Permissions,
    Io,
    Missing,
    Corrupt,
    Version,
    Identity,
    Limit,
    Git,
    Projection,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cache: {self:?}")
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;
fn valid_oid(s: &str) -> bool {
    [40, 64].contains(&s.len())
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn selected(path: &str) -> bool {
    path.ends_with(".xml") || path.ends_with(".bsl")
}
fn key(commit: &str) -> Result<String> {
    if !valid_oid(commit) {
        return Err(Error::Identity);
    }
    Ok(format!("v1-{commit}.bslcache"))
}
#[cfg(unix)]
fn uid() -> u32 {
    unsafe extern "C" {
        fn geteuid() -> u32;
    }
    unsafe { geteuid() }
}
#[cfg(unix)]
fn private(meta: &fs::Metadata, dir: bool) -> Result<()> {
    if meta.uid() != uid()
        || (meta.mode() & 0o7777) != if dir { 0o700 } else { 0o600 }
        || if dir {
            !meta.is_dir()
        } else {
            !meta.is_file() || meta.nlink() != 1
        }
    {
        return Err(Error::Permissions);
    }
    Ok(())
}
#[cfg(unix)]
fn directory(path: &Path, create: bool) -> Result<PathBuf> {
    if !cfg!(target_os = "linux") {
        return Err(Error::Platform);
    }
    if !path.is_absolute() {
        return Err(Error::Path);
    }
    let mut part = PathBuf::new();
    let components: Vec<_> = path.components().collect();
    for (i, c) in components.iter().enumerate() {
        if !matches!(c, Component::RootDir | Component::Normal(_)) {
            return Err(Error::Path);
        }
        part.push(c.as_os_str());
        let last = i + 1 == components.len();
        let meta = match fs::symlink_metadata(&part) {
            Ok(m) => m,
            Err(e) if last && create && e.kind() == std::io::ErrorKind::NotFound => {
                let mut b = fs::DirBuilder::new();
                b.mode(0o700);
                match b.create(&part) {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(_) => return Err(Error::Io),
                }
                fs::symlink_metadata(&part).map_err(|_| Error::Io)?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(Error::Missing),
            Err(_) => return Err(Error::Io),
        };
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err(Error::Path);
        }
        if last {
            private(&meta, true)?;
        } else if !safe_ancestor(meta.uid(), meta.mode(), uid()) {
            return Err(Error::Permissions);
        }
    }
    Ok(part)
}
#[cfg(not(unix))]
fn directory(_: &Path, _: bool) -> Result<PathBuf> {
    Err(Error::Platform)
}

/// Build/rebuild explicitly from a full bounded Git read. Only selected XML/BSL bytes are cached.
pub fn build(repo: &Path, commit: &str, cache_dir: &Path) -> Result<GitStructure> {
    let name = key(commit)?;
    let dir = directory(cache_dir, true)?;
    let manifest = Manifest::read(repo, commit, Limits::default()).map_err(|_| Error::Git)?;
    let core = GitStructure::from_manifest(&manifest).map_err(|_| Error::Projection)?;
    let data = encode(&manifest)?;
    publish(&dir, &name, &data)?;
    Ok(core)
}
/// Open without fallback. A miss/corruption must be handled explicitly by the caller.
pub fn open(repo: &Path, commit: &str, cache_dir: &Path) -> Result<GitStructure> {
    let name = key(commit)?;
    let dir = directory(cache_dir, false)?;
    let raw = read_file(&dir.join(name))?;
    let (context, files) = decode(&raw, commit)?;
    // Real commit/tree objects bind every cached path/mode/OID, not just a supplied SHA.
    let inventory = Manifest::inventory(repo, commit).map_err(|_| Error::Git)?;
    let selected_bytes: usize = files.iter().map(|f| f.bytes().len()).sum();
    checked_total(inventory.checked_bytes(), selected_bytes)?;

    if inventory.tree() != context.tree
        || inventory.files().len() != context.selected_files + context.other_files
    {
        return Err(Error::Identity);
    }
    let entries: Vec<_> = inventory
        .files()
        .iter()
        .filter(|f| selected(&f.path))
        .collect();
    if entries.len() != files.len() {
        return Err(Error::Identity);
    }
    for (expected, actual) in entries.into_iter().zip(&files) {
        if expected.path != actual.path
            || expected.mode != actual.mode
            || expected.oid != actual.oid
        {
            return Err(Error::Identity);
        }
    }
    GitStructure::from_cached(context, files).map_err(|_| Error::Projection)
}
#[cfg(unix)]
fn read_file(path: &Path) -> Result<Vec<u8>> {
    let mut f = OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW | 0x800)
        .open(path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::Missing
            } else {
                Error::Io
            }
        })?;
    let meta = f.metadata().map_err(|_| Error::Io)?;
    private(&meta, false)?;
    if meta.len() > MAX_CACHE as u64 {
        return Err(Error::Limit);
    }
    let mut raw = Vec::with_capacity(meta.len() as usize);
    (&mut f)
        .take(MAX_CACHE as u64 + 1)
        .read_to_end(&mut raw)
        .map_err(|_| Error::Io)?;
    if raw.len() > MAX_CACHE {
        return Err(Error::Limit);
    }
    if raw.len() as u64 != meta.len() {
        return Err(Error::Corrupt);
    }
    Ok(raw)
}
#[cfg(not(unix))]
fn read_file(_: &Path) -> Result<Vec<u8>> {
    Err(Error::Platform)
}
#[cfg(unix)]
fn publish(dir: &Path, name: &str, raw: &[u8]) -> Result<()> {
    let target = dir.join(name);
    match fs::symlink_metadata(&target) {
        Ok(m) => {
            private(&m, false)?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(Error::Io),
    }
    let temp = dir.join(format!(
        ".tmp-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(NOFOLLOW | 0x800)
        .open(&temp)
        .map_err(|_| Error::Io)?;
    let result = (|| {
        f.write_all(raw).map_err(|_| Error::Io)?;
        f.sync_all().map_err(|_| Error::Io)?;
        drop(f);
        fs::rename(&temp, &target).map_err(|_| Error::Io)?;
        fs::File::open(dir)
            .and_then(|d| d.sync_all())
            .map_err(|_| Error::Io)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
#[cfg(not(unix))]
fn publish(_: &Path, _: &str, _: &[u8]) -> Result<()> {
    Err(Error::Platform)
}
fn put(raw: &mut Vec<u8>, bytes: &[u8]) -> Result<()> {
    if raw
        .len()
        .checked_add(4)
        .and_then(|n| n.checked_add(bytes.len()))
        .is_none_or(|n| n > MAX_CACHE - 32)
    {
        return Err(Error::Limit);
    }
    raw.extend_from_slice(&(u32::try_from(bytes.len()).map_err(|_| Error::Limit)?).to_le_bytes());
    raw.extend_from_slice(bytes);
    Ok(())
}
fn encode(m: &Manifest) -> Result<Vec<u8>> {
    let selected: Vec<_> = m.files().iter().filter(|f| selected(&f.path)).collect();
    let mut raw = MAGIC.to_vec();
    put(&mut raw, m.commit().as_bytes())?;
    put(&mut raw, m.tree().as_bytes())?;
    raw.extend_from_slice(&(selected.len() as u32).to_le_bytes());
    raw.extend_from_slice(&((m.files().len() - selected.len()) as u32).to_le_bytes());
    for f in selected {
        put(&mut raw, f.path.as_bytes())?;
        raw.extend_from_slice(&f.mode.to_le_bytes());
        put(&mut raw, f.oid.as_bytes())?;
        put(&mut raw, f.bytes())?;
    }
    let digest = Sha256::digest(&raw);
    raw.extend_from_slice(&digest);
    Ok(raw)
}
struct Decoder<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Decoder<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or(Error::Limit)?;
        let s = self.bytes.get(self.at..end).ok_or(Error::Corrupt)?;
        self.at = end;
        Ok(s)
    }
    fn number(&mut self) -> Result<usize> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().map_err(|_| Error::Corrupt)?) as usize)
    }
    fn bytes(&mut self, cap: usize) -> Result<&'a [u8]> {
        let n = self.number()?;
        if n > cap {
            return Err(Error::Limit);
        }
        self.take(n)
    }
    fn text(&mut self, cap: usize) -> Result<String> {
        String::from_utf8(self.bytes(cap)?.to_vec()).map_err(|_| Error::Corrupt)
    }
}
fn decode(raw: &[u8], commit: &str) -> Result<(Context, Vec<File>)> {
    if raw.len() > MAX_CACHE {
        return Err(Error::Limit);
    }
    if raw.len() < MAGIC.len() + 32 {
        return Err(Error::Corrupt);
    }
    let (data, digest) = raw.split_at(raw.len() - 32);
    if Sha256::digest(data).as_slice() != digest {
        return Err(Error::Corrupt);
    }
    let mut d = Decoder { bytes: data, at: 0 };
    if d.take(16)? != MAGIC {
        return Err(Error::Version);
    }
    let actual_commit = d.text(64)?;
    let tree = d.text(64)?;
    if actual_commit != commit || !valid_oid(&tree) || tree.len() != commit.len() {
        return Err(Error::Identity);
    }
    let selected_files = d.number()?;
    let other_files = d.number()?;
    let limits = Limits::default();
    if selected_files
        .checked_add(other_files)
        .is_none_or(|n| n > limits.files)
    {
        return Err(Error::Limit);
    }
    let mut files = Vec::with_capacity(selected_files);
    let mut total = 0usize;
    let mut last = String::new();
    for _ in 0..selected_files {
        let path = d.text(1024)?;
        let mode = d.number()? as u32;
        let oid = d.text(64)?;
        if !selected(&path)
            || path <= last
            || ![0o100644, 0o100755].contains(&mode)
            || oid.len() != commit.len()
        {
            return Err(Error::Identity);
        }
        last = path.clone();
        let bytes = d.bytes(limits.object_bytes)?;
        total = total.checked_add(bytes.len()).ok_or(Error::Limit)?;
        if total > limits.bytes {
            return Err(Error::Limit);
        }
        files.push(File::cached(path, mode, oid, bytes.to_vec()).map_err(|_| Error::Identity)?);
    }
    if d.at != data.len() {
        return Err(Error::Corrupt);
    }
    Ok((
        Context {
            commit: actual_commit,
            tree,
            selected_files,
            other_files,
        },
        files,
    ))
}

fn safe_ancestor(owner: u32, mode: u32, current: u32) -> bool {
    (owner == 0 || owner == current) && (mode & 0o022 == 0 || (owner == 0 && mode & 0o1000 != 0))
}
fn checked_total(tree_bytes: usize, selected_bytes: usize) -> Result<()> {
    if tree_bytes
        .checked_add(selected_bytes)
        .is_none_or(|n| n > Limits::default().bytes)
    {
        Err(Error::Limit)
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ancestor_uid_and_combined_budget_boundaries() {
        assert!(safe_ancestor(0, 0o755, 1000));
        assert!(safe_ancestor(1000, 0o700, 1000));
        assert!(safe_ancestor(0, 0o1777, 1000));
        assert!(!safe_ancestor(1001, 0o755, 1000));
        assert!(!safe_ancestor(1001, 0o1777, 1000));
        assert!(!safe_ancestor(1000, 0o777, 1000));
        assert!(checked_total(1024, Limits::default().bytes - 1024).is_ok());
        assert_eq!(
            checked_total(1024, Limits::default().bytes - 1023),
            Err(Error::Limit)
        );
        assert_eq!(checked_total(usize::MAX, 1), Err(Error::Limit));
    }
}
