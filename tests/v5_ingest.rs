#![cfg(all(feature = "experimental-v5-ingest", target_os = "linux"))]
use bslens_core::git_manifest::{
    self,
    ingest::{ingest, Error, Limits},
    Code,
};
use sha2::{Digest, Sha256};
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

fn run(
    r: &Repo,
    commit: &str,
    limits: Limits,
) -> Result<bslens_core::git_manifest::ingest::Receipt, Error> {
    let tree = r.git(&["rev-parse", &format!("{commit}^{{tree}}")]);
    ingest(&r.path, commit, &tree, limits, |_, _, _| Ok(()))
}
#[test]
fn crosses_legacy_count_and_bytes_streams_reproducibly() {
    let r = Repo::new(false);
    let body = vec![b'x'; 9000];
    for i in 0..4200 {
        fs::write(r.path.join(format!("{i:05}.bsl")), &body).unwrap();
    }
    // Unselected bytes exceed per-object ceiling and are never loaded.
    fs::write(r.path.join("unselected.dat"), vec![0; 17 * 1024 * 1024]).unwrap();
    let commit = r.commit();
    let tree = r.git(&["rev-parse", "HEAD^{tree}"]);
    fs::write(r.path.join("00000.bsl"), b"dirty").unwrap();
    let mut count = 0;
    let mut bytes = 0;
    let expected_hash = format!("{:x}", Sha256::digest(&body));
    let receipt = ingest(&r.path, &commit, &tree, Limits::MAX, |entry, sha, raw| {
        assert_eq!(entry.path, format!("{count:05}.bsl"));
        assert_eq!(raw, body);
        assert_eq!(sha, expected_hash);
        count += 1;
        bytes += raw.len();
        Ok(())
    })
    .unwrap();
    assert_eq!(count, 4200);
    assert!(bytes > 32 * 1024 * 1024);
    assert_eq!(receipt.manifest.entries().len(), 4201);
    assert_eq!(receipt.manifest.commit(), commit);
    assert_eq!(receipt.manifest.tree(), tree);
    assert_eq!(receipt.reads, 4202);
    let again = run(&r, &commit, Limits::MAX).unwrap();
    assert_eq!(receipt.manifest.entries(), again.manifest.entries());
    assert_eq!(receipt.bytes, again.bytes);
    assert_eq!(
        git_manifest::Manifest::read(&r.path, &commit, git_manifest::Limits::default())
            .unwrap_err()
            .code,
        Code::Limit
    );
    assert!(matches!(
        run(
            &r,
            &commit,
            Limits {
                entries: 4200,
                ..Limits::MAX
            }
        ),
        Err(Error::Limit {
            name: "entries",
            actual: 4201,
            ..
        })
    ));
    let mut calls = 0;
    let error = ingest(
        &r.path,
        &commit,
        &tree,
        Limits {
            reads: 4201,
            ..Limits::MAX
        },
        |_, _, _| {
            calls += 1;
            Ok(())
        },
    )
    .unwrap_err();
    assert!(matches!(
        error,
        Error::Limit {
            name: "reads",
            actual: 4202,
            ..
        }
    ));
    assert_eq!(calls, 0);
    assert!(matches!(
        run(
            &r,
            &commit,
            Limits {
                total_bytes: 32 * 1024 * 1024,
                ..Limits::MAX
            }
        ),
        Err(Error::Read {
            code: Code::Limit,
            budget: "total_bytes",
            ..
        })
    ));
    // Isolate the old file-count ceiling independently of bytes.
    let empty = r.object("blob", b"");
    let oid: Vec<u8> = (0..empty.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&empty[i..i + 2], 16).unwrap())
        .collect();
    let mut raw_tree = Vec::new();
    for i in 0..4200 {
        raw_tree.extend(format!("100644 {i:05}.bsl\0").as_bytes());
        raw_tree.extend(&oid);
    }
    let small_tree = r.object("tree", &raw_tree);
    let small_commit = r.commit_tree(&small_tree);
    assert_eq!(
        git_manifest::Manifest::read(&r.path, &small_commit, git_manifest::Limits::default())
            .unwrap_err()
            .code,
        Code::Limit
    );
    assert_eq!(run(&r, &small_commit, Limits::MAX).unwrap().selected, 4200);
}
#[test]
fn exact_identity_selection_sha256_and_failures() {
    for sha256 in [false, true] {
        let r = Repo::new(sha256);
        fs::create_dir(r.path.join("Dir")).unwrap();
        fs::write(r.path.join("Dir/A.xml"), b"original XML bytes").unwrap();
        fs::write(r.path.join("large.bsl"), vec![b'x'; 4096]).unwrap();
        fs::write(r.path.join("skip.BSL"), b"not selected").unwrap();
        let commit = r.commit();
        let tree = r.git(&["rev-parse", "HEAD^{tree}"]);
        assert_eq!(run(&r, &commit, Limits::MAX).unwrap().selected, 2);
        for bad in ["HEAD", &commit[..12], &commit.to_uppercase()] {
            assert!(matches!(
                ingest(&r.path, bad, &tree, Limits::MAX, |_, _, _| Ok(())),
                Err(Error::Git(Code::InvalidSha))
            ));
        }
        assert!(matches!(
            ingest(
                &r.path,
                &commit,
                &"0".repeat(commit.len()),
                Limits::MAX,
                |_, _, _| Ok(())
            ),
            Err(Error::Git(Code::Identity))
        ));
        assert!(matches!(
            run(
                &r,
                &commit,
                Limits {
                    trees: 1,
                    ..Limits::MAX
                }
            ),
            Err(Error::Limit {
                name: "trees",
                actual: 2,
                ..
            })
        ));
        assert!(matches!(
            run(
                &r,
                &commit,
                Limits {
                    object_bytes: 1024,
                    ..Limits::MAX
                }
            ),
            Err(Error::Read {
                code: Code::Limit,
                budget: "object_bytes",
                ..
            })
        ));
        assert!(matches!(
            run(
                &r,
                &commit,
                Limits {
                    deadline: std::time::Duration::from_nanos(1),
                    ..Limits::MAX
                }
            ),
            Err(Error::Git(Code::Timeout))
        ));
        assert!(
            matches!(ingest(&r.path, &commit, &tree, Limits::MAX, |_,_,_| Err("rollback".into())), Err(Error::Consumer(s)) if s == "rollback")
        );
        // Source config, refs and working tree are not authority.
        fs::write(r.path.join(".git/config"), b"invalid config").unwrap();
        fs::write(r.path.join("Dir/A.xml"), b"dirty").unwrap();
        assert!(run_without_refs(&r, &commit, &tree).is_ok());
    }
}
fn run_without_refs(
    r: &Repo,
    commit: &str,
    tree: &str,
) -> Result<bslens_core::git_manifest::ingest::Receipt, Error> {
    ingest(&r.path, commit, tree, Limits::MAX, |_, _, _| Ok(()))
}
#[test]
fn positive_ceilings_and_legacy_contract() {
    let r = Repo::new(false);
    fs::write(r.path.join("a.bsl"), b"x").unwrap();
    let commit = r.commit();
    for limits in [
        Limits {
            entries: 0,
            ..Limits::MAX
        },
        Limits {
            trees: 0,
            ..Limits::MAX
        },
        Limits {
            reads: 0,
            ..Limits::MAX
        },
        Limits {
            object_bytes: 0,
            ..Limits::MAX
        },
        Limits {
            total_bytes: 0,
            ..Limits::MAX
        },
        Limits {
            deadline: std::time::Duration::ZERO,
            ..Limits::MAX
        },
        Limits {
            entries: Limits::MAX.entries + 1,
            ..Limits::MAX
        },
        Limits {
            trees: Limits::MAX.trees + 1,
            ..Limits::MAX
        },
        Limits {
            reads: Limits::MAX.reads + 1,
            ..Limits::MAX
        },
        Limits {
            object_bytes: Limits::MAX.object_bytes + 1,
            ..Limits::MAX
        },
        Limits {
            total_bytes: Limits::MAX.total_bytes + 1,
            ..Limits::MAX
        },
        Limits {
            deadline: Limits::MAX.deadline + std::time::Duration::from_secs(1),
            ..Limits::MAX
        },
    ] {
        assert!(matches!(
            run(&r, &commit, limits),
            Err(Error::InvalidLimit { .. })
        ));
    }
    let old = git_manifest::Limits::default();
    assert_eq!(
        (
            old.files,
            old.bytes,
            old.object_bytes,
            old.objects,
            old.deadline.as_secs()
        ),
        (4096, 32 * 1024 * 1024, 8 * 1024 * 1024, 8192, 30)
    );
    assert_eq!(
        git_manifest::Manifest::read(
            &r.path,
            &commit,
            git_manifest::Limits { files: 4097, ..old }
        )
        .unwrap_err()
        .code,
        Code::Limit
    );
    assert_eq!(
        git_manifest::Manifest::read(
            &r.path,
            &commit,
            git_manifest::Limits {
                bytes: 32 * 1024 * 1024 + 1,
                ..old
            }
        )
        .unwrap_err()
        .code,
        Code::Limit
    );
    assert!(git_manifest::Manifest::read(&r.path, &commit, old).is_ok());
}
#[test]
fn malformed_types_paths_and_identity_are_not_weakened() {
    let r = Repo::new(false);
    fs::write(r.path.join("a.bsl"), b"original").unwrap();
    let commit = r.commit();
    let blob = r.git(&["rev-parse", "HEAD:a.bsl"]);
    let oid: Vec<u8> = (0..blob.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&blob[i..i + 2], 16).unwrap())
        .collect();
    for (mode, name, code) in [
        ("160000", "sub", Code::UnsupportedType),
        ("120000", "link", Code::UnsupportedType),
        ("100644", "..", Code::Path),
        ("100644", "a/b", Code::Path),
        ("100644", "a\\b", Code::Path),
    ] {
        let mut raw = format!("{mode} {name}\0").into_bytes();
        raw.extend(&oid);
        let tree = r.object("tree", &raw);
        let c = r.commit_tree(&tree);
        assert!(
            matches!(ingest(&r.path,&c,&tree,Limits::MAX,|_,_,_| panic!("no callbacks")), Err(Error::Git(c)) if c == code)
        );
    }
    let mut raw = b"100644 duplicate\0".to_vec();
    raw.extend(&oid);
    raw.extend(raw.clone());
    let tree = r.object("tree", &raw);
    let c = r.commit_tree(&tree);
    assert!(matches!(
        ingest(&r.path, &c, &tree, Limits::MAX, |_, _, _| Ok(())),
        Err(Error::Git(Code::Path))
    ));
    let other = r.object("blob", b"different");
    let loc = |s: &str| r.path.join(".git/objects").join(&s[..2]).join(&s[2..]);
    let replacement = fs::read(loc(&other)).unwrap();
    fs::remove_file(loc(&blob)).unwrap();
    fs::write(loc(&blob), replacement).unwrap();
    assert!(matches!(
        run(&r, &commit, Limits::MAX),
        Err(Error::Git(Code::Identity))
    ));
}

#[test]
fn consumer_stages_blobs_and_rolls_back_partial_failure() {
    let r = Repo::new(false);
    fs::write(r.path.join("a.xml"), b"first").unwrap();
    fs::write(r.path.join("z.bsl"), b"last").unwrap();
    let commit = r.commit();
    let tree = r.git(&["rev-parse", "HEAD^{tree}"]);
    let mut db = rusqlite::Connection::open_in_memory().unwrap();
    db.execute_batch("CREATE TABLE sources(path TEXT PRIMARY KEY, oid TEXT, sha TEXT, bytes BLOB)")
        .unwrap();
    for fail in [true, false] {
        let tx = db.transaction().unwrap();
        let result = ingest(&r.path, &commit, &tree, Limits::MAX, |entry, sha, raw| {
            if fail && entry.path == "z.bsl" {
                return Err("synthetic sink failure".into());
            }
            tx.execute(
                "INSERT INTO sources VALUES (?1,?2,?3,?4)",
                rusqlite::params![entry.path, entry.oid, sha, raw],
            )
            .map_err(|e| e.to_string())?;
            Ok(())
        });
        if fail {
            assert!(matches!(result, Err(Error::Consumer(_))));
            tx.rollback().unwrap();
            assert_eq!(
                db.query_row("SELECT count(*) FROM sources", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                0
            );
        } else {
            result.unwrap();
            tx.commit().unwrap();
            assert_eq!(
                db.query_row("SELECT bytes FROM sources WHERE path='z.bsl'", [], |r| r
                    .get::<_, Vec<u8>>(0))
                    .unwrap(),
                b"last"
            );
        }
    }
}

#[test]
fn callback_deadline_and_forbidden_object_stores() {
    let r = Repo::new(false);
    fs::write(r.path.join("a.bsl"), b"x").unwrap();
    let commit = r.commit();
    let tree = r.git(&["rev-parse", "HEAD^{tree}"]);
    let mut called = false;
    let result = ingest(
        &r.path,
        &commit,
        &tree,
        Limits {
            deadline: std::time::Duration::from_secs(1),
            ..Limits::MAX
        },
        |_, _, _| {
            called = true;
            std::thread::sleep(std::time::Duration::from_millis(1100));
            Ok(())
        },
    );
    assert!(called);
    assert!(matches!(result, Err(Error::Git(Code::Timeout))));
    for path in [
        ".git/objects/info/alternates",
        ".git/objects/info/http-alternates",
        ".git/objects/pack/missing.promisor",
    ] {
        fs::write(r.path.join(path), b"").unwrap();
        assert!(matches!(
            run(&r, &commit, Limits::MAX),
            Err(Error::Git(Code::Layout))
        ));
        fs::remove_file(r.path.join(path)).unwrap();
    }
    std::os::unix::fs::symlink("/tmp", r.path.join(".git/objects/link")).unwrap();
    assert!(matches!(
        run(&r, &commit, Limits::MAX),
        Err(Error::Git(Code::Layout))
    ));
}
