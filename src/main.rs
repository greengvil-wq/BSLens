use std::path::Path;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
fn run() -> bslens_core::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(feature = "synthetic-dev")]
    if args.first().map(String::as_str) == Some("synthetic-dev") {
        if args.len() < 4 {
            return Err(bslens_core::Error(
                "synthetic-dev <file> <snapshot-id> metadata | source <address> <start> <end>"
                    .into(),
            ));
        }
        let snapshot =
            bslens_core::synthetic::SyntheticSnapshot::open(Path::new(&args[1]), &args[2])?;
        let data = match (args[3].as_str(), args.len()) {
            ("metadata", 4) => snapshot.metadata(),
            ("source", 7) => snapshot.source(
                &args[2],
                &args[4],
                args[5]
                    .parse()
                    .map_err(|_| bslens_core::Error("invalid start".into()))?,
                args[6]
                    .parse()
                    .map_err(|_| bslens_core::Error("invalid end".into()))?,
            )?,
            _ => return Err(bslens_core::Error("unknown command or arguments".into())),
        };
        println!(
            "{}",
            serde_json::json!({"validation":"synthetic-partial-NOT-PRODUCTION", "unvalidated":["XML adapter and coverage", "BSL facts and scope", "lexical navigation and scope", "full SQLite schema definition"], "data":data})
        );
        return Ok(());
    }
    if args.len() != 3 || args[0] != "metadata" {
        return Err(bslens_core::Error(
            "Production reader unavailable in R01. Usage: metadata <file> <snapshot-id>".into(),
        ));
    }
    bslens_core::ValidatedSnapshot::open(Path::new(&args[1]), &args[2])?;
    Ok(())
}
