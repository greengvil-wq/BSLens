#![cfg(all(feature = "gitlab", target_os = "linux"))]
use bslens_core::gitlab::{
    graph::check_graph,
    process::{GitError, GitReader, ProcessGit},
    types::{LocalInput, ReasonCode},
};
use std::{
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Repo(PathBuf);
impl Repo {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "bslens-gitlab-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        let r = Self(p);
        r.git(&["init", "-q"]);
        r
    }
    fn git(&self, args: &[&str]) -> String {
        let o = Command::new("/usr/bin/git")
            .current_dir(&self.0)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "git failed: {:?}", args);
        String::from_utf8(o.stdout).unwrap().trim().into()
    }
    fn commit(&self, file: &str, content: &str) -> String {
        std::fs::write(self.0.join(file), content).unwrap();
        self.git(&["add", "."]);
        self.git(&["commit", "-qm", content]);
        self.git(&["rev-parse", "HEAD"])
    }
    fn reader(&self) -> ProcessGit {
        ProcessGit::new(self.0.join(".git"), 1024 * 1024)
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn input(base: &str, start: &str, head: &str, target: &str) -> LocalInput {
    LocalInput {
        base: base.into(),
        start: start.into(),
        head: head.into(),
        target: target.into(),
    }
}
#[test]
fn graph_real_repository() {
    let r = Repo::new();
    let base = r.commit("a", "base");
    let start = r.commit("a", "start");
    r.git(&["checkout", "-q", "--detach", &base]);
    let head = r.commit("b", "head");
    let git = r.reader();
    let ok = input(&base, &start, &head, &start);
    assert!(check_graph(&ok, &git).is_ok());
    let advanced = input(&base, &start, &head, &head);
    assert_eq!(
        check_graph(&advanced, &git).unwrap_err().reason,
        ReasonCode::StaleTarget
    );
    let mismatch = input(&head, &start, &head, &start);
    assert_eq!(
        check_graph(&mismatch, &git).unwrap_err().reason,
        ReasonCode::MergeBaseMismatch
    );
    let missing = "1".repeat(40);
    assert_eq!(
        check_graph(&input(&missing, &start, &head, &start), &git)
            .unwrap_err()
            .reason,
        ReasonCode::MissingObject
    );
    assert_eq!(
        check_graph(
            &input(&base.to_ascii_uppercase(), &start, &head, &start),
            &git
        )
        .unwrap_err()
        .reason,
        ReasonCode::MissingObject
    );
    let tree = r.git(&["rev-parse", "HEAD^{tree}"]);
    assert_eq!(
        check_graph(&input(&tree, &start, &head, &start), &git)
            .unwrap_err()
            .reason,
        ReasonCode::NotACommit
    );
    assert_eq!(
        check_graph(&input("1234", &start, &head, &start), &git)
            .unwrap_err()
            .reason,
        ReasonCode::MissingObject
    );
    assert_eq!(git.rev_list(&base, &head).unwrap(), vec![head]);
}
#[test]
fn shallow_clone_is_rejected() {
    let r = Repo::new();
    let base = r.commit("a", "base");
    r.commit("a", "next");
    let clone = Repo::new();
    let dest = clone.0.join("copy");
    let url = format!("file://{}", r.0.display());
    assert!(Command::new("/usr/bin/git")
        .args(["clone", "-q", "--depth", "1", &url, dest.to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    let sha = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(&dest)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let sha = String::from_utf8(sha.stdout).unwrap().trim().to_string();
    let git = ProcessGit::new(dest.join(".git"), 1024 * 1024);
    assert_eq!(
        check_graph(&input(&sha, &sha, &sha, &sha), &git)
            .unwrap_err()
            .reason,
        ReasonCode::ShallowRepository
    );
    let _ = base;
}
struct Multiple;
impl GitReader for Multiple {
    fn object_format(&self) -> Result<usize, GitError> {
        Ok(40)
    }
    fn object_type(&self, _: &str) -> Result<String, GitError> {
        Ok("commit".into())
    }
    fn is_shallow(&self) -> Result<bool, GitError> {
        Ok(false)
    }
    fn merge_bases(&self, _: &str, _: &str) -> Result<Vec<String>, GitError> {
        Ok(vec!["a".repeat(40), "b".repeat(40)])
    }
    fn rev_list(&self, _: &str, _: &str) -> Result<Vec<String>, GitError> {
        unreachable!()
    }
    fn diff(&self, _: &str, _: &str) -> Result<Vec<u8>, GitError> {
        unreachable!()
    }
    fn tree(&self, _: &str, _: &str) -> Result<Vec<u8>, GitError> {
        unreachable!()
    }
    fn blob_oid(&self, _: &str, _: &str) -> Result<Option<String>, GitError> {
        unreachable!()
    }
    fn blob(&self, _: &str) -> Result<Vec<u8>, GitError> {
        unreachable!()
    }
}
#[test]
fn multiple_bases_are_rejected() {
    let s = "a".repeat(40);
    assert_eq!(
        check_graph(&input(&s, &s, &s, &s), &Multiple)
            .unwrap_err()
            .reason,
        ReasonCode::MultipleMergeBases
    );
}

#[test]
fn changed_paths_and_modes() {
    use bslens_core::gitlab::{
        paths::changed_paths,
        types::{Change, Limits, VendorPolicy},
    };
    let r = Repo::new();
    let base = r.commit("old", "hello");
    r.git(&["mv", "old", "B"]);
    r.git(&["update-index", "--chmod=+x", "B"]);
    std::fs::set_permissions(
        r.0.join("B"),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    std::fs::write(r.0.join("a"), "a").unwrap();
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "next"]);
    let head = r.git(&["rev-parse", "HEAD"]);
    let git = r.reader();
    let policy = VendorPolicy::default();
    let limits = Limits::default();
    let result = changed_paths(&input(&base, &base, &head, &base), &policy, &limits, &git).unwrap();
    assert_eq!(
        result
            .paths
            .iter()
            .map(|p| (p.path.as_str(), p.change))
            .collect::<Vec<_>>(),
        vec![
            ("B", Change::Added),
            ("a", Change::Added),
            ("old", Change::Deleted)
        ]
    );
    assert_eq!(result.scope_count, 3);
    let empty = changed_paths(&input(&head, &head, &head, &head), &policy, &limits, &git).unwrap();
    assert!(empty.paths.is_empty());
    assert_eq!(empty.scope_count, 2);
    r.git(&["update-index", "--chmod=-x", "B"]);
    r.git(&["commit", "-qm", "mode"]);
    let mode = r.git(&["rev-parse", "HEAD"]);
    let result = changed_paths(&input(&head, &head, &mode, &head), &policy, &limits, &git).unwrap();
    assert_eq!(result.paths[0].change, Change::Modified);
    let limited = Limits {
        max_paths: 1,
        ..limits
    };
    assert_eq!(
        changed_paths(&input(&base, &base, &head, &base), &policy, &limited, &git)
            .unwrap_err()
            .reason,
        ReasonCode::LimitExceeded
    );
}
#[test]
fn symlink_and_gitlink_are_unsupported() {
    use bslens_core::gitlab::{
        paths::changed_paths,
        types::{Limits, VendorPolicy},
    };
    use std::os::unix::fs::symlink;
    let r = Repo::new();
    let base = r.commit("a", "a");
    symlink("a", r.0.join("link")).unwrap();
    r.git(&["add", "link"]);
    r.git(&["commit", "-qm", "link"]);
    let head = r.git(&["rev-parse", "HEAD"]);
    assert_eq!(
        changed_paths(
            &input(&base, &base, &head, &base),
            &VendorPolicy::default(),
            &Limits::default(),
            &r.reader()
        )
        .unwrap_err()
        .reason,
        ReasonCode::UnsupportedEntry
    );
    r.git(&["reset", "--hard", &base]);
    r.git(&[
        "update-index",
        "--add",
        "--cacheinfo",
        &format!("160000,{base},sub"),
    ]);
    r.git(&["commit", "-qm", "sub"]);
    let head = r.git(&["rev-parse", "HEAD"]);
    assert_eq!(
        changed_paths(
            &input(&base, &base, &head, &base),
            &VendorPolicy::default(),
            &Limits::default(),
            &r.reader()
        )
        .unwrap_err()
        .reason,
        ReasonCode::UnsupportedEntry
    );
}

struct PathMock {
    diff: Result<Vec<u8>, GitError>,
}
impl GitReader for PathMock {
    fn object_format(&self) -> Result<usize, GitError> {
        Ok(40)
    }
    fn object_type(&self, _: &str) -> Result<String, GitError> {
        Ok("commit".into())
    }
    fn is_shallow(&self) -> Result<bool, GitError> {
        Ok(false)
    }
    fn merge_bases(&self, _: &str, _: &str) -> Result<Vec<String>, GitError> {
        unreachable!()
    }
    fn rev_list(&self, _: &str, _: &str) -> Result<Vec<String>, GitError> {
        unreachable!()
    }
    fn diff(&self, _: &str, _: &str) -> Result<Vec<u8>, GitError> {
        self.diff.clone()
    }
    fn tree(&self, _: &str, _: &str) -> Result<Vec<u8>, GitError> {
        Ok(Vec::new())
    }
    fn blob_oid(&self, _: &str, _: &str) -> Result<Option<String>, GitError> {
        unreachable!()
    }
    fn blob(&self, _: &str) -> Result<Vec<u8>, GitError> {
        unreachable!()
    }
}
#[test]
fn mock_paths_report_errors_and_rename() {
    use bslens_core::gitlab::{
        paths::changed_paths,
        types::{Change, Limits, VendorPolicy},
    };
    let s = "a".repeat(40);
    let input = input(&s, &s, &s, &s);
    let policy = VendorPolicy::default();
    let limits = Limits::default();
    let call = |diff| changed_paths(&input, &policy, &limits, &PathMock { diff });
    let header = |status: &str| {
        format!(
            ":100644 100644 {} {} {status}\0",
            "a".repeat(40),
            "b".repeat(40)
        )
        .into_bytes()
    };
    let raw = |status: &str, path: &[u8]| {
        let mut v = header(status);
        v.extend_from_slice(path);
        v.push(0);
        v
    };
    assert_eq!(
        call(Ok(raw("A", b"\xff"))).unwrap_err().reason,
        ReasonCode::NonUtf8Path
    );
    assert_eq!(
        call(Err(GitError::Failure)).unwrap_err().reason,
        ReasonCode::GitFailure
    );
    assert_eq!(
        call(Ok(raw("X", b"a"))).unwrap_err().reason,
        ReasonCode::UnsupportedEntry
    );
    let mut rename = raw("R100", b"a");
    rename.extend_from_slice(b"b\0");
    let renamed = call(Ok(rename)).unwrap();
    assert_eq!(renamed.paths[0].change, Change::Deleted);
    assert_eq!(renamed.paths[1].change, Change::Added);
}
#[test]
#[ignore = "requires a separately supplied local repository and SHA"]
fn real_repository_read_only() {
    use bslens_core::gitlab::{
        paths::changed_paths,
        types::{Limits, VendorPolicy},
    };
    let repo = std::env::var("BSLENS_GITLAB_REPO").unwrap();
    let sha = std::env::var("BSLENS_GITLAB_SHA").unwrap();
    let root = std::env::var("BSLENS_GITLAB_ROOT").unwrap();
    let input = input(&sha, &sha, &sha, &sha);
    let limits = Limits::default();
    let git = ProcessGit::new(PathBuf::from(repo).join(".git"), limits.max_output_bytes);
    check_graph(&input, &git).unwrap();
    let policy = VendorPolicy {
        config_root: root,
        ..VendorPolicy::default()
    };
    let result = changed_paths(&input, &policy, &limits, &git).unwrap();
    assert!(result.paths.is_empty());
    assert!(result.scope_count > 0);
    println!(
        "graph=ok changed_paths={} scope_count={}",
        result.paths.len(),
        result.scope_count
    );
}

#[test]
fn criss_cross_real_graph_has_two_bases() {
    let r = Repo::new();
    let base = r.commit("a", "base");
    let left = r.commit("a", "left");
    r.git(&["checkout", "-q", "--detach", &base]);
    let right = r.commit("b", "right");
    let tree = r.git(&["rev-parse", "HEAD^{tree}"]);
    let merge1 = r.git(&[
        "commit-tree",
        &tree,
        "-p",
        &left,
        "-p",
        &right,
        "-m",
        "merge1",
    ]);
    let merge2 = r.git(&[
        "commit-tree",
        &tree,
        "-p",
        &right,
        "-p",
        &left,
        "-m",
        "merge2",
    ]);
    assert_eq!(
        check_graph(&input(&base, &merge1, &merge2, &merge1), &r.reader())
            .unwrap_err()
            .reason,
        ReasonCode::MultipleMergeBases
    );
}

#[test]
fn object_and_path_failures_are_distinct() {
    let r = Repo::new();
    let sha = r.commit("a", "content");
    let missing = "1".repeat(40);
    assert_eq!(
        r.reader().object_type(&missing),
        Err(GitError::MissingObject)
    );
    let broken = ProcessGit::new("/nonexistent/.git", 1024 * 1024);
    assert_eq!(broken.object_type(&missing), Err(GitError::Failure));
    assert_eq!(r.reader().blob_oid(&sha, "absent"), Ok(None));
    assert_eq!(broken.blob_oid(&sha, "absent"), Err(GitError::Failure));
}

#[test]
fn diff_covers_paths_outside_scope() {
    use bslens_core::gitlab::{
        paths::changed_paths,
        types::{Change, Limits, VendorPolicy},
    };
    let r = Repo::new();
    std::fs::create_dir_all(r.0.join("cfg")).unwrap();
    std::fs::create_dir_all(r.0.join("other")).unwrap();
    std::fs::write(r.0.join("cfg/a"), "a").unwrap();
    let base = r.commit("other/a", "old");
    let head = r.commit("other/a", "new");
    let policy = VendorPolicy {
        config_root: "cfg".into(),
        ..VendorPolicy::default()
    };
    let result = changed_paths(
        &input(&base, &base, &head, &base),
        &policy,
        &Limits::default(),
        &r.reader(),
    )
    .unwrap();
    assert_eq!(result.scope_count, 1);
    assert_eq!(
        (result.paths[0].path.as_str(), result.paths[0].change),
        ("other/a", Change::Modified)
    );
    std::os::unix::fs::symlink("a", r.0.join("other/link")).unwrap();
    r.git(&["add", "."]);
    r.git(&["commit", "-qm", "link"]);
    let link = r.git(&["rev-parse", "HEAD"]);
    assert_eq!(
        changed_paths(
            &input(&head, &head, &link, &head),
            &policy,
            &Limits::default(),
            &r.reader()
        )
        .unwrap_err()
        .reason,
        ReasonCode::UnsupportedEntry
    );
}

#[test]
fn unrelated_history_has_no_merge_base() {
    let r = Repo::new();
    let base = r.commit("a", "base");
    r.git(&["checkout", "-q", "--orphan", "unrelated"]);
    r.git(&["rm", "-q", "-rf", "."]);
    let head = r.commit("b", "unrelated");
    assert_eq!(
        r.reader().merge_bases(&base, &head).unwrap(),
        Vec::<String>::new()
    );
    assert_eq!(
        check_graph(&input(&base, &base, &head, &base), &r.reader())
            .unwrap_err()
            .reason,
        ReasonCode::MergeBaseMismatch
    );
}
