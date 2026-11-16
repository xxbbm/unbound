//! USB presence checks are independent of foreground jobs. Do not read the full
//! lockdown record or disable the UI for an unchanged device.
use apple_toolkit::{
    mdm,
    runner::{self, Event, Run},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct Presence {
    missing: u8,
}
#[derive(Debug, PartialEq)]
pub enum Change {
    None,
    Read,
    Disconnected,
    Multiple,
}
impl Presence {
    fn observe(&mut self, ids: &[String], current: Option<&str>) -> Change {
        if ids.is_empty() {
            self.missing = self.missing.saturating_add(1);
            return if current.is_some() && self.missing >= 2 {
                Change::Disconnected
            } else {
                Change::None
            };
        }
        self.missing = 0;
        if ids.len() != 1 {
            return Change::Multiple;
        }
        if current.is_some_and(|id| id.eq_ignore_ascii_case(&ids[0])) {
            Change::None
        } else {
            Change::Read
        }
    }
}

pub struct Monitor {
    run: Option<Run>,
    output: String,
    last: Instant,
    presence: Presence,
}
impl Default for Monitor {
    fn default() -> Self {
        Self {
            run: None,
            output: String::new(),
            last: Instant::now() - Duration::from_secs(3),
            presence: Presence::default(),
        }
    }
}
impl Monitor {
    pub fn pause(&mut self) {
        // Dropping Run cancels the short-lived child. No device writes occur.
        self.run = None;
        self.output.clear();
        self.last = Instant::now();
    }
    pub fn tick(&mut self, root: &Path, current: Option<&str>) -> Change {
        if self.run.is_none() && self.last.elapsed() >= Duration::from_secs(3) {
            self.output.clear();
            self.run = Some(runner::start(runner::Request {
                program: root
                    .join("Tools/NormalMode")
                    .join(format!("idevice_id{}", std::env::consts::EXE_SUFFIX)),
                args: vec!["-l".into()],
                timeout: Duration::from_secs(5),
            }));
        }
        for _ in 0..32 {
            let event = match self.run.as_ref().map(|r| r.events.try_recv()) {
                Some(Ok(e)) => e,
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                    self.pause();
                    break;
                }
                _ => break,
            };
            match event {
                Event::Output {
                    stderr: false,
                    text,
                } => {
                    if self.output.len() + text.len() > 65536 {
                        self.pause();
                        break;
                    }
                    self.output.push_str(&text);
                }
                Event::Finished {
                    code,
                    cancelled,
                    timed_out,
                    dropped,
                } => {
                    let ids: Vec<String> = self
                        .output
                        .lines()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(String::from)
                        .collect();
                    self.pause();
                    if code == Some(0)
                        && !cancelled
                        && !timed_out
                        && dropped == 0
                        && ids.iter().all(|s| mdm::valid_udid(s))
                    {
                        return self.presence.observe(&ids, current);
                    }
                    // A failed USB query is not evidence that the phone unplugged.
                    self.presence.missing = 0;
                    break;
                }
                Event::Error(_) => {
                    self.pause();
                    self.presence.missing = 0;
                    break;
                }
                _ => (),
            }
        }
        Change::None
    }
}

/// 安卓侧轮询：每 3 秒跑一次 `adb devices -l`。
///
/// 与 iOS 的 [`Monitor`] **完全独立**：adb 没装、服务没起来、或设备在待授权，
/// 都不应该让 iOS 侧判定为断连，反之亦然。
///
/// 查询失败时**保留上一次结果**（返回 `None`），不把"查不到"当成"设备拔了"。
pub struct AndroidMonitor {
    program: std::path::PathBuf,
    run: Option<Run>,
    output: String,
    last: Instant,
}

impl Default for AndroidMonitor {
    fn default() -> Self {
        Self::new(apple_toolkit::android::find_program(
            apple_toolkit::android::ADB,
            apple_toolkit::android::ENV_ADB,
        ))
    }
}

impl AndroidMonitor {
    pub fn new(program: std::path::PathBuf) -> Self {
        Self {
            program,
            run: None,
            output: String::new(),
            last: Instant::now() - Duration::from_secs(3),
        }
    }

    /// 返回 `Some(设备列表)` 表示这一轮扫描成功；`None` 表示还没结果或查询失败。
    pub fn tick(&mut self) -> Option<Vec<apple_toolkit::platform::AdbDevice>> {
        if self.run.is_none() && self.last.elapsed() >= Duration::from_secs(3) {
            self.output.clear();
            self.last = Instant::now();
            self.run = Some(runner::start(runner::Request {
                program: self.program.clone(),
                args: apple_toolkit::android::devices_args()
                    .into_iter()
                    .map(std::ffi::OsString::from)
                    .collect(),
                timeout: Duration::from_secs(5),
            }));
        }
        for _ in 0..32 {
            let event = match self.run.as_ref().map(|r| r.events.try_recv()) {
                Some(Ok(e)) => e,
                Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                    self.run = None;
                    break;
                }
                _ => break,
            };
            match event {
                Event::Output { stderr: false, text } => {
                    if self.output.len() + text.len() > 65536 {
                        self.run = None;
                        break;
                    }
                    self.output.push_str(&text);
                }
                Event::Finished {
                    code,
                    cancelled,
                    timed_out,
                    dropped,
                } => {
                    self.run = None;
                    if code == Some(0) && !cancelled && !timed_out && dropped == 0 {
                        return Some(apple_toolkit::platform::parse_adb_devices(&self.output));
                    }
                    return None;
                }
                Event::Error(_) => {
                    self.run = None;
                    return None;
                }
                _ => (),
            }
        }
        None
    }
}

/// 安卓设备状态的后台探测。
///
/// `collect_status` 会连着跑几条 `adb shell`，在 UI 线程里做会卡界面，所以放到
/// 独立线程，每帧 `poll` 一次取结果。同一时刻只允许一个探测在飞。
#[derive(Default)]
pub struct AndroidProbe {
    rx: Option<std::sync::mpsc::Receiver<Result<apple_toolkit::android::Status, String>>>,
}

impl AndroidProbe {
    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }

    pub fn start(&mut self, serial: String) {
        if self.rx.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(apple_toolkit::android::collect_status(&serial));
        });
    }

    /// 有结果就返回并复位；没结果返回 `None`。
    pub fn poll(&mut self) -> Option<Result<apple_toolkit::android::Status, String>> {
        let rx = self.rx.as_ref()?;
        match rx.try_recv() {
            Ok(value) => {
                self.rx = None;
                Some(value)
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.rx = None;
                Some(Err("探测线程异常结束".into()))
            }
        }
    }

    /// 切换设备或手动刷新时丢弃在飞结果。
    pub fn cancel(&mut self) {
        self.rx = None;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn stable_connection_and_single_missed_scan_do_not_reread_or_disconnect() {
        use super::*;
        let id = "00008140-000205A03EBB001C";
        let mut p = Presence::default();
        assert_eq!(p.observe(&[id.into()], None), Change::Read);
        for _ in 0..10 {
            assert_eq!(p.observe(&[id.into()], Some(id)), Change::None);
        }
        assert_eq!(p.observe(&[], Some(id)), Change::None);
        assert_eq!(p.observe(&[id.into()], Some(id)), Change::None);
        assert_eq!(p.observe(&[], Some(id)), Change::None);
        assert_eq!(p.observe(&[], Some(id)), Change::Disconnected);
    }
    #[test]
    fn changed_or_multiple_devices_are_never_treated_as_the_old_target() {
        use super::*;
        let mut p = Presence::default();
        assert_eq!(p.observe(&["new".into()], Some("old")), Change::Read);
        assert_eq!(
            p.observe(&["old".into(), "new".into()], Some("old")),
            Change::Multiple
        );
    }
}
