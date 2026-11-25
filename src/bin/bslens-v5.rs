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
    if args.first().is_some_and(|s| {
        matches!(
            s.as_str(),
            "build-strings"
                | "build-literals"
                | "build-wrapped-headers"
                | "build-corpus-refinements"
                | "build-annotated-vars"
                | "build-dates-members"
                | "build-async-casing"
                | "build-mobile-vars"
                | "build-method-regions"
                | "build-header-forms"
                | "build-source-limits"
                | "build-long-headers"
                | "build-lexical-tail"
                | "build-source-anomalies"
                | "build-navigator-agreement"
                | "build-navigator-owners"
        )
    }) {
        return string_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-conditionals") {
        return conditional_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-parameters") {
        return parameter_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-context") {
        return context_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-root-sidecars") {
        return sidecar_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-optional-form") {
        return optional_form_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-diagnostics") {
        return diagnostics_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-corpus-config") {
        return corpus_config_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-corpus-help") {
        return corpus_help_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-corpus-bin") {
        return corpus_bin_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-corpus") {
        return corpus_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-workflows") {
        return workflow_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-reports") {
        return report_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-registers") {
        return register_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-charts") {
        return chart_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-services") {
        return service_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-journals") {
        return journal_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-leaves") {
        return leaf_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-shared") {
        return shared_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-roots") {
        return root_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-common-modules") {
        return common_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-companions") {
        return companion_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-inline") {
        return inline_command(&args);
    }
    if args.first().is_some_and(|s| s == "build-stream") {
        return stream_command(&args);
    }
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
            "--max-diagnostics-bytes",
            "--max-opaque-bin-bytes",
            "--max-sources",
            "--max-source-bytes",
            "--max-total-bytes",
            "--max-scratch-bytes",
            "--max-facts",
            "--max-coverage-bytes",
            "--facts-time-ms",
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
            let limits = Limits {
                source_count: required("--max-sources")?.parse()?,
                source_bytes: required("--max-source-bytes")?.parse()?,
                total_source_bytes: required("--max-total-bytes")?.parse()?,
                sqlite_bytes: required("--max-scratch-bytes")?.parse()?,
            };
            if [
                bslens_core::snapshot_v5::streaming_writer::PROFILE,
                bslens_core::snapshot_v5::inline_writer::PROFILE,
                bslens_core::snapshot_v5::companion_writer::PROFILE,
                bslens_core::snapshot_v5::common_writer::PROFILE,
                bslens_core::snapshot_v5::root_writer::PROFILE,
                bslens_core::snapshot_v5::shared_writer::PROFILE,
                bslens_core::snapshot_v5::leaf_writer::PROFILE,
                bslens_core::snapshot_v5::journal_writer::PROFILE,
                bslens_core::snapshot_v5::service_writer::PROFILE,
                bslens_core::snapshot_v5::chart_writer::PROFILE,
                bslens_core::snapshot_v5::register_writer::PROFILE,
                bslens_core::snapshot_v5::report_writer::PROFILE,
                bslens_core::snapshot_v5::workflow_writer::PROFILE,
                bslens_core::snapshot_v5::corpus_writer::PROFILE,
                bslens_core::snapshot_v5::corpus_bin_writer::PROFILE,
                bslens_core::snapshot_v5::corpus_help_writer::PROFILE,
                bslens_core::snapshot_v5::corpus_config_writer::PROFILE,
                bslens_core::snapshot_v5::diagnostics_writer::PROFILE,
                bslens_core::snapshot_v5::optional_form_writer::PROFILE,
                bslens_core::snapshot_v5::sidecar_writer::PROFILE,
                bslens_core::snapshot_v5::context_writer::PROFILE,
                bslens_core::snapshot_v5::parameter_writer::PROFILE,
                bslens_core::snapshot_v5::conditional_writer::PROFILE,
                bslens_core::snapshot_v5::string_writer::PROFILE,
                bslens_core::snapshot_v5::literal_writer::PROFILE,
                bslens_core::snapshot_v5::wrapped_writer::PROFILE,
                bslens_core::snapshot_v5::refined_writer::PROFILE,
                bslens_core::snapshot_v5::annotated_vars_writer::PROFILE,
                bslens_core::snapshot_v5::dates_members_writer::PROFILE,
                bslens_core::snapshot_v5::async_casing_writer::PROFILE,
                bslens_core::snapshot_v5::mobile_vars_writer::PROFILE,
                bslens_core::snapshot_v5::method_regions_writer::PROFILE,
                bslens_core::snapshot_v5::header_forms_writer::PROFILE,
                bslens_core::snapshot_v5::source_limits_writer::PROFILE,
                bslens_core::snapshot_v5::long_headers_writer::PROFILE,
                bslens_core::snapshot_v5::lexical_tail_writer::PROFILE,
                bslens_core::snapshot_v5::source_anomalies_writer::PROFILE,
                bslens_core::snapshot_v5::navigator_agreement_writer::PROFILE,
                bslens_core::snapshot_v5::navigator_owners_writer::PROFILE,
            ]
            .contains(&reader.info()?["profile"].as_str().unwrap_or(""))
            {
                let policy = bslens_core::snapshot_v5::streaming_facts::Limits {
                    sources: limits,
                    facts: required("--max-facts")?.parse()?,
                    coverage_bytes: required("--max-coverage-bytes")?.parse()?,
                    time: std::time::Duration::from_millis(required("--facts-time-ms")?.parse()?),
                };
                if let Some(n) = options.get("--max-diagnostics-bytes") {
                    reader.verify_streaming_with_diagnostics(
                        policy,
                        required("--max-opaque-bin-bytes")?.parse()?,
                        n.parse()?,
                    )?;
                } else if let Some(n) = options.get("--max-opaque-bin-bytes") {
                    reader.verify_streaming_with_opaque_bin(policy, n.parse()?)?;
                } else {
                    reader.verify_streaming(policy)?;
                }
            } else {
                if [
                    "--max-diagnostics-bytes",
                    "--max-opaque-bin-bytes",
                    "--max-facts",
                    "--max-coverage-bytes",
                    "--facts-time-ms",
                ]
                .iter()
                .any(|k| options.contains_key(k))
                {
                    return Err("Параметры пофайловых фактов требуют stream-профиль".into());
                }
                reader.verify(limits)?;
            }
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

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn stream_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{streaming_facts, streaming_writer, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-stream REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = streaming_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":streaming_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn inline_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{inline_writer, streaming_facts, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-inline REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = inline_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":inline_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

fn companion_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{companion_writer, streaming_facts, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-companions REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = companion_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":companion_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

fn common_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{common_writer, streaming_facts, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-common-modules REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = common_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":common_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

fn root_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{root_writer, streaming_facts, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-roots REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = root_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":root_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

fn shared_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{shared_writer, streaming_facts, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-shared REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = shared_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":shared_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

fn leaf_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{leaf_writer, streaming_facts, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-leaves REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = leaf_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":leaf_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

fn journal_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{journal_writer, streaming_facts, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-journals REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = journal_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":journal_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

fn service_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{service_writer, streaming_facts, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-services REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = service_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":service_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

fn chart_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{chart_writer, streaming_facts, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-charts REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = chart_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":chart_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

fn register_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{register_writer, streaming_facts, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-registers REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = register_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":register_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

fn report_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{report_writer, streaming_facts, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-reports REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = report_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":report_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

fn workflow_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{streaming_facts, workflow_writer, Limits},
    };
    if args.len() != 19 {
        return Err("Использование: bslens-v5 build-workflows REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS".into());
    }
    let result = workflow_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
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
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":workflow_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

fn corpus_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{corpus_writer, streaming_facts, Limits},
    };
    if args.len() != 20 {
        return Err("Использование: bslens-v5 build-corpus REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS GLOBAL_OPAQUE_JSON".into());
    }
    let opaque = corpus_writer::parse_global_opaque(&args[19])?;
    let result = corpus_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
        },
        ingest::Limits {
            entries: args[10].parse()?,
            trees: args[11].parse()?,
            reads: args[12].parse()?,
            object_bytes: args[13].parse()?,
            total_bytes: args[14].parse()?,
            deadline: std::time::Duration::from_millis(args[15].parse()?),
        },
        &opaque,
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":corpus_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn corpus_bin_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{corpus_bin_writer, corpus_writer, streaming_facts, Limits},
    };
    if args.len() != 21 {
        return Err("Использование: bslens-v5 build-corpus-bin REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS GLOBAL_OPAQUE_JSON MAX_OPAQUE_BIN_BYTES".into());
    }
    let opaque = corpus_writer::parse_global_opaque(&args[19])?;
    let result = corpus_bin_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
        },
        ingest::Limits {
            entries: args[10].parse()?,
            trees: args[11].parse()?,
            reads: args[12].parse()?,
            object_bytes: args[13].parse()?,
            total_bytes: args[14].parse()?,
            deadline: std::time::Duration::from_millis(args[15].parse()?),
        },
        &opaque,
        args[20].parse()?,
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":corpus_bin_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn corpus_help_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{corpus_help_writer, corpus_writer, streaming_facts, Limits},
    };
    if args.len() != 21 {
        return Err("Использование: bslens-v5 build-corpus-help REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS GLOBAL_OPAQUE_JSON MAX_OPAQUE_BIN_BYTES".into());
    }
    let opaque = corpus_writer::parse_global_opaque(&args[19])?;
    let result = corpus_help_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
        },
        ingest::Limits {
            entries: args[10].parse()?,
            trees: args[11].parse()?,
            reads: args[12].parse()?,
            object_bytes: args[13].parse()?,
            total_bytes: args[14].parse()?,
            deadline: std::time::Duration::from_millis(args[15].parse()?),
        },
        &opaque,
        args[20].parse()?,
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":corpus_help_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn corpus_config_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{corpus_config_writer, corpus_writer, streaming_facts, Limits},
    };
    if args.len() != 21 {
        return Err("Использование: bslens-v5 build-corpus-config REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS GLOBAL_OPAQUE_JSON MAX_OPAQUE_BIN_BYTES".into());
    }
    let opaque = corpus_writer::parse_global_opaque(&args[19])?;
    let result = corpus_config_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
        },
        ingest::Limits {
            entries: args[10].parse()?,
            trees: args[11].parse()?,
            reads: args[12].parse()?,
            object_bytes: args[13].parse()?,
            total_bytes: args[14].parse()?,
            deadline: std::time::Duration::from_millis(args[15].parse()?),
        },
        &opaque,
        args[20].parse()?,
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":corpus_config_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn diagnostics_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{corpus_writer, diagnostics_writer, streaming_facts, Limits},
    };
    if args.len() != 22 {
        return Err("Использование: bslens-v5 build-diagnostics REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS GLOBAL_OPAQUE_JSON MAX_OPAQUE_BIN_BYTES MAX_DIAGNOSTICS_BYTES".into());
    }
    let opaque = corpus_writer::parse_global_opaque(&args[19])?;
    let result = diagnostics_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
        },
        ingest::Limits {
            entries: args[10].parse()?,
            trees: args[11].parse()?,
            reads: args[12].parse()?,
            object_bytes: args[13].parse()?,
            total_bytes: args[14].parse()?,
            deadline: std::time::Duration::from_millis(args[15].parse()?),
        },
        &opaque,
        args[20].parse()?,
        args[21].parse()?,
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":diagnostics_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn optional_form_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{corpus_writer, optional_form_writer, streaming_facts, Limits},
    };
    if args.len() != 22 {
        return Err("Использование: bslens-v5 build-optional-form REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS GLOBAL_OPAQUE_JSON MAX_OPAQUE_BIN_BYTES MAX_DIAGNOSTICS_BYTES".into());
    }
    let opaque = corpus_writer::parse_global_opaque(&args[19])?;
    let result = optional_form_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
        },
        ingest::Limits {
            entries: args[10].parse()?,
            trees: args[11].parse()?,
            reads: args[12].parse()?,
            object_bytes: args[13].parse()?,
            total_bytes: args[14].parse()?,
            deadline: std::time::Duration::from_millis(args[15].parse()?),
        },
        &opaque,
        args[20].parse()?,
        args[21].parse()?,
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":optional_form_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn sidecar_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{corpus_writer, sidecar_writer, streaming_facts, Limits},
    };
    if args.len() != 22 {
        return Err("Использование: bslens-v5 build-root-sidecars REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS GLOBAL_OPAQUE_JSON MAX_OPAQUE_BIN_BYTES MAX_DIAGNOSTICS_BYTES".into());
    }
    let opaque = corpus_writer::parse_global_opaque(&args[19])?;
    let result = sidecar_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
        },
        ingest::Limits {
            entries: args[10].parse()?,
            trees: args[11].parse()?,
            reads: args[12].parse()?,
            object_bytes: args[13].parse()?,
            total_bytes: args[14].parse()?,
            deadline: std::time::Duration::from_millis(args[15].parse()?),
        },
        &opaque,
        args[20].parse()?,
        args[21].parse()?,
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":sidecar_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn context_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{context_writer, corpus_writer, streaming_facts, Limits},
    };
    if args.len() != 22 {
        return Err("Использование: bslens-v5 build-context REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS GLOBAL_OPAQUE_JSON MAX_OPAQUE_BIN_BYTES MAX_DIAGNOSTICS_BYTES".into());
    }
    let opaque = corpus_writer::parse_global_opaque(&args[19])?;
    let result = context_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
        },
        ingest::Limits {
            entries: args[10].parse()?,
            trees: args[11].parse()?,
            reads: args[12].parse()?,
            object_bytes: args[13].parse()?,
            total_bytes: args[14].parse()?,
            deadline: std::time::Duration::from_millis(args[15].parse()?),
        },
        &opaque,
        args[20].parse()?,
        args[21].parse()?,
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":context_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn parameter_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{corpus_writer, parameter_writer, streaming_facts, Limits},
    };
    if args.len() != 22 {
        return Err("Использование: bslens-v5 build-parameters REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS GLOBAL_OPAQUE_JSON MAX_OPAQUE_BIN_BYTES MAX_DIAGNOSTICS_BYTES".into());
    }
    let opaque = corpus_writer::parse_global_opaque(&args[19])?;
    let result = parameter_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
        },
        ingest::Limits {
            entries: args[10].parse()?,
            trees: args[11].parse()?,
            reads: args[12].parse()?,
            object_bytes: args[13].parse()?,
            total_bytes: args[14].parse()?,
            deadline: std::time::Duration::from_millis(args[15].parse()?),
        },
        &opaque,
        args[20].parse()?,
        args[21].parse()?,
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":parameter_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn conditional_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{conditional_writer, corpus_writer, streaming_facts, Limits},
    };
    if args.len() != 22 {
        return Err("Использование: bslens-v5 build-conditionals REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS GLOBAL_OPAQUE_JSON MAX_OPAQUE_BIN_BYTES MAX_DIAGNOSTICS_BYTES".into());
    }
    let opaque = corpus_writer::parse_global_opaque(&args[19])?;
    let result = conditional_writer::build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
        },
        ingest::Limits {
            entries: args[10].parse()?,
            trees: args[11].parse()?,
            reads: args[12].parse()?,
            object_bytes: args[13].parse()?,
            total_bytes: args[14].parse()?,
            deadline: std::time::Duration::from_millis(args[15].parse()?),
        },
        &opaque,
        args[20].parse()?,
        args[21].parse()?,
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":conditional_writer::PROFILE,"experimental":true})
    );
    Ok(())
}

#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn string_command(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    use bslens_core::{
        git_manifest::ingest,
        snapshot_v5::{
            annotated_vars_writer, async_casing_writer, corpus_writer, dates_members_writer,
            header_forms_writer, lexical_tail_writer, literal_writer, long_headers_writer,
            method_regions_writer, mobile_vars_writer, navigator_agreement_writer,
            navigator_owners_writer, refined_writer, source_anomalies_writer, source_limits_writer,
            streaming_facts, string_writer, wrapped_writer, Limits,
        },
    };
    if args.len() != 22 {
        return Err("Использование: bslens-v5 {build-strings|build-literals|build-wrapped-headers|build-corpus-refinements|build-annotated-vars|build-dates-members|build-async-casing|build-mobile-vars|build-method-regions|build-header-forms|build-source-limits|build-long-headers|build-lexical-tail|build-source-anomalies|build-navigator-agreement|build-navigator-owners} REPO FULL_COMMIT EXPECTED_TREE SOURCE_ID OUTPUT_DIR MAX_SOURCES MAX_SOURCE_BYTES MAX_TOTAL_BYTES MAX_SQLITE_BYTES GIT_ENTRIES GIT_TREES GIT_READS GIT_OBJECT_BYTES GIT_TOTAL_BYTES GIT_DEADLINE_MS MAX_FACTS MAX_COVERAGE_BYTES FACTS_TIME_MS GLOBAL_OPAQUE_JSON MAX_OPAQUE_BIN_BYTES MAX_DIAGNOSTICS_BYTES".into());
    }
    let opaque = corpus_writer::parse_global_opaque(&args[19])?;
    let build = match args[0].as_str() {
        "build-annotated-vars" => annotated_vars_writer::build,
        "build-dates-members" => dates_members_writer::build,
        "build-async-casing" => async_casing_writer::build,
        "build-mobile-vars" => mobile_vars_writer::build,
        "build-method-regions" => method_regions_writer::build,
        "build-header-forms" => header_forms_writer::build,
        "build-source-limits" => source_limits_writer::build,
        "build-long-headers" => long_headers_writer::build,
        "build-lexical-tail" => lexical_tail_writer::build,
        "build-source-anomalies" => source_anomalies_writer::build,
        "build-navigator-agreement" => navigator_agreement_writer::build,
        "build-navigator-owners" => navigator_owners_writer::build,
        "build-corpus-refinements" => refined_writer::build,
        "build-wrapped-headers" => wrapped_writer::build,
        "build-literals" => literal_writer::build,
        _ => string_writer::build,
    };
    let profile = match args[0].as_str() {
        "build-annotated-vars" => annotated_vars_writer::PROFILE,
        "build-dates-members" => dates_members_writer::PROFILE,
        "build-async-casing" => async_casing_writer::PROFILE,
        "build-mobile-vars" => mobile_vars_writer::PROFILE,
        "build-method-regions" => method_regions_writer::PROFILE,
        "build-header-forms" => header_forms_writer::PROFILE,
        "build-source-limits" => source_limits_writer::PROFILE,
        "build-long-headers" => long_headers_writer::PROFILE,
        "build-lexical-tail" => lexical_tail_writer::PROFILE,
        "build-source-anomalies" => source_anomalies_writer::PROFILE,
        "build-navigator-agreement" => navigator_agreement_writer::PROFILE,
        "build-navigator-owners" => navigator_owners_writer::PROFILE,
        "build-corpus-refinements" => refined_writer::PROFILE,
        "build-wrapped-headers" => wrapped_writer::PROFILE,
        "build-literals" => literal_writer::PROFILE,
        _ => string_writer::PROFILE,
    };
    let result = build(
        std::path::Path::new(&args[1]),
        &args[2],
        &args[3],
        &args[4],
        std::path::Path::new(&args[5]),
        streaming_facts::Limits {
            sources: Limits {
                source_count: args[6].parse()?,
                source_bytes: args[7].parse()?,
                total_source_bytes: args[8].parse()?,
                sqlite_bytes: args[9].parse()?,
            },
            facts: args[16].parse()?,
            coverage_bytes: args[17].parse()?,
            time: std::time::Duration::from_millis(args[18].parse()?),
        },
        ingest::Limits {
            entries: args[10].parse()?,
            trees: args[11].parse()?,
            reads: args[12].parse()?,
            object_bytes: args[13].parse()?,
            total_bytes: args[14].parse()?,
            deadline: std::time::Duration::from_millis(args[15].parse()?),
        },
        &opaque,
        args[20].parse()?,
        args[21].parse()?,
    )?;
    println!(
        "{}",
        serde_json::json!({"path":result.path,"snapshot_id":result.snapshot_id,"facts_sha256":result.facts_sha256,"file_sha256":result.file_sha256,"reused":result.reused,"profile":profile,"experimental":true})
    );
    Ok(())
}
