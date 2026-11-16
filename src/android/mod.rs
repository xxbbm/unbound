//! Android 侧能力：ADB / fastboot 命令构造。
//!
//! # 证据来源
//!
//! 取自 `搞机助手` 4.9.1 的静态特征（.NET，随包附带 `adb.exe`、`AdbWinApi.dll`、
//! `AdbWinUsbApi.dll`、`AdbkillPort.exe`）。可确认它做这些事：
//!
//! | 证据 | 对应能力 |
//! |---|---|
//! | 随包 `adb.exe` + `adb5037` + `Adbprot` | ADB 服务与端口处理 |
//! | `fastboot flash boot boot.img` | fastboot 刷机 |
//! | `HKLM\…\usbflags\18D1D00D0100` 的 `osvc` / `SkipBOSDescriptorQuery` | 修 USB 描述符标志，让 ADB/fastboot 认得设备 |
//! | `classes.dex`（内嵌） | 随包 dex |
//!
//! 命令一律**固定 argv、不拼 shell 字符串**，与项目既有约定一致。
//! 需要 shell 语义时（`shell` 子命令）只把用户命令作为一个参数传进去。

use crate::platform::AdbDevice;

pub mod apk;
pub use apk::{ApkInfo, Facts};

pub const ADB: &str = "adb.exe";
pub const FASTBOOT: &str = "fastboot.exe";
/// ADB 服务默认端口；`Adbprot` 读的就是它。
pub const ADB_PORT: u16 = 5037;

/// `adb devices -l`
pub fn devices_args() -> Vec<String> {
    vec!["devices".into(), "-l".into()]
}

/// `adb start-server` / `adb kill-server`。
pub fn server_args(start: bool) -> Vec<String> {
    vec![if start { "start-server" } else { "kill-server" }.into()]
}

/// `adb -s <序列号> shell <命令>`。
///
/// 命令作为**单个参数**传入，不拆分 —— 拆了就等于让调用方拼 shell。
pub fn shell_args(device: &AdbDevice, command: &str) -> Vec<String> {
    vec![
        "-s".into(),
        device.serial.clone(),
        "shell".into(),
        command.to_string(),
    ]
}

/// `adb -s <序列号> install [-r] <apk>`
pub fn install_args(device: &AdbDevice, apk: &std::path::Path, replace: bool) -> Vec<String> {
    let mut args = vec!["-s".into(), device.serial.clone(), "install".into()];
    if replace {
        args.push("-r".into());
    }
    args.push(apk.display().to_string());
    args
}

/// `adb -s <序列号> uninstall <包名>`
pub fn uninstall_args(device: &AdbDevice, package: &str) -> Vec<String> {
    vec![
        "-s".into(),
        device.serial.clone(),
        "uninstall".into(),
        package.to_string(),
    ]
}

/// `adb -s <序列号> push <本地> <远端>`
pub fn push_args(device: &AdbDevice, local: &std::path::Path, remote: &str) -> Vec<String> {
    vec![
        "-s".into(),
        device.serial.clone(),
        "push".into(),
        local.display().to_string(),
        remote.to_string(),
    ]
}

/// `adb -s <序列号> pull <远端> <本地>`
pub fn pull_args(device: &AdbDevice, remote: &str, local: &std::path::Path) -> Vec<String> {
    vec![
        "-s".into(),
        device.serial.clone(),
        "pull".into(),
        remote.to_string(),
        local.display().to_string(),
    ]
}

/// `adb -s <序列号> reboot [target]`
pub fn reboot_args(device: &AdbDevice, target: Option<&str>) -> Vec<String> {
    let mut args = vec!["-s".into(), device.serial.clone(), "reboot".into()];
    if let Some(target) = target {
        args.push(target.to_string());
    }
    args
}

/// `fastboot -s <序列号> flash <分区> <镜像>`
pub fn fastboot_flash_args(serial: &str, partition: &str, image: &std::path::Path) -> Vec<String> {
    vec![
        "-s".into(),
        serial.to_string(),
        "flash".into(),
        partition.to_string(),
        image.display().to_string(),
    ]
}

/// `fastboot devices`
pub fn fastboot_devices_args() -> Vec<String> {
    vec!["devices".into()]
}

/// 修 USB 描述符标志的注册表命令（逐字取自 `搞机助手`）。
///
/// 键名 `18D1D00D0100` 是 `VID=18D1`（Google）+ `PID=D00D` + `BCDDevice=0100`。
/// 两个值分别关掉 `osvc` 与 `SkipBOSDescriptorQuery` 的干扰。
///
/// **需要管理员权限**，且会改注册表 —— 调用方必须先征得用户同意再执行。
pub fn usb_flag_commands() -> Vec<Vec<String>> {
    const KEY: &str = r"HKLM\SYSTEM\CurrentControlSet\Control\usbflags\18D1D00D0100";
    let mut commands = Vec::new();
    for name in ["osvc", "SkipBOSDescriptorQuery"] {
        commands.push(vec![
            "add".into(),
            KEY.into(),
            "/v".into(),
            name.into(),
            "/t".into(),
            "REG_BINARY".into(),
            "/d".into(),
            "0000".into(),
            "/f".into(),
        ]);
    }
    commands
}

/// 关键能力清单，供界面分组显示。`true` 表示会改动设备或系统。
pub fn capabilities() -> &'static [(&'static str, bool)] {
    &[
        ("查看设备与授权状态", false),
        ("执行 shell 命令", false),
        ("安装 / 卸载应用", true),
        ("推送 / 拉取文件", true),
        ("重启（普通 / bootloader / recovery）", true),
        ("fastboot 刷写分区", true),
        ("修复 USB 描述符标志（需管理员）", true),
    ]
}

// ---------------------------------------------------------------- 动作入口

/// 一条要执行的命令。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    pub program: &'static str,
    pub args: Vec<String>,
}

/// 界面上的安卓动作 id。
pub const ACTIONS: [&str; 12] = [
    "android-devices",
    "android-info",
    "android-apk-info",
    "android-shell",
    "android-install",
    "android-uninstall",
    "android-push",
    "android-pull",
    "android-reboot",
    "android-fastboot-flash",
    "android-usb-flags",
    "android-fastboot-devices",
];

pub fn is_action(id: &str) -> bool {
    ACTIONS.contains(&id)
}

/// 把安卓动作用到的 `-s <序列号>` 前缀拼出来；空序列号表示交给 adb 自己选。
fn device_prefix(serial: &str) -> Vec<String> {
    if serial.trim().is_empty() {
        Vec::new()
    } else {
        vec!["-s".to_string(), serial.trim().to_string()]
    }
}

/// 只做校验，不执行。与 iOS 侧 `Task::validate` 的职责一致。
pub fn validate(id: &str, input: &str, output: &str) -> Result<(), String> {
    match id {
        "android-shell" if input.trim().is_empty() => Err("请填写要在设备上执行的命令".into()),
        "android-install" if !std::path::Path::new(input).is_file() => {
            Err("请选择存在的 APK 文件".into())
        }
        "android-apk-info" if !std::path::Path::new(input).is_file() => {
            Err("请选择存在的 APK 文件".into())
        }
        "android-uninstall" if input.trim().is_empty() => Err("请填写应用包名".into()),
        "android-push" if !std::path::Path::new(input).is_file() => {
            Err("请选择要推送的文件".into())
        }
        "android-push" if output.trim().is_empty() => {
            Err("请填写设备上的目标路径（例如 /sdcard/）".into())
        }
        "android-pull" if input.trim().is_empty() => Err("请填写设备上的文件路径".into()),
        "android-pull" if output.trim().is_empty() => Err("请选择电脑上的保存文件夹".into()),
        "android-fastboot-flash" if !std::path::Path::new(input).is_file() => {
            Err("请选择要刷写的镜像文件".into())
        }
        "android-fastboot-flash" if output.trim().is_empty() => {
            Err("请填写要刷写的分区名（例如 boot）".into())
        }
        "android-fastboot-flash" => {
            let partition = output.trim();
            if partition.starts_with('-') || partition.chars().any(char::is_whitespace) {
                return Err("分区名不能含空白或以减号开头".into());
            }
            Ok(())
        }
        "android-reboot" => {
            let target = input.trim();
            if !matches!(target, "" | "bootloader" | "recovery") {
                return Err("重启目标只能是空、bootloader 或 recovery".into());
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// 构造动作要执行的命令。不做 I/O，便于单测。
///
/// `serial` 为空时不下发 `-s`，由 adb 在"只有一台设备"时自行选择 ——
/// 多台设备时 adb 会自己报错，比我们猜一台更安全。
pub fn plan(id: &str, serial: &str, input: &str, output: &str) -> Result<Invocation, String> {
    let prefix = device_prefix(serial);
    let mut args = prefix.clone();
    let program = match id {
        "android-devices" => {
            return Ok(Invocation {
                program: ADB,
                args: devices_args(),
            })
        }
        "android-shell" => {
            if input.trim().is_empty() {
                return Err("请填写要在设备上执行的命令".into());
            }
            args.extend(["shell".to_string(), input.to_string()]);
            ADB
        }
        "android-install" => {
            let path = std::path::Path::new(input);
            if !path.is_file() {
                return Err("请选择存在的 APK 文件".into());
            }
            args.push("install".into());
            args.push("-r".into());
            args.push(path.display().to_string());
            ADB
        }
        "android-uninstall" => {
            let package = input.trim();
            if package.is_empty() {
                return Err("请填写应用包名".into());
            }
            if package.starts_with('-') || package.chars().any(char::is_whitespace) {
                return Err("包名不能含空白或以减号开头".into());
            }
            args.extend(["uninstall".to_string(), package.to_string()]);
            ADB
        }
        "android-push" => {
            let path = std::path::Path::new(input);
            if !path.is_file() {
                return Err("请选择要推送的文件".into());
            }
            if output.trim().is_empty() {
                return Err("请填写设备上的目标路径".into());
            }
            args.extend([
                "push".to_string(),
                path.display().to_string(),
                output.trim().to_string(),
            ]);
            ADB
        }
        "android-pull" => {
            if input.trim().is_empty() {
                return Err("请填写设备上的文件路径".into());
            }
            if output.trim().is_empty() {
                return Err("请选择电脑上的保存文件夹".into());
            }
            args.extend([
                "pull".to_string(),
                input.trim().to_string(),
                output.trim().to_string(),
            ]);
            ADB
        }
        "android-reboot" => {
            let target = input.trim();
            if !matches!(target, "" | "bootloader" | "recovery") {
                return Err("重启目标只能是空、bootloader 或 recovery".into());
            }
            args.push("reboot".into());
            if !target.is_empty() {
                args.push(target.to_string());
            }
            ADB
        }
        "android-usb-flags" => {
            // 注册表写入走 reg.exe，不是 adb。这里只返回第一条，第二条由 run 处理。
            let commands = usb_flag_commands();
            return Ok(Invocation {
                program: "reg.exe",
                args: commands[0].clone(),
            });
        }
        "android-info" => {
            args.extend(["shell".to_string(), "getprop".to_string()]);
            ADB
        }
        "android-apk-info" => {
            // 纯本地解析，不需要设备；run 会在执行前拦下这个动作。
            return Ok(Invocation {
                program: ADB,
                args: Vec::new(),
            });
        }
        "android-fastboot-devices" => {
            return Ok(Invocation {
                program: FASTBOOT,
                args: fastboot_devices_args(),
            })
        }
        "android-fastboot-flash" => {
            // 真正的流程由 run_fastboot 编排；这里给出第一步的命令形态。
            return Ok(Invocation {
                program: ADB,
                args: reboot_args_for(serial, "bootloader"),
            });
        }
        other => return Err(format!("未知的安卓动作：{other}")),
    };
    Ok(Invocation { program, args })
}

/// 用项目自带的受限运行器跑一条命令（固定 argv、超时、输出上限）。
fn run_bounded(
    program: &std::path::Path,
    args: &[String],
    seconds: u64,
) -> Result<String, String> {
    use crate::runner::{self, Event, Options, Request};
    let run = runner::start_with_options(
        Request {
            program: program.to_path_buf(),
            args: args.iter().map(std::ffi::OsString::from).collect(),
            timeout: std::time::Duration::from_secs(seconds),
        },
        Options::default(),
    );
    let mut stdout = String::new();
    let mut stderr = String::new();
    loop {
        match run.events.recv().map_err(|e| e.to_string())? {
            Event::Output { stderr: is_err, text } => {
                let out = if is_err { &mut stderr } else { &mut stdout };
                if out.len() + text.len() > 2 * 1024 * 1024 {
                    run.cancel();
                    return Err("命令输出超过大小限制".into());
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
                if cancelled || timed_out || dropped > 0 {
                    return Err(format!(
                        "命令未正常结束（超时 {timed_out}）：{}",
                        stderr.chars().take(800).collect::<String>()
                    ));
                }
                if code != Some(0) {
                    return Err(format!(
                        "命令返回 {code:?}：{}",
                        stderr.chars().take(800).collect::<String>()
                    ));
                }
                return Ok(stdout);
            }
            Event::Started(_) => (),
        }
    }
}

/// 执行一个安卓动作。`serial` 为空表示交给 adb 选唯一设备。
pub fn run(id: &str, serial: &str, input: &str, output: &str) -> Result<String, String> {
    validate(id, input, output)?;
    // 确保内置 adb 已就位（幂等）
    let _ = ensure_tools();

    // APK 身份：纯本地解析，不碰设备
    if id == "android-apk-info" {
        let info = apk::read(std::path::Path::new(input))?;
        return Ok(info.summary());
    }

    let invocation = plan(id, serial, input, output)?;
    let adb = find_program(ADB, ENV_ADB);
    let fastboot = find_program(FASTBOOT, ENV_FASTBOOT);

    // USB 标志要写两条注册表值
    if id == "android-usb-flags" {
        for command in usb_flag_commands() {
            run_bounded(std::path::Path::new("reg.exe"), &command, 30)
                .map_err(|e| format!("写入 usbflags 失败：{e}"))?;
        }
        return Ok("已写入 usbflags 的 osvc 与 SkipBOSDescriptorQuery 值。\
插入设备后若仍不识别，请重新插拔一次。"
            .to_string());
    }

    // 设备信息：拿 getprop 解析成摘要，而不是把原始 key 全倒给用户
    if id == "android-info" {
        let text = run_bounded(&adb, &invocation.args, 60)?;
        let info = AndroidInfo::from_getprop(&parse_getprop(&text));
        return Ok(info.summary());
    }

    // fastboot 刷写：按阶段表推进，设备没进 fastboot 就不刷
    if id == "android-fastboot-flash" {
        let image = std::path::Path::new(input);
        let partition = output.trim().to_string();
        let log = run_fastboot(
            serial,
            &partition,
            image,
            |program, args, seconds| {
                let path = if program == FASTBOOT {
                    fastboot.as_path()
                } else {
                    adb.as_path()
                };
                run_bounded(path, args, seconds)
            },
            |serial, seconds| {
                let deadline =
                    std::time::Instant::now() + std::time::Duration::from_secs(seconds);
                while std::time::Instant::now() < deadline {
                    if let Ok(text) =
                        run_bounded(&fastboot, &fastboot_devices_args(), 20)
                    {
                        if parse_fastboot_devices(&text)
                            .iter()
                            .any(|s| s.eq_ignore_ascii_case(serial))
                        {
                            return Ok(true);
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
                Ok(false)
            },
        )?;
        return Ok(log.join("\n"));
    }

    let seconds = match id {
        "android-install" | "android-push" | "android-pull" => 600,
        "android-shell" => 120,
        _ => 60,
    };
    let program = if invocation.program == FASTBOOT {
        fastboot.as_path()
    } else {
        adb.as_path()
    };
    let text = run_bounded(program, &invocation.args, seconds)?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        Ok(format!("{} 已完成。", id))
    } else {
        Ok(trimmed.to_string())
    }
}

// ---------------------------------------------------------------- 设备状态

/// 电池信息，取自 `dumpsys battery`。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Battery {
    pub level: Option<u8>,
    pub charging: Option<bool>,
    /// 摄氏度。`dumpsys` 给的是十分之一度。
    pub temperature_c: Option<f32>,
    pub voltage_mv: Option<u32>,
}

impl Battery {
    pub fn summary(&self) -> String {
        let level = self
            .level
            .map(|v| format!("{v}%"))
            .unwrap_or_else(|| "?".into());
        let state = match self.charging {
            Some(true) => "充电中",
            Some(false) => "未充电",
            None => "充电状态未知",
        };
        let mut text = format!("{level}（{state}）");
        if let Some(temp) = self.temperature_c {
            text.push_str(&format!("  {temp:.1}°C"));
        }
        if let Some(mv) = self.voltage_mv {
            text.push_str(&format!("  {:.3}V", mv as f32 / 1000.0));
        }
        text
    }
}

/// 存储信息，取自 `df /data`。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Storage {
    pub total_bytes: Option<u64>,
    pub available_bytes: Option<u64>,
}

impl Storage {
    pub fn summary(&self) -> String {
        let gb = |v: u64| v as f64 / 1024.0 / 1024.0 / 1024.0;
        match (self.total_bytes, self.available_bytes) {
            (Some(total), Some(avail)) => {
                format!("可用 {:.1} GB / 共 {:.1} GB", gb(avail), gb(total))
            }
            (Some(total), None) => format!("共 {:.1} GB", gb(total)),
            (None, Some(avail)) => format!("可用 {:.1} GB", gb(avail)),
            (None, None) => "存储未知".into(),
        }
    }
}

/// 屏幕信息，取自 `wm size` / `wm density`。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Screen {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub density: Option<u32>,
}

impl Screen {
    pub fn summary(&self) -> String {
        let size = match (self.width, self.height) {
            (Some(w), Some(h)) => format!("{w}×{h}"),
            _ => "分辨率未知".into(),
        };
        match self.density {
            Some(dpi) => format!("{size}  {dpi} dpi"),
            None => size,
        }
    }
}

/// 一台安卓设备的只读摘要。所有字段都可能缺失，缺失就是 `None`。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub info: AndroidInfo,
    pub battery: Battery,
    pub storage: Storage,
    pub screen: Screen,
}

/// 解析 `dumpsys battery`。
///
/// ```text
///   USB powered: true
///   level: 87
///   voltage: 4102
///   temperature: 310        ← 十分之一摄氏度
/// ```
pub fn parse_dumpsys_battery(text: &str) -> Battery {
    let mut battery = Battery::default();
    let mut ac = None;
    let mut usb = None;
    for line in text.lines() {
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key.trim() {
            "level" => battery.level = value.parse().ok(),
            "voltage" => battery.voltage_mv = value.parse().ok(),
            "temperature" => {
                battery.temperature_c = value.parse::<i32>().ok().map(|t| t as f32 / 10.0)
            }
            "AC powered" => ac = value.parse::<bool>().ok(),
            "USB powered" => usb = value.parse::<bool>().ok(),
            _ => {}
        }
    }
    // 任一电源为真即视为充电中；两个都明确为假才算未充电
    battery.charging = match (ac, usb) {
        (Some(a), Some(u)) => Some(a || u),
        (Some(a), None) => Some(a),
        (None, Some(u)) => Some(u),
        (None, None) => None,
    };
    battery
}

/// 解析 `df -k /data` 的最后一行。数值单位是 1K 块。
pub fn parse_df(text: &str) -> Storage {
    let mut storage = Storage::default();
    for line in text.lines().rev() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // Filesystem 1K-blocks Used Available Use% Mounted
        if parts.len() < 5 {
            continue;
        }
        let Ok(total) = parts[1].parse::<u64>() else {
            continue;
        };
        let Ok(avail) = parts[3].parse::<u64>() else {
            continue;
        };
        storage.total_bytes = Some(total * 1024);
        storage.available_bytes = Some(avail * 1024);
        break;
    }
    storage
}

/// 解析 `wm size`，优先取覆盖值（若有）。
pub fn parse_wm_size(text: &str) -> (Option<u32>, Option<u32>) {
    let mut physical: Option<(u32, u32)> = None;
    let mut overlay: Option<(u32, u32)> = None;
    for line in text.lines() {
        let (target, value) = if let Some((_, rest)) = line.split_once("Override size:") {
            (&mut overlay, rest)
        } else if let Some((_, rest)) = line.split_once("Physical size:") {
            (&mut physical, rest)
        } else {
            continue;
        };
        if let Some((w, h)) = value.trim().split_once('x') {
            if let (Ok(w), Ok(h)) = (w.trim().parse::<u32>(), h.trim().parse::<u32>()) {
                *target = Some((w, h));
            }
        }
    }
    match overlay.or(physical) {
        Some((w, h)) => (Some(w), Some(h)),
        None => (None, None),
    }
}

/// 解析 `wm density`。
pub fn parse_wm_density(text: &str) -> Option<u32> {
    let mut physical: Option<u32> = None;
    let mut overlay: Option<u32> = None;
    for line in text.lines() {
        if let Some((_, rest)) = line.split_once("Override density:") {
            overlay = rest.trim().parse().ok().or(overlay);
        } else if let Some((_, rest)) = line.split_once("Physical density:") {
            physical = rest.trim().parse().ok().or(physical);
        }
    }
    overlay.or(physical)
}

/// 跑几条只读命令，凑出一份摘要。任何一条失败都只影响对应字段。
pub fn collect_status(serial: &str) -> Result<Status, String> {
    let adb = find_program(ADB, ENV_ADB);
    let prefix = device_prefix(serial);
    let shell = |command: &str| -> Result<String, String> {
        let mut args = prefix.clone();
        args.extend(["shell".to_string(), command.to_string()]);
        run_bounded(&adb, &args, 30)
    };

    // 设备信息是必需的：拿不到就说明设备没连上
    let info = AndroidInfo::from_getprop(&parse_getprop(&shell("getprop")?));
    let battery = shell("dumpsys battery")
        .map(|t| parse_dumpsys_battery(&t))
        .unwrap_or_default();
    let storage = shell("df -k /data")
        .map(|t| parse_df(&t))
        .unwrap_or_default();
    let (width, height) = shell("wm size")
        .map(|t| parse_wm_size(&t))
        .unwrap_or((None, None));
    let density = shell("wm density").map(|t| parse_wm_density(&t)).ok().flatten();
    Ok(Status {
        info,
        battery,
        storage,
        screen: Screen {
            width,
            height,
            density,
        },
    })
}

// ---------------------------------------------------------------- 工具定位

/// 环境变量覆盖：设了就优先用它，方便用户指向自己的 platform-tools。
pub const ENV_ADB: &str = "UNBOUND_ADB";
pub const ENV_FASTBOOT: &str = "UNBOUND_FASTBOOT";

/// 在某组基准目录下，按顺序给出工具候选路径。
///
/// 顺序即优先级：先看应用自己带的，再看同名子目录，最后回落到裸名字（走 PATH）。
/// 抽成纯函数是为了能在没有磁盘的情况下单测顺序。
pub fn candidate_paths(base_dirs: &[std::path::PathBuf], name: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    for base in base_dirs {
        out.push(base.join(name));
        out.push(base.join("tools").join(name));
        out.push(base.join("platform-tools").join(name));
    }
    // 最后一档：交给 PATH 解析
    out.push(std::path::PathBuf::from(name));
    out
}

/// 内置工具落盘的位置：用户数据目录下的 `tools`。
pub fn tools_dir() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("local", "AppleToolkit", "AppleToolkit")
        .map(|dirs| dirs.data_local_dir().join("android-tools"))
}

/// 把内置的 `adb.exe` 与两个 dll 解到 [`tools_dir`]。
///
/// 幂等：已存在就不覆盖（用户可能自己换过版本）。写失败只报错不 panic ——
/// 拿不到 adb 只是安卓功能不可用，不该影响整个程序。
pub fn ensure_tools() -> Result<std::path::PathBuf, String> {
    let dir = tools_dir().ok_or("无法确定用户数据目录")?;
    crate::bypass::resources::write_all(&dir, crate::bypass::resources::ANDROID_TOOLS)?;
    Ok(dir)
}

/// 找工具：环境变量 → 内置目录 → 可执行文件同目录/子目录 → PATH。
///
/// 找不到时**返回裸名字**而不是报错 —— `std::process::Command` 会用 PATH 解析，
/// 真没有时执行阶段会给出明确错误，比在探测阶段就静默失败好。
pub fn find_program(name: &str, env_key: &str) -> std::path::PathBuf {
    if let Ok(value) = std::env::var(env_key) {
        let path = std::path::PathBuf::from(value.trim());
        if path.is_file() {
            return path;
        }
    }
    // 内置的 adb 优先于系统 PATH 里的版本，避免版本不一致导致的怪问题
    if let Some(dir) = tools_dir() {
        let bundled = dir.join(name);
        if bundled.is_file() {
            return bundled;
        }
    }
    let mut bases = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            bases.push(dir.to_path_buf());
        }
    }
    for candidate in candidate_paths(&bases, name) {
        if candidate.is_file() {
            return candidate;
        }
    }
    std::path::PathBuf::from(name)
}

// ---------------------------------------------------------------- 设备信息

/// 从 `adb shell getprop` 的输出里取值。
///
/// 格式固定为 `[key]: [value]`，未设置的键值是空字符串。
pub fn parse_getprop(text: &str) -> std::collections::BTreeMap<String, String> {
    let mut map = std::collections::BTreeMap::new();
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some((key, tail)) = rest.split_once("]: [") else {
            continue;
        };
        let Some(value) = tail.strip_suffix(']') else {
            continue;
        };
        map.insert(key.to_string(), value.to_string());
    }
    map
}

/// 安卓设备的只读摘要。字段缺失就是 `None`，不编造。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AndroidInfo {
    pub model: Option<String>,
    pub version: Option<String>,
    pub sdk: Option<String>,
    pub serial: Option<String>,
    pub abi: Option<String>,
}

impl AndroidInfo {
    pub fn from_getprop(map: &std::collections::BTreeMap<String, String>) -> Self {
        let pick = |key: &str| -> Option<String> {
            map.get(key).filter(|v| !v.trim().is_empty()).cloned()
        };
        Self {
            model: pick("ro.product.model").or_else(|| pick("ro.product.name")),
            version: pick("ro.build.version.release"),
            sdk: pick("ro.build.version.sdk"),
            serial: pick("ro.serialno"),
            abi: pick("ro.product.cpu.abi"),
        }
    }

    /// 一行摘要，供界面显示。
    pub fn summary(&self) -> String {
        let part = |v: &Option<String>, empty: &str| v.clone().unwrap_or_else(|| empty.into());
        format!(
            "{} / Android {} (API {}) / {} / 序列号 {}",
            part(&self.model, "未知机型"),
            part(&self.version, "?"),
            part(&self.sdk, "?"),
            part(&self.abi, "?"),
            part(&self.serial, "?")
        )
    }
}

// ---------------------------------------------------------------- fastboot 流程

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FastbootStep {
    /// `adb reboot bootloader`
    EnterBootloader,
    /// 轮询 `fastboot devices` 直到出现目标序列号
    WaitBootloader,
    /// `fastboot flash <分区> <镜像>`
    Flash { partition: String, image: String },
    /// `fastboot reboot`
    Reboot,
}

/// 刷写流程的阶段顺序。只构造，不执行。
pub fn fastboot_plan(partition: &str, image: &std::path::Path) -> Vec<FastbootStep> {
    vec![
        FastbootStep::EnterBootloader,
        FastbootStep::WaitBootloader,
        FastbootStep::Flash {
            partition: partition.to_string(),
            image: image.display().to_string(),
        },
        FastbootStep::Reboot,
    ]
}

/// 解析 `fastboot devices`：每行是 `序列号<TAB>fastboot`。
pub fn parse_fastboot_devices(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let serial = parts.next()?;
            let state = parts.next().unwrap_or("");
            if state.eq_ignore_ascii_case("fastboot") {
                Some(serial.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// 执行刷写流程。`run` 与 `wait` 注入，便于完整测试而不碰设备。
///
/// 失败即停：进入 bootloader 后若在 `wait` 期间没看到设备，就不刷写、不重启。
pub fn run_fastboot<F, W>(
    serial: &str,
    partition: &str,
    image: &std::path::Path,
    mut run: F,
    mut wait: W,
) -> Result<Vec<String>, String>
where
    F: FnMut(&str, &[String], u64) -> Result<String, String>,
    W: FnMut(&str, u64) -> Result<bool, String>,
{
    if !image.is_file() {
        return Err(format!("找不到镜像：{}", image.display()));
    }
    let mut log = Vec::new();

    run(ADB, &reboot_args_for(serial, "bootloader"), 60)?;
    log.push("已请求进入 bootloader".to_string());

    if !wait(serial, 60)? {
        return Err("等待设备进入 fastboot 超时，未开始刷写".into());
    }
    log.push("设备已进入 fastboot".to_string());

    let mut flash = Vec::new();
    if !serial.trim().is_empty() {
        flash.push("-s".to_string());
        flash.push(serial.trim().to_string());
    }
    flash.push("flash".to_string());
    flash.push(partition.to_string());
    flash.push(image.display().to_string());
    run(FASTBOOT, &flash, 900)?;
    log.push(format!("已刷写 {partition}"));

    run(FASTBOOT, &reboot_cmd(serial), 120)?;
    log.push("已重启回系统".to_string());
    Ok(log)
}

fn reboot_args_for(serial: &str, target: &str) -> Vec<String> {
    let mut args = Vec::new();
    if !serial.trim().is_empty() {
        args.push("-s".to_string());
        args.push(serial.trim().to_string());
    }
    args.push("reboot".to_string());
    args.push(target.to_string());
    args
}

fn reboot_cmd(serial: &str) -> Vec<String> {
    let mut args = Vec::new();
    if !serial.trim().is_empty() {
        args.push("-s".to_string());
        args.push(serial.trim().to_string());
    }
    args.push("reboot".to_string());
    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::parse_adb_devices;

    fn device() -> AdbDevice {
        parse_adb_devices("R58M12345AB device model:SM_G973F\n")
            .pop()
            .unwrap()
    }

    #[test]
    fn devices_command_shape() {
        assert_eq!(devices_args(), vec!["devices", "-l"]);
        assert_eq!(server_args(true), vec!["start-server"]);
        assert_eq!(server_args(false), vec!["kill-server"]);
    }

    #[test]
    fn shell_command_stays_one_argument() {
        let args = shell_args(&device(), "ls -l /sdcard | grep foo");
        assert_eq!(args[0], "-s");
        assert_eq!(args[1], "R58M12345AB");
        assert_eq!(args[2], "shell");
        // 整条命令是一个参数，没有被拆开
        assert_eq!(args[3], "ls -l /sdcard | grep foo");
        assert_eq!(args.len(), 4);
    }

    #[test]
    fn install_switches_replace_flag() {
        let apk = std::path::Path::new("C:/tmp/app.apk");
        let with = install_args(&device(), apk, true);
        assert!(with.contains(&"-r".to_string()));
        let without = install_args(&device(), apk, false);
        assert!(!without.contains(&"-r".to_string()));
        assert!(with.last().unwrap().ends_with("app.apk"));
    }

    #[test]
    fn push_pull_and_reboot_are_serial_scoped() {
        let local = std::path::Path::new("C:/tmp/a.bin");
        let push = push_args(&device(), local, "/sdcard/a.bin");
        assert_eq!(&push[..3], &["-s", "R58M12345AB", "push"]);
        let pull = pull_args(&device(), "/sdcard/a.bin", local);
        assert_eq!(&pull[..3], &["-s", "R58M12345AB", "pull"]);
        let reboot = reboot_args(&device(), Some("bootloader"));
        assert_eq!(reboot.last().unwrap(), "bootloader");
        assert_eq!(reboot_args(&device(), None).len(), 3);
    }

    #[test]
    fn fastboot_flash_matches_the_recovered_command() {
        let args = fastboot_flash_args("R58M12345AB", "boot", std::path::Path::new("boot.img"));
        assert_eq!(args[0], "-s");
        assert_eq!(args[2], "flash");
        assert_eq!(args[3], "boot");
        assert!(args[4].ends_with("boot.img"));
    }

    #[test]
    fn usb_flag_commands_match_the_recovered_registry_edit() {
        let commands = usb_flag_commands();
        assert_eq!(commands.len(), 2);
        let first = &commands[0];
        assert_eq!(first[0], "add");
        assert!(first[1].ends_with(r"usbflags\18D1D00D0100"));
        assert!(first.contains(&"osvc".to_string()));
        assert!(first.contains(&"REG_BINARY".to_string()));
        assert!(first.contains(&"0000".to_string()));
        assert!(first.contains(&"/f".to_string()));
        assert!(commands[1].contains(&"SkipBOSDescriptorQuery".to_string()));
    }

    #[test]
    fn capabilities_are_listed_with_risk_flags() {
        let caps = capabilities();
        assert!(!caps.is_empty());
        assert_eq!(caps[0].0, "查看设备与授权状态");
        assert!(!caps[0].1, "只读能力不该标危险");
        assert!(caps.iter().any(|(_, risky)| *risky));
    }

    #[test]
    fn action_ids_are_recognised() {
        for id in ACTIONS {
            assert!(is_action(id), "{id} 应被识别");
        }
        assert!(!is_action("android-nope"));
        assert!(!is_action("backup-info"), "不能把 iOS 动作当安卓动作");
    }

    #[test]
    fn plan_scopes_to_serial_only_when_given() {
        let scoped = plan("android-shell", "R58M12345AB", "getprop ro.build.version.release", "")
            .unwrap();
        assert_eq!(scoped.program, ADB);
        assert_eq!(&scoped.args[..2], &["-s", "R58M12345AB"]);
        assert_eq!(scoped.args[2], "shell");

        // 空序列号 → 不传 -s，交给 adb 自己选唯一设备
        let unscoped = plan("android-devices", "", "", "").unwrap();
        assert_eq!(unscoped.args, vec!["devices", "-l"]);
        assert!(!unscoped.args.iter().any(|a| a == "-s"));
    }

    #[test]
    fn plan_rejects_missing_arguments() {
        assert!(plan("android-shell", "", "   ", "").is_err());
        assert!(plan("android-install", "", "C:/nope.apk", "").is_err());
        assert!(plan("android-push", "", "C:/nope.bin", "/sdcard/").is_err());
        assert!(plan("android-pull", "", "", "C:/out").is_err());
        assert!(plan("android-nope", "", "", "").is_err());
    }

    #[test]
    fn uninstall_rejects_injection_ish_package_names() {
        assert!(plan("android-uninstall", "", "-rf", "").is_err());
        assert!(plan("android-uninstall", "", "com.a b", "").is_err());
        assert!(plan("android-uninstall", "", "com.example.app", "").is_ok());
    }

    #[test]
    fn reboot_whitelists_targets() {
        assert!(plan("android-reboot", "", "", "").is_ok());
        assert!(plan("android-reboot", "", "bootloader", "").is_ok());
        assert!(plan("android-reboot", "", "recovery", "").is_ok());
        assert!(plan("android-reboot", "", "sideload", "").is_err());
    }

    #[test]
    fn usb_flags_go_through_reg_not_adb() {
        let invocation = plan("android-usb-flags", "", "", "").unwrap();
        assert_eq!(invocation.program, "reg.exe");
        assert_eq!(invocation.args[0], "add");
    }

    #[test]
    fn candidate_order_prefers_bundled_then_subdirs_then_path() {
        let base = std::path::PathBuf::from("C:/app");
        let paths = candidate_paths(std::slice::from_ref(&base), "adb.exe");
        assert!(paths[0].ends_with("adb.exe"));
        assert_eq!(paths[0], base.join("adb.exe"));
        assert_eq!(paths[1], base.join("tools/adb.exe"));
        assert_eq!(paths[2], base.join("platform-tools/adb.exe"));
        // 最后一档是裸名字，交给 PATH
        assert_eq!(paths.last().unwrap(), &std::path::PathBuf::from("adb.exe"));
    }

    #[test]
    fn getprop_parsing_handles_empty_and_missing() {
        let text = "[ro.product.model]: [SM-G973F]\n\
[ro.build.version.release]: [12]\n\
[ro.build.version.sdk]: [31]\n\
[ro.serialno]: []\n\
[ro.product.cpu.abi]: [arm64-v8a]\n";
        let map = parse_getprop(text);
        assert_eq!(map.get("ro.product.model").map(String::as_str), Some("SM-G973F"));
        let info = AndroidInfo::from_getprop(&map);
        assert_eq!(info.model.as_deref(), Some("SM-G973F"));
        assert_eq!(info.version.as_deref(), Some("12"));
        assert_eq!(info.sdk.as_deref(), Some("31"));
        assert_eq!(info.abi.as_deref(), Some("arm64-v8a"));
        // 空值视为没有，不编造
        assert_eq!(info.serial, None);
        assert!(info.summary().contains("SM-G973F"));
        assert!(info.summary().contains("序列号 ?"));
    }

    #[test]
    fn getprop_ignores_noise_lines() {
        let map = parse_getprop("garbage\n\n[only-a-key]:\n[a]: [b]\n");
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("a").map(String::as_str), Some("b"));
    }

    #[test]
    fn fastboot_devices_only_lists_fastboot_mode() {
        let text = "R58M12345AB\tfastboot\nDEADBEEF\tunauthorized\n\nE1234\tfastboot\n";
        assert_eq!(parse_fastboot_devices(text), vec!["R58M12345AB", "E1234"]);
        assert!(parse_fastboot_devices("").is_empty());
    }

    #[test]
    fn fastboot_plan_is_ordered_and_complete() {
        let plan = fastboot_plan("boot", std::path::Path::new("boot.img"));
        assert_eq!(plan.len(), 4);
        assert_eq!(plan[0], FastbootStep::EnterBootloader);
        assert_eq!(plan[1], FastbootStep::WaitBootloader);
        assert!(matches!(&plan[2], FastbootStep::Flash { partition, image }
            if partition == "boot" && image.ends_with("boot.img")));
        assert_eq!(plan[3], FastbootStep::Reboot);
    }

    #[test]
    fn fastboot_flow_runs_in_order_and_stops_when_device_never_arrives() {
        let dir = std::env::temp_dir().join(format!("atk-fb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let image = dir.join("boot.img");
        std::fs::write(&image, b"img").unwrap();

        let mut calls: Vec<String> = Vec::new();
        let log = run_fastboot(
            "R58M12345AB",
            "boot",
            &image,
            |program, args, _| {
                calls.push(format!("{program} {}", args.join(" ")));
                Ok(String::new())
            },
            |_, _| Ok(true),
        )
        .unwrap();
        assert_eq!(log.len(), 4);
        assert!(calls[0].contains("reboot bootloader"));
        assert!(calls[1].contains("flash boot"));
        assert!(calls[2].contains("fastboot reboot") || calls[2].contains("reboot"));
        assert!(calls.iter().all(|c| c.contains("R58M12345AB")), "每一步都要带序列号");

        // 设备没进 fastboot → 不刷写、不重启
        let mut calls2: Vec<String> = Vec::new();
        let err = run_fastboot(
            "R58M12345AB",
            "boot",
            &image,
            |program, args, _| {
                calls2.push(format!("{program} {}", args.join(" ")));
                Ok(String::new())
            },
            |_, _| Ok(false),
        )
        .unwrap_err();
        assert!(err.contains("超时"), "实际：{err}");
        assert_eq!(calls2.len(), 1, "只应执行进入 bootloader 那一步");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fastboot_refuses_a_missing_image() {
        let err = run_fastboot(
            "R58M12345AB",
            "boot",
            std::path::Path::new("C:/nope/boot.img"),
            |_, _, _| Ok(String::new()),
            |_, _| Ok(true),
        )
        .unwrap_err();
        assert!(err.contains("找不到镜像"));
    }

    #[test]
    fn battery_parsing_reads_level_temp_and_power() {
        let text = "Current Battery Service state:\n\
  AC powered: false\n\
  USB powered: true\n\
  level: 87\n\
  voltage: 4102\n\
  temperature: 310\n";
        let b = parse_dumpsys_battery(text);
        assert_eq!(b.level, Some(87));
        assert_eq!(b.charging, Some(true));
        assert_eq!(b.voltage_mv, Some(4102));
        assert_eq!(b.temperature_c, Some(31.0));
        let s = b.summary();
        assert!(s.contains("87%"));
        assert!(s.contains("充电中"));
        assert!(s.contains("31.0°C"));
    }

    #[test]
    fn battery_without_power_flags_reports_unknown() {
        let b = parse_dumpsys_battery("  level: 50\n");
        assert_eq!(b.charging, None);
        assert!(b.summary().contains("充电状态未知"));
    }

    #[test]
    fn df_parsing_converts_1k_blocks() {
        let text = "Filesystem      1K-blocks     Used Available Use% Mounted on\n\
/dev/block/dm-5  118253516 98765432   1950884  99% /data\n";
        let s = parse_df(text);
        assert_eq!(s.total_bytes, Some(118_253_516 * 1024));
        assert_eq!(s.available_bytes, Some(1_950_884 * 1024));
        assert!(s.summary().contains("GB"));
    }

    #[test]
    fn wm_size_prefers_override_then_physical() {
        let (w, h) = parse_wm_size("Physical size: 1080x2400\n");
        assert_eq!((w, h), (Some(1080), Some(2400)));
        let (w, h) = parse_wm_size("Physical size: 1080x2400\nOverride size: 720x1600\n");
        assert_eq!((w, h), (Some(720), Some(1600)));
        assert_eq!(parse_wm_size("garbage"), (None, None));
    }

    #[test]
    fn wm_density_prefers_override() {
        assert_eq!(parse_wm_density("Physical density: 420\n"), Some(420));
        assert_eq!(
            parse_wm_density("Physical density: 420\nOverride density: 320\n"),
            Some(320)
        );
        assert_eq!(parse_wm_density("garbage"), None);
    }

    #[test]
    fn status_summaries_are_human_readable() {
        let status = Status {
            info: AndroidInfo {
                model: Some("SM-G973F".into()),
                version: Some("12".into()),
                sdk: Some("31".into()),
                serial: Some("R58M12345AB".into()),
                abi: Some("arm64-v8a".into()),
            },
            battery: Battery {
                level: Some(87),
                charging: Some(true),
                temperature_c: Some(31.0),
                voltage_mv: Some(4102),
            },
            storage: Storage {
                total_bytes: Some(128_000_000_000),
                available_bytes: Some(64_000_000_000),
            },
            screen: Screen {
                width: Some(1080),
                height: Some(2400),
                density: Some(420),
            },
        };
        assert!(status.info.summary().contains("SM-G973F"));
        assert!(status.battery.summary().contains("87%"));
        assert!(status.storage.summary().contains("可用"));
        assert_eq!(status.screen.summary(), "1080×2400  420 dpi");
    }
}
