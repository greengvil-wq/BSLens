#![cfg(all(feature = "experimental-git", target_os = "linux"))]
use bslens_core::git_manifest::{Code, Limits, Manifest};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
struct Repo {
    path: PathBuf,
}
impl Repo {
    fn new(sha256: bool) -> Self {
        let path = std::env::temp_dir().join(format!(
            "bslens-r03-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir(&path).unwrap();
        let r = Self { path };
        if sha256 {
            r.git(&["init", "-q", "--object-format=sha256"]);
        } else {
            r.git(&["init", "-q"]);
        }
        r
    }
    fn git(&self, args: &[&str]) -> String {
        let o = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&self.path)
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            o.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&o.stderr)
        );
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
    fn object(&self, kind: &str, raw: &[u8]) -> String {
        let mut child = Command::new("/usr/bin/git")
            .arg("-C")
            .arg(&self.path)
            .args(["hash-object", "-w", "--stdin", "-t", kind, "--literally"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(raw).unwrap();
        let o = child.wait_with_output().unwrap();
        assert!(o.status.success());
        String::from_utf8(o.stdout).unwrap().trim().into()
    }
    fn commit_tree(&self, tree: &str) -> String {
        self.object("commit",format!("tree {tree}\nauthor Synthetic <synthetic@invalid> 0 +0000\ncommitter Synthetic <synthetic@invalid> 0 +0000\n\nfixture\n").as_bytes())
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn disk_bytes(root: &std::path::Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut result = std::collections::BTreeMap::new();
    let mut dirs = vec![root.to_path_buf()];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                dirs.push(entry.path());
            } else {
                result.insert(
                    entry.path().strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    result
}
#[test]
fn exact_commits_dirty_worktree_and_blob_identity() {
    let r = Repo::new(false);
    fs::create_dir(r.path.join("Dir")).unwrap();
    fs::write(r.path.join("Dir/A.bsl"), b"\xef\xbb\xbf// old\r\n").unwrap();
    fs::write(r.path.join("z.txt"), b"other").unwrap();
    let first = r.commit();
    fs::write(r.path.join("Dir/A.bsl"), b"// second\n").unwrap();
    let second = r.commit();
    fs::write(r.path.join("Dir/A.bsl"), b"dirty").unwrap();
    fs::write(r.path.join("untracked"), b"never read").unwrap();
    let before = r.git(&["status", "--porcelain"]);
    let before_disk = disk_bytes(&r.path);
    let a = Manifest::read(&r.path, &first, Limits::default()).unwrap();
    let b = Manifest::read(&r.path, &second, Limits::default()).unwrap();
    assert_eq!(a.commit(), first);
    assert_eq!(
        a.tree(),
        r.git(&["rev-parse", &format!("{first}^{{tree}}")])
    );
    assert_eq!(a.files()[0].bytes(), b"\xef\xbb\xbf// old\r\n");
    assert_eq!(b.files()[0].bytes(), b"// second\n");
    assert_eq!(
        a.files()[0].oid,
        r.git(&["rev-parse", &format!("{first}:Dir/A.bsl")])
    );
    assert_eq!(
        a.files()
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        ["Dir/A.bsl", "z.txt"]
    );
    assert_eq!(
        a.files(),
        Manifest::read(&r.path, &first, Limits::default())
            .unwrap()
            .files()
    );
    assert_eq!(a.xml_inputs().len(), 1);
    assert_eq!(disk_bytes(&r.path), before_disk);
    assert_eq!(r.git(&["status", "--porcelain"]), before);
}
#[test]
fn sha256_git_objects_supported() {
    let r = Repo::new(true);
    fs::write(r.path.join("Empty.bsl"), b"").unwrap();
    let sha = r.commit();
    assert_eq!(sha.len(), 64);
    let m = Manifest::read(&r.path, &sha, Limits::default()).unwrap();
    assert_eq!(m.files()[0].oid.len(), 64);
    assert_eq!(m.files()[0].bytes(), b"");
}
#[test]
fn original_config_and_refs_are_not_used() {
    let r = Repo::new(false);
    fs::write(r.path.join("A.bsl"), b"x").unwrap();
    let sha = r.commit();
    fs::write(
        r.path.join(".git/config"),
        b"deliberately invalid configuration\n[include]\npath=/does/not/exist\n",
    )
    .unwrap();
    fs::write(r.path.join(".git/HEAD"), b"bad-ref").unwrap();
    assert!(Manifest::read(&r.path, &sha, Limits::default()).is_ok());
}
#[test]
fn expressions_unknown_objects_and_budgets_refused() {
    let r = Repo::new(false);
    fs::write(r.path.join("A.bsl"), b"x").unwrap();
    fs::write(r.path.join("B.bsl"), vec![b'x'; 1024]).unwrap();
    let sha = r.commit();
    for bad in [
        "HEAD",
        "main",
        "HEAD~1",
        "--help",
        &sha[..12],
        &sha.to_uppercase(),
    ] {
        assert_eq!(
            Manifest::read(&r.path, bad, Limits::default())
                .unwrap_err()
                .code,
            Code::InvalidSha
        );
    }
    assert!(Manifest::read(&r.path, &"0".repeat(40), Limits::default()).is_err());
    for limit in [
        Limits {
            files: 1,
            ..Limits::default()
        },
        Limits {
            bytes: 10,
            ..Limits::default()
        },
        Limits {
            object_bytes: 10,
            ..Limits::default()
        },
        Limits {
            objects: 1,
            ..Limits::default()
        },
    ] {
        assert_eq!(
            Manifest::read(&r.path, &sha, limit).unwrap_err().code,
            Code::Limit
        );
    }
}
#[test]
fn symlinks_alternates_promisor_and_corrupt_objects_refused() {
    let r = Repo::new(false);
    fs::write(r.path.join("A.bsl"), b"hello").unwrap();
    let sha = r.commit();
    for p in [
        ".git/objects/info/alternates",
        ".git/objects/pack/test.promisor",
    ] {
        fs::write(r.path.join(p), b"").unwrap();
        assert_eq!(
            Manifest::read(&r.path, &sha, Limits::default())
                .unwrap_err()
                .code,
            Code::Layout
        );
        fs::remove_file(r.path.join(p)).unwrap();
    }
    std::os::unix::fs::symlink("/tmp", r.path.join(".git/objects/bad")).unwrap();
    assert_eq!(
        Manifest::read(&r.path, &sha, Limits::default())
            .unwrap_err()
            .code,
        Code::Layout
    );
    fs::remove_file(r.path.join(".git/objects/bad")).unwrap();
    let oid = r.git(&["rev-parse", "HEAD:A.bsl"]);
    let object_path = r.path.join(".git/objects").join(&oid[..2]).join(&oid[2..]);
    fs::remove_file(&object_path).unwrap();
    fs::write(object_path, b"corrupt").unwrap();
    assert!(Manifest::read(&r.path, &sha, Limits::default()).is_err());
}
#[test]
fn unexpected_types_and_malformed_tree_refused() {
    let r = Repo::new(false);
    fs::write(r.path.join("A.bsl"), b"x").unwrap();
    let sha = r.commit();
    let blob = r.git(&["rev-parse", "HEAD:A.bsl"]);
    assert_eq!(
        Manifest::read(&r.path, &blob, Limits::default())
            .unwrap_err()
            .code,
        Code::Object
    );
    std::os::unix::fs::symlink("A.bsl", r.path.join("Link.bsl")).unwrap();
    let link = r.commit();
    assert_eq!(
        Manifest::read(&r.path, &link, Limits::default())
            .unwrap_err()
            .code,
        Code::UnsupportedType
    );
    let tree = r.object("tree", b"100644 missing-nul-and-oid");
    let bad = r.commit_tree(&tree);
    assert!(Manifest::read(&r.path, &bad, Limits::default()).is_err());
    let tree = r.git(&["rev-parse", &format!("{sha}^{{tree}}")]);
    let raw = format!("tree {tree}");
    let incomplete = r.object("commit", raw.as_bytes());
    assert_eq!(
        Manifest::read(&r.path, &incomplete, Limits::default())
            .unwrap_err()
            .code,
        Code::Object
    );
}
#[cfg(feature = "experimental-xml")]
#[test]
fn exact_tree_to_typed_xml_without_snapshot_promotion() {
    let r = Repo::new(false);
    fs::write(r.path.join("Configuration.xml"),format!("<MetaDataObject xmlns=\"{}\" version=\"2.21\"><Configuration><Properties><Name>Root</Name></Properties></Configuration></MetaDataObject>",bslens_core::xml221::NS)).unwrap();
    let sha = r.commit();
    let manifest = Manifest::read(&r.path, &sha, Limits::default()).unwrap();
    let xml = bslens_core::xml221::Xml221Subset::inspect(&manifest.xml_inputs()).unwrap();
    assert_eq!(xml.objects()[0].name, "Root");
    assert!(bslens_core::ValidatedSnapshot::open(&r.path, &sha).is_err());
}

#[test]
fn decompressed_content_must_match_requested_git_oid() {
    let r = Repo::new(false);
    fs::write(r.path.join("A.bsl"), b"original").unwrap();
    let sha = r.commit();
    let original = r.git(&["rev-parse", "HEAD:A.bsl"]);
    let different = r.object("blob", b"different");
    let location = |oid: &str| r.path.join(".git/objects").join(&oid[..2]).join(&oid[2..]);
    let replacement = fs::read(location(&different)).unwrap();
    fs::remove_file(location(&original)).unwrap();
    fs::write(location(&original), replacement).unwrap();
    assert_eq!(
        Manifest::read(&r.path, &sha, Limits::default())
            .unwrap_err()
            .code,
        Code::Identity
    );
}

#[test]
fn tree_paths_and_gitlinks_are_not_regular_files() {
    let r = Repo::new(false);
    fs::write(r.path.join("A.bsl"), b"x").unwrap();
    r.commit();
    let blob = r.git(&["rev-parse", "HEAD:A.bsl"]);
    let bytes: Vec<u8> = (0..blob.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&blob[i..i + 2], 16).unwrap())
        .collect();
    for (mode, name, expected) in [
        ("100644", "..", Code::Path),
        ("100644", "a/b", Code::Path),
        ("160000", "submodule", Code::UnsupportedType),
    ] {
        let mut raw = format!("{mode} {name}\0").into_bytes();
        raw.extend(&bytes);
        let tree = r.object("tree", &raw);
        let commit = r.commit_tree(&tree);
        assert_eq!(
            Manifest::read(&r.path, &commit, Limits::default())
                .unwrap_err()
                .code,
            expected
        );
    }
    let mut raw = b"100644 duplicate\0".to_vec();
    raw.extend(&bytes);
    let repeated = raw.clone();
    raw.extend(&repeated);
    let tree = r.object("tree", &raw);
    let commit = r.commit_tree(&tree);
    assert_eq!(
        Manifest::read(&r.path, &commit, Limits::default())
            .unwrap_err()
            .code,
        Code::Path
    );
}
