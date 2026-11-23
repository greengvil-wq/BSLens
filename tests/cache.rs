#![cfg(all(feature = "experimental-cache", target_os = "linux"))]
use bslens_core::{
    cache,
    git_manifest::{Limits, Manifest},
    pipeline::GitStructure,
};
use sha2::{Digest, Sha256};
use std::os::unix::fs::{symlink, PermissionsExt};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Repo {
    path: PathBuf,
}
impl Repo {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "bslens-r08-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        let r = Self { path };
        r.git(&["init", "-q"]);
        for (path, bytes) in [
            (
                "Configuration.xml",
                include_bytes!("fixtures/xml221/Configuration.xml").as_slice(),
            ),
            (
                "Catalogs/Items.xml",
                include_bytes!("fixtures/xml221/Catalogs/Items.xml").as_slice(),
            ),
            (
                "Documents/Order.xml",
                include_bytes!("fixtures/xml221/Documents/Order.xml").as_slice(),
            ),
            (
                "InformationRegisters/Events.xml",
                include_bytes!("fixtures/xml221/InformationRegisters/Events.xml").as_slice(),
            ),
            (
                "Catalogs/Items/Ext/ObjectModule.bsl",
                include_bytes!("fixtures/xml221/Catalogs/Items/Ext/ObjectModule.bsl").as_slice(),
            ),
            (
                "Documents/Order/Ext/ObjectModule.bsl",
                include_bytes!("fixtures/xml221/Documents/Order/Ext/ObjectModule.bsl").as_slice(),
            ),
            ("Ignored.txt", b"authored non-XML file".as_slice()),
        ] {
            let p = r.path.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, bytes).unwrap();
        }
        r
    }
    fn git(&self, args: &[&str]) -> String {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&self.path)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8(o.stdout).unwrap().trim().into()
    }
    fn commit(&self) -> String {
        self.git(&["add", "-A"]);
        self.git(&[
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@invalid",
            "commit",
            "-qm",
            "fixture",
        ]);
        self.git(&["rev-parse", "HEAD"])
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn compare(a: &GitStructure, b: &GitStructure) {
    assert_eq!(a.context(), b.context());
    let mut todo = vec![None];
    while let Some(node) = todo.pop() {
        let mut offset = 0;
        loop {
            let x = a.page(node.as_deref(), offset, 2).unwrap();
            let y = b.page(node.as_deref(), offset, 2).unwrap();
            assert_eq!(x, y);
            for m in &x.methods {
                if let Some(r) = &m.source {
                    assert_eq!(a.source(r).unwrap(), b.source(r).unwrap());
                }
            }
            for n in &x.page.items {
                todo.push(Some(n.id.clone()));
            }
            if let Some(next) = x.page.next_offset {
                offset = next
            } else {
                break;
            }
        }
    }
}
fn file(dir: &std::path::Path, sha: &str) -> PathBuf {
    dir.join(format!("v1-{sha}.bslcache"))
}
fn sign(raw: &mut Vec<u8>) {
    let n = raw.len() - 32;
    let hash = Sha256::digest(&raw[..n]);
    raw[n..].copy_from_slice(&hash);
}
#[test]
fn cold_warm_byte_exact_and_original_blobs_not_needed() {
    let r = Repo::new();
    let sha = r.commit();
    let dir = r.path.join("cache");
    let direct = GitStructure::read(&r.path, &sha, Limits::default()).unwrap();
    let cold = cache::build(&r.path, &sha, &dir).unwrap();
    compare(&direct, &cold);
    let original = fs::read(file(&dir, &sha)).unwrap();
    assert!(!original
        .windows(b"authored non-XML file".len())
        .any(|w| w == b"authored non-XML file"));
    let manifest = Manifest::read(&r.path, &sha, Limits::default()).unwrap();
    for f in manifest.files() {
        fs::remove_file(
            r.path
                .join(".git/objects")
                .join(&f.oid[..2])
                .join(&f.oid[2..]),
        )
        .unwrap();
    }
    fs::write(r.path.join("Configuration.xml"), b"dirty invalid").unwrap();
    let warm = cache::open(&r.path, &sha, &dir).unwrap();
    compare(&direct, &warm);
    assert!(GitStructure::read(&r.path, &sha, Limits::default()).is_err());
    assert_eq!(fs::read(file(&dir, &sha)).unwrap(), original);
}
#[test]
fn missing_corruption_version_and_rebuild_are_explicit() {
    let r = Repo::new();
    let sha = r.commit();
    let dir = r.path.join("cache");
    assert_eq!(
        cache::open(&r.path, &sha, &dir).err(),
        Some(cache::Error::Missing)
    );
    cache::build(&r.path, &sha, &dir).unwrap();
    let p = file(&dir, &sha);
    let original = fs::read(&p).unwrap();
    for n in [0, 1, 16, 32, original.len() - 1] {
        fs::write(&p, &original[..n]).unwrap();
        assert!(cache::open(&r.path, &sha, &dir).is_err());
    }
    let mut raw = original.clone();
    raw[0] ^= 1;
    sign(&mut raw);
    fs::write(&p, raw).unwrap();
    assert_eq!(
        cache::open(&r.path, &sha, &dir).err(),
        Some(cache::Error::Version)
    );
    let mut raw = original.clone();
    let n = raw.len() - 33;
    raw[n] ^= 1;
    fs::write(&p, raw).unwrap();
    assert_eq!(
        cache::open(&r.path, &sha, &dir).err(),
        Some(cache::Error::Corrupt)
    );
    cache::build(&r.path, &sha, &dir).unwrap();
    assert_eq!(fs::read(&p).unwrap(), original);
}
#[test]
fn redigested_path_mapping_and_missing_git_anchor_rejected() {
    let r = Repo::new();
    let sha = r.commit();
    let dir = r.path.join("cache");
    cache::build(&r.path, &sha, &dir).unwrap();
    let p = file(&dir, &sha);
    let original = fs::read(&p).unwrap();
    let mut raw = original.clone();
    let needle = b"Catalogs/Items.xml";
    let pos = raw.windows(needle.len()).position(|w| w == needle).unwrap();
    raw[pos + 9] = b'Z';
    sign(&mut raw);
    fs::write(&p, raw).unwrap();
    assert!(cache::open(&r.path, &sha, &dir).is_err());
    fs::write(&p, original).unwrap();
    fs::remove_file(r.path.join(".git/objects").join(&sha[..2]).join(&sha[2..])).unwrap();
    assert_eq!(
        cache::open(&r.path, &sha, &dir).err(),
        Some(cache::Error::Git)
    );
}
#[test]
fn paths_permissions_hardlinks_and_size_denied() {
    let r = Repo::new();
    let sha = r.commit();
    let dir = r.path.join("cache");
    cache::build(&r.path, &sha, &dir).unwrap();
    let p = file(&dir, &sha);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        cache::open(&r.path, &sha, &dir).err(),
        Some(cache::Error::Permissions)
    );
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        cache::open(&r.path, &sha, &dir).err(),
        Some(cache::Error::Permissions)
    );
    fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
    let link = r.path.join("linked");
    symlink(&dir, &link).unwrap();
    assert!(cache::open(&r.path, &sha, &link).is_err());
    let hard = r.path.join("hard");
    fs::hard_link(&p, &hard).unwrap();
    assert!(cache::open(&r.path, &sha, &dir).is_err());
    fs::remove_file(hard).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(&p)
        .unwrap()
        .set_len(40 * 1024 * 1024 + 1)
        .unwrap();
    assert_eq!(
        cache::open(&r.path, &sha, &dir).err(),
        Some(cache::Error::Limit)
    );
    fs::remove_file(&p).unwrap();
    symlink(r.path.join("Configuration.xml"), &p).unwrap();
    assert!(cache::open(&r.path, &sha, &dir).is_err());
    assert!(cache::build(&r.path, &sha, &dir).is_err());
    assert!(cache::open(&r.path, "HEAD", &dir).is_err());
    assert!(cache::open(&r.path, &sha, std::path::Path::new("relative")).is_err());
}
