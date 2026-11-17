#[test]
fn production_is_closed_even_for_existing_files() {
    let error = bslens_core::ValidatedSnapshot::open(std::path::Path::new("Cargo.toml"), "any")
        .err()
        .unwrap();
    assert!(error
        .to_string()
        .contains("production validation not implemented"));
}
