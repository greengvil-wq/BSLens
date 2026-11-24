//! Локальный Linux CLI build/open/tree/source/verify; MCP не используется.
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
    if args.first().is_some_and(|s| s == "build-ingest") {
        return ingest_command(&args);
    }
    if args.first().is_some_and(|s| s != "build") {
        return read_command(&args);
    }
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

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn read_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        linux_v5_reader::{LinuxSnapshotReader, TreeLimits},
        snapshot_v5::Limits,
    };
    use std::{collections::BTreeMap, io::Write, path::Path};
    let command = args.first().ok_or("Нет команды")?.as_str();
    if !["open", "tree", "source", "verify"].contains(&command) || args.len() < 2 {
        return Err("Команды: build, open, tree, source, verify. Чтение: COMMAND FILE --staging-dir DIR --max-sqlite-bytes N [--expected-sha256 SHA]".into());
    }
    let mut options = BTreeMap::new();
    for pair in args[2..].chunks(2) {
        if pair.len() != 2
            || !pair[0].starts_with("--")
            || options.insert(pair[0].as_str(), pair[1].as_str()).is_some()
        {
            return Err("Неверный или повторный параметр".into());
        }
    }
    let common = ["--staging-dir", "--max-sqlite-bytes", "--expected-sha256"];
    let specific: &[&str] = match command {
        "tree" => &[
            "--level",
            "--parent-id",
            "--kind",
            "--module-path",
            "--max-rows",
            "--max-bytes",
        ],
        "source" => &["--path", "--start-line", "--end-line", "--max-bytes"],
        "verify" => &[
            "--max-sources",
            "--max-source-bytes",
            "--max-total-bytes",
            "--max-scratch-bytes",
        ],
        _ => &[],
    };
    if options
        .keys()
        .any(|k| !common.contains(k) && !specific.contains(k))
    {
        return Err("Неизвестный параметр команды".into());
    }
    let required = |key: &str| -> Result<&str, Box<dyn std::error::Error>> {
        options
            .get(key)
            .copied()
            .ok_or_else(|| format!("Обязательный параметр {key}").into())
    };
    let reader = LinuxSnapshotReader::open(
        Path::new(&args[1]),
        Path::new(required("--staging-dir")?),
        options.get("--expected-sha256").copied(),
        required("--max-sqlite-bytes")?.parse()?,
    )?;
    match command {
        "open" => println!("{}", reader.info()?),
        "source" => {
            let bytes = reader.source(
                required("--path")?,
                required("--start-line")?.parse()?,
                required("--end-line")?.parse()?,
                required("--max-bytes")?.parse()?,
            )?;
            std::io::stdout().lock().write_all(&bytes)?;
        }
        "tree" => {
            let nodes = reader.tree(
                required("--level")?,
                options.get("--parent-id").map(|s| s.parse()).transpose()?,
                options.get("--kind").copied(),
                options.get("--module-path").copied(),
                TreeLimits {
                    rows: required("--max-rows")?.parse()?,
                    bytes: required("--max-bytes")?.parse()?,
                },
            )?;
            let nodes:Vec<_>=nodes.into_iter().map(|n|serde_json::json!({"type":n.node_type,"key":n.key,"name":n.name,"status":n.status,"reason":n.reason})).collect();
            println!(
                "{}",
                serde_json::json!({"snapshot":reader.info()?,"nodes":nodes})
            );
        }
        "verify" => {
            reader.verify(Limits {
                source_count: required("--max-sources")?.parse()?,
                source_bytes: required("--max-source-bytes")?.parse()?,
                total_source_bytes: required("--max-total-bytes")?.parse()?,
                sqlite_bytes: required("--max-scratch-bytes")?.parse()?,
            })?;
            println!(
                "{}",
                serde_json::json!({"snapshot":reader.info()?,"offline_facts_verified":true,"git_provenance":"not_verified"})
            );
        }
        _ => unreachable!(),
    }
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn ingest_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{ingest_writer, Limits},
    };
    if args.len() != 16 {
        return Err("Использование: bslens-v5 build-ingest REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS".into());
    }
    let result = ingest_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        Limits {
            source_count: args[6].parse()?,
            source_bytes: args[7].parse()?,
            total_source_bytes: args[8].parse()?,
            sqlite_bytes: args[9].parse()?,
        },
        ingest::Limits {
            entries: args[10].parse()?,
            trees: args[11].parse()?,
            reads: args[12].parse()?,
            object_bytes: args[13].parse()?,
            total_bytes: args[14].parse()?,
            deadline: std::time::Duration::from_millis(args[15].parse()?),
        },
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":ingest_writer::PROFILE,"experimental":true})
    );
    Ok(())
}
