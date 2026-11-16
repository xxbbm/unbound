//! C ABI only. Never call recovered C++ SDK vtables with guessed layouts.
use super::Task;
use crate::mdm::Device;
use libloading::Library;
use plist::{Dictionary, Value};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    ffi::{c_char, c_void, CStr, CString},
    io::{Read, Write},
    path::{Path, PathBuf},
    ptr,
    time::Duration,
};
type P = *mut c_void;
type Free = unsafe extern "C" fn(P) -> i32;
pub struct Session {
    mobile: Library,
    plist: Library,
    device: P,
    lock: P,
}
struct Client {
    p: P,
    free: Free,
}
impl Drop for Client {
    fn drop(&mut self) {
        if !self.p.is_null() {
            unsafe {
                (self.free)(self.p);
            }
        }
    }
}
struct Node<'a> {
    p: P,
    s: &'a Session,
}
impl Drop for Node<'_> {
    fn drop(&mut self) {
        if let Ok(f) = self.s.function::<unsafe extern "C" fn(P)>("plist_free") {
            unsafe { f(self.p) }
        }
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        unsafe {
            if !self.lock.is_null() {
                if let Ok(f) = self.function::<Free>("lockdownd_client_free") {
                    f(self.lock);
                }
            }
            if !self.device.is_null() {
                if let Ok(f) = self.function::<Free>("idevice_free") {
                    f(self.device);
                }
            }
        }
    }
}
fn c(s: &str) -> Result<CString, String> {
    CString::new(s).map_err(|_| "参数含 NUL 字符".into())
}
fn ok(code: i32, what: &str) -> Result<(), String> {
    if code == 0 {
        Ok(())
    } else {
        Err(format!(
            "{what}失败（设备返回 {code}）。请解锁、信任电脑；若仍失败，此系统可能没有开放该服务。"
        ))
    }
}
pub(super) fn load(path: &Path) -> Result<Library, String> {
    let path = path.canonicalize().map_err(|e| e.to_string())?;
    unsafe {
        #[cfg(windows)]
        {
            libloading::os::windows::Library::load_with_flags(&path, 0x100 | 0x1000)
                .map(Into::into)
                .map_err(|e| format!("无法加载设备组件 {}：{e:?}", path.display()))
        }
        #[cfg(not(windows))]
        {
            Library::new(&path).map_err(|e| e.to_string())
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    pub directory: bool,
    pub size: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppEntry {
    pub name: String,
    pub identifier: String,
    pub version: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProfileEntry {
    pub name: String,
    pub identifier: String,
    pub removable: Option<bool>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetricSample {
    pub read_at: u64,
    pub values: BTreeMap<String, String>,
    pub errors: Vec<String>,
}
impl Session {
    pub fn open(root: &Path, id: &str) -> Result<Self, String> {
        if !crate::mdm::valid_udid(id) {
            return Err("设备标识不完整，拒绝使用默认设备".into());
        }
        let (mobile, plist) = if cfg!(windows) {
            (
                root.join("Engine/libimobiledevice.dll"),
                root.join("Engine/libplist.dll"),
            )
        } else {
            let base = [
                root.join("Engine"),
                PathBuf::from("/opt/homebrew/lib"),
                PathBuf::from("/usr/local/lib"),
            ]
            .into_iter()
            .find(|p| p.join("libimobiledevice-1.0.dylib").is_file())
            .ok_or("macOS 需要 tools/Engine 中的原生 libimobiledevice 和 libplist 组件")?;
            (
                base.join("libimobiledevice-1.0.dylib"),
                base.join("libplist-2.0.dylib"),
            )
        };
        let plist = load(&plist)?;
        let mobile = load(&mobile)?;
        let mut s = Self {
            mobile,
            plist,
            device: ptr::null_mut(),
            lock: ptr::null_mut(),
        };
        let new = s.function::<unsafe extern "C" fn(*mut P, *const c_char, i32) -> i32>(
            "idevice_new_with_options",
        )?;
        let handshake = s.function::<unsafe extern "C" fn(P, *mut P, *const c_char) -> i32>(
            "lockdownd_client_new_with_handshake",
        )?;
        unsafe {
            ok(new(&mut s.device, c(id)?.as_ptr(), 1), "连接指定 USB 设备")?;
            if s.device.is_null() {
                return Err("设备连接为空".into());
            }
            ok(
                handshake(s.device, &mut s.lock, c"AppleToolkit".as_ptr()),
                "建立设备信任会话",
            )?;
        }
        Ok(s)
    }
    fn function<T: Copy>(&self, name: &str) -> Result<T, String> {
        let name = c(name)?;
        unsafe {
            let lib = if name.to_bytes().starts_with(b"plist_") {
                &self.plist
            } else {
                &self.mobile
            };
            lib.get::<T>(name.as_bytes_with_nul())
                .map(|s| *s)
                .map_err(|e| format!("设备组件缺少 {}：{e}", name.to_string_lossy()))
        }
    }
    pub fn verify(&self, d: &Device) -> Result<(), String> {
        let serial = self
            .get(None, Some("SerialNumber"))?
            .as_string()
            .unwrap_or("")
            .to_owned();
        let product = self.get(None, Some("ProductType"))?;
        if serial != d.serial || product.as_string() != Some(d.product.as_str()) {
            return Err("连接的手机已变化，已停止操作；请重新检测设备".into());
        }
        Ok(())
    }
    fn node(&self, value: Value) -> Result<Node<'_>, String> {
        let mut b = Vec::new();
        value.to_writer_xml(&mut b).map_err(|e| e.to_string())?;
        let f = self.function::<unsafe extern "C" fn(*const c_char, u32, *mut P) -> i32>(
            "plist_from_xml",
        )?;
        let mut p = ptr::null_mut();
        unsafe {
            ok(
                f(b.as_ptr().cast(), b.len() as u32, &mut p),
                "构造 plist 消息",
            )?;
        }
        if p.is_null() {
            return Err("无法构造设备消息".into());
        }
        Ok(Node { p, s: self })
    }
    fn value(&self, p: P) -> Result<Value, String> {
        if p.is_null() {
            return Err("设备未返回数据".into());
        }
        let n = Node { p, s: self };
        let f = self.function::<unsafe extern "C" fn(P, *mut *mut c_char, *mut u32) -> i32>(
            "plist_to_xml",
        )?;
        let free = self.function::<unsafe extern "C" fn(P)>("plist_mem_free")?;
        let mut data = ptr::null_mut();
        let mut len = 0;
        unsafe {
            ok(f(n.p, &mut data, &mut len), "编码 plist 数据")?;
        }
        if data.is_null() {
            return Err("设备返回空消息".into());
        }
        let result = unsafe {
            Value::from_reader_xml(std::slice::from_raw_parts(data.cast::<u8>(), len as usize))
                .map_err(|e| e.to_string())
        };
        unsafe { free(data.cast()) };
        result
    }
    fn get(&self, domain: Option<&str>, key: Option<&str>) -> Result<Value, String> {
        let d = domain.map(c).transpose()?;
        let k = key.map(c).transpose()?;
        let f = self
            .function::<unsafe extern "C" fn(P, *const c_char, *const c_char, *mut P) -> i32>(
                "lockdownd_get_value",
            )?;
        let mut p = ptr::null_mut();
        unsafe {
            ok(
                f(
                    self.lock,
                    d.as_ref().map_or(ptr::null(), |s| s.as_ptr()),
                    k.as_ref().map_or(ptr::null(), |s| s.as_ptr()),
                    &mut p,
                ),
                "读取设备数据",
            )?
        }
        self.value(p)
    }
    pub fn ecid(&self) -> Result<u64, String> {
        let v = self.get(None, Some("UniqueChipID"))?;
        v.as_unsigned_integer()
            .or_else(|| {
                v.as_string().and_then(|s| {
                    if let Some(s) = s.strip_prefix("0x") {
                        u64::from_str_radix(s, 16).ok()
                    } else {
                        s.parse().ok()
                    }
                })
            })
            .filter(|n| *n != 0)
            .ok_or("无法读取此设备的 ECID，拒绝选择默认刷机目标".into())
    }
    fn service(&self, prefix: &str) -> Result<Client, String> {
        let start = self.function::<unsafe extern "C" fn(P, *mut P, *const c_char) -> i32>(
            &format!("{prefix}_client_start_service"),
        )?;
        let free = self.function::<Free>(&format!("{prefix}_client_free"))?;
        let mut p = ptr::null_mut();
        unsafe {
            ok(
                start(self.device, &mut p, c"AppleToolkit".as_ptr()),
                &format!("打开 {prefix} 服务"),
            )?
        }
        if p.is_null() {
            return Err("设备服务为空".into());
        }
        Ok(Client { p, free })
    }
    fn service_named(&self, name: &str, prefix: &str) -> Result<Client, String> {
        let start = self.function::<unsafe extern "C" fn(P, *const c_char, *mut P) -> i32>(
            "lockdownd_start_service",
        )?;
        let descriptor_free = self.function::<Free>("lockdownd_service_descriptor_free")?;
        let new = self.function::<unsafe extern "C" fn(P, P, *mut P) -> i32>(&format!(
            "{prefix}_client_new"
        ))?;
        let free = self.function::<Free>(&format!("{prefix}_client_free"))?;
        let mut service = ptr::null_mut();
        unsafe {
            ok(
                start(self.lock, c(name)?.as_ptr(), &mut service),
                "开启手机服务",
            )?
        };
        let descriptor = Client {
            p: service,
            free: descriptor_free,
        };
        let mut p = ptr::null_mut();
        unsafe { ok(new(self.device, descriptor.p, &mut p), "连接手机服务")? };
        Ok(Client { p, free })
    }
    fn save(&self, path: &str, value: &Value) -> Result<(), String> {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| format!("无法保存（不会覆盖已有文件）：{e}"))?;
        value.to_writer_xml(file).map_err(|e| e.to_string())
    }
    fn erase(&self) -> Result<String, String> {
        let client = self.service("mobilebackup2")?;
        let f = self.function::<unsafe extern "C" fn(P, *const c_char, P) -> i32>(
            "mobilebackup2_send_message",
        )?;
        unsafe {
            ok(
                f(client.p, c"EraseDevice".as_ptr(), ptr::null_mut()),
                "提交抹除请求",
            )?
        };
        Ok("抹除请求已提交。等待手机重新启动；此消息不等于已确认擦除完成，也没有解除 Apple ID 锁。".into())
    }
    pub fn execute(&self, t: &Task) -> Result<String, String> {
        match t.id.as_str() {
            "erase" => self.erase(),
            "deactivate" => {
                let client = self.service("mobileactivation")?;
                let f = self.function::<Free>("mobileactivation_deactivate")?;
                unsafe { ok(f(client.p), "反激活设备")? };
                Ok("设备已接受反激活请求。请在手机上核对状态；这不会解除激活锁。".into())
            }
            "privacy" => {
                let afc = self.service("afc")?;
                self.fill(&afc, t)?;
                drop(afc);
                if cancelled(t) {
                    return Err("已取消，未提交抹除请求".into());
                }
                self.verify(&t.device)?;
                self.erase()
            }
            "reboot" | "shutdown" => {
                let cl = self.service("diagnostics_relay")?;
                let name = if t.id == "reboot" {
                    "diagnostics_relay_restart"
                } else {
                    "diagnostics_relay_shutdown"
                };
                let f = self.function::<unsafe extern "C" fn(P, i32) -> i32>(name)?;
                unsafe { ok(f(cl.p, 4), "提交电源操作")? };
                Ok("手机已接受电源操作请求，请观察手机状态。".into())
            }
            "recovery" => {
                let f =
                    self.function::<unsafe extern "C" fn(P) -> i32>("lockdownd_enter_recovery")?;
                unsafe { ok(f(self.lock), "进入恢复模式")? };
                Ok("恢复模式请求已提交。设备断开正常模式连接属于预期行为。".into())
            }
            "voiceover" | "zoom" | "invert" | "assistive" | "wifi" => {
                let (domain, key) = match t.id.as_str() {
                    "voiceover" => ("com.apple.Accessibility", "VoiceOverTouchEnabledByiTunes"),
                    "zoom" => ("com.apple.Accessibility", "ZoomTouchEnabledByiTunes"),
                    "invert" => ("com.apple.Accessibility", "InvertDisplayEnabledByiTunes"),
                    "assistive" => ("com.apple.Accessibility", "AssistiveTouchEnabledByiTunes"),
                    _ => (
                        "com.apple.mobile.wireless_lockdown",
                        "EnableWifiConnections",
                    ),
                };
                let n = self.node(Value::Boolean(t.enabled))?;
                let f = self
                    .function::<unsafe extern "C" fn(P, *const c_char, *const c_char, P) -> i32>(
                        "lockdownd_set_value",
                    )?;
                unsafe {
                    ok(
                        f(self.lock, c(domain)?.as_ptr(), c(key)?.as_ptr(), n.p),
                        "修改设备设置",
                    )?
                };
                Ok("设备已接受设置，请在手机上核对效果。".into())
            }
            "syscfg" => {
                let value = self.get(Some("com.apple.mobile.internal"), Some("SysCfgData"))?;
                self.save(&t.output, &value)?;
                Ok(format!("已保存设备提供的配置：{}", t.output))
            }
            "device-monitor" => self.monitor(),
            "screenshot" | "screen" => self.screens(t),
            "syslog" => self.logs(),
            "apps" | "uninstall" | "install" | "ipcc" => self.apps(t),
            "profiles" | "profile-install" | "profile-remove" => self.profiles(t),
            "files" | "file-upload" | "file-download" | "file-delete" | "crashes"
            | "crash-export" | "crash-remove" | "crash-download" => self.files(t),
            "ssh" => self.ssh(),
            "driver-check" => {
                let out = std::process::Command::new("sc.exe")
                    .args(["query", "Apple Mobile Device Service"])
                    .output()
                    .map_err(|e| e.to_string())?;
                println!("{}", String::from_utf8_lossy(&out.stdout));
                if out.status.success() {
                    Ok("Apple 驱动服务状态见上方输出。".into())
                } else {
                    Err("未找到或无法查询 Apple 移动设备服务；请安装 Apple 官方设备驱动。".into())
                }
            }
            _ => Err("这个项目尚未接通执行后端".into()),
        }
    }
    fn monitor(&self) -> Result<String, String> {
        loop {
            let mut sample = MetricSample {
                read_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| e.to_string())?
                    .as_secs(),
                values: BTreeMap::new(),
                errors: vec![],
            };
            for (label, domain, key, percent) in [
                (
                    "电量",
                    "com.apple.mobile.battery",
                    "BatteryCurrentCapacity",
                    true,
                ),
                (
                    "充电状态",
                    "com.apple.mobile.battery",
                    "BatteryIsCharging",
                    false,
                ),
                (
                    "连接外部电源",
                    "com.apple.mobile.battery",
                    "ExternalConnected",
                    false,
                ),
                (
                    "总存储空间（字节）",
                    "com.apple.disk_usage",
                    "TotalDiskCapacity",
                    false,
                ),
                (
                    "可用数据空间（字节）",
                    "com.apple.disk_usage",
                    "AmountDataAvailable",
                    false,
                ),
            ] {
                match self.get(Some(domain), Some(key)) {
                    Ok(v) => {
                        if let Some(s) = crate::inspection::text(&v) {
                            sample
                                .values
                                .insert(label.into(), if percent { format!("{s}%") } else { s });
                        } else {
                            sample.errors.push(format!("{label}：设备未提供标量值"));
                        }
                    }
                    Err(e) => sample.errors.push(format!("{label}：{e}")),
                }
            }
            if sample.values.is_empty() {
                return Err(format!(
                    "设备没有开放状态读数：{}",
                    sample.errors.join("；")
                ));
            }
            println!(
                "[METRICS] {}",
                serde_json::to_string(&sample).map_err(|e| e.to_string())?
            );
            std::io::stdout().flush().map_err(|e| e.to_string())?;
            std::thread::sleep(Duration::from_secs(2));
        }
    }
    fn screenshot(&self, client: &Client) -> Result<Vec<u8>, String> {
        let f = self.function::<unsafe extern "C" fn(P, *mut *mut c_char, *mut u64) -> i32>(
            "screenshotr_take_screenshot",
        )?;
        let free = self.function::<unsafe extern "C" fn(P)>("idevice_mem_free")?;
        let mut data = ptr::null_mut();
        let mut len = 0;
        unsafe { ok(f(client.p, &mut data, &mut len), "读取手机截图")? };
        if data.is_null() || len > 64 * 1024 * 1024 {
            if !data.is_null() {
                unsafe { free(data.cast()) }
            }
            return Err("截图为空或超过大小限制".into());
        }
        let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len as usize).to_vec() };
        unsafe { free(data.cast()) };
        Ok(bytes)
    }
    fn screens(&self, t: &Task) -> Result<String, String> {
        let cl = self.service("screenshotr")?;
        if t.id == "screenshot" {
            let bytes = self.screenshot(&cl)?;
            let image = image::load_from_memory(&bytes)
                .map_err(|e| format!("设备图像格式暂不支持：{e}"))?;
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&t.output)
                .map_err(|e| format!("无法保存（不会覆盖已有文件）：{e}"))?;
            image
                .write_to(&mut file, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            return Ok(format!("截图已保存：{}", t.output));
        }
        let folder = crate::mdm::default_workspace().join("preview");
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let path = folder.join(format!("screen-{}.png", std::process::id()));
        loop {
            let bytes = self.screenshot(&cl)?;
            let image = image::load_from_memory(&bytes).map_err(|e| e.to_string())?;
            let tmp = path.with_extension("tmp");
            image
                .save_with_format(&tmp, image::ImageFormat::Png)
                .map_err(|e| e.to_string())?;
            if path.exists() {
                std::fs::remove_file(&path).map_err(|e| e.to_string())?
            }
            std::fs::rename(tmp, &path).map_err(|e| e.to_string())?;
            println!(
                "[FRAME] {}",
                serde_json::to_string(&path).map_err(|e| e.to_string())?
            );
            std::thread::sleep(Duration::from_millis(700));
        }
    }
    fn logs(&self) -> Result<String, String> {
        let cl = self.service("syslog_relay")?;
        let f = self.function::<unsafe extern "C" fn(P, *mut c_char, u32, *mut u32, u32) -> i32>(
            "syslog_relay_receive_with_timeout",
        )?;
        let mut b = [0; 4096];
        println!("[STEP] 正在读取手机日志，点击停止结束。");
        loop {
            let mut n = 0;
            let code = unsafe { f(cl.p, b.as_mut_ptr().cast(), b.len() as u32, &mut n, 1000) };
            if n > 0 {
                std::io::stdout()
                    .write_all(&b[..n as usize])
                    .map_err(|e| e.to_string())?;
                std::io::stdout().flush().map_err(|e| e.to_string())?;
            }
            if code != 0 && code != -4 && code != -5 {
                return Err(format!("日志连接已结束（{code}）"));
            }
        }
    }
    fn profile_request(&self, request: Value) -> Result<Value, String> {
        let cl = self.service_named("com.apple.mobile.MCInstall", "property_list_service")?;
        let n = self.node(request)?;
        let send = self.function::<unsafe extern "C" fn(P, P) -> i32>(
            "property_list_service_send_xml_plist",
        )?;
        let recv = self.function::<unsafe extern "C" fn(P, *mut P, u32) -> i32>(
            "property_list_service_receive_plist_with_timeout",
        )?;
        let mut p = ptr::null_mut();
        unsafe {
            ok(send(cl.p, n.p), "发送描述文件请求")?;
            ok(recv(cl.p, &mut p, 10000), "读取描述文件响应")?
        };
        let value = self.value(p)?;
        let status = value
            .as_dictionary()
            .and_then(|d| d.get("Status"))
            .and_then(Value::as_string)
            .unwrap_or("");
        if status != "Acknowledged" {
            return Err(format!("设备没有确认请求：{}", xml_text(&value)));
        }
        Ok(value)
    }
    fn profiles(&self, t: &Task) -> Result<String, String> {
        let mut d = Dictionary::new();
        d.insert(
            "RequestType".into(),
            Value::String(
                match t.id.as_str() {
                    "profiles" => "GetProfileList",
                    "profile-install" => "InstallProfile",
                    _ => "RemoveProfile",
                }
                .into(),
            ),
        );
        if t.id == "profile-install" {
            let b = std::fs::read(&t.input).map_err(|e| e.to_string())?;
            if b.len() > 16 * 1024 * 1024 {
                return Err("描述文件过大".into());
            }
            d.insert("Payload".into(), Value::Data(b));
        }
        if t.id == "profile-remove" {
            d.insert("ProfileIdentifier".into(), Value::String(t.input.clone()));
        }
        let value = self.profile_request(Value::Dictionary(d))?;
        if t.id == "profiles" {
            let metadata = value
                .as_dictionary()
                .and_then(|d| d.get("ProfileMetadata"))
                .and_then(Value::as_dictionary);
            let mut rows = vec![];
            if let Some(metadata) = metadata {
                for (id, entry) in metadata {
                    let text = |key: &str| {
                        entry
                            .as_dictionary()
                            .and_then(|d| d.get(key))
                            .and_then(Value::as_string)
                            .unwrap_or("")
                            .to_owned()
                    };
                    rows.push(ProfileEntry {
                        name: text("PayloadDisplayName"),
                        identifier: id.clone(),
                        removable: entry
                            .as_dictionary()
                            .and_then(|d| d.get("PayloadRemovalDisallowed"))
                            .and_then(Value::as_boolean)
                            .map(|v| !v),
                    });
                }
            }
            println!(
                "[PROFILES] {}",
                serde_json::to_string(&rows).map_err(|e| e.to_string())?
            );
            return Ok(format!("读取到 {} 个描述文件。", rows.len()));
        }
        println!("{}", xml_text(&value));
        Ok(if t.id == "profile-install" {
            "文件已发送，请在手机设置中核对并完成安装。"
        } else if t.id == "profiles" {
            "描述文件列表读取完成。"
        } else {
            "设备已确认移除请求。"
        }
        .into())
    }
    fn apps(&self, t: &Task) -> Result<String, String> {
        let cl = self.service("instproxy")?;
        if t.id == "apps" {
            let f =
                self.function::<unsafe extern "C" fn(P, P, *mut P) -> i32>("instproxy_browse")?;
            let mut p = ptr::null_mut();
            let mut options = Dictionary::new();
            options.insert(
                "ReturnAttributes".into(),
                Value::Array(
                    [
                        "CFBundleIdentifier",
                        "CFBundleDisplayName",
                        "CFBundleName",
                        "CFBundleShortVersionString",
                    ]
                    .iter()
                    .map(|s| Value::String((*s).into()))
                    .collect(),
                ),
            );
            let options = self.node(Value::Dictionary(options))?;
            unsafe { ok(f(cl.p, options.p, &mut p), "读取应用列表")? };
            let value = self.value(p)?;
            let rows: Vec<_> = value
                .as_array()
                .ok_or("应用列表格式不正确")?
                .iter()
                .filter_map(|v| {
                    let d = v.as_dictionary()?;
                    let text = |key: &str| {
                        d.get(key)
                            .and_then(Value::as_string)
                            .unwrap_or("")
                            .to_owned()
                    };
                    let id = text("CFBundleIdentifier");
                    if id.is_empty() {
                        return None;
                    }
                    let name = [
                        text("CFBundleDisplayName"),
                        text("CFBundleName"),
                        id.clone(),
                    ]
                    .into_iter()
                    .find(|s| !s.is_empty())
                    .unwrap_or_default();
                    Some(AppEntry {
                        name,
                        identifier: id,
                        version: text("CFBundleShortVersionString"),
                    })
                })
                .collect();
            println!(
                "[APPS] {}",
                serde_json::to_string(&rows).map_err(|e| e.to_string())?
            );
            return Ok(format!(
                "已读取 {} 个应用。可在列表中选择要卸载的应用。",
                rows.len()
            ));
        }
        let mut options = Dictionary::new();
        let input = if t.id == "uninstall" {
            t.input.clone()
        } else {
            let afc = self.service("afc")?;
            let filename = Path::new(&t.input)
                .file_name()
                .and_then(|s| s.to_str())
                .ok_or("文件名无效")?;
            let _ = self.mkdir(&afc, "/PublicStaging");
            let remote = format!(
                "/PublicStaging/AppleToolkit-{}-{filename}",
                std::process::id()
            );
            self.upload(&afc, Path::new(&t.input), &remote)?;
            if t.id == "ipcc" {
                options.insert("PackageType".into(), Value::String("CarrierBundle".into()));
            }
            remote
        };
        let n = self.node(Value::Dictionary(options))?;
        // A null callback uses the library's synchronous status processing path.
        let name = if t.id == "uninstall" {
            "instproxy_uninstall"
        } else {
            "instproxy_install"
        };
        let f = self.function::<unsafe extern "C" fn(
            P,
            *const c_char,
            P,
            Option<unsafe extern "C" fn(P, P, P)>,
            P,
        ) -> i32>(name)?;
        let result = unsafe {
            ok(
                f(cl.p, c(&input)?.as_ptr(), n.p, None, ptr::null_mut()),
                "应用安装/卸载",
            )
        };
        if t.id != "uninstall" {
            if let Ok(afc) = self.service("afc") {
                let _ = self.remove(&afc, &input);
            }
        }
        result?;
        Ok("安装代理任务已结束，请在手机上确认应用或运营商配置。".into())
    }
    fn dictionary(
        &self,
        cl: &Client,
        path: Option<&str>,
        method: &str,
    ) -> Result<Vec<String>, String> {
        let mut p = ptr::null_mut::<*mut c_char>();
        let code = if let Some(path) = path {
            let f = self
                .function::<unsafe extern "C" fn(P, *const c_char, *mut *mut *mut c_char) -> i32>(
                    method,
                )?;
            unsafe { f(cl.p, c(path)?.as_ptr(), &mut p) }
        } else {
            let f =
                self.function::<unsafe extern "C" fn(P, *mut *mut *mut c_char) -> i32>(method)?;
            unsafe { f(cl.p, &mut p) }
        };
        ok(code, "读取手机文件信息")?;
        let free =
            self.function::<unsafe extern "C" fn(*mut *mut c_char) -> i32>("afc_dictionary_free")?;
        let mut items = vec![];
        if !p.is_null() {
            unsafe {
                for i in 0..100000 {
                    let s = *p.add(i);
                    if s.is_null() {
                        break;
                    }
                    items.push(CStr::from_ptr(s).to_string_lossy().into_owned())
                }
                free(p);
            }
        }
        Ok(items)
    }
    fn info(&self, cl: &Client, path: &str) -> Result<BTreeMap<String, String>, String> {
        let v = self.dictionary(cl, Some(path), "afc_get_file_info")?;
        Ok(v.as_chunks::<2>()
            .0
            .iter()
            .map(|v| (v[0].clone(), v[1].clone()))
            .collect())
    }
    fn list(&self, cl: &Client, path: &str) -> Result<Vec<Entry>, String> {
        let mut entries = vec![];
        for name in self.dictionary(cl, Some(path), "afc_read_directory")? {
            if name == "." || name == ".." {
                continue;
            }
            let p = format!("{}/{name}", path.trim_end_matches('/'));
            let meta = self.info(cl, &p)?;
            entries.push(Entry {
                name,
                path: p,
                directory: meta.get("st_ifmt").is_some_and(|s| s == "S_IFDIR"),
                size: meta
                    .get("st_size")
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0),
            });
        }
        Ok(entries)
    }
    fn mkdir(&self, cl: &Client, path: &str) -> Result<(), String> {
        let f =
            self.function::<unsafe extern "C" fn(P, *const c_char) -> i32>("afc_make_directory")?;
        unsafe { ok(f(cl.p, c(path)?.as_ptr()), "创建共享目录") }
    }
    fn remove(&self, cl: &Client, path: &str) -> Result<(), String> {
        if path.trim_matches('/').is_empty() {
            return Err("不能删除共享根目录".into());
        }
        let f =
            self.function::<unsafe extern "C" fn(P, *const c_char) -> i32>("afc_remove_path")?;
        unsafe { ok(f(cl.p, c(path)?.as_ptr()), "删除指定文件") }
    }
    fn file(&self, cl: &Client, path: &str, mode: u32) -> Result<RemoteFile, String> {
        let f = self.function::<unsafe extern "C" fn(P, *const c_char, u32, *mut u64) -> i32>(
            "afc_file_open",
        )?;
        let close = self.function::<unsafe extern "C" fn(P, u64) -> i32>("afc_file_close")?;
        let mut handle = 0;
        unsafe {
            ok(
                f(cl.p, c(path)?.as_ptr(), mode, &mut handle),
                "打开手机文件",
            )?
        }
        Ok(RemoteFile {
            client: cl.p,
            handle,
            close,
        })
    }
    fn upload(&self, cl: &Client, local: &Path, remote: &str) -> Result<(), String> {
        let mut input = std::fs::File::open(local).map_err(|e| e.to_string())?;
        let target = self.file(cl, remote, 3)?;
        let write = self
            .function::<unsafe extern "C" fn(P, u64, *const c_char, u32, *mut u32) -> i32>(
                "afc_file_write",
            )?;
        let mut buf = vec![0; 1024 * 1024];
        let mut total = 0;
        loop {
            let n = input.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            let mut offset = 0;
            while offset < n {
                let mut done = 0;
                unsafe {
                    ok(
                        write(
                            cl.p,
                            target.handle,
                            buf[offset..n].as_ptr().cast(),
                            (n - offset) as u32,
                            &mut done,
                        ),
                        "上传文件",
                    )?
                }
                if done == 0 || done as usize > n - offset {
                    return Err("上传进度无效".into());
                }
                offset += done as usize;
            }
            total += n;
            println!("已上传 {} MiB", total / (1024 * 1024));
        }
        Ok(())
    }
    fn download(&self, cl: &Client, remote: &str, local: &Path) -> Result<(), String> {
        let info = self.info(cl, remote)?;
        if info.get("st_ifmt").is_some_and(|s| s != "S_IFREG") {
            return Err("只支持下载普通文件，不跟随符号链接".into());
        }
        let expected = info
            .get("st_size")
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or("手机没有提供文件长度")?;
        let target = self.file(cl, remote, 1)?;
        let read = self
            .function::<unsafe extern "C" fn(P, u64, *mut c_char, u32, *mut u32) -> i32>(
                "afc_file_read",
            )?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(local)
            .map_err(|e| format!("无法保存（不会覆盖已有文件）：{e}"))?;
        let mut b = vec![0; 1024 * 1024];
        let mut total = 0u64;
        while total < expected {
            let mut n = 0;
            unsafe {
                ok(
                    read(
                        cl.p,
                        target.handle,
                        b.as_mut_ptr().cast(),
                        b.len() as u32,
                        &mut n,
                    ),
                    "下载文件",
                )?
            }
            if n == 0 || n as usize > b.len() {
                return Err("下载意外中断，已保留未完成文件".into());
            }
            output
                .write_all(&b[..n as usize])
                .map_err(|e| e.to_string())?;
            total += n as u64;
        }
        output.sync_all().map_err(|e| e.to_string())?;
        Ok(())
    }
    fn files(&self, t: &Task) -> Result<String, String> {
        let crashes = t.id.starts_with("crash");
        let cl = if crashes {
            self.service_named("com.apple.crashreportcopymobile", "afc")?
        } else {
            self.service("afc")?
        };
        let path = if t.input.trim().is_empty() {
            "/"
        } else {
            &t.input
        };
        match t.id.as_str() {
            "files" | "crashes" => {
                let entries = self.list(&cl, path)?;
                for e in &entries {
                    println!(
                        "{} {} ({} 字节)",
                        if e.directory { "目录" } else { "文件" },
                        e.path,
                        e.size
                    )
                }
                println!(
                    "[FILES] {}",
                    serde_json::to_string(&entries).map_err(|e| e.to_string())?
                );
            }
            "file-upload" => {
                let _ = self.mkdir(&cl, "/AppleToolkit");
                let name = Path::new(path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .ok_or("文件名无效")?;
                let remote = format!("/AppleToolkit/{name}");
                if self.info(&cl, &remote).is_ok() {
                    return Err("手机中已有同名文件，请重命名后再上传".into());
                }
                self.upload(&cl, Path::new(path), &remote)?;
            }
            "file-download" | "crash-download" => self.download(&cl, path, Path::new(&t.output))?,
            "file-delete" | "crash-remove" => self.remove(&cl, path)?,
            "crash-export" => {
                std::fs::create_dir_all(&t.input).map_err(|e| e.to_string())?;
                let mut stack = vec![("/".to_owned(), PathBuf::from(&t.input), 0)];
                let mut count = 0;
                while let Some((remote, local, depth)) = stack.pop() {
                    if depth > 16 {
                        return Err("崩溃日志目录层级过深".into());
                    }
                    for e in self.list(&cl, &remote)? {
                        if e.name.contains(['/', '\\']) || e.name == ".." {
                            return Err("设备返回非法文件名".into());
                        }
                        let dest = local.join(&e.name);
                        if e.directory {
                            std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
                            stack.push((e.path, dest, depth + 1))
                        } else {
                            self.download(&cl, &e.path, &dest)?;
                            count += 1;
                            if count > 10000 {
                                return Err("日志数量超过上限".into());
                            }
                            println!("已导出 {}", dest.display());
                        }
                    }
                }
            }
            _ => (),
        };
        Ok("文件任务已完成。".into())
    }
    fn fill(&self, cl: &Client, t: &Task) -> Result<(), String> {
        let v = self.dictionary(cl, None, "afc_get_device_info")?;
        let meta: BTreeMap<_, _> = v
            .as_chunks::<2>()
            .0
            .iter()
            .map(|v| (v[0].as_str(), v[1].as_str()))
            .collect();
        let free = meta
            .get("FSFreeBytes")
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or("无法确定空闲空间，拒绝开始填充")?;
        let target = free
            .checked_sub(512 * 1024 * 1024)
            .filter(|n| *n > 0)
            .ok_or("可用空间小于 512 MiB，不进行填充")?;
        let dir = format!("/AppleToolkit-fill-{}", std::process::id());
        self.mkdir(cl, &dir)?;
        let mut paths = vec![];
        let write = self
            .function::<unsafe extern "C" fn(P, u64, *const c_char, u32, *mut u32) -> i32>(
                "afc_file_write",
            )?;
        let b = vec![255u8; 1024 * 1024];
        let result: Result<(), String> = (|| {
            let mut total = 0u64;
            let mut index = 0;
            while total < target {
                let path = format!("{dir}/{index}.bin");
                paths.push(path.clone());
                let file = self.file(cl, &path, 3)?;
                let max = (target - total).min(1024 * 1024 * 1024);
                let mut written = 0;
                while written < max {
                    let size = (max - written).min(b.len() as u64) as u32;
                    if cancelled(t) {
                        return Err("已取消填充，已进入临时文件清理流程".into());
                    }
                    let mut n = 0;
                    unsafe {
                        ok(
                            write(cl.p, file.handle, b.as_ptr().cast(), size, &mut n),
                            "写入填充数据",
                        )?
                    }
                    if n == 0 || n > size {
                        return Err("设备返回无效填充进度".into());
                    }
                    written += n as u64;
                    total += n as u64;
                    println!("[STEP] 正在填充空闲空间：{}%", total * 100 / target);
                }
                index += 1;
            }
            Ok(())
        })();
        let mut cleanup_failed = false;
        for path in paths {
            if self.remove(cl, &path).is_err() {
                cleanup_failed = true;
            }
        }
        if self.remove(cl, &dir).is_err() {
            cleanup_failed = true;
        }
        if cleanup_failed {
            return Err(format!(
                "填充文件没有全部清理，已停止抹除步骤；临时目录 {dir}，原始状态：{result:?}"
            ));
        }
        result?;
        println!("填充与临时文件清理结束。正在提交抹除请求。");
        Ok(())
    }
    fn ssh(&self) -> Result<String, String> {
        use std::net::TcpListener;
        let listener = TcpListener::bind(("127.0.0.1", 2222))
            .map_err(|e| format!("无法监听本机 2222 端口：{e}"))?;
        let connect =
            self.function::<unsafe extern "C" fn(P, u16, *mut P) -> i32>("idevice_connect")?;
        let disconnect = self.function::<Free>("idevice_disconnect")?;
        let send = self.function::<unsafe extern "C" fn(P, *const c_char, u32, *mut u32) -> i32>(
            "idevice_connection_send",
        )?;
        let recv = self
            .function::<unsafe extern "C" fn(P, *mut c_char, u32, *mut u32, u32) -> i32>(
                "idevice_connection_receive_timeout",
            )?;
        println!("[STEP] SSH 通道已监听 127.0.0.1:2222。点击停止关闭；手机必须已有 SSH 服务。");
        for incoming in listener.incoming() {
            let mut stream = incoming.map_err(|e| e.to_string())?;
            stream
                .set_read_timeout(Some(Duration::from_millis(20)))
                .map_err(|e| e.to_string())?;
            stream
                .set_write_timeout(Some(Duration::from_secs(5)))
                .map_err(|e| e.to_string())?;
            let mut p = ptr::null_mut();
            if let Err(e) =
                unsafe { ok(connect(self.device, 22, &mut p), "连接手机 SSH 端口") }
            {
                println!("{e}");
                continue;
            }
            let connection = Client {
                p,
                free: disconnect,
            };
            let mut b = [0; 32768];
            loop {
                match stream.read(&mut b) {
                    Ok(0) => break,
                    Ok(n) => {
                        let mut offset = 0;
                        while offset < n {
                            let mut sent = 0;
                            unsafe {
                                ok(
                                    send(
                                        connection.p,
                                        b[offset..n].as_ptr().cast(),
                                        (n - offset) as u32,
                                        &mut sent,
                                    ),
                                    "SSH 转发",
                                )?
                            }
                            if sent == 0 {
                                break;
                            }
                            offset += sent as usize;
                        }
                    }
                    Err(e)
                        if [std::io::ErrorKind::WouldBlock, std::io::ErrorKind::TimedOut]
                            .contains(&e.kind()) => {}
                    Err(_) => break,
                };
                let mut n = 0;
                let code = unsafe {
                    recv(
                        connection.p,
                        b.as_mut_ptr().cast(),
                        b.len() as u32,
                        &mut n,
                        20,
                    )
                };
                if n > 0 {
                    stream
                        .write_all(&b[..n as usize])
                        .map_err(|e| e.to_string())?
                }
                if code != 0 && code != -7 {
                    break;
                }
            }
        }
        Ok("SSH 转发已停止".into())
    }
}
struct RemoteFile {
    client: P,
    handle: u64,
    close: unsafe extern "C" fn(P, u64) -> i32,
}
impl Drop for RemoteFile {
    fn drop(&mut self) {
        unsafe {
            (self.close)(self.client, self.handle);
        }
    }
}
fn xml_text(value: &Value) -> String {
    let mut b = vec![];
    let _ = value.to_writer_xml(&mut b);
    String::from_utf8_lossy(&b).into_owned()
}

fn cancelled(t: &Task) -> bool {
    !t.stop_file.is_empty() && Path::new(&t.stop_file).exists()
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    #[test]
    fn bundled_plist_abi_roundtrips_messages_without_a_device() {
        let root = crate::bundled::ensure_at(
            &std::env::temp_dir().join(format!("atk-04-plist-{}", std::process::id())),
        )
        .unwrap();
        let plist = load(&root.join("Engine/libplist.dll")).unwrap();
        let mobile = load(&root.join("Engine/libimobiledevice.dll")).unwrap();
        let session = Session {
            mobile,
            plist,
            device: ptr::null_mut(),
            lock: ptr::null_mut(),
        };
        let mut data = Dictionary::new();
        data.insert("enabled".into(), Value::Boolean(true));
        data.insert("name".into(), Value::String("中文 & <test>".into()));
        let expected = Value::Dictionary(data);
        let node = session.node(expected.clone()).unwrap();
        let p = node.p;
        std::mem::forget(node);
        assert_eq!(session.value(p).unwrap(), expected);
    }
    #[test]
    fn bundled_backend_exports_match_the_adapter() {
        let root = crate::bundled::ensure_at(
            &std::env::temp_dir().join(format!("atk-04-abi-{}", std::process::id())),
        )
        .unwrap();
        let _plist = load(&root.join("Engine/libplist.dll")).unwrap();
        let library = load(&root.join("Engine/libimobiledevice.dll")).unwrap();
        for name in [
            "idevice_new_with_options",
            "lockdownd_client_new_with_handshake",
            "lockdownd_get_value",
            "lockdownd_set_value",
            "diagnostics_relay_client_start_service",
            "diagnostics_relay_restart",
            "diagnostics_relay_shutdown",
            "mobilebackup2_send_message",
            "mobileactivation_deactivate",
            "afc_client_start_service",
            "afc_file_open",
            "afc_file_read",
            "afc_file_write",
            "afc_dictionary_free",
            "instproxy_client_start_service",
            "instproxy_browse",
            "instproxy_install",
            "property_list_service_client_new",
            "property_list_service_send_xml_plist",
            "property_list_service_receive_plist_with_timeout",
            "syslog_relay_receive_with_timeout",
            "screenshotr_take_screenshot",
            "idevice_mem_free",
            "idevice_connect",
            "idevice_connection_receive_timeout",
        ] {
            let symbol = c(name).unwrap();
            unsafe {
                assert!(
                    library
                        .get::<unsafe extern "C" fn()>(symbol.as_bytes_with_nul())
                        .is_ok(),
                    "missing {name}"
                );
            }
        }
    }
}

pub fn driver_status() -> Result<String, String> {
    if !cfg!(windows) {
        return Ok("macOS 不使用 Windows Apple Mobile Device Service。请检查 Finder 是否识别设备及 libimobiledevice 原生组件。".into());
    }
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("C:/Windows"));
    let mut cmd = std::process::Command::new(root.join("System32/sc.exe"));
    cmd.args(["query", "Apple Mobile Device Service"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    let out = cmd.output().map_err(|e| e.to_string())?;
    println!("{}", String::from_utf8_lossy(&out.stdout));
    if out.status.success() {
        Ok("驱动服务查询完成。STATE=4/RUNNING 表示服务正在运行；请同时检查 USB 数据线及设备上的信任提示。".into())
    } else {
        Err("未找到或无法查询 Apple 移动设备服务。请安装/修复 Apple 官方设备驱动。没有修改电脑驱动。".into())
    }
}
