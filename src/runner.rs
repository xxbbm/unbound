//! 后台运行器：两个读取线程同时排空管道，避免 stdout/stderr 相互阻塞。
use std::{
    ffi::OsString,
    io::Read,
    path::PathBuf,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

#[derive(Debug)]
pub enum Event {
    Started(u32),
    Output {
        stderr: bool,
        text: String,
    },
    Finished {
        code: Option<i32>,
        cancelled: bool,
        timed_out: bool,
        dropped: usize,
    },
    Error(String),
}
pub struct Run {
    pub events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
}
impl Run {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Drop for Run {
    fn drop(&mut self) {
        self.cancel();
    }
}

pub struct Request {
    pub program: PathBuf,
    pub args: Vec<OsString>,
    pub timeout: Duration,
}

fn reader(
    mut pipe: impl Read + Send + 'static,
    stderr: bool,
    tx: SyncSender<Event>,
    dropped: Arc<AtomicUsize>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut bytes = [0; 4096];
        // 保留块末尾未完成的 UTF-8 字符，不把跨块中文误显示为乱码。
        let mut pending = Vec::new();
        loop {
            match pipe.read(&mut bytes) {
                Ok(0) => break,
                Ok(n) => {
                    pending.extend_from_slice(&bytes[..n]);
                    let cut = match std::str::from_utf8(&pending) {
                        Ok(_) => pending.len(),
                        Err(e) if e.error_len().is_none() => e.valid_up_to(),
                        Err(_) => pending.len(),
                    };
                    if cut > 0 {
                        let text = String::from_utf8_lossy(&pending[..cut]).into_owned();
                        pending.drain(..cut);
                        match tx.try_send(Event::Output { stderr, text }) {
                            Ok(()) => (),
                            Err(mpsc::TrySendError::Full(_)) => {
                                dropped.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(mpsc::TrySendError::Disconnected(_)) => return,
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    let _ = tx.try_send(Event::Output {
                        stderr: true,
                        text: format!("\n[读取管道失败: {e}]\n"),
                    });
                    break;
                }
            }
        }
        if !pending.is_empty() {
            let _ = tx.try_send(Event::Output {
                stderr,
                text: String::from_utf8_lossy(&pending).into_owned(),
            });
        }
    })
}

pub fn start(request: Request) -> Run {
    start_with_options(request, Options::default())
}

pub struct Options {
    pub cwd: Option<PathBuf>,
    pub env: Vec<(OsString, OsString)>,
    pub new_process_group: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            cwd: None,
            env: vec![],
            new_process_group: true,
        }
    }
}

pub fn start_with_options(request: Request, options: Options) -> Run {
    let (tx, events) = mpsc::sync_channel(256);
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    thread::spawn(move || {
        let result = execute(request, flag, &tx, options);
        if let Err(e) = result {
            let _ = tx.send(Event::Error(e));
        }
    });
    Run { events, cancel }
}

fn execute(
    request: Request,
    cancel: Arc<AtomicBool>,
    tx: &SyncSender<Event>,
    options: Options,
) -> Result<(), String> {
    let program = request
        .program
        .canonicalize()
        .map_err(|e| format!("找不到可执行文件: {e}"))?;
    if !program.is_file() {
        return Err("可执行文件路径不是文件".into());
    }
    #[cfg(target_os = "macos")]
    if program
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("exe"))
    {
        return Err("macOS 需要原生 Mach-O 可执行文件，不能直接运行 Windows .exe".into());
    }
    let mut command = Command::new(&program);
    command
        .args(&request.args)
        .current_dir(
            options
                .cwd
                .as_deref()
                .unwrap_or(program.parent().ok_or("程序没有父目录")?),
        )
        .envs(options.env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        if options.new_process_group {
            command.process_group(0);
        }
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("无法启动 {}: {e}", program.display()))?;
    let pid = child.id();
    let _ = tx.send(Event::Started(pid));
    let dropped = Arc::new(AtomicUsize::new(0));
    let out = reader(
        child.stdout.take().ok_or("stdout 不可用")?,
        false,
        tx.clone(),
        dropped.clone(),
    );
    let err = reader(
        child.stderr.take().ok_or("stderr 不可用")?,
        true,
        tx.clone(),
        dropped.clone(),
    );
    let began = Instant::now();
    let mut cancelled = false;
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            break status;
        }
        cancelled = cancel.load(Ordering::Relaxed);
        timed_out = began.elapsed() >= request.timeout;
        if cancelled || timed_out {
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                // CLI 子进程树取消；失败时仍尝试结束直接子进程。
                let system_root =
                    std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
                let _ = Command::new(PathBuf::from(system_root).join("System32/taskkill.exe"))
                    .args(["/PID", &pid.to_string(), "/T", "/F"])
                    .creation_flags(0x08000000)
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
            }
            #[cfg(unix)]
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
            let _ = child.kill();
            break child.wait().map_err(|e| e.to_string())?;
        }
        thread::sleep(Duration::from_millis(40));
    };
    // 正常退出后给管道读取线程时间排空。不让继承管道的后台孙进程无限阻塞状态更新。
    let drain = Instant::now();
    while (!out.is_finished() || !err.is_finished()) && drain.elapsed() < Duration::from_secs(1) {
        thread::sleep(Duration::from_millis(10));
    }
    if out.is_finished() {
        let _ = out.join();
    }
    if err.is_finished() {
        let _ = err.join();
    }
    let _ = tx.send(Event::Finished {
        code: status.code(),
        cancelled,
        timed_out,
        dropped: dropped.load(Ordering::Relaxed),
    });
    Ok(())
}
