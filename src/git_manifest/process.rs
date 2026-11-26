//! Trusted platform backend. Callers cannot supply an executable, environment or Git options.
use super::{err, io, Code, Result};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
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
    batch: Option<Batch>,
    sizes: Option<Batch>,
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
            batch: None,
            sizes: None,
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
    fn command(&self, header_only: bool) -> Result<Command> {
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
            .args([
                "cat-file",
                if header_only {
                    "--batch-check"
                } else {
                    "--batch"
                },
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        Ok(command)
    }
    pub(super) fn read(
        &mut self,
        oid: &str,
        kind: &str,
        cap: usize,
        deadline: Instant,
    ) -> Result<Vec<u8>> {
        if self.batch.is_none() {
            self.batch = Some(Batch::spawn(self.command(false)?, deadline)?);
        }
        self.batch.as_mut().unwrap().read(oid, kind, cap, deadline)
    }
    pub(super) fn size(&mut self, oid: &str, kind: &str, deadline: Instant) -> Result<usize> {
        if self.sizes.is_none() {
            self.sizes = Some(Batch::spawn(self.command(true)?, deadline)?);
        }
        self.sizes
            .as_mut()
            .ok_or_else(|| err(Code::Process))?
            .size(oid, kind, deadline)
    }
    pub(super) fn finish(&mut self, deadline: Instant) -> Result<()> {
        if let Some(sizes) = &mut self.sizes {
            sizes.finish(deadline)?;
        }
        self.batch
            .as_mut()
            .ok_or_else(|| err(Code::Process))?
            .finish(deadline)
    }
}
impl Drop for Shell {
    fn drop(&mut self) {
        drop(self.batch.take());
        drop(self.sizes.take());
        let _ = fs::remove_dir_all(&self.path);
    }
}

// One worker owns both pipes: the coordinator can enforce an absolute deadline even
// if either write or read blocks. Only one bounded request/reply is outstanding.
enum Request {
    Object {
        oid: String,
        kind: String,
        cap: Option<usize>,
    },
    Finish,
}
struct Batch {
    child: Child,
    send: Option<mpsc::SyncSender<Request>>,
    recv: mpsc::Receiver<Result<Vec<u8>>>,
    worker: Option<thread::JoinHandle<()>>,
    closed: bool,
}
impl Batch {
    fn spawn(mut command: Command, deadline: Instant) -> Result<Self> {
        if Instant::now() >= deadline {
            return Err(err(Code::Timeout));
        }
        let mut child = command.spawn().map_err(|_| err(Code::Process))?;
        let pipes = child.stdin.take().zip(child.stdout.take());
        let Some((mut stdin, stdout)) = pipes else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(err(Code::Process));
        };
        let (send, requests) = mpsc::sync_channel(1);
        let (replies, recv) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("bslens-cat-file".into())
            .spawn(move || {
                let mut stdout = BufReader::new(stdout);
                while let Ok(request) = requests.recv() {
                    match request {
                        Request::Object { oid, kind, cap } => {
                            let result = (|| {
                                stdin
                                    .write_all(oid.as_bytes())
                                    .map_err(|_| err(Code::Process))?;
                                stdin.write_all(b"\n").map_err(|_| err(Code::Process))?;
                                stdin.flush().map_err(|_| err(Code::Process))?;
                                if let Some(cap) = cap {
                                    read_reply(&mut stdout, &oid, &kind, cap)
                                } else {
                                    // Не выделяем буфер по размеру blob: передаём только число, тело не запрашиваем.
                                    let (_, size) =
                                        read_header(&mut stdout, &oid, &kind, usize::MAX)?;
                                    Ok(size.to_be_bytes().to_vec())
                                }
                            })();
                            let failed = result.is_err();
                            if replies.send(result).is_err() || failed {
                                break;
                            }
                        }
                        Request::Finish => {
                            drop(stdin);
                            let mut byte = [0];
                            let result = match stdout.read(&mut byte) {
                                Ok(0) => Ok(vec![]),
                                Ok(_) => Err(err(Code::Object)),
                                Err(_) => Err(err(Code::Process)),
                            };
                            let _ = replies.send(result);
                            break;
                        }
                    }
                }
            });
        match worker {
            Ok(worker) => Ok(Self {
                child,
                send: Some(send),
                recv,
                worker: Some(worker),
                closed: false,
            }),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(err(Code::Process))
            }
        }
    }
    fn exchange(&mut self, request: Request, deadline: Instant) -> Result<Vec<u8>> {
        if self.closed {
            return Err(err(Code::Process));
        }
        let result = (|| {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| err(Code::Timeout))?;
            // try_send never blocks; a full channel is a broken internal invariant.
            self.send
                .as_ref()
                .ok_or_else(|| err(Code::Process))?
                .try_send(request)
                .map_err(|_| err(Code::Process))?;
            let raw = self.recv.recv_timeout(remaining).map_err(|e| {
                err(match e {
                    mpsc::RecvTimeoutError::Timeout => Code::Timeout,
                    mpsc::RecvTimeoutError::Disconnected => Code::Process,
                })
            })??;
            if Instant::now() >= deadline {
                return Err(err(Code::Timeout));
            }
            Ok(raw)
        })();
        if result.is_err() {
            self.abort();
        }
        result
    }
    fn read(&mut self, oid: &str, kind: &str, cap: usize, deadline: Instant) -> Result<Vec<u8>> {
        self.exchange(
            Request::Object {
                oid: oid.into(),
                kind: kind.into(),
                cap: Some(cap),
            },
            deadline,
        )
    }
    fn size(&mut self, oid: &str, kind: &str, deadline: Instant) -> Result<usize> {
        let bytes = self.exchange(
            Request::Object {
                oid: oid.into(),
                kind: kind.into(),
                cap: None,
            },
            deadline,
        )?;
        Ok(usize::from_be_bytes(
            bytes.try_into().map_err(|_| err(Code::Process))?,
        ))
    }
    fn finish(&mut self, deadline: Instant) -> Result<()> {
        self.exchange(Request::Finish, deadline)?;
        let result = loop {
            if Instant::now() >= deadline {
                break Err(err(Code::Timeout));
            }
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    break if status.success() {
                        Ok(())
                    } else {
                        Err(err(Code::Process))
                    }
                }
                Ok(None) => thread::sleep(Duration::from_millis(1)),
                Err(_) => break Err(err(Code::Process)),
            }
        };
        if result.is_err() {
            self.abort();
        } else {
            self.closed = true;
            self.send.take();
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
        result
    }
    fn abort(&mut self) {
        if self.closed {
            return;
        }
        self.closed = true;
        self.send.take();
        // The trusted Git process does not spawn pipe-holding helpers. Kill/reap
        // precedes joining, releasing a worker blocked in either pipe direction.
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
impl Drop for Batch {
    fn drop(&mut self) {
        self.abort();
    }
}

fn read_header<R: BufRead>(
    input: &mut R,
    oid: &str,
    kind: &str,
    cap: usize,
) -> Result<(Vec<u8>, usize)> {
    // Header must be complete within 128 bytes including LF, before allocation
    // based on its size. BufReader retains the rest of the stream for later calls.
    let mut header = Vec::with_capacity(128);
    loop {
        if header.len() == 128 {
            return Err(err(Code::Limit));
        }
        let mut byte = [0];
        input.read_exact(&mut byte).map_err(|_| err(Code::Object))?;
        header.push(byte[0]);
        if byte[0] == b'\n' {
            break;
        }
    }
    let text = std::str::from_utf8(&header[..header.len() - 1]).map_err(|_| err(Code::Object))?;
    let parts: Vec<_> = text.split(' ').collect();
    if parts.len() != 3 || parts[0] != oid || parts[1] != kind {
        return Err(err(Code::Object));
    }
    let size: usize = parts[2].parse().map_err(|_| err(Code::Object))?;
    if size > cap || parts[2] != size.to_string() {
        return Err(err(Code::Limit));
    }
    Ok((header, size))
}

fn read_reply<R: BufRead>(input: &mut R, oid: &str, kind: &str, cap: usize) -> Result<Vec<u8>> {
    let (mut header, size) = read_header(input, oid, kind, cap)?;
    let total = header
        .len()
        .checked_add(size)
        .and_then(|n| n.checked_add(1))
        .ok_or_else(|| err(Code::Limit))?;
    let offset = header.len();
    header.resize(total, 0);
    input
        .read_exact(&mut header[offset..])
        .map_err(|_| err(Code::Object))?;
    if header.last() != Some(&b'\n') {
        return Err(err(Code::Object));
    }
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn command(code: &str) -> Command {
        let mut c = Command::new("/usr/bin/python3");
        c.args(["-c", code])
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        c
    }
    fn deadline() -> Instant {
        Instant::now() + Duration::from_secs(2)
    }
    #[test]
    fn bounded_header_and_exact_body_contract() {
        let oid = "a".repeat(40);
        let good = format!("{oid} blob 3\nabc\n").into_bytes();
        let mut both = good.clone();
        both.extend(&good);
        let mut input = std::io::BufReader::new(Cursor::new(both));
        assert_eq!(read_reply(&mut input, &oid, "blob", 3).unwrap(), good);
        assert_eq!(read_reply(&mut input, &oid, "blob", 3).unwrap(), good);
        for raw in [
            format!("{oid} blob 03\nabc\n"),
            format!("{oid} blob 4\n"),
            format!("{oid} tree 3\nabc\n"),
            format!("{oid} blob 3\nabcX"),
            format!("{oid} blob 3\nab"),
            format!("{oid} missing\n"),
            format!("{oid} blob 3\r\nabc\n"),
            format!("{} blob 3\nabc\n", "b".repeat(40)),
        ] {
            assert!(read_reply(&mut Cursor::new(raw), &oid, "blob", 3).is_err());
        }
        let mut huge = Cursor::new(vec![b'x'; 10000]);
        assert_eq!(
            read_reply(&mut huge, &oid, "blob", 3).unwrap_err().code,
            Code::Limit
        );
        assert_eq!(huge.position(), 128);
        let raw = format!("{oid} blob 99999999999999999999999999999999999\n");
        assert!(read_reply(&mut Cursor::new(raw), &oid, "blob", 3).is_err());
    }
    #[test]
    fn two_requests_reuse_one_process_and_clean_finish() {
        let mut b=Batch::spawn(command("import sys\nfor line in sys.stdin:\n sys.stdout.write(line.strip()+' blob 3\\nabc\\n');sys.stdout.flush()"),deadline()).unwrap();
        let pid = b.child.id();
        for _ in 0..2 {
            assert!(b.read(&"a".repeat(40), "blob", 3, deadline()).is_ok());
            assert_eq!(b.child.id(), pid);
        }
        b.finish(deadline()).unwrap();
        assert!(b.child.try_wait().unwrap().unwrap().success());
        assert!(b.worker.is_none());
        assert!(b.read(&"a".repeat(40), "blob", 3, deadline()).is_err());
    }
    #[test]
    fn timeout_poison_reap_and_no_reuse() {
        let start = Instant::now();
        let mut b = Batch::spawn(command("import time; time.sleep(10)"), deadline()).unwrap();
        assert_eq!(
            b.read(
                &"a".repeat(40),
                "blob",
                3,
                start + Duration::from_millis(100)
            )
            .unwrap_err()
            .code,
            Code::Timeout
        );
        assert!(start.elapsed() < Duration::from_secs(2));
        assert!(b.child.try_wait().unwrap().is_some());
        assert!(b.worker.is_none());
        assert!(b.read(&"a".repeat(40), "blob", 3, deadline()).is_err());
    }
    #[test]
    fn malformed_reply_poison_reap() {
        let mut b=Batch::spawn(command("import sys,time;sys.stdin.readline();sys.stdout.write('bad\\n');sys.stdout.flush();time.sleep(10)"),deadline()).unwrap();
        assert_eq!(
            b.read(&"a".repeat(40), "blob", 3, deadline())
                .unwrap_err()
                .code,
            Code::Object
        );
        assert!(b.child.try_wait().unwrap().is_some());
        assert!(b.worker.is_none());
    }
    #[test]
    fn finish_rejects_extra_bytes_nonzero_exit_and_no_exit() {
        for (suffix, expected) in [
            ("sys.stdout.write('x');sys.stdout.flush()", Code::Object),
            ("sys.exit(7)", Code::Process),
            ("sys.stdout.close();time.sleep(10)", Code::Timeout),
        ] {
            let code=format!("import sys,time\nsys.stdin.readline()\nsys.stdout.write('a'*40+' blob 0\\n\\n');sys.stdout.flush()\nsys.stdin.read()\n{suffix}");
            let mut b = Batch::spawn(command(&code), deadline()).unwrap();
            b.read(&"a".repeat(40), "blob", 0, deadline()).unwrap();
            assert_eq!(
                b.finish(Instant::now() + Duration::from_millis(100))
                    .unwrap_err()
                    .code,
                expected
            );
            assert!(b.child.try_wait().unwrap().is_some());
            assert!(b.worker.is_none());
        }
    }
    #[test]
    fn drop_terminates_worker_waiting_for_next_request_and_private_shell() {
        let start = Instant::now();
        let mut b=Batch::spawn(command("import sys,time;sys.stdin.readline();sys.stdout.write('a'*40+' blob 0\\n\\n');sys.stdout.flush();time.sleep(10)"),deadline()).unwrap();
        b.read(&"a".repeat(40), "blob", 0, deadline()).unwrap();
        drop(b);
        assert!(start.elapsed() < Duration::from_secs(2));
        let shell = Shell::new(Path::new("/tmp"), 40).unwrap();
        let path = shell.path.clone();
        drop(shell);
        assert!(!path.exists());
    }
}
