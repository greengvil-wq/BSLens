//! Публикация в приватном каталоге Linux x86_64. Каталог не меняется недоверенными процессами.
#![cfg(all(target_os = "linux", target_arch = "x86_64"))]
use super::{contract, BuiltSnapshot, Error, Result};
use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};
use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    io::Read,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        },
    },
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
const NOFOLLOW: i32 = 0x20000;
const NONBLOCK: i32 = 0x800;
static NEXT: AtomicU64 = AtomicU64::new(0);
unsafe extern "C" {
    fn geteuid() -> u32;
    fn flock(fd: i32, operation: i32) -> i32;
    fn renameat2(
        oldfd: i32,
        old: *const std::ffi::c_char,
        newfd: i32,
        new: *const std::ffi::c_char,
        flags: u32,
    ) -> i32;
}
fn private(meta: &fs::Metadata, dir: bool) -> Result<()> {
    if meta.uid() != unsafe { geteuid() }
        || meta.mode() & 0o7777 != if dir { 0o700 } else { 0o600 }
        || if dir {
            !meta.is_dir()
        } else {
            !meta.is_file() || meta.nlink() != 1
        }
    {
        return Err(Error::Contract("publication_permissions"));
    }
    Ok(())
}
pub(super) fn directory(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(Error::Contract("output_absolute_path"));
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        if !matches!(part, Component::RootDir | Component::Normal(_)) {
            return Err(Error::Contract("output_path"));
        }
        current.push(part);
        let last = current == path;
        if last && !current.exists() {
            match fs::DirBuilder::new().mode(0o700).create(&current) {
                Ok(()) => {
                    if let Some(parent) = current.parent() {
                        File::open(parent)?.sync_all()?;
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        let meta = fs::symlink_metadata(&current)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(Error::Contract("output_symlink"));
        }
        if last {
            private(&meta, true)?;
        } else if !((meta.uid() == 0 || meta.uid() == unsafe { geteuid() })
            && (meta.mode() & 0o022 == 0 || (meta.uid() == 0 && meta.mode() & 0o1000 != 0)))
        {
            return Err(Error::Contract("output_parent_permissions"));
        }
    }
    Ok(path.into())
}
pub(super) struct Temporary {
    pub path: PathBuf,
}
impl Temporary {
    pub fn new(dir: &Path) -> Result<Self> {
        for _ in 0..100 {
            let path = dir.join(format!(
                ".bslens-v5-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(NOFOLLOW)
                .open(&path)
            {
                Ok(_) => return Ok(Self { path }),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        Err(Error::PublicationConflict)
    }
    pub fn ensure_standalone(&self) -> Result<()> {
        for suffix in ["-journal", "-wal", "-shm"] {
            if fs::symlink_metadata(format!("{}{suffix}", self.path.display())).is_ok() {
                return Err(Error::Contract("sqlite_sidecar"));
            }
        }
        Ok(())
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        for suffix in ["-journal", "-wal", "-shm"] {
            let _ = fs::remove_file(format!("{}{suffix}", self.path.display()));
        }
    }
}
fn checked_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(NOFOLLOW | NONBLOCK)
        .open(path)?;
    private(&file.metadata()?, false)?;
    Ok(file)
}
fn hash(file: &mut File, maximum: u64) -> Result<String> {
    let mut h = Sha256::new();
    let mut bytes = 0u64;
    let mut buf = [0; 65536];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        super::Limits::check("sqlite_bytes", maximum, bytes)?;
        h.update(&buf[..n]);
    }
    Ok(contract::hex(&h.finalize()))
}
struct Lock(File);
impl Lock {
    fn acquire(dir: &Path, id: &str) -> Result<Self> {
        let path = dir.join(format!(".{id}.lock"));
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(NOFOLLOW | NONBLOCK)
            .open(path)?;
        private(&file.metadata()?, false)?;
        loop {
            if unsafe { flock(file.as_raw_fd(), 2) } == 0 {
                return Ok(Self(file));
            }
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::Interrupted {
                return Err(e.into());
            }
        }
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        unsafe {
            flock(self.0.as_raw_fd(), 8);
        }
    }
}
pub(super) fn publish(
    dir: &Path,
    temp: Temporary,
    id: String,
    facts: String,
    maximum: u64,
) -> Result<BuiltSnapshot> {
    let mut file = checked_file(&temp.path)?;
    file.sync_all()?;
    let digest = hash(&mut file, maximum)?;
    drop(file);
    let _lock = Lock::acquire(dir, &id)?;
    let prefix = format!("{id}.");
    let mut existing = vec![];
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with(&prefix) {
            existing.push(entry.path());
        }
    }
    if existing.len() > 1 {
        return Err(Error::PublicationConflict);
    }
    if let Some(path) = existing.pop() {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(Error::PublicationConflict)?;
        let expected = name
            .strip_prefix(&prefix)
            .and_then(|s| s.strip_suffix(".sqlite"))
            .ok_or(Error::PublicationConflict)?;
        let actual = hash(&mut checked_file(&path)?, maximum)?;
        if expected != actual {
            return Err(Error::Contract("published_file_sha256"));
        }
        // Это проверка собственной публикации в приватном каталоге, не S4 reader.
        let mut db = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        db.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF;")?;
        let tx = db.transaction()?;
        contract::validate_schema_profile(&tx)?;
        let (stored_id, stored_facts): (String, String) =
            tx.query_row("SELECT snapshot_id,facts_sha256 FROM metadata", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        if stored_id != id || contract::snapshot_id(&tx)? != id {
            return Err(Error::Contract("published_identity"));
        }
        let actual_facts = contract::facts_sha256(&tx)?;
        if stored_facts != actual_facts {
            return Err(Error::Contract("published_facts"));
        }
        if actual_facts != facts {
            return Err(Error::NonReproducible);
        }
        return Ok(BuiltSnapshot {
            path,
            snapshot_id: id,
            facts_sha256: facts,
            file_sha256: actual,
            reused: true,
        });
    }
    let path = dir.join(format!("{id}.{digest}.sqlite"));
    let old = CString::new(temp.path.as_os_str().as_bytes())
        .map_err(|_| Error::Contract("output_nul"))?;
    let new =
        CString::new(path.as_os_str().as_bytes()).map_err(|_| Error::Contract("output_nul"))?;
    // RENAME_NOREPLACE: даже посторонний файл с конечным именем не затирается.
    if unsafe { renameat2(-100, old.as_ptr(), -100, new.as_ptr(), 1) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    File::open(dir)?
        .sync_all()
        .map_err(|e| Error::PublishedSync {
            path: path.clone(),
            error: e,
        })?;
    Ok(BuiltSnapshot {
        path,
        snapshot_id: id,
        facts_sha256: facts,
        file_sha256: digest,
        reused: false,
    })
}
