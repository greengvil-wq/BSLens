#![cfg(feature = "experimental-xml")]
use bslens_core::xml221::{
    Code, Kind, Xml221Subset, MAX_DEPTH, MAX_FILES, MAX_TOTAL_BYTES, MAX_XML_BYTES, NS,
};
use std::collections::BTreeMap;
fn fixture() -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([
        (
            "Configuration.xml".into(),
            include_bytes!("fixtures/xml221/Configuration.xml").to_vec(),
        ),
        (
            "Catalogs/Items.xml".into(),
            include_bytes!("fixtures/xml221/Catalogs/Items.xml").to_vec(),
        ),
        (
            "Documents/Order.xml".into(),
            include_bytes!("fixtures/xml221/Documents/Order.xml").to_vec(),
        ),
        (
            "InformationRegisters/Events.xml".into(),
            include_bytes!("fixtures/xml221/InformationRegisters/Events.xml").to_vec(),
        ),
        (
            "Catalogs/Items/Ext/ObjectModule.bsl".into(),
            include_bytes!("fixtures/xml221/Catalogs/Items/Ext/ObjectModule.bsl").to_vec(),
        ),
        (
            "Documents/Order/Ext/ObjectModule.bsl".into(),
            include_bytes!("fixtures/xml221/Documents/Order/Ext/ObjectModule.bsl").to_vec(),
        ),
    ])
}
fn replace(b: &mut BTreeMap<String, Vec<u8>>, p: &str, a: &str, z: &str) {
    let s = String::from_utf8(b[p].clone()).unwrap();
    assert!(s.contains(a), "{a}");
    b.insert(p.into(), s.replace(a, z).into_bytes());
}
fn fails(b: &BTreeMap<String, Vec<u8>>, code: Code) {
    assert_eq!(Xml221Subset::inspect(b).unwrap_err().code, code);
}
fn single(raw: String) -> BTreeMap<String, Vec<u8>> {
    BTreeMap::from([("Configuration.xml".into(), raw.into_bytes())])
}
fn minimal() -> String {
    format!("<MetaDataObject xmlns=\"{NS}\" version=\"2.21\"><Configuration><Properties><Name>Root</Name></Properties><ChildObjects/></Configuration></MetaDataObject>")
}

#[test]
fn matches_python_authored_objects_modules_and_deterministic_order() {
    let b = fixture();
    let result = Xml221Subset::inspect(&b).unwrap();
    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/xml221/expected.json")).unwrap();
    assert_eq!(result.objects().len(), 4);
    assert!(result.unverified().len() > 0);
    for (actual, expected) in result
        .objects()
        .iter()
        .zip(expected["objects"].as_array().unwrap())
    {
        assert_eq!(actual.path, expected["path"]);
        assert_eq!(actual.name, expected["name"]);
        assert_eq!(actual.kind.name(), expected["type"]);
        assert_eq!(serde_json::json!(actual.uuid), expected["uuid"]);
        let modules: Vec<_> = actual
            .modules
            .iter()
            .map(|m| serde_json::json!([m.kind, m.path]))
            .collect();
        let wanted: Vec<_> = expected["modules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| serde_json::json!([m["kind"], m["path"]]))
            .collect();
        assert_eq!(modules, wanted);
    }
    assert_eq!(
        result.objects(),
        Xml221Subset::inspect(&b).unwrap().objects()
    );
}
#[test]
fn root_module_and_unassigned_are_not_guessed() {
    let mut b = fixture();
    b.insert("Ext/SessionModule.bsl".into(), b"// authored".to_vec());
    b.insert("Other/ObjectModule.bsl".into(), vec![]);
    let r = Xml221Subset::inspect(&b).unwrap();
    assert_eq!(r.unassigned_modules(), ["Other/ObjectModule.bsl"]);
    let root = r
        .objects()
        .iter()
        .find(|o| o.kind == Kind::Configuration)
        .unwrap();
    assert_eq!(root.modules[0].path, "Ext/SessionModule.bsl");
}
#[test]
fn namespace_prefix_bom_and_scalar_comments() {
    let raw = minimal().replace(&format!("xmlns=\"{NS}\""), &format!("xmlns:m=\"{NS}\""));
    let raw = raw
        .replace("<MetaDataObject", "<m:MetaDataObject")
        .replace("</MetaDataObject", "</m:MetaDataObject");
    // A default namespace on descendants is equivalent to a prefixed root.
    let raw = raw.replace(
        "<Configuration>",
        &format!("<Configuration xmlns=\"{NS}\">"),
    );
    let raw = raw.replace("Root", "Ro<!-- comment -->ot");
    let r = Xml221Subset::inspect(&single(format!("\u{feff}{raw}"))).unwrap();
    assert_eq!(r.objects()[0].name, "Root");
    fails(
        &single(format!("\u{feff}\u{feff}{}", minimal())),
        Code::Malformed,
    );
}
#[test]
fn unsafe_xml_and_encoding_fail_before_facts() {
    for (prefix, code) in [
        (
            "<!DOCTYPE MetaDataObject [<!ENTITY x SYSTEM 'file:///etc/passwd'>]>",
            Code::Dtd,
        ),
        ("<?evil data?>", Code::ProcessingInstruction),
        ("<?xml version='1.0' encoding='UTF-16'?>", Code::Encoding),
    ] {
        fails(&single(format!("{prefix}{}", minimal())), code);
    }
    let mut b = single(minimal());
    b.insert("Configuration.xml".into(), vec![0xff, 0xfe]);
    fails(&b, Code::Utf8);
    fails(
        &single(minimal().replace("Root", "&unknown;")),
        Code::Malformed,
    );
    fails(&single(minimal().replace("Root", "<bad>")), Code::Malformed);
}
#[test]
fn namespace_version_case_path_and_missing_reference() {
    fails(&single(minimal().replace(NS, "urn:wrong")), Code::Namespace);
    fails(&single(minimal().replace("2.21", "2.20")), Code::Version);
    let mut b = fixture();
    let raw = b.remove("Catalogs/Items.xml").unwrap();
    b.insert("Catalogs/items.xml".into(), raw);
    fails(&b, Code::Missing);
    let mut b = fixture();
    replace(
        &mut b,
        "Catalogs/Items.xml",
        "<Name>Items</Name>",
        "<Name>items</Name>",
    );
    fails(&b, Code::NameMismatch);
    for path in ["../bad.bsl", "/bad.xml", "C:/bad.xml", "bad\\bad.xml"] {
        let mut b = fixture();
        b.insert(path.into(), vec![]);
        fails(&b, Code::Path);
    }
    let mut b = fixture();
    b.remove("Configuration.xml");
    fails(&b, Code::Missing);
}
#[test]
fn duplicates_and_invalid_identities() {
    let mut b = fixture();
    replace(
        &mut b,
        "Configuration.xml",
        "<Catalog>Items</Catalog>",
        "<Catalog>Items</Catalog><Catalog>Items</Catalog>",
    );
    fails(&b, Code::Duplicate);
    let mut b = fixture();
    replace(
        &mut b,
        "Catalogs/Items.xml",
        "<Name>Items</Name>",
        "<Name>Items</Name><Name>Other</Name>",
    );
    fails(&b, Code::Shape);
    let mut b = fixture();
    replace(
        &mut b,
        "Catalogs/Items.xml",
        "<Name>Items</Name>",
        "<Name>../Items</Name>",
    );
    fails(&b, Code::Identifier);
    let mut b = single(minimal().replace("<Configuration>", "<Configuration uuid='invalid'>"));
    fails(&b, Code::Uuid);
    b = fixture();
    let catalog = String::from_utf8(b["Catalogs/Items.xml"].clone()).unwrap();
    let doc = String::from_utf8(b["Documents/Order.xml"].clone()).unwrap();
    let uuid = |s: &str| {
        s.split("uuid=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_string()
    };
    replace(&mut b, "Documents/Order.xml", &uuid(&doc), &uuid(&catalog));
    fails(&b, Code::Duplicate);
}
#[test]
fn unsupported_nested_orphan_are_refusals_not_empty_coverage() {
    let mut b = fixture();
    replace(
        &mut b,
        "Configuration.xml",
        "<Catalog>Items</Catalog>",
        "<Role>Items</Role>",
    );
    fails(&b, Code::UnsupportedType);
    let mut b = fixture();
    replace(
        &mut b,
        "Catalogs/Items.xml",
        "<ChildObjects></ChildObjects>",
        "<ChildObjects><Attribute/></ChildObjects>",
    );
    fails(&b, Code::NestedUnsupported);
    let mut b = fixture();
    b.insert("Roles/Extra.xml".into(), minimal().into_bytes());
    fails(&b, Code::OrphanXml);
    let mut b = fixture();
    replace(&mut b, "Configuration.xml", "<Catalog>Items</Catalog>", "");
    fails(&b, Code::OrphanXml);
}
#[test]
fn bounded_files_bytes_depth_nodes_and_events() {
    let mut b = single(minimal());
    b.insert("TooBig.xml".into(), vec![b' '; MAX_XML_BYTES + 1]);
    fails(&b, Code::Limit);
    let mut b = single(minimal());
    for n in 0..MAX_FILES {
        b.insert(format!("M{n}.bsl"), vec![]);
    }
    fails(&b, Code::Limit);
    let mut b = single(minimal());
    b.insert("Large.bsl".into(), vec![0; MAX_TOTAL_BYTES]);
    fails(&b, Code::Limit);
    let deep = format!(
        "{}{}{}",
        "<x>".repeat(MAX_DEPTH + 1),
        "a",
        "</x>".repeat(MAX_DEPTH + 1)
    );
    fails(&single(deep), Code::Limit);
    fails(
        &single(format!("<x>{}</x>", "<a/>".repeat(10000))),
        Code::Limit,
    );
    fails(
        &single(format!("<x>{}</x>", "<!--x-->".repeat(20000))),
        Code::Limit,
    );
}
