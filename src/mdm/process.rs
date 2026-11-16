//! Bounded read-only CLI queries, inherited into the GUI's cancellable worker.
use crate::runner::{self, Event, Options, Request};
use std::{ffi::OsString, path::Path, time::Duration};
pub struct Output {
    pub stdout: String,
}
pub(super) fn command(
    program: &Path,
    args: &[OsString],
    cwd: Option<&Path>,
    seconds: u64,
) -> Result<Output, String> {
    let run = runner::start_with_options(
        Request {
            program: program.into(),
            args: args.to_vec(),
            timeout: Duration::from_secs(seconds),
        },
        Options {
            cwd: cwd.map(Path::to_path_buf),
            new_process_group: false,
            ..Default::default()
        },
    );
    let mut stdout = String::new();
    let mut stderr = String::new();
    loop {
        match run.events.recv().map_err(|e| e.to_string())? {
            Event::Output {
                stderr: is_err,
                text,
            } => {
                let out = if is_err { &mut stderr } else { &mut stdout };
                if out.len() + text.len() > 4 * 1024 * 1024 {
                    run.cancel();
                    return Err("设备输出超过大小限制".into());
                }
                out.push_str(&text);
            }
            Event::Error(e) => return Err(e),
            Event::Finished {
                code,
                cancelled,
                timed_out,
                dropped,
            } => {
                if code != Some(0) || cancelled || timed_out || dropped > 0 {
                    return Err(format!(
                        "设备查询未完成（退出码 {code:?}，超时 {timed_out}）：{}",
                        stderr.chars().take(1500).collect::<String>()
                    ));
                }
                return Ok(Output { stdout });
            }
            Event::Started(_) => (),
        }
    }
}
