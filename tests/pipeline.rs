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
    assert_eq!(leaf.page.total, 1);
    assert_eq!(
        leaf.page.coverage.methods,
        "lexical zero-argument subset; no body/call validation; absence unknown"
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

fn module_node(s: &GitStructure, path: &str) -> bslens_core::structure::Node {
    let mut queue = vec![s.page(None, 0, 20).unwrap().page.node];
    while let Some(node) = queue.pop() {
        if node.kind == NodeKind::Module {
            if node.address.as_deref() == Some(path) {
                return node;
            }
            continue;
        }
        let mut offset = 0;
        loop {
            let p = s.page(Some(&node.id), offset, 20).unwrap();
            queue.extend(p.page.items);
            match p.page.next_offset {
                Some(n) => offset = n,
                None => break,
            }
        }
    }
    panic!("module missing {path}")
}
#[test]
fn exact_methods_source_duplicates_dirty_and_commit_move() {
    let r = Repo::new();
    let path = "Catalogs/Items/Ext/ObjectModule.bsl";
    let raw="\u{feff}// 😀\r\n  Процедура ЁЖ()\r\n // тело\r\nКонецПроцедуры // хвост\r\nФункция ёж()\r\nВозврат 1;\r\nКонецФункции\r\n";
    fs::write(r.path.join(path), raw).unwrap();
    let a = r.commit();
    fs::write(r.path.join(path), "dirty").unwrap();
    let before = bytes(&r.path);
    let sa = GitStructure::read(&r.path, &a, Limits::default()).unwrap();
    let module = module_node(&sa, path);
    let p = sa.page(Some(&module.id), 0, 1).unwrap();
    assert_eq!(p.page.total, 2);
    assert_eq!(p.page.next_offset, Some(1));
    assert_eq!(p.methods[0].boundary.name, "ёж");
    assert_eq!(p.methods[0].boundary.occurrences, 2);
    assert!(p.methods[0].boundary.duplicate);
    let second = sa.page(Some(&module.id), 1, 1).unwrap();
    assert_ne!(p.page.items[0].id, second.page.items[0].id);
    assert!(sa
        .page(Some(&module.id), usize::MAX, 1)
        .unwrap()
        .page
        .items
        .is_empty());
    let method = sa.page(Some(&p.page.items[0].id), 0, 1).unwrap();
    assert_eq!(method.page.total, 0);
    assert_eq!(method.methods, p.methods);
    let reference = p.methods[0].source.as_ref().unwrap();
    let source = sa.source(reference).unwrap();
    assert_eq!(source.context.commit, a);
    assert_eq!(source.reference.blob_oid, p.methods[0].blob_oid);
    assert_eq!(
        source.text.as_bytes(),
        &raw.as_bytes()[source.start_byte..source.end_byte]
    );
    assert!(source.text.starts_with("  Процедура"));
    assert!(source.text.ends_with("// хвост\r\n"));
    let fact = &p.methods[0].boundary;
    assert!(
        std::str::from_utf8(&raw.as_bytes()[fact.start_byte..fact.end_byte])
            .unwrap()
            .ends_with("КонецПроцедуры")
    );
    assert_eq!(before, bytes(&r.path));
    fs::write(
        r.path.join(path),
        format!("// moved\r\n{}", raw.trim_start_matches('\u{feff}')),
    )
    .unwrap();
    let b = r.commit();
    let sb = GitStructure::read(&r.path, &b, Limits::default()).unwrap();
    assert!(matches!(
        sb.page(Some(&p.page.items[0].id), 0, 1),
        Err(Error::ContextMismatch)
    ));
    assert!(matches!(sb.source(reference), Err(Error::ContextMismatch)));
    let mb = module_node(&sb, path);
    let moved = sb.page(Some(&mb.id), 0, 1).unwrap();
    assert_eq!(moved.methods[0].boundary.start_line, fact.start_line + 1);
    let mut bad = reference.clone();
    bad.blob_oid = "0".repeat(40);
    assert!(matches!(sa.source(&bad), Err(Error::SourceIdentity)));
    bad = reference.clone();
    bad.start_line = 0;
    assert!(matches!(sa.source(&bad), Err(Error::SourceRange)));
    bad = reference.clone();
    bad.end_line = usize::MAX;
    assert!(matches!(sa.source(&bad), Err(Error::SourceRange)));
}
#[test]
fn lexical_failures_are_explicit_and_unassigned_is_honest() {
    let r = Repo::new();
    let path = "Other/Непривязанный.bsl";
    fs::create_dir_all(r.path.join("Other")).unwrap();
    fs::write(r.path.join(path), "Процедура Тест()\nКонецПроцедуры").unwrap();
    let a = r.commit();
    let s = GitStructure::read(&r.path, &a, Limits::default()).unwrap();
    let m = module_node(&s, path);
    assert_eq!(m.ownership, Some("unverified"));
    let p = s.page(Some(&m.id), 0, 20).unwrap();
    assert_eq!(p.page.items[0].ownership, Some("unverified"));
    let reference = p.methods[0].source.as_ref().unwrap();
    assert!(s.source(reference).unwrap().text.contains("Тест"));
    for raw in [
        "Procedure X()\n// hidden\0\nEndProcedure",
        "#If Server Then\nProcedure X()\nEndProcedure\n#EndIf",
    ] {
        fs::write(r.path.join(path), raw).unwrap();
        let sha = r.commit();
        let s = GitStructure::read(&r.path, &sha, Limits::default()).unwrap();
        let m = module_node(&s, path);
        assert!(matches!(s.page(Some(&m.id), 0, 20), Err(Error::Lexical(_))));
    }
}
#[test]
fn method_source_and_page_budgets_do_not_clip() {
    let r = Repo::new();
    let path = "Catalogs/Items/Ext/ObjectModule.bsl";
    let raw = format!("Procedure X()\n{}EndProcedure\n", "// я\n".repeat(130));
    fs::write(r.path.join(path), &raw).unwrap();
    let sha = r.commit();
    let s = GitStructure::read(&r.path, &sha, Limits::default()).unwrap();
    let m = module_node(&s, path);
    let p = s.page(Some(&m.id), 0, 20).unwrap();
    assert_eq!(p.methods[0].source.as_ref().unwrap().end_line, 100);
    assert_eq!(p.methods[0].source_next_line, Some(101));
    let mut reference = p.methods[0].source.clone().unwrap();
    assert_eq!(s.source(&reference).unwrap().positions.len(), 100);
    reference.end_line = 101;
    assert!(matches!(s.source(&reference), Err(Error::SourceRange)));
    let raw = format!("Procedure X() // {}\nEndProcedure\n", "я".repeat(9000));
    fs::write(r.path.join(path), raw).unwrap();
    let sha = r.commit();
    let s = GitStructure::read(&r.path, &sha, Limits::default()).unwrap();
    let m = module_node(&s, path);
    let p = s.page(Some(&m.id), 0, 20).unwrap();
    assert!(p.methods[0].source.is_none());
    assert!(p.methods[0].source_unavailable.is_some());
    let raw = (0..20)
        .map(|i| format!("Procedure {}{i}()\nEndProcedure\n", "Ж".repeat(200)))
        .collect::<String>();
    fs::write(r.path.join(path), raw).unwrap();
    let sha = r.commit();
    let s = GitStructure::read(&r.path, &sha, Limits::default()).unwrap();
    let m = module_node(&s, path);
    assert!(matches!(
        s.page(Some(&m.id), 0, 20),
        Err(Error::Structure(bslens_core::structure::Error::TextBudget))
    ));
    assert_eq!(s.page(Some(&m.id), 0, 1).unwrap().page.items.len(), 1);
}

#[test]
fn source_multibyte_budget_exact_edge_and_forged_method_ids() {
    let r = Repo::new();
    let path = "Catalogs/Items/Ext/ObjectModule.bsl";
    let initial = r.commit();
    let tmp = GitStructure::read(&r.path, &initial, Limits::default()).unwrap();
    let m = module_node(&tmp, path);
    let p = tmp.page(Some(&m.id), 0, 1).unwrap();
    let overhead = initial.len() * 2
        + tmp.context().tree.len()
        + p.methods[0].blob_oid.len()
        + path.len()
        + p.methods[0].sha256.len();
    let capacity = bslens_core::structure::MAX_TEXT_BYTES - overhead;
    let content = capacity - 4;
    let comment = format!(
        "//{}{}\r\n",
        "я".repeat(content / 2),
        "x".repeat(content % 2)
    );
    assert_eq!(comment.len(), capacity);
    for (extra, expected_ok) in [("", true), ("x", false)] {
        let body = comment.replace("\r\n", &format!("{extra}\r\n"));
        fs::write(
            r.path.join(path),
            format!("Procedure X()\r\n{body}EndProcedure\r\n"),
        )
        .unwrap();
        let sha = r.commit();
        let s = GitStructure::read(&r.path, &sha, Limits::default()).unwrap();
        let m = module_node(&s, path);
        let p = s.page(Some(&m.id), 0, 1).unwrap();
        let reference = bslens_core::pipeline::SourceRef {
            commit: sha,
            blob_oid: p.methods[0].blob_oid.clone(),
            path: path.into(),
            start_line: 2,
            end_line: 2,
        };
        let result = s.source(&reference);
        assert_eq!(result.is_ok(), expected_ok);
        if expected_ok {
            assert_eq!(result.unwrap().text.as_bytes(), body.as_bytes());
        } else {
            assert!(matches!(
                result,
                Err(Error::Structure(bslens_core::structure::Error::TextBudget))
            ));
        }
        for suffix in [":m:00", ":m:-1", ":m:999", ":m:١", ":m:0:m:0"] {
            assert!(s.page(Some(&format!("{}{suffix}", m.id)), 0, 1).is_err());
        }
        let root = s.page(None, 0, 1).unwrap().page.node;
        assert!(s.page(Some(&format!("{}:m:0", root.id)), 0, 1).is_err());
    }
}
