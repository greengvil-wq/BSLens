#![cfg(all(feature = "experimental-pipeline", target_os = "linux"))]
use bslens_core::{
    git_manifest::{Code, Limits, Manifest},
    pipeline::{Error, GitStructure},
    structure::NodeKind,
    xml221,
};
use std::{
    collections::BTreeMap,
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
            "bslens-r05-{}-{}",
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
    fn replace(&self, path: &str, from: &str, to: &str) {
        let p = self.path.join(path);
        let s = fs::read_to_string(&p).unwrap();
        assert!(s.contains(from));
        fs::write(p, s.replace(from, to)).unwrap();
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn bytes(root: &std::path::Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut dirs = vec![root.to_owned()];
    let mut out = BTreeMap::new();
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                dirs.push(entry.path());
            } else {
                out.insert(
                    entry.path().strip_prefix(root).unwrap().to_owned(),
                    fs::read(entry.path()).unwrap(),
                );
            }
        }
    }
    out
}
#[test]
fn exact_git_to_module_is_deterministic_and_ignores_dirty_worktree() {
    let r = Repo::new();
    let sha = r.commit();
    r.replace("Configuration.xml", "2.21", "broken");
    fs::write(r.path.join("Catalogs/Items/Ext/ObjectModule.bsl"), b"dirty").unwrap();
    fs::write(r.path.join("untracked.xml"), b"broken").unwrap();
    let before = bytes(&r.path);
    let s = GitStructure::read(&r.path, &sha, Limits::default()).unwrap();
    let manifest = Manifest::read(&r.path, &sha, Limits::default()).unwrap();
    let again = GitStructure::from_manifest(&manifest).unwrap();
    let root = s.page(None, 0, 20).unwrap();
    assert_eq!(root, again.page(None, 0, 20).unwrap());
    assert_eq!(root.context.commit, sha);
    assert_eq!(root.context.tree, manifest.tree());
    assert_eq!(
        (root.context.selected_files, root.context.other_files),
        (6, 1)
    );
    let group = root
        .page
        .items
        .iter()
        .find(|n| n.kind == NodeKind::ObjectType && n.label == "Catalog")
        .unwrap();
    let object = s.page(Some(&group.id), 0, 20).unwrap().page.items.remove(0);
    let module = s
        .page(Some(&object.id), 0, 20)
        .unwrap()
        .page
        .items
        .remove(0);
    assert_eq!(module.kind, NodeKind::Module);
    assert_eq!(
        module.address.as_deref(),
        Some("Catalogs/Items/Ext/ObjectModule.bsl")
    );
    let leaf = s.page(Some(&module.id), 0, 20).unwrap();
    assert_eq!(leaf.page.total, 0);
    assert_eq!(
        leaf.page.coverage.methods,
        "not implemented; no validated lexical facts"
    );
    assert_eq!(before, bytes(&r.path));
    assert!(bslens_core::ValidatedSnapshot::open(&r.path, &sha).is_err());
}
#[test]
fn code_only_commit_changes_binding_not_structure_scope() {
    let r = Repo::new();
    let first = r.commit();
    let a = GitStructure::read(&r.path, &first, Limits::default()).unwrap();
    fs::write(
        r.path.join("Catalogs/Items/Ext/ObjectModule.bsl"),
        b"// another body\n",
    )
    .unwrap();
    let second = r.commit();
    let b = GitStructure::read(&r.path, &second, Limits::default()).unwrap();
    let pa = a.page(None, 0, 20).unwrap();
    let pb = b.page(None, 0, 20).unwrap();
    assert_ne!(pa.context.commit, pb.context.commit);
    assert_ne!(pa.page.node.id, pb.page.node.id);
    assert_eq!(pa.page.node.label, pb.page.node.label);
    assert!(matches!(
        b.page(Some(&pa.page.node.id), 0, 20),
        Err(Error::ContextMismatch)
    ));
}
#[test]
fn pages_have_no_duplicates_and_keep_exact_context() {
    let r = Repo::new();
    let sha = r.commit();
    let s = GitStructure::read(&r.path, &sha, Limits::default()).unwrap();
    let all = s.page(None, 0, 20).unwrap();
    let mut offset = 0;
    let mut items = vec![];
    loop {
        let p = s.page(None, offset, 1).unwrap();
        assert_eq!(p.context, s.context().clone());
        items.extend(p.page.items);
        if let Some(next) = p.page.next_offset {
            offset = next;
        } else {
            break;
        }
    }
    assert_eq!(items, all.page.items);
    assert!(s.page(Some("p1:foreign"), 0, 1).is_err());
    assert!(s.page(None, 0, 0).is_err());
}
#[test]
fn typed_failures_do_not_return_partial_structure() {
    let r = Repo::new();
    let sha = r.commit();
    assert!(matches!(
        GitStructure::read(&r.path, "HEAD", Limits::default()),
        Err(Error::Git(Code::InvalidSha))
    ));
    assert!(matches!(
        GitStructure::read(
            &r.path,
            &sha,
            Limits {
                files: 1,
                ..Limits::default()
            }
        ),
        Err(Error::Git(Code::Limit))
    ));
    r.replace(
        "Configuration.xml",
        "<Catalog>Items</Catalog>",
        "<Role>Items</Role>",
    );
    let bad = r.commit();
    assert!(matches!(
        GitStructure::read(&r.path, &bad, Limits::default()),
        Err(Error::Xml(xml221::XmlError {
            code: xml221::Code::UnsupportedType,
            ..
        }))
    ));
    assert!(GitStructure::read(&r.path, &sha, Limits::default()).is_ok());
}

#[test]
fn commit_prefix_is_included_in_page_text_budget() {
    let r = Repo::new();
    // Long Git paths, with every physical filename component below filesystem limits.
    let dirs = vec!["a".repeat(200); 4].join("/");
    for i in 0..9 {
        let path = r
            .path
            .join(format!("Unassigned/{dirs}/N{i}{}.bsl", "x".repeat(29)));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"// authored").unwrap();
    }
    let sha = r.commit();
    let s = GitStructure::read(&r.path, &sha, Limits::default()).unwrap();
    let root = s.page(None, 0, 20).unwrap();
    let group = root
        .page
        .items
        .iter()
        .find(|n| n.kind == NodeKind::UnassignedModules)
        .unwrap();
    assert!(matches!(
        s.page(Some(&group.id), 0, 9),
        Err(Error::Structure(bslens_core::structure::Error::TextBudget))
    ));
    let small = s.page(Some(&group.id), 0, 8).unwrap();
    assert_eq!(small.page.items.len(), 8);
    assert_eq!(small.page.next_offset, Some(8));
}
