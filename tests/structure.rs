#![cfg(feature = "experimental-tree")]
use bslens_core::{
    structure::{Error, NodeKind, Structure},
    xml221::{Xml221Subset, NS},
};
use std::collections::BTreeMap;
fn minimal() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([("Configuration.xml".into(),format!("<MetaDataObject xmlns='{NS}' version='2.21'><Configuration><Properties><Name>Root</Name></Properties><ChildObjects/></Configuration></MetaDataObject>").into_bytes())])
}
fn project(b: &BTreeMap<String, Vec<u8>>) -> Structure {
    Structure::from_subset(&Xml221Subset::inspect(b).unwrap()).unwrap()
}
#[test]
fn empty_and_strict_pagination() {
    let s = project(&minimal());
    let page = s.page(None, 0, 20).unwrap();
    assert_eq!(page.node.kind, NodeKind::Configuration);
    assert!(page.items.is_empty());
    assert_eq!(page.total, 0);
    assert_eq!(page.next_offset, None);
    assert!(s.page(None, usize::MAX, 1).unwrap().items.is_empty());
    assert_eq!(s.page(None, 0, 0), Err(Error::InvalidLimit));
    assert_eq!(s.page(None, 0, 21), Err(Error::InvalidLimit));
    assert_eq!(s.page(Some("unknown"), 0, 1), Err(Error::UnknownNode));
    assert_eq!(
        page.coverage.methods,
        "not implemented; no validated lexical facts"
    );
}
#[test]
fn typed_hierarchy_and_unverified_coverage() {
    let mut b = minimal();
    let root = String::from_utf8(b["Configuration.xml"].clone())
        .unwrap()
        .replace(
            "<ChildObjects/>",
            "<ChildObjects><Catalog>Товары</Catalog></ChildObjects>",
        )
        .replace("</Properties>", "<Custom>x</Custom></Properties>");
    b.insert("Configuration.xml".into(), root.into_bytes());
    b.insert("Catalogs/Товары.xml".into(),format!("<MetaDataObject xmlns='{NS}' version='2.21'><Catalog><Properties><Name>Товары</Name></Properties><ChildObjects/></Catalog></MetaDataObject>").into_bytes());
    b.insert(
        "Catalogs/Товары/Ext/ObjectModule.bsl".into(),
        b"// opaque bytes".to_vec(),
    );
    b.insert("Ext/SessionModule.bsl".into(), vec![0xff]);
    b.insert("Other/Module.bsl".into(), vec![]);
    let s = project(&b);
    let root = s.page(None, 0, 20).unwrap();
    assert_eq!(root.coverage.unverified_entries, 1);
    assert_eq!(root.coverage.unassigned_modules, 1);
    let group = root
        .items
        .iter()
        .find(|n| n.kind == NodeKind::ObjectType)
        .unwrap();
    let obj = s.page(Some(&group.id), 0, 1).unwrap().items.remove(0);
    assert_eq!(obj.address.as_deref(), Some("Catalogs/Товары.xml"));
    let module = s.page(Some(&obj.id), 0, 1).unwrap().items.remove(0);
    assert_eq!(module.ownership, Some("xml-subset-exact-path"));
    assert_eq!(s.page(Some(&module.id), 0, 20).unwrap().total, 0);
    let unassigned = root
        .items
        .iter()
        .find(|n| n.kind == NodeKind::UnassignedModules)
        .unwrap();
    assert_eq!(
        s.page(Some(&unassigned.id), 0, 20).unwrap().items[0].ownership,
        Some("unverified")
    );
}
#[test]
fn stable_scope_and_full_page_walk() {
    let mut b = minimal();
    for i in (0..45).rev() {
        b.insert(format!("Unassigned/{i:03}.bsl"), vec![]);
    }
    let s = project(&b);
    let root = s.page(None, 0, 20).unwrap();
    let group = &root.items[0].id;
    assert_eq!(root, project(&b).page(None, 0, 20).unwrap());
    let mut offset = 0;
    let mut all = vec![];
    loop {
        let p = s.page(Some(group), offset, 7).unwrap();
        all.extend(p.items);
        match p.next_offset {
            Some(n) => offset = n,
            None => break,
        }
    }
    assert_eq!(all.len(), 45);
    assert_eq!(all[0].label, "Unassigned/000.bsl");
    assert_eq!(all[44].label, "Unassigned/044.bsl");
    assert!(s.page(Some(group), 45, 7).unwrap().items.is_empty());
    assert!(s.page(Some(group), usize::MAX, 7).unwrap().items.is_empty());
    let xml = String::from_utf8(b["Configuration.xml"].clone())
        .unwrap()
        .replace("Root", "Other");
    b.insert("Configuration.xml".into(), xml.into_bytes());
    assert_eq!(project(&b).page(Some(group), 0, 1), Err(Error::UnknownNode));
}
#[test]
fn explicit_text_budget_no_silent_clipping() {
    let mut b = minimal();
    for i in 0..20 {
        b.insert(format!("{}{:02}.bsl", "я".repeat(450), i), vec![]);
    }
    let s = project(&b);
    let group = s.page(None, 0, 20).unwrap().items.remove(0);
    assert_eq!(s.page(Some(&group.id), 0, 20), Err(Error::TextBudget));
    let p = s.page(Some(&group.id), 0, 1).unwrap();
    assert_eq!(p.total, 20);
    assert_eq!(p.items.len(), 1);
    assert_eq!(p.next_offset, Some(1));
}
#[test]
fn projection_does_not_claim_body_or_provenance() {
    let mut b = minimal();
    b.insert("Ext/SessionModule.bsl".into(), b"original".to_vec());
    let before = project(&b).page(None, 0, 20).unwrap();
    b.insert("Ext/SessionModule.bsl".into(), b"changed".to_vec());
    assert_eq!(before, project(&b).page(None, 0, 20).unwrap());
    assert!(before.coverage.provenance.contains("not checked"));
    assert!(
        bslens_core::ValidatedSnapshot::open(std::path::Path::new("Cargo.toml"), "any").is_err()
    );
    let raw = String::from_utf8(b["Configuration.xml"].clone())
        .unwrap()
        .replace(
            "<ChildObjects/>",
            "<ChildObjects><Role>Hidden</Role></ChildObjects>",
        );
    b.insert("Configuration.xml".into(), raw.into_bytes());
    assert!(Xml221Subset::inspect(&b).is_err());
}
