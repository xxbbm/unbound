//! Device tasks run in an isolated child process. No shell strings and no default device.
pub mod local;
pub mod native;

use crate::{mdm::Device, runner};
use serde::{Deserialize, Serialize};
use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone, Copy, PartialEq)]
pub enum Input {
    None,
    File,
    Directory,
    Output,
    Text,
    BackupPassword,
}
#[derive(Clone, Copy)]
pub struct Action {
    pub id: &'static str,
    pub group: &'static str,
    pub name: &'static str,
    pub help: &'static str,
    pub input: Input,
    pub label: &'static str,
    pub dangerous: bool,
}
macro_rules! a {
    ($id:literal,$group:literal,$name:literal,$help:literal,$input:ident,$label:literal,$danger:literal) => {
        Action {
            id: $id,
            group: $group,
            name: $name,
            help: $help,
            input: Input::$input,
            label: $label,
            dangerous: $danger,
        }
    };
}
pub fn catalog() -> Vec<Action> {
    vec![
 a!("device-monitor","常用操作","实时电量与存储监控","每两秒读取实际电量、充电与存储状态。此服务不提供 CPU/GPU 性能数据；点击停止结束。",None,"",false),
 a!("screenshot","常用操作","保存手机截图","手机需支持截图服务；某些系统要求开发者组件。",Output,"截图保存位置",false),
 a!("reboot","常用操作","重启设备","手机将重新启动。",None,"",true),
 a!("shutdown","常用操作","关闭设备","手机将关机，需要手动开机。",None,"",true),
 a!("crashes","常用操作","查看崩溃记录","列出设备开放的崩溃日志文件。",Text,"手机日志目录（默认 /）",false),
 a!("crash-download","常用操作","下载一条崩溃记录","从手机日志目录下载指定文件。",Text,"手机日志路径",false),
 a!("crash-export","常用操作","导出崩溃记录","从设备导出崩溃日志；不把文件名猜测成硬件故障结论。",Directory,"日志保存文件夹",false),
 a!("backup","备份与刷机","备份手机资料","在电脑上保存此设备的本地备份。请保持手机解锁。",Directory,"备份保存位置",false),
 a!("restore-backup","备份与刷机","从备份恢复资料","仅支持当前设备的备份。会覆盖设备现有资料，并在完成后重启。",Directory,"选择备份文件夹",true),
 a!("backup-info","备份与刷机","查看备份信息","离线读取本地备份的机型、序列号、时间和加密状态；无需连接手机。可选单个备份或 Backup 父目录。",Directory,"选择备份文件夹",false),
 a!("backup-copy","备份与刷机","复制备份到其他磁盘","复制单个已完成的 Apple 本地备份，逐文件校验后提交；保留原备份，不修改 iTunes 系统路径。",Directory,"选择单个备份目录",false),
 a!("backup-password","备份与刷机","设置备份加密密码","开启或关闭备份加密。关闭时必须提供当前密码。密码不写入诊断日志。",BackupPassword,"备份密码",true),
 a!("backup-change-password","备份与刷机","修改已有备份密码","需要当前备份密码，不能找回或破解未知密码。",BackupPassword,"当前备份密码",true),
 a!("firmware-info","备份与刷机","检查固件信息","读取本地 IPSW 的系统版本、构建号和支持机型，不连接或修改手机。",File,"选择固件 (.ipsw)",false),
 a!("flash","备份与刷机","重新安装 iOS","选择适合此设备的 IPSW。默认清空资料；保资料更新也需要先备份。固件必须仍被 Apple 签名。",File,"选择固件 (.ipsw)",true),
 a!("erase","备份与刷机","抹除所有内容","保留当前系统，清除手机资料。需要记得 Apple ID 密码；此功能不会解除激活锁。",None,"",true),
 a!("apps","应用与文件","查看已安装应用","读取应用名称与 Bundle ID。",None,"",false),
 a!("install","应用与文件","安装 IPA","安装有效签名的 IPA 文件。设备可能要求确认；无法绕过应用签名。",File,"选择 IPA 文件",true),
 a!("uninstall","应用与文件","卸载应用","输入应用 Bundle ID；会移除该应用及其数据。",Text,"应用 Bundle ID",true),
 a!("files","应用与文件","查看手机共享文件","浏览 AFC 共享目录。双击下方目录进入，选择文件可下载。不是手机根目录。",Text,"手机目录（默认 /）",false),
 a!("file-upload","应用与文件","上传共享文件","把电脑文件上传到手机 AFC 的 /AppleToolkit 目录。",File,"选择电脑文件",true),
 a!("file-download","应用与文件","下载共享文件","输入 AFC 中的文件路径，运行时选择电脑保存位置。",Text,"手机文件路径",false),
 a!("file-delete","应用与文件","删除共享文件","只删除指定文件或空目录，不递归清空手机。",Text,"手机文件路径",true),
 a!("bypass-prepare","解锁与配置","iOS 绕配置 · 准备","先把要用的文件准备好，不动手机。按系统主版本自动选路线：低于 27 走旧路线（GavalLegacy 备份，并按本机改写 Info.plist / Manifest.plist 里的设备身份，含 __GAVAL_*__ 模板令牌）；等于 27 走现代路线（保留备份原始字节，改用 -s 指明来源）。只在电脑上写任务目录。",Directory,"任务目录",false),
 a!("bypass-apply","解锁与配置","iOS 绕配置 · 开始","对准备好的目录执行恢复并确认激活。旧路线：MDM_SETUP.exe，空 argv、cwd = 任务目录/ikey。现代路线：idevicebackup2 restore -u <目标UDID> -s <源UDID> --system --settings --remove --reboot --password <口令> ./。会清空手机资料，请先备份；现代路线需要服务端下发的备份口令。",BackupPassword,"备份口令",true),
 a!("bypass-ssh-prepare","解锁与配置","iOS 绕激活锁 · 准备","按系统版本选 mobileactivationd 替换件，校验 5 个分片的 SHA-256 并拼成整包，写出远端 shell 脚本。不连接设备。仅适用于已越狱的 A5–A11。",Directory,"任务目录（含分片）",false),
 a!("bypass-ssh-apply","解锁与配置","iOS 绕激活锁 · 开始","经 iproxy 把分片传到 /var/mobile/Media/，cat 拼接后替换 /usr/libexec/mobileactivationd，重载 com.apple.mobileactivationd.plist 并跳过设置向导。设备必须已越狱并开放 root SSH（默认口令 alpine）。",None,"",true),
 a!("android-devices","安卓设备","查看安卓设备","列出 adb 识别的设备、授权状态与机型。只读。",None,"",false),
 a!("android-info","安卓设备","读取安卓设备信息","读取机型、系统版本、API 级别、架构与序列号。只读。",None,"",false),
 a!("android-shell","安卓设备","执行 shell 命令","在设备上执行一条命令并返回输出；整条命令作为单个参数传入。",Text,"命令",false),
 a!("android-install","安卓设备","安装 APK","安装 APK 并保留已有数据。装之前可先用「解析 APK」确认包名与版本。",File,"选择 APK",true),
 a!("android-apk-info","安卓设备","解析 APK","离线读取 APK 的包名、版本、SDK 与权限；不需要连接设备。",File,"选择 APK",false),
 a!("android-uninstall","安卓设备","卸载应用","按包名卸载并清除该应用数据。",Text,"应用包名",true),
 a!("android-push","安卓设备","推送文件","把电脑上的文件推到设备路径。",File,"选择电脑文件",true),
 a!("android-pull","安卓设备","拉取文件","把设备上的文件拉到电脑文件夹。",Text,"设备文件路径",false),
 a!("android-reboot","安卓设备","重启安卓设备","重启到系统 / bootloader / recovery。",Text,"目标（空、bootloader 或 recovery）",true),
 a!("android-fastboot-flash","安卓设备","刷入分区镜像（fastboot）","先重启到 bootloader，确认设备进入 fastboot 后再刷写指定分区，成功后重启回系统。设备没进 fastboot 就停，不会硬刷。会改动设备。",File,"选择镜像文件",true),
 a!("android-fastboot-devices","安卓设备","查看 fastboot 设备","列出处于 fastboot 模式的设备。只读。",None,"",false),
 a!("android-usb-flags","安卓设备","修复设备不被识别","写入 usbflags 注册表值（osvc / SkipBOSDescriptorQuery），让 ADB 与 fastboot 认出设备。需要管理员权限，会修改注册表。",None,"",true),
]
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub device: Device,
    pub input: String,
    pub output: String,
    pub enabled: bool,
    pub full: bool,
    pub password: String,
    #[serde(default)]
    pub password_new: String,
    pub confirm: String,
    #[serde(default)]
    pub stop_file: String,
}
impl Task {
    pub fn validate(&self) -> Result<Action, String> {
        let a = catalog()
            .into_iter()
            .find(|a| a.id == self.id)
            .ok_or("未知操作")?;
        if local::is_local(&self.id) {
            local::validate(self)?;
            if self.id == "firmware-info" && !Path::new(&self.input).is_file() {
                return Err("请选择固件文件".into());
            }
            return Ok(a);
        }
        // 安卓动作不走 iOS 的 UDID 规则；其余校验（危险操作确认、参数类型）照旧。
        if crate::android::is_action(&self.id) {
            let serial = self.device.serial.trim();
            if !serial.is_empty()
                && crate::platform::platform_of(serial)
                    != Some(crate::platform::Platform::Android)
            {
                return Err("设备序列号不像安卓设备，请重新选择设备".into());
            }
            // 会改动设备的动作必须指定具体设备，不能靠 adb 自己猜
            if a.dangerous && serial.is_empty() {
                return Err("请先选择一台安卓设备".into());
            }
            crate::android::validate(&self.id, &self.input, &self.output)?;
        } else {
            self.device.validate()?;
        }
        if self.id == "backup-change-password" && self.password_new.is_empty() {
            return Err("请输入新备份密码".into());
        }
        if a.dangerous && self.confirm != self.device.serial {
            return Err("请先输入当前手机序列号确认操作".into());
        }
        if [
            &self.input,
            &self.output,
            &self.password,
            &self.password_new,
        ]
        .iter()
        .any(|s| s.contains('\0'))
        {
            return Err("参数包含无效字符".into());
        }
        match a.input {
            Input::File if !Path::new(&self.input).is_file() => {
                return Err("请选择存在的文件".into())
            }
            Input::Directory if self.input.trim().is_empty() => return Err("请选择文件夹".into()),
            Input::Output if self.output.trim().is_empty() => return Err("请选择保存位置".into()),
            Input::Text
                if !["files", "crashes"].contains(&self.id.as_str())
                    && self.input.trim().is_empty() =>
            {
                return Err(format!("请填写{}", a.label))
            }
            Input::BackupPassword if self.password.is_empty() => {
                return Err("请输入备份密码".into())
            }
            _ => (),
        }
        if ["uninstall", "profile-remove"].contains(&self.id.as_str())
            && (self.input.starts_with('-') || self.input.chars().any(char::is_whitespace))
        {
            return Err("标识不能含空白或以减号开头".into());
        }
        if self.id == "flash" {
            validate_ipsw(Path::new(&self.input), &self.device.product)?;
        }
        if (self.id.starts_with("file-") || self.id.starts_with("crash-"))
            && self.input.split(['/', '\\']).any(|p| p == "..")
        {
            return Err("文件路径不能包含上级目录".into());
        }
        if ["file-download", "crash-download"].contains(&self.id.as_str()) && self.output.is_empty()
        {
            return Err("请选择文件保存位置".into());
        }
        Ok(a)
    }
}
pub fn validate_ipsw(path: &Path, product: &str) -> Result<(), String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("固件不是有效的 IPSW：{e}"))?;
    let mut item = zip
        .by_name("BuildManifest.plist")
        .map_err(|_| "固件缺少 BuildManifest.plist")?;
    if item.size() > 32 * 1024 * 1024 {
        return Err("固件清单异常过大".into());
    }
    let value = plist::Value::from_reader(std::io::Cursor::new({
        let mut b = Vec::new();
        std::io::Read::read_to_end(&mut item, &mut b).map_err(|e| e.to_string())?;
        b
    }))
    .map_err(|e| e.to_string())?;
    let supported = value
        .as_dictionary()
        .and_then(|d| d.get("SupportedProductTypes"))
        .and_then(|v| v.as_array())
        .ok_or("固件没有可核对的机型清单")?;
    if !supported.iter().any(|v| v.as_string() == Some(product)) {
        return Err(format!("此固件不支持当前机型 {product}"));
    }
    Ok(())
}
pub fn flash_args(ecid: u64, ipsw: &Path, erase: bool) -> Vec<OsString> {
    let mut a = vec![
        "-d".into(),
        "-P".into(),
        "-y".into(),
        "-i".into(),
        format!("{ecid:x}").into(),
    ];
    if erase {
        a.push("-e".into())
    }
    a.push(ipsw.as_os_str().to_owned());
    a
}
pub fn worker_main(path: &Path) -> i32 {
    let result = (|| {
        let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(path);
        let task: Task = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        task.validate()?;
        if task.id == "firmware-info" {
            return firmware_summary(Path::new(&task.input));
        }
        if task.id == "backup-info" {
            return local::backup_info(Path::new(&task.input));
        }
        if task.id == "backup-copy" {
            return local::copy_backup(Path::new(&task.input), Path::new(&task.output));
        }
        // 配置锁绕过：判断与执行都在 bypass 模块内，这里只转调。
        if crate::android::is_action(&task.id) {
            return crate::android::run(
                &task.id,
                &task.device.serial,
                &task.input,
                &task.output,
            );
        }
        if task.id.starts_with("bypass-ssh") {
            return crate::bypass::ssh::run(
                &task.id,
                &task.device,
                &task.input,
                crate::bypass::ssh::DEFAULT_LOCAL_PORT,
            );
        }
        if task.id.starts_with("bypass-") {
            return crate::bypass::exec::handle(
                &task.id,
                &task.device,
                &task.input,
                &task.password,
            );
        }
        let root = crate::bundled::ensure_runtime()?;
        run(&root, &task)
    })();
    match result {
        Ok(message) => {
            println!(
                "[TASK_RESULT_JSON] {}",
                serde_json::to_string(&message).unwrap_or_else(|_| "\"结果编码失败\"".into())
            );
            0
        }
        Err(e) => {
            eprintln!("[STOP] {e}");
            1
        }
    }
}
fn cli(program: PathBuf, args: Vec<OsString>, timeout: u64) -> Result<String, String> {
    let run = runner::start_with_options(
        runner::Request {
            program,
            args,
            timeout: Duration::from_secs(timeout),
        },
        runner::Options {
            new_process_group: false,
            ..Default::default()
        },
    );
    loop {
        match run.events.recv().map_err(|e| e.to_string())? {
            runner::Event::Output { stderr, text } => {
                if stderr {
                    eprint!("{text}")
                } else {
                    print!("{text}")
                }
            }
            runner::Event::Error(e) => return Err(e),
            runner::Event::Finished {
                code,
                cancelled,
                timed_out,
                dropped,
            } => {
                if dropped > 0 {
                    eprintln!("部分日志未保留：{dropped}")
                }
                if code == Some(0) && !cancelled && !timed_out {
                    return Ok("后端任务结束。涉及重启或写入的结果，请在手机上确认。".into());
                }
                return Err(format!(
                    "后端未完成：退出码 {code:?}，超时 {timed_out}，取消 {cancelled}"
                ));
            }
            _ => (),
        }
    }
}
fn run(root: &Path, t: &Task) -> Result<String, String> {
    let session = native::Session::open(root, &t.device.udid)?;
    session.verify(&t.device)?;
    if t.id == "flash" {
        let ecid = session.ecid()?;
        drop(session);
        let program = resolve_program(
            root,
            "flash",
            "Engine/xdm-exec-restore.exe",
            "idevicerestore",
        );
        return cli(
            program,
            flash_args(ecid, Path::new(&t.input), !t.enabled),
            7200,
        );
    }
    if [
        "backup",
        "restore-backup",
        "backup-info",
        "backup-password",
        "backup-change-password",
    ]
    .contains(&t.id.as_str())
    {
        let mut args: Vec<OsString> = vec!["-u".into(), t.device.udid.clone().into()];
        match t.id.as_str() {
            "backup-change-password" => args.extend([
                "changepw".into(),
                t.password.clone().into(),
                t.password_new.clone().into(),
            ]),
            "backup" => {
                std::fs::create_dir_all(&t.input).map_err(|e| e.to_string())?;
                args.push("backup".into());
                if t.full {
                    args.push("--full".into())
                }
                args.push(t.input.clone().into())
            }
            "backup-password" => {
                args.extend([
                    "encryption".into(),
                    if t.enabled { "on".into() } else { "off".into() },
                    t.password.clone().into(),
                ]);
            }
            _ => {
                let (parent, source) = backup_location(Path::new(&t.input), &t.device.udid)?;
                if source != t.device.udid {
                    return Err("本版只允许恢复当前设备自己的备份".into());
                }
                args.push(if t.id == "backup-info" {
                    "info".into()
                } else {
                    "restore".into()
                });
                if t.id == "restore-backup" {
                    args.extend(["--system".into(), "--settings".into()]);
                    if !t.password.is_empty() {
                        args.extend(["--password".into(), t.password.clone().into()])
                    }
                }
                args.push(parent.into_os_string());
            }
        }
        let program = resolve_program(
            root,
            "backup",
            "MixIos27/idevicebackup2.exe",
            "idevicebackup2",
        );
        drop(session);
        return cli(program, args, 7200);
    }
    session.execute(t)
}
pub fn firmware_summary(path: &Path) -> Result<String, String> {
    let mut zip = zip::ZipArchive::new(std::fs::File::open(path).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let mut entry = zip
        .by_name("BuildManifest.plist")
        .map_err(|_| "固件缺少 BuildManifest.plist")?;
    if entry.size() > 32 * 1024 * 1024 {
        return Err("固件清单过大".into());
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes).map_err(|e| e.to_string())?;
    let value =
        plist::Value::from_reader(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let d = value.as_dictionary().ok_or("固件清单不是字典")?;
    let text = |key: &str| {
        d.get(key)
            .and_then(plist::Value::as_string)
            .unwrap_or("未提供")
    };
    let models = d
        .get("SupportedProductTypes")
        .and_then(plist::Value::as_array)
        .ok_or("固件没有机型清单")?
        .iter()
        .filter_map(plist::Value::as_string)
        .collect::<Vec<_>>()
        .join("、");
    Ok(format!(
        "系统 {} · 构建 {} · 支持机型：{}。尚未在线验证签名状态。",
        text("ProductVersion"),
        text("ProductBuildVersion"),
        models
    ))
}
/// Explicit user configuration wins; otherwise use the embedded Windows backend
/// or an installed native macOS binary, never a shell lookup by name.
pub fn resolve_program(root: &Path, key: &str, windows_relative: &str, unix_name: &str) -> PathBuf {
    if let Ok(config) = crate::config::Config::load() {
        if let Some(path) = config.executables.get(key).filter(|s| !s.is_empty()) {
            return PathBuf::from(path);
        }
    }
    let portable = crate::config::bundled_path(if cfg!(windows) && key == "flash" {
        "xdm-exec-restore"
    } else {
        unix_name
    });
    if portable.is_file() {
        return portable;
    }
    if cfg!(windows) {
        root.join(windows_relative)
    } else {
        [
            PathBuf::from("/opt/homebrew/bin").join(unix_name),
            PathBuf::from("/usr/local/bin").join(unix_name),
        ]
        .into_iter()
        .find(|p| p.is_file())
        .unwrap_or(portable)
    }
}
pub fn backup_location(path: &Path, udid: &str) -> Result<(PathBuf, String), String> {
    if path.join("Info.plist").is_file() {
        let id = path
            .file_name()
            .and_then(|v| v.to_str())
            .ok_or("备份目录名称无效")?;
        if !crate::mdm::valid_udid(id) {
            return Err("请选择以设备标识命名的标准备份目录，或其父目录".into());
        }
        return Ok((
            path.parent().ok_or("备份目录缺少父目录")?.to_path_buf(),
            id.into(),
        ));
    }
    if path.join(udid).join("Info.plist").is_file() {
        return Ok((path.to_path_buf(), udid.into()));
    }
    Err("没有找到当前设备的标准备份 Info.plist".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flash_binds_ecid_and_keeps_unicode_path_one_argument() {
        let args = flash_args(0x123abc, Path::new("C:/固件 folder/phone.ipsw"), true);
        assert_eq!(args[4], "123abc");
        assert_eq!(args.last().unwrap(), "C:/固件 folder/phone.ipsw");
        assert!(args.iter().any(|a| a == "-e"));
        assert!(!flash_args(7, Path::new("x"), false)
            .iter()
            .any(|a| a == "-e"));
    }
    #[test]
    fn no_duplicate_actions() {
        let c = catalog();
        let ids: std::collections::BTreeSet<_> = c.iter().map(|a| a.id).collect();
        assert_eq!(ids.len(), c.len());
        assert!(c.iter().filter(|a| a.dangerous).count() > 8);
        for removed in [
            "ringtone",
            "ssh",
            "screen",
            "syscfg",
            "privacy",
            "recovery-scan",
            "activate",
            "deactivate",
            "profiles",
            "voiceover",
            "device-label",
        ] {
            assert!(
                !c.iter().any(|a| a.id == removed),
                "discarded action remains: {removed}"
            );
        }
    }
    #[test]
    fn ipsw_preflight_rejects_wrong_model_before_starting_any_engine() {
        use std::io::Write;
        let path = std::env::temp_dir().join(format!("atk-firmware-{}.ipsw", std::process::id()));
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file(
            "BuildManifest.plist",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>SupportedProductTypes</key><array><string>iPhone17,1</string></array></dict></plist>").unwrap();
        zip.finish().unwrap();
        assert!(validate_ipsw(&path, "iPhone17,1").is_ok());
        assert!(validate_ipsw(&path, "iPhone16,1").is_err());
        assert!(firmware_summary(&path).unwrap().contains("iPhone17,1"));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn destructive_tasks_require_the_target_serial() {
        let d = Device {
            udid: "0".repeat(40),
            serial: "TEST123".into(),
            product: "iPhone17,1".into(),
            version: "18.0".into(),
            build: "22A".into(),
            name: "Test".into(),
            imei: String::new(),
            activation: "Activated".into(),
            facts: Default::default(),
        };
        let mut t = Task {
            id: "erase".into(),
            device: d,
            input: String::new(),
            output: String::new(),
            enabled: false,
            full: false,
            password: String::new(),
            password_new: String::new(),
            confirm: String::new(),
            stop_file: String::new(),
        };
        assert!(t.validate().is_err());
        t.confirm = "TEST123".into();
        assert!(t.validate().is_ok());
        t.device.name = "<script>alert(1)</script>".into();
        t.id = "backup-change-password".into();
        t.password = "current".into();
        assert!(t.validate().is_err());
        t.password_new = "new".into();
        assert!(t.validate().is_ok());
        t.id = "arbitrary".into();
        assert!(t.validate().is_err());
    }
}
