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
    let mut args = std::env::args_os().skip(1);
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
