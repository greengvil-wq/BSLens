//! Fixed local read-only MCP stdio endpoint. No dynamic repository switching.
use bslens_core::{
    git_manifest::Limits, mcp_router::CoreSession, mcp_transport::Exit, pipeline::GitStructure,
};
use std::{ffi::OsString, io, path::PathBuf, process::ExitCode};
fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}
fn run() -> Result<(), &'static str> {
    let all: Vec<_> = std::env::args_os().skip(1).collect();
    if all.iter().any(|v| {
        [
            "--snapshot",
            "--staging-dir",
            "--max-sqlite-bytes",
            "--expected-sha256",
        ]
        .iter()
        .any(|flag| v == flag)
    }) {
        return snapshot_run(&all);
    }
    let mut args = all.into_iter();
    let mut repo: Option<PathBuf> = None;
    let mut commit: Option<String> = None;
    let mut cache_dir: Option<PathBuf> = None;
    let mut rebuild = false;
    while let Some(arg) = args.next() {
        if arg == OsString::from("--repo") && repo.is_none() {
            repo = Some(
                args.next()
                    .ok_or("Expected --repo ABSOLUTE_PATH --commit FULL_SHA")?
                    .into(),
            );
        } else if arg == OsString::from("--commit") && commit.is_none() {
            commit = Some(
                args.next()
                    .and_then(|v| v.into_string().ok())
                    .ok_or("Expected --repo ABSOLUTE_PATH --commit FULL_SHA")?,
            );
        } else if cfg!(feature = "experimental-cache")
            && arg == OsString::from("--cache-dir")
            && cache_dir.is_none()
        {
            cache_dir = Some(
                args.next()
                    .ok_or("Expected --cache-dir ABSOLUTE_PATH")?
                    .into(),
            );
        } else if cfg!(feature = "experimental-cache")
            && arg == OsString::from("--rebuild-cache")
            && !rebuild
        {
            rebuild = true;
        } else {
            return Err("Expected --repo ABSOLUTE_PATH --commit FULL_SHA");
        }
    }
    let repo = repo.ok_or("Expected --repo ABSOLUTE_PATH --commit FULL_SHA")?;
    let commit = commit.ok_or("Expected --repo ABSOLUTE_PATH --commit FULL_SHA")?;
    if !repo.is_absolute() {
        return Err("Expected --repo ABSOLUTE_PATH --commit FULL_SHA");
    }
    if rebuild && cache_dir.is_none() {
        return Err("--rebuild-cache requires --cache-dir");
    }
    if cache_dir.as_ref().is_some_and(|p| !p.is_absolute()) {
        return Err("Expected --cache-dir ABSOLUTE_PATH");
    }
    // Core enforces exact SHA, bounded trusted Git access and platform constraints.
    // Do not echo caller paths, source bytes or detailed Git errors on failure.
    let core = if let Some(dir) = cache_dir {
        #[cfg(feature = "experimental-cache")]
        {
            use bslens_core::cache;
            if rebuild {
                cache::build(&repo, &commit, &dir).map_err(|_| "Cache rebuild failed")?
            } else {
                match cache::open(&repo, &commit, &dir) {
                    Ok(core) => core,
                    Err(cache::Error::Missing) => {
                        cache::build(&repo, &commit, &dir).map_err(|_| "Cache build failed")?
                    }
                    Err(_) => {
                        return Err(
                            "Cache rejected; inspect cache or explicitly use --rebuild-cache",
                        )
                    }
                }
            }
        }
        #[cfg(not(feature = "experimental-cache"))]
        {
            let _ = dir;
            return Err("Cache feature unavailable");
        }
    } else {
        GitStructure::read(&repo, &commit, Limits::default())
            .map_err(|_| "Unable to open bounded exact Git context")?
    };
    let mut session = CoreSession::new(core);
    let input = io::stdin();
    let output = io::stdout();
    match session
        .serve(&mut input.lock(), &mut output.lock())
        .map_err(|_| "MCP stdio I/O failed")?
    {
        Exit::Eof | Exit::SessionLimit => Ok(()),
        Exit::FrameRejected => Err("MCP frame rejected; session closed"),
    }
}

#[cfg(all(
    feature = "experimental-snapshot-mcp",
    target_os = "linux",
    target_arch = "x86_64"
))]
fn snapshot_run(args: &[OsString]) -> Result<(), &'static str> {
    use bslens_core::{
        linux_v5_reader::LinuxSnapshotReader,
        snapshot_mcp::{startup_code, SnapshotSession},
    };
    let mut file = None;
    let mut staging = None;
    let mut maximum = None;
    let mut expected = None;
    for pair in args.chunks(2) {
        if pair.len() != 2 {
            return Err("snapshot_arguments");
        }
        if pair[0] == "--snapshot" && file.is_none() {
            file = Some(PathBuf::from(&pair[1]));
        } else if pair[0] == "--staging-dir" && staging.is_none() {
            staging = Some(PathBuf::from(&pair[1]));
        } else if pair[0] == "--max-sqlite-bytes" && maximum.is_none() {
            maximum = Some(
                pair[1]
                    .to_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .filter(|n| *n > 0)
                    .ok_or("snapshot_arguments")?,
            );
        } else if pair[0] == "--expected-sha256" && expected.is_none() {
            expected = Some(pair[1].to_str().ok_or("snapshot_arguments")?.to_owned());
        } else {
            return Err("snapshot_arguments");
        }
    }
    let file = file.ok_or("snapshot_arguments")?;
    let staging = staging.ok_or("snapshot_arguments")?;
    if !file.is_absolute() || !staging.is_absolute() {
        return Err("snapshot_arguments");
    }
    let reader = LinuxSnapshotReader::open(
        &file,
        &staging,
        expected.as_deref(),
        maximum.ok_or("snapshot_arguments")?,
    )
    .map_err(|e| startup_code(&e))?;
    let mut session = SnapshotSession::new(reader).map_err(|e| startup_code(&e))?;
    match session
        .serve(&mut io::stdin().lock(), &mut io::stdout().lock())
        .map_err(|_| "MCP stdio I/O failed")?
    {
        Exit::Eof | Exit::SessionLimit => Ok(()),
        Exit::FrameRejected => Err("MCP frame rejected; session closed"),
    }
}
#[cfg(not(all(
    feature = "experimental-snapshot-mcp",
    target_os = "linux",
    target_arch = "x86_64"
)))]
fn snapshot_run(_: &[OsString]) -> Result<(), &'static str> {
    Err("snapshot_mode_unavailable: requires experimental-snapshot-mcp on Linux x86_64")
}
