//! Trusted platform backend. Callers cannot supply an executable, environment or Git options.
use super::{err, io, Code, Result};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
pub(super) struct Shell {
    path: PathBuf,
    objects: PathBuf,
}
impl Shell {
    pub(super) fn new(objects: &Path, oid_len: usize) -> Result<Self> {
        if !cfg!(target_os = "linux") {
            return Err(err(Code::UnsupportedPlatform));
        }
        let mut path = None;
        for _ in 0..100 {
            let p = std::env::temp_dir().join(format!(
                "bslens-git-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::SeqCst)
            ));
            let mut builder = fs::DirBuilder::new();
            #[cfg(unix)]
            {
                use std::os::unix::fs::DirBuilderExt;
                builder.mode(0o700);
            }
            match builder.create(&p) {
                Ok(()) => {
                    path = Some(p);
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(_) => return Err(err(Code::Layout)),
            }
        }
        let shell = Self {
            path: path.ok_or_else(|| err(Code::Layout))?,
            objects: objects.into(),
        };
        io(fs::create_dir(shell.path.join("objects")))?;
        io(fs::create_dir(shell.path.join("refs")))?;
        io(fs::write(
            shell.path.join("HEAD"),
            b"ref: refs/heads/unused\n",
        ))?;
        let config = if oid_len == 64 {
            "[core]\nrepositoryformatversion=1\nbare=true\n[extensions]\nobjectformat=sha256\n"
        } else {
            "[core]\nrepositoryformatversion=0\nbare=true\n"
        };
        io(fs::write(shell.path.join("config"), config))?;
        Ok(shell)
    }
    fn command(&self) -> Result<Command> {
        // Linux-only resolver for R03. Future Windows backend needs separately reviewed trust rules.
        if !cfg!(target_os = "linux") {
            return Err(err(Code::UnsupportedPlatform));
        }
        let mut command = Command::new("/usr/bin/git");
        command
            .current_dir(&self.path)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OBJECT_DIRECTORY", &self.objects)
            .arg("--no-pager")
            .arg("--no-replace-objects")
            .arg("--git-dir")
            .arg(&self.path)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        Ok(command)
    }
    pub(super) fn read(&self, oid: &str, cap: usize, deadline: Instant) -> Result<Vec<u8>> {
        let command = self.command()?;
        run(command, format!("{oid}\n").as_bytes(), cap + 128, deadline)
    }
}
impl Drop for Shell {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
fn run(mut command: Command, input: &[u8], cap: usize, deadline: Instant) -> Result<Vec<u8>> {
    if Instant::now() >= deadline {
        return Err(err(Code::Timeout));
    }
    let mut child = command.spawn().map_err(|_| err(Code::Process))?;
    let stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let result = (|| {
        let mut stdin = stdin.ok_or_else(|| err(Code::Process))?;
        stdin.write_all(input).map_err(|_| err(Code::Process))?;
        drop(stdin);
        let stdout = stdout.ok_or_else(|| err(Code::Process))?;
        let (send, recv) = mpsc::channel();
        let worker = thread::spawn(move || {
            let mut raw = vec![];
            let outcome = stdout
                .take(cap as u64 + 1)
                .read_to_end(&mut raw)
                .map(|_| raw);
            let _ = send.send(outcome);
        });
        let output = loop {
            if Instant::now() >= deadline {
                break Err(err(Code::Timeout));
            }
            match recv.recv_timeout(Duration::from_millis(2)) {
                Ok(Ok(raw)) => {
                    break if raw.len() > cap {
                        Err(err(Code::Limit))
                    } else {
                        Ok(raw)
                    }
                }
                Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break Err(err(Code::Process))
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        };
        if output.is_err() {
            let _ = child.kill();
        }
        // Even after EOF, wait is bounded: a child may close stdout before exiting.
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break Ok(s),
                Err(_) => break Err(err(Code::Process)),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        break Err(err(Code::Timeout));
                    }
                    thread::sleep(Duration::from_millis(2));
                }
            }
        };
        let _ = child.wait();
        let _ = worker.join();
        let raw = output?;
        if !status?.success() {
            return Err(err(Code::Process));
        }
        Ok(raw)
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_shell_removed_after_success_or_error() {
        let shell = Shell::new(Path::new("/tmp"), 40).unwrap();
        let path = shell.path.clone();
        assert!(path.exists());
        assert!(shell.read(&"0".repeat(40), 100, Instant::now()).is_err());
        drop(shell);
        assert!(!path.exists());
    }
    #[test]
    fn process_timeout_and_output_limit() {
        // Internal runner test only: executable/arguments are never public input.
        let mut sleepy = Command::new("/bin/sleep");
        sleepy
            .arg("2")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let start = Instant::now();
        assert_eq!(
            run(sleepy, b"", 100, start + Duration::from_millis(30))
                .unwrap_err()
                .code,
            Code::Timeout
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        let mut noisy = Command::new("/usr/bin/yes");
        noisy
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        assert_eq!(
            run(noisy, b"", 100, Instant::now() + Duration::from_secs(1))
                .unwrap_err()
                .code,
            Code::Limit
        );
    }
}
