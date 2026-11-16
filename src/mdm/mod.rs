//! Device identity and read-only inspection workers. No management bypass workflow.

mod process;

use plist::Value;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Device {
    pub udid: String,
    pub serial: String,
    pub product: String,
    pub version: String,
    pub build: String,
    pub name: String,
    pub imei: String,
    pub activation: String,
    #[serde(default)]
    pub facts: BTreeMap<String, String>,
}
impl Device {
    pub fn major(&self) -> Result<u32, String> {
        self.version
            .split('.')
            .next()
            .unwrap_or("")
            .parse()
            .map_err(|_| "系统版本无效".into())
    }
    pub fn validate(&self) -> Result<(), String> {
        if !valid_udid(&self.udid)
            || [&self.serial, &self.product, &self.version, &self.build]
                .iter()
                .any(|s| s.is_empty())
        {
            return Err("设备 UDID、序列号、机型、系统或构建版本不完整".into());
        }
        self.major()?;
        Ok(())
    }
    pub fn same_identity(&self, other: &Self) -> bool {
        self.udid.eq_ignore_ascii_case(&other.udid)
            && self.serial.eq_ignore_ascii_case(&other.serial)
            && self.product == other.product
            && self.version == other.version
            && self.build == other.build
    }
}
pub fn valid_udid(s: &str) -> bool {
    (s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        || (s.len() == 25
            && s.as_bytes()[8] == b'-'
            && s.bytes()
                .enumerate()
                .all(|(i, b)| i == 8 || b.is_ascii_hexdigit()))
}

pub fn default_source() -> PathBuf {
    directories::UserDirs::new()
        .and_then(|d| d.desktop_dir().map(|p| p.join("苹果工具")))
        .unwrap_or_default()
}
pub fn default_workspace() -> PathBuf {
    directories::ProjectDirs::from("local", "AppleToolkit", "AppleToolkit")
        .map(|p| p.data_local_dir().join("mdm-jobs"))
        .unwrap_or_else(std::env::temp_dir)
}

/// 只扫描用户指定的资源根目录；不跟随符号链接，不扫描整个磁盘。
pub fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    fn visit(root: &Path, name: &str, depth: usize, count: &mut usize) -> Option<PathBuf> {
        if depth > 12 || *count > 20000 {
            return None;
        }
        let mut entries: Vec<_> = std::fs::read_dir(root)
            .ok()?
            .filter_map(Result::ok)
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in &entries {
            *count += 1;
            let ty = entry.file_type().ok()?;
            if ty.is_file()
                && entry
                    .file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(name)
            {
                return Some(entry.path());
            }
        }
        for entry in entries {
            if entry.file_type().ok()?.is_dir() {
                if let Some(p) = visit(&entry.path(), name, depth + 1, count) {
                    return Some(p);
                }
            }
        }
        None
    }
    visit(root, name, 0, &mut 0)
}

struct Context {
    root: PathBuf,
    info: PathBuf,
    ids: PathBuf,
}
impl Context {
    fn new(root: PathBuf) -> Result<Self, String> {
        if !root.is_dir() {
            return Err("请选择原始苹果工具资源目录".into());
        }

        let normal = root.join("Tools/NormalMode");
        let info = normal.join(format!("ideviceinfo{}", std::env::consts::EXE_SUFFIX));
        let ids = normal.join(format!("idevice_id{}", std::env::consts::EXE_SUFFIX));
        let info = if info.is_file() {
            info
        } else {
            find_file(
                &root,
                &format!("ideviceinfo{}", std::env::consts::EXE_SUFFIX),
            )
            .ok_or("缺少当前平台的 ideviceinfo 及依赖")?
        };
        let ids = if ids.is_file() {
            ids
        } else {
            find_file(
                &root,
                &format!("idevice_id{}", std::env::consts::EXE_SUFFIX),
            )
            .ok_or("缺少当前平台的 idevice_id 及依赖")?
        };
        Ok(Self { root, info, ids })
    }
    fn identifiers(&self) -> Result<Vec<String>, String> {
        let out = process::command(&self.ids, &["-l".into()], None, 10)?;
        let ids: Vec<String> = out
            .stdout
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        if ids.iter().any(|s| !valid_udid(s)) {
            return Err("USB 列表返回了无效设备标识，停止操作".into());
        }
        Ok(ids)
    }
    fn bound(&self, udid: &str, allow_absent: bool) -> Result<bool, String> {
        let ids = self.identifiers()?;
        if ids.is_empty() && allow_absent {
            return Ok(false);
        }
        if ids.len() != 1 || !ids[0].eq_ignore_ascii_case(udid) {
            return Err("原设备已断开或同时连接了其他设备；不会自动切换目标".into());
        }
        Ok(true)
    }
    fn read(&self, udid: &str) -> Result<Device, String> {
        let out = process::command(
            &self.info,
            &["-u".into(), udid.into(), "-x".into()],
            None,
            15,
        )?;
        let p = Value::from_reader(std::io::Cursor::new(out.stdout.as_bytes()))
            .map_err(|e| format!("设备信息不是有效 plist: {e}"))?;
        let d = p.as_dictionary().ok_or("设备信息缺少字典")?;
        let get = |key: &str| {
            d.get(key)
                .and_then(Value::as_string)
                .unwrap_or("")
                .to_owned()
        };
        let reported = get("UniqueDeviceID");
        if !reported.is_empty() && !reported.eq_ignore_ascii_case(udid) {
            return Err("查询返回的设备身份不一致".into());
        }
        let mut facts = BTreeMap::new();
        for key in [
            "ModelNumber",
            "RegionInfo",
            "RegulatoryModelNumber",
            "WiFiAddress",
            "BluetoothAddress",
            "EthernetAddress",
            "MLBSerialNumber",
            "HardwareModel",
            "CPUArchitecture",
            "DeviceColor",
        ] {
            if let Some(text) = d
                .get(key)
                .and_then(crate::inspection::text)
                .filter(|s| !s.is_empty())
            {
                facts.insert(key.into(), text);
            }
        }
        for (domain, keys) in [
            (
                "com.apple.disk_usage",
                &[
                    "TotalDiskCapacity",
                    "TotalDataCapacity",
                    "AmountDataAvailable",
                ][..],
            ),
            (
                "com.apple.mobile.battery",
                &[
                    "BatteryCurrentCapacity",
                    "BatteryIsCharging",
                    "BatteryFullyCharged",
                ][..],
            ),
        ] {
            if let Ok(output) = process::command(
                &self.info,
                &[
                    "-u".into(),
                    udid.into(),
                    "-q".into(),
                    domain.into(),
                    "-x".into(),
                ],
                None,
                8,
            ) {
                if let Ok(value) =
                    Value::from_reader(std::io::Cursor::new(output.stdout.as_bytes()))
                {
                    if let Some(dict) = value.as_dictionary() {
                        for key in keys {
                            if let Some(text) = dict.get(key).and_then(crate::inspection::text) {
                                facts.insert((*key).into(), text);
                            }
                        }
                    }
                }
            }
        }
        if let Some(bytes) = facts
            .get("TotalDiskCapacity")
            .and_then(|s| s.parse::<u64>().ok())
        {
            facts.insert(
                "CapacityLabel".into(),
                crate::inspection::capacity_label(bytes),
            );
        }
        if let Some(level) = facts
            .get("BatteryCurrentCapacity")
            .and_then(|s| s.parse::<u8>().ok())
            .filter(|n| *n <= 100)
        {
            facts.insert("BatteryLevelLabel".into(), format!("{level}%"));
        }
        let device = Device {
            udid: udid.into(),
            serial: get("SerialNumber"),
            product: get("ProductType"),
            version: get("ProductVersion"),
            build: get("BuildVersion"),
            name: get("DeviceName"),
            imei: get("InternationalMobileEquipmentIdentity"),
            activation: get("ActivationState"),
            facts,
        };
        device.validate()?;
        Ok(device)
    }
    fn verify_identity(&self, expected: &Device) -> Result<Device, String> {
        self.bound(&expected.udid, false)?;
        let actual = self.read(&expected.udid)?;
        if !expected.same_identity(&actual) {
            return Err("设备身份或系统信息改变，停止操作".into());
        }
        Ok(actual)
    }
}
pub fn worker_main() -> i32 {
    match worker(std::env::args_os().skip(2).collect()) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("[STOP] {e}");
            1
        }
    }
}
fn worker(args: Vec<OsString>) -> Result<(), String> {
    let (pairs, remaining) = args.as_chunks::<2>();
    if !remaining.is_empty() {
        return Err("MDM 后台参数不完整".into());
    }
    let opts: BTreeMap<String, OsString> = pairs
        .iter()
        .map(|p| (p[0].to_string_lossy().into_owned(), p[1].clone()))
        .collect();
    let root = opts
        .get("--source-root")
        .map(PathBuf::from)
        .ok_or("缺少资源目录")?;
    let action = opts
        .get("--action")
        .and_then(|s| s.to_str())
        .ok_or("缺少操作")?;
    let context = Context::new(root)?;
    let ids = context.identifiers()?;
    if ids.len() != 1 {
        return Err(format!(
            "检测到 {} 台 USB 设备，请只连接本次要处理的一台设备",
            ids.len()
        ));
    }
    let target = opts
        .get("--udid")
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or(&ids[0]);
    context.bound(target, false)?;
    let device = context.read(target)?;
    println!(
        "[DEVICE] {}",
        serde_json::to_string(&device).map_err(|e| e.to_string())?
    );
    if action == "probe" {
        return Ok(());
    }
    if action == "inspection" {
        let mut report = crate::inspection::Report::base(device.clone());
        println!(
            "[REPORT] {}",
            serde_json::to_string(&report).map_err(|e| e.to_string())?
        );
        report.inspect(&context.root);
        context.verify_identity(&device)?;
        println!(
            "[REPORT] {}",
            serde_json::to_string(&report).map_err(|e| e.to_string())?
        );
        println!("[RESULT] 设备详细信息读取完成。");
        return Ok(());
    }
    Err("此版本只支持设备识别和详细信息读取".into())
}
