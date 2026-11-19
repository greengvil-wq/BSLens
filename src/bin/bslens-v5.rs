//! Отдельный локальный CLI S3; существующий MCP не меняется.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn main() {
    if let Err(e) = run() {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
fn main() {
    eprintln!("v5: поддерживается только Linux x86_64; эта платформа не поддерживается");
    std::process::exit(1);
}
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::snapshot_v5::{build, Limits};
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 9 || args[0] != "build" {
        return Err("Использование: bslens-v5 build REPO FULL_COMMIT SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES".into());
    }
    let result = build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        std::path::Path::new(&args[4]),
        Limits {
            source_count: args[5].parse()?,
            source_bytes: args[6].parse()?,
            total_source_bytes: args[7].parse()?,
            sqlite_bytes: args[8].parse()?,
        },
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"experimental":true})
    );
    Ok(())
}
