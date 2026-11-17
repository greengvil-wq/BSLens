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
        } else {
            return Err("Expected --repo ABSOLUTE_PATH --commit FULL_SHA");
        }
    }
    let repo = repo.ok_or("Expected --repo ABSOLUTE_PATH --commit FULL_SHA")?;
    let commit = commit.ok_or("Expected --repo ABSOLUTE_PATH --commit FULL_SHA")?;
    if !repo.is_absolute() {
        return Err("Expected --repo ABSOLUTE_PATH --commit FULL_SHA");
    }
    // Core enforces exact SHA, bounded trusted Git access and platform constraints.
    // Do not echo caller paths, source bytes or detailed Git errors on failure.
    let core = GitStructure::read(&repo, &commit, Limits::default())
        .map_err(|_| "Unable to open bounded exact Git context")?;
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
