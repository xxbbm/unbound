//! 平台识别：同一套界面同时服务 iOS 与 Android。
//!
//! # 两边的标识完全不同
//!
//! * **iOS** 的 UDID 是 40 位十六进制，或 `XXXXXXXX-XXXXXXXXXXXXXXXX`（8+16）。
//!   这一点沿用 [`crate::mdm::valid_udid`]，不另立标准。
//! * **Android** 的序列号是设备自报的短串（`R58M12345AB`），或 `emulator-5554`；
//!   长度 4–32，字母数字加 `-` `_` `.` `:`。
//!
//! 所以**先比 iOS 规则，再比 Android 规则**，两者不重叠。判不出来就返回 `None`
//! 而不是猜 —— 猜错会把一台安卓机送进 iOS 的恢复流程。
//!
//! # `adb devices -l` 的输出
//!
//! ```text
//! List of devices attached
//! R58M12345AB            device product:beyond1q model:SM_G973F device:beyond1qlte transport_id:1
//! 0123456789ABCDEF       unauthorized usb:1-2
//! emulator-5554          device
//! ```
//!
//! 第二列是状态；`device` 才是可操作状态，`unauthorized` 需要用户在手机上点允许，
//! `offline` 说明 adb 连接坏了。这三种必须区分开，不能一律当"已连接"。

use crate::mdm::valid_udid;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    Ios,
    Android,
}

impl Platform {
    pub fn label(self) -> &'static str {
        match self {
            Platform::Ios => "iOS",
            Platform::Android => "Android",
        }
    }
    /// 界面上平台切换器的顺序。
    pub fn all() -> [Platform; 2] {
        [Platform::Ios, Platform::Android]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AdbState {
    /// 可直接操作。
    Device,
    /// 手机上还没点"允许 USB 调试"。
    Unauthorized,
    /// 连接异常，通常要 `adb kill-server`。
    Offline,
    /// 未识别的状态字符串，原样保留，别当成可用。
    Other(String),
}

impl AdbState {
    pub fn usable(&self) -> bool {
        matches!(self, AdbState::Device)
    }
    pub fn label(&self) -> &str {
        match self {
            AdbState::Device => "已授权",
            AdbState::Unauthorized => "待授权（请在手机上允许 USB 调试）",
            AdbState::Offline => "离线（可能需要重启 adb 服务）",
            AdbState::Other(s) => s,
        }
    }
}

/// 一台被 `adb devices -l` 报出来的安卓设备。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdbDevice {
    pub serial: String,
    pub state: AdbState,
    pub model: Option<String>,
    pub product: Option<String>,
    pub transport_id: Option<u32>,
}

/// 解析 `adb devices -l`。首行是表头，空行忽略。
pub fn parse_adb_devices(text: &str) -> Vec<AdbDevice> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("List of devices") || line.starts_with('*') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(serial) = parts.next() else { continue };
        let Some(state) = parts.next() else { continue };
        let state = match state {
            "device" => AdbState::Device,
            "unauthorized" => AdbState::Unauthorized,
            "offline" => AdbState::Offline,
            other => AdbState::Other(other.to_string()),
        };
        let mut model = None;
        let mut product = None;
        let mut transport_id = None;
        for token in parts {
            if let Some((key, value)) = token.split_once(':') {
                match key {
                    "model" => model = Some(value.to_string()),
                    "product" => product = Some(value.to_string()),
                    "transport_id" => transport_id = value.parse().ok(),
                    _ => {}
                }
            }
        }
        out.push(AdbDevice {
            serial: serial.to_string(),
            state,
            model,
            product,
            transport_id,
        });
    }
    out
}

/// 安卓序列号规则：4–32 位，字符集受限；`emulator-5554` 这类也符合。
pub fn looks_like_android_serial(text: &str) -> bool {
    let len = text.len();
    (4..=32).contains(&len)
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

/// 判断一个标识属于哪个平台。两边规则不重叠，先 iOS 后 Android。
pub fn platform_of(id: &str) -> Option<Platform> {
    if valid_udid(id) {
        return Some(Platform::Ios);
    }
    if looks_like_android_serial(id) && id.chars().any(|c| c.is_ascii_alphanumeric()) {
        return Some(Platform::Android);
    }
    None
}

/// 当前该显示哪一屏。
///
/// * 用户手动选了平台 → 以用户为准（但若该平台没设备，仍要在界面上说明）。
/// * 自动模式 → 有谁就连谁；两边都有时**优先 iOS**，与"一台电脑一次只处理一台"
///   的既有假设一致，且避免安卓设备插着不动时把 iOS 流程挤掉。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Selection {
    /// 以检测结果为准。
    #[default]
    Auto,
    Fixed(Platform),
}

pub fn resolve(
    selection: Selection,
    ios_devices: usize,
    android_devices: usize,
) -> Option<Platform> {
    match selection {
        Selection::Fixed(platform) => Some(platform),
        Selection::Auto => {
            if ios_devices > 0 {
                Some(Platform::Ios)
            } else if android_devices > 0 {
                Some(Platform::Android)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "List of devices attached\r\n\
R58M12345AB            device product:beyond1q model:SM_G973F device:beyond1qlte transport_id:1\r\n\
0123456789ABCDEF       unauthorized usb:1-2\r\n\
emulator-5554          device\r\n\
deadbeef               offline\r\n\r\n";

    #[test]
    fn parses_states_and_attributes() {
        let devices = parse_adb_devices(SAMPLE);
        assert_eq!(devices.len(), 4);
        assert_eq!(devices[0].serial, "R58M12345AB");
        assert_eq!(devices[0].state, AdbState::Device);
        assert_eq!(devices[0].model.as_deref(), Some("SM_G973F"));
        assert_eq!(devices[0].product.as_deref(), Some("beyond1q"));
        assert_eq!(devices[0].transport_id, Some(1));
        assert_eq!(devices[1].state, AdbState::Unauthorized);
        assert_eq!(devices[2].serial, "emulator-5554");
        assert_eq!(devices[3].state, AdbState::Offline);
    }

    #[test]
    fn only_authorized_is_usable() {
        assert!(AdbState::Device.usable());
        assert!(!AdbState::Unauthorized.usable());
        assert!(!AdbState::Offline.usable());
        assert!(!AdbState::Other("recovery".into()).usable());
        assert!(AdbState::Unauthorized.label().contains("允许"));
    }

    #[test]
    fn empty_or_headers_only_yield_nothing() {
        assert!(parse_adb_devices("").is_empty());
        assert!(parse_adb_devices("List of devices attached\n\n").is_empty());
        assert!(parse_adb_devices("* daemon started successfully *\n").is_empty());
    }

    #[test]
    fn platform_rules_do_not_overlap() {
        // iOS：40 hex / 8+16
        assert_eq!(platform_of("00008110-000A1C2E3F40001E"), Some(Platform::Ios));
        assert_eq!(
            platform_of(&"a".repeat(40)),
            Some(Platform::Ios)
        );
        // Android：短序列号
        assert_eq!(platform_of("R58M12345AB"), Some(Platform::Android));
        assert_eq!(platform_of("emulator-5554"), Some(Platform::Android));
        // 都不是
        assert_eq!(platform_of("ab"), None);
        assert_eq!(platform_of(&"z".repeat(33)), None);
        assert_eq!(platform_of("has space"), None);
    }

    #[test]
    fn auto_prefers_ios_then_android_then_none() {
        assert_eq!(resolve(Selection::Auto, 1, 1), Some(Platform::Ios));
        assert_eq!(resolve(Selection::Auto, 0, 2), Some(Platform::Android));
        assert_eq!(resolve(Selection::Auto, 0, 0), None);
    }

    #[test]
    fn manual_selection_wins_over_detection() {
        assert_eq!(
            resolve(Selection::Fixed(Platform::Android), 3, 0),
            Some(Platform::Android)
        );
        assert_eq!(
            resolve(Selection::Fixed(Platform::Ios), 0, 3),
            Some(Platform::Ios)
        );
    }

    #[test]
    fn labels_are_stable() {
        assert_eq!(Platform::all().len(), 2);
        assert_eq!(Platform::Ios.label(), "iOS");
        assert_eq!(Platform::Android.label(), "Android");
    }
}
