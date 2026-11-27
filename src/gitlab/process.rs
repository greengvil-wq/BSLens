//! Read-only Git access with bounded output and an isolated environment.
use std::ffi::OsStr;
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GitError {
    UnsupportedPlatform,
    LimitExceeded,
    Failure,
    MissingObject,
}
pub type GitResult<T> = Result<T, GitError>;

/// Raw zero-delimited Git output is kept as bytes until explicitly validated.
pub trait GitReader {
    fn object_format(&self) -> GitResult<usize>;
    fn object_type(&self, oid: &str) -> GitResult<String>;
    fn is_shallow(&self) -> GitResult<bool>;
    fn merge_bases(&self, a: &str, b: &str) -> GitResult<Vec<String>>;
    fn rev_list(&self, a: &str, b: &str) -> GitResult<Vec<String>>;
    fn diff(&self, a: &str, b: &str) -> GitResult<Vec<u8>>;
    fn tree(&self, tree: &str, root: &str) -> GitResult<Vec<u8>>;
    fn blob_oid(&self, tree: &str, path: &str) -> GitResult<Option<String>>;
    fn blob(&self, oid: &str) -> GitResult<Vec<u8>>;
}

pub struct ProcessGit {
    git_dir: PathBuf,
    max_output_bytes: usize,
}
impl ProcessGit {
    pub fn new(git_dir: impl Into<PathBuf>, max_output_bytes: usize) -> Self {
        Self {
            git_dir: git_dir.into(),
            max_output_bytes,
        }
    }
    fn run<I, S>(&self, args: I) -> GitResult<Vec<u8>>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        if !cfg!(target_os = "linux") {
            return Err(GitError::UnsupportedPlatform);
        }
        let mut child = Command::new("/usr/bin/git")
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .arg("--no-pager")
            .arg("--no-replace-objects")
            .arg("--git-dir")
            .arg(&self.git_dir)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|_| GitError::Failure)?;
        let mut output = Vec::new();
        let result = child
            .stdout
            .take()
            .ok_or(GitError::Failure)?
            .take(self.max_output_bytes as u64 + 1)
            .read_to_end(&mut output);
        if result.is_err() || output.len() > self.max_output_bytes {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if result.is_err() {
                GitError::Failure
            } else {
                GitError::LimitExceeded
            });
        }
        if !child.wait().map_err(|_| GitError::Failure)?.success() {
            return Err(GitError::Failure);
        }
        Ok(output)
    }
    fn lines(&self, args: &[&str]) -> GitResult<Vec<String>> {
        let out = self.run(args)?;
        let s = std::str::from_utf8(&out).map_err(|_| GitError::Failure)?;
        Ok(s.lines().map(str::to_owned).collect())
    }
}
impl GitReader for ProcessGit {
    fn object_format(&self) -> GitResult<usize> {
        match self
            .lines(&["rev-parse", "--show-object-format"])?
            .as_slice()
        {
            [x] if x == "sha1" => Ok(40),
            [x] if x == "sha256" => Ok(64),
            _ => Err(GitError::Failure),
        }
    }
    fn object_type(&self, oid: &str) -> GitResult<String> {
        // A full OID cannot be interpreted as an option.
        let args = ["cat-file", "-t", oid];
        match self.lines(&args) {
            Ok(v) if v.len() == 1 => Ok(v[0].clone()),
            Err(GitError::Failure) => Err(GitError::MissingObject),
            Err(e) => Err(e),
            _ => Err(GitError::Failure),
        }
    }
    fn is_shallow(&self) -> GitResult<bool> {
        match self
            .lines(&["rev-parse", "--is-shallow-repository"])?
            .as_slice()
        {
            [x] if x == "true" => Ok(true),
            [x] if x == "false" => Ok(false),
            _ => Err(GitError::Failure),
        }
    }
    fn merge_bases(&self, a: &str, b: &str) -> GitResult<Vec<String>> {
        self.lines(&["merge-base", "--all", a, b])
    }
    fn rev_list(&self, a: &str, b: &str) -> GitResult<Vec<String>> {
        self.lines(&["rev-list", &format!("{a}..{b}")])
    }
    fn diff(&self, a: &str, b: &str) -> GitResult<Vec<u8>> {
        self.run(["diff", "--name-status", "-z", "--no-renames", a, b])
    }
    fn tree(&self, tree: &str, root: &str) -> GitResult<Vec<u8>> {
        self.run(["ls-tree", "-r", "-z", tree, "--", root])
    }
    fn blob_oid(&self, tree: &str, path: &str) -> GitResult<Option<String>> {
        let spec = format!("{tree}:{path}");
        let out = self.lines(&["rev-parse", "--verify", &spec]);
        match out {
            Ok(v) if v.len() == 1 => Ok(Some(v[0].clone())),
            Err(GitError::Failure) => Ok(None),
            Err(e) => Err(e),
            _ => Err(GitError::Failure),
        }
    }
    fn blob(&self, oid: &str) -> GitResult<Vec<u8>> {
        self.run(["cat-file", "blob", oid])
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    #[test]
    fn runner_enforces_limit_and_exit_status() {
        let reader = ProcessGit::new("/nonexistent", 4);
        assert_eq!(reader.run(["--version"]), Err(GitError::LimitExceeded));
        let reader = ProcessGit::new("/nonexistent", 1024);
        assert_eq!(reader.run(["not-a-command"]), Err(GitError::Failure));
    }
}
