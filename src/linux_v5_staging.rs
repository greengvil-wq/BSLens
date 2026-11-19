//! Linux S4: bounded descriptor copy before SQLite. No parser or facts hash here.
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::File,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

const NOFOLLOW: i32 = 0x20000;
const DIRECTORY: i32 = 0x10000;
const NONBLOCK: i32 = 0x800;
const CLOEXEC: i32 = 0x80000;
const WRONLY: i32 = 1;
const CREAT: i32 = 0x40;
const EXCL: i32 = 0x80;
static NEXT: AtomicU64 = AtomicU64::new(0);
unsafe extern "C" {
    fn openat(fd: i32, path: *const std::ffi::c_char, flags: i32, ...) -> i32;
    fn unlinkat(fd: i32, path: *const std::ffi::c_char, flags: i32) -> i32;
    fn geteuid() -> u32;
}

#[derive(Debug)]
pub enum Error {
    Path,
    Permissions,
    FileType,
    Limit,
    Name,
    Digest,
    Io(io::Error),
    Staging {
        directory: PathBuf,
        source: io::Error,
    },
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Staging { directory, source } => {
                write!(f, "v5 staging {}: {source}", directory.display())
            }
            other => write!(f, "v5: {other:?}"),
        }
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}
type Result<T> = std::result::Result<T, Error>;

fn at(dir: &File, name: &std::ffi::OsStr, flags: i32) -> Result<File> {
    let name = CString::new(name.as_bytes()).map_err(|_| Error::Path)?;
    // All traversals are relative to a pinned directory descriptor; never follow links.
    let fd = unsafe {
        openat(
            dir.as_raw_fd(),
            name.as_ptr(),
            flags | CLOEXEC | NOFOLLOW,
            0o600u32,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error().into());
    }
    Ok(unsafe { File::from_raw_fd(fd) })
}

fn components(path: &Path) -> Result<Vec<&std::ffi::OsStr>> {
    if !path.is_absolute() {
        return Err(Error::Path);
    }
    let mut names = Vec::new();
    for c in path.components() {
        match c {
            Component::RootDir => {}
            Component::Normal(s) => names.push(s),
            _ => return Err(Error::Path),
        }
    }
    if names.is_empty() {
        return Err(Error::Path);
    }
    Ok(names)
}

fn input(path: &Path, limit: u64) -> Result<File> {
    let names = components(path)?;
    let mut dir = File::open("/")?;
    for name in &names[..names.len() - 1] {
        dir = at(&dir, name, DIRECTORY)?;
    }
    let file = at(&dir, names[names.len() - 1], NONBLOCK)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(Error::FileType);
    }
    if meta.len() > limit {
        return Err(Error::Limit);
    }
    Ok(file)
}

fn staging_directory(path: &Path) -> Result<File> {
    let names = components(path)?;
    let uid = unsafe { geteuid() };
    let mut dir = File::open("/")?;
    for (i, name) in names.iter().enumerate() {
        let parent = dir.metadata()?;
        if !(parent.uid() == 0 || parent.uid() == uid)
            || !(parent.mode() & 0o022 == 0 || (parent.uid() == 0 && parent.mode() & 0o1000 != 0))
        {
            return Err(Error::Permissions);
        }
        dir = at(&dir, name, DIRECTORY)?;
        if i + 1 == names.len() {
            let m = dir.metadata()?;
            if m.uid() != uid || m.mode() & 0o7777 != 0o700 {
                return Err(Error::Permissions);
            }
        }
    }
    Ok(dir)
}

fn hex(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn identity(path: &Path, expected: Option<&str>) -> Result<(String, String)> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or(Error::Name)?;
    let (id, sha) = name
        .strip_suffix(".sqlite")
        .and_then(|n| n.rsplit_once('.'))
        .ok_or(Error::Name)?;
    if !id.strip_prefix("bslens-v5-").is_some_and(hex) || !hex(sha) {
        return Err(Error::Name);
    }
    if expected.is_some_and(|e| !hex(e) || e != sha) {
        return Err(Error::Digest);
    }
    Ok((id.to_owned(), sha.to_owned()))
}

/// Owns only this session's copy. Reader must drop its SQLite connection before this guard.
/// Kept crate-private until the shared S3 contract is integrated.
pub(crate) struct StagedSnapshot {
    directory: File,
    name: CString,
    pub(crate) path: PathBuf,
    pub(crate) snapshot_id: String,
}
impl Drop for StagedSnapshot {
    fn drop(&mut self) {
        unsafe {
            unlinkat(self.directory.as_raw_fd(), self.name.as_ptr(), 0);
        }
    }
}

pub(crate) fn stage(
    input_path: &Path,
    staging: &Path,
    expected: Option<&str>,
    limit: u64,
) -> Result<StagedSnapshot> {
    stage_with(input_path, staging, expected, limit, |f, bytes| {
        f.write_all(bytes)
    })
}

fn stage_with(
    input_path: &Path,
    staging: &Path,
    expected: Option<&str>,
    limit: u64,
    mut write: impl FnMut(&mut File, &[u8]) -> io::Result<()>,
) -> Result<StagedSnapshot> {
    let (snapshot_id, expected_sha) = identity(input_path, expected)?;
    let mut source = input(input_path, limit)?;
    let (mut copy, mut target) = temporary(staging)?;
    copy.snapshot_id = snapshot_id;
    let result = (|| {
        let meta = target.metadata()?;
        if meta.mode() & 0o7777 != 0o600 || meta.uid() != unsafe { geteuid() } || meta.nlink() != 1
        {
            return Err(Error::Permissions);
        }
        let mut hash = Sha256::new();
        let mut size = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = source.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            size = size
                .checked_add(n as u64)
                .filter(|n| *n <= limit)
                .ok_or(Error::Limit)?;
            write(&mut target, &buffer[..n]).map_err(|source| Error::Staging {
                directory: staging.to_owned(),
                source,
            })?;
            hash.update(&buffer[..n]);
        }
        if format!("{:x}", hash.finalize()) != expected_sha {
            return Err(Error::Digest);
        }
        Ok(())
    })();
    // Close writer before either removing a failed copy or passing it to SQLite.
    drop(target);
    result?;
    Ok(copy)
}

/// Exclusive owner-only scratch, removed by its guard after all handles close.
pub(crate) fn temporary(staging: &Path) -> Result<(StagedSnapshot, File)> {
    let directory = staging_directory(staging)?;
    let (name, target) = loop {
        let name = format!(
            "bslens-v5-reader-{}-{}.sqlite",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        match at(&directory, name.as_ref(), WRONLY | CREAT | EXCL) {
            Ok(f) => break (name, f),
            Err(Error::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(Error::Io(source)) => {
                return Err(Error::Staging {
                    directory: staging.to_owned(),
                    source,
                })
            }
            Err(e) => return Err(e),
        }
    };
    let copy = StagedSnapshot {
        directory,
        path: staging.join(&name),
        name: CString::new(name).map_err(|_| Error::Name)?,
        snapshot_id: String::new(),
    };
    let checked = target.metadata().map_err(Error::from).and_then(|meta| {
        if meta.mode() & 0o7777 != 0o600 || meta.uid() != unsafe { geteuid() } || meta.nlink() != 1
        {
            Err(Error::Permissions)
        } else {
            Ok(())
        }
    });
    if let Err(error) = checked {
        drop(target);
        return Err(error);
    }
    Ok((copy, target))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::{symlink, PermissionsExt};
    struct Fixture {
        root: PathBuf,
        staging: PathBuf,
    }
    impl Fixture {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!(
                "bslens-s4-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            let staging = root.join("stage");
            fs::create_dir(&staging).unwrap();
            fs::set_permissions(&staging, fs::Permissions::from_mode(0o700)).unwrap();
            Self { root, staging }
        }
        fn file(&self, bytes: &[u8]) -> PathBuf {
            let path = self
                .root
                .join(format!("bslens-v5-0000000000000000000000000000000000000000000000000000000000000000.{:x}.sqlite", Sha256::digest(bytes)));
            fs::write(&path, bytes).unwrap();
            path
        }
        fn count(&self) -> usize {
            fs::read_dir(&self.staging).unwrap().count()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.root).unwrap();
        }
    }

    #[test]
    fn hash_before_database_and_cleanup() {
        let f = Fixture::new();
        let path = f.file(b"not SQLite");
        fs::write(&path, b"corrupted!").unwrap();
        assert!(matches!(
            stage(&path, &f.staging, None, 100),
            Err(Error::Digest)
        ));
        assert_eq!(f.count(), 0);
        let p = f.file(b"not SQLite");
        assert!(stage(&p, &f.staging, Some(&"0".repeat(64)), 100).is_err());
        assert_eq!(f.count(), 0);
    }
    #[test]
    fn two_sessions_and_input_replacement() {
        let f = Fixture::new();
        let p = f.file(b"original");
        let a = stage(&p, &f.staging, None, 100).unwrap();
        let b = stage(&p, &f.staging, None, 100).unwrap();
        fs::rename(&p, f.root.join("old")).unwrap();
        fs::write(&p, b"replacement").unwrap();
        assert_eq!(fs::read(&a.path).unwrap(), b"original");
        assert_eq!(fs::metadata(&a.path).unwrap().mode() & 0o7777, 0o600);
        assert_eq!(
            a.snapshot_id,
            "bslens-v5-0000000000000000000000000000000000000000000000000000000000000000"
        );
        assert_eq!(f.count(), 2);
        drop(a);
        assert_eq!(f.count(), 1);
        assert_eq!(fs::read(&b.path).unwrap(), b"original");
        drop(b);
        assert_eq!(f.count(), 0);
    }
    #[test]
    fn enospc_after_partial_write_removes_only_own_copy() {
        let f = Fixture::new();
        let p = f.file(&[1; 100]);
        let keep = stage(&p, &f.staging, None, 100).unwrap();
        let error = stage_with(&p, &f.staging, None, 100, |file, bytes| {
            file.write_all(&bytes[..3])?;
            Err(io::Error::from_raw_os_error(28))
        });
        assert!(
            matches!(error,Err(Error::Staging { source, .. }) if source.raw_os_error()==Some(28))
        );
        assert_eq!(f.count(), 1);
        assert!(keep.path.exists());
    }
    #[test]
    fn file_types_links_and_limits() {
        let f = Fixture::new();
        let p = f.file(b"bytes");
        assert!(matches!(stage(&p, &f.staging, None, 4), Err(Error::Limit)));
        let link = f.root.join("link");
        symlink(&f.root, &link).unwrap();
        assert!(stage(&link.join(p.file_name().unwrap()), &f.staging, None, 100).is_err());
        fs::remove_file(&p).unwrap();
        symlink("missing", &p).unwrap();
        assert!(stage(&p, &f.staging, None, 100).is_err());
        fs::remove_file(&p).unwrap();
        fs::create_dir(&p).unwrap();
        assert!(matches!(
            stage(&p, &f.staging, None, 100),
            Err(Error::FileType)
        ));
        fs::remove_dir(&p).unwrap();
        unsafe extern "C" {
            fn mkfifo(path: *const std::ffi::c_char, mode: u32) -> i32;
        }
        let fifo = CString::new(p.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert!(matches!(
            stage(&p, &f.staging, None, 100),
            Err(Error::FileType)
        ));
        assert_eq!(f.count(), 0);
    }
    #[test]
    fn private_directory_required() {
        let f = Fixture::new();
        let p = f.file(b"bytes");
        fs::set_permissions(&f.staging, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(matches!(
            stage(&p, &f.staging, None, 100),
            Err(Error::Permissions)
        ));
        assert!(matches!(
            stage(&p, Path::new("relative"), None, 100),
            Err(Error::Path)
        ));
    }
    #[test]
    fn descriptor_survives_replacement_during_copy_and_growth_is_bounded() {
        let f = Fixture::new();
        let data = vec![7; 130_000];
        let p = f.file(&data);
        let mut replaced = false;
        let copy = stage_with(&p, &f.staging, None, 140_000, |file, bytes| {
            if !replaced {
                fs::rename(&p, f.root.join("old")).unwrap();
                fs::write(&p, b"replacement").unwrap();
                replaced = true;
            }
            file.write_all(bytes)
        })
        .unwrap();
        assert_eq!(fs::read(&copy.path).unwrap(), data);
        drop(copy);
        let p = f.file(&data);
        let mut grew = false;
        let result = stage_with(&p, &f.staging, None, 130_000, |file, bytes| {
            if !grew {
                fs::OpenOptions::new()
                    .append(true)
                    .open(&p)
                    .unwrap()
                    .write_all(&[9; 10])
                    .unwrap();
                grew = true;
            }
            file.write_all(bytes)
        });
        assert!(matches!(result, Err(Error::Limit)));
        assert_eq!(f.count(), 0);
    }
}
