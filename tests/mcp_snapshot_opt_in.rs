#![cfg(all(
    feature = "experimental-router",
    not(feature = "experimental-snapshot-mcp")
))]
use std::process::Command;
#[test]
fn snapshot_mode_requires_its_explicit_feature_without_git_fallback() {
    let output = Command::new(env!("CARGO_BIN_EXE_bslens-mcp"))
        .args([
            "--snapshot",
            "/missing.sqlite",
            "--staging-dir",
            "/missing",
            "--max-sqlite-bytes",
            "1024",
        ])
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(
        output.stderr,
        b"snapshot_mode_unavailable: requires experimental-snapshot-mcp on Linux x86_64\n"
    );
}
