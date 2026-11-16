//! 只读 diagnostics_relay 绑定。这里不加载重启、关机、休眠或恢复函数。
use plist::Value;
use std::path::Path;
pub struct Tree {
    pub source: String,
    pub value: Value,
}
#[cfg(all(test, windows))]
mod tests {
    #[test]
    #[ignore = "requires an explicitly selected, connected device; read-only field inventory"]
    fn actual_hardware_field_inventory() {
        let id = std::env::var("ATK_READONLY_TEST_UDID").expect("ATK_READONLY_TEST_UDID");
        assert!(crate::mdm::valid_udid(&id));
        let root = crate::bundled::ensure_runtime().unwrap();
        let (trees, notes) = super::read(&root, &id);
        fn walk(value: &plist::Value, path: &str, depth: usize) {
            if depth > 64 {
                return;
            }
            match value {
                plist::Value::Dictionary(d) => {
                    for (k, v) in d {
                        let next = format!("{path}/{k}");
                        let lower = k.to_lowercase();
                        if [
                            "serial",
                            "camera",
                            "manufact",
                            "health",
                            "capacity",
                            "cycle",
                            "battery",
                            "panel",
                            "display",
                            "ioregistryentryname",
                        ]
                        .iter()
                        .any(|needle| lower.contains(needle))
                        {
                            if let Some(t) = crate::inspection::text(v) {
                                println!("{next} = {}", t.chars().take(300).collect::<String>());
                            }
                        }
                        walk(v, &next, depth + 1);
                    }
                }
                plist::Value::Array(a) => {
                    for (n, v) in a.iter().enumerate() {
                        walk(v, &format!("{path}/{n}"), depth + 1);
                    }
                }
                _ => (),
            }
        }
        for tree in trees {
            walk(&tree.value, &tree.source, 0);
        }
        println!("DIAGNOSTIC_NOTES {notes:?}");
    }
    #[test]
    fn bundled_diagnostics_load_and_reject_a_missing_target() {
        let parent =
            std::env::temp_dir().join(format!("atk-native-diagnostics-{}", std::process::id()));
        let root = crate::bundled::ensure_at(&parent).unwrap();
        let (trees, notes) = super::read(&root, &"0".repeat(40));
        assert!(trees.is_empty());
        assert!(notes.iter().any(|n| n.contains("USB连接失败")), "{notes:?}");
    }
}

#[cfg(not(windows))]
pub fn read(_root: &Path, _udid: &str) -> (Vec<Tree>, Vec<String>) {
    (
        vec![],
        vec!["当前发布包的扩展诊断后端仅支持Windows。".into()],
    )
}

#[cfg(windows)]
pub fn read(root: &Path, udid: &str) -> (Vec<Tree>, Vec<String>) {
    match windows::Session::open(root, udid) {
        Ok(session) => {
            let mut trees = vec![];
            let mut notes = vec![];
            for (kind, key) in [
                ("diagnostics", "GasGauge"),
                ("entry", "AppleSmartBattery"),
                ("plane", "IODeviceTree"),
                ("plane", "IOService"),
                ("gestalt", "Hardware"),
            ] {
                println!("[STEP] 正在读取设备硬件诊断信息…");
                match session.query(kind, key) {
                    Ok(value) => trees.push(Tree {
                        source: format!("diagnostics_relay/{kind}/{key}"),
                        value,
                    }),
                    Err(e) => notes.push(format!("{key}：{e}")),
                }
            }
            (trees, notes)
        }
        Err(e) => (vec![], vec![format!("扩展诊断暂不可用：{e}")]),
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use libloading::Library;
    use std::{
        ffi::{c_char, c_void, CString},
        ptr,
    };
    type Ptr = *mut c_void;
    type Free = unsafe extern "C" fn(Ptr) -> i32;
    type Start = unsafe extern "C" fn(Ptr, *mut Ptr, *const c_char) -> i32;
    type New = unsafe extern "C" fn(*mut Ptr, *const c_char, i32) -> i32;
    type Query = unsafe extern "C" fn(Ptr, *const c_char, *mut Ptr) -> i32;
    type Entry = unsafe extern "C" fn(Ptr, *const c_char, *const c_char, *mut Ptr) -> i32;
    type Xml = unsafe extern "C" fn(Ptr, *mut *mut c_char, *mut u32);
    type PlistFree = unsafe extern "C" fn(Ptr);
    type Gestalt = unsafe extern "C" fn(Ptr, Ptr, *mut Ptr) -> i32;
    pub struct Session {
        _mobile: Library,
        _plist: Library,
        device: Ptr,
        client: Ptr,
        free_device: Free,
        free_client: Free,
        query_diagnostics: Query,
        query_plane: Query,
        query_entry: Entry,
        to_xml: Xml,
        free_plist: PlistFree,
        free_xml: PlistFree,
        query_gestalt: Gestalt,
        new_array: unsafe extern "C" fn() -> Ptr,
        new_string: unsafe extern "C" fn(*const c_char) -> Ptr,
        append_item: unsafe extern "C" fn(Ptr, Ptr),
    }
    fn library(path: &Path) -> Result<Library, String> {
        // 将依赖搜索限定到库自身目录及Windows默认系统目录，避免从任意cwd加载同名库。
        unsafe {
            libloading::os::windows::Library::load_with_flags(path, 0x00000100 | 0x00001000)
                .map(Into::into)
                .map_err(|e| e.to_string())
        }
    }
    unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T, String> {
        library
            .get::<T>(name)
            .map(|s| *s)
            .map_err(|e| e.to_string())
    }
    impl Session {
        pub fn open(root: &Path, udid: &str) -> Result<Self, String> {
            let directory = root.join("Tools/NormalMode");
            let plist = library(&directory.join("libplist-2.0.dll"))?;
            let mobile = library(&directory.join("libimobiledevice-1.0.dll"))?;
            unsafe {
                let new: New = symbol(&mobile, b"idevice_new_with_options\0")?;
                let start: Start = symbol(&mobile, b"diagnostics_relay_client_start_service\0")?;
                let mut s = Self {
                    free_device: symbol(&mobile, b"idevice_free\0")?,
                    free_client: symbol(&mobile, b"diagnostics_relay_client_free\0")?,
                    query_diagnostics: symbol(&mobile, b"diagnostics_relay_request_diagnostics\0")?,
                    query_plane: symbol(&mobile, b"diagnostics_relay_query_ioregistry_plane\0")?,
                    query_entry: symbol(&mobile, b"diagnostics_relay_query_ioregistry_entry\0")?,
                    to_xml: symbol(&plist, b"plist_to_xml\0")?,
                    free_plist: symbol(&plist, b"plist_free\0")?,
                    free_xml: symbol(&plist, b"plist_mem_free\0")?,
                    query_gestalt: symbol(&mobile, b"diagnostics_relay_query_mobilegestalt\0")?,
                    new_array: symbol(&plist, b"plist_new_array\0")?,
                    new_string: symbol(&plist, b"plist_new_string\0")?,
                    append_item: symbol(&plist, b"plist_array_append_item\0")?,
                    _mobile: mobile,
                    _plist: plist,
                    device: ptr::null_mut(),
                    client: ptr::null_mut(),
                };
                let id = CString::new(udid).map_err(|e| e.to_string())?;
                let code = new(&mut s.device, id.as_ptr(), 1);
                if code != 0 || s.device.is_null() {
                    return Err(format!("USB连接失败（{code}）"));
                }
                let code = start(s.device, &mut s.client, c"AppleToolkit".as_ptr());
                if code != 0 || s.client.is_null() {
                    return Err(format!("设备未开放诊断服务（{code}），请解锁并信任此电脑"));
                }
                Ok(s)
            }
        }
        pub fn query(&self, kind: &str, key: &str) -> Result<Value, String> {
            let key = CString::new(key).map_err(|e| e.to_string())?;
            let mut node = ptr::null_mut();
            unsafe {
                let code = match kind {
                    "entry" => {
                        (self.query_entry)(self.client, key.as_ptr(), c"".as_ptr(), &mut node)
                    }
                    "plane" => (self.query_plane)(self.client, key.as_ptr(), &mut node),
                    "gestalt" => {
                        let keys = (self.new_array)();
                        if keys.is_null() {
                            return Err("无法建立硬件查询".into());
                        }
                        for name in [
                            c"RegulatoryModelNumber",
                            c"MLBSerialNumber",
                            c"FrontFacingCameraSerialNumber",
                            c"RearFacingCameraSerialNumber",
                            c"DisplaySerialNumber",
                            c"PanelSerialNumber",
                            c"CoverGlassSerialNumber",
                            c"BatterySerialNumber",
                        ] {
                            let value = (self.new_string)(name.as_ptr());
                            if !value.is_null() {
                                (self.append_item)(keys, value);
                            }
                        }
                        let result = (self.query_gestalt)(self.client, keys, &mut node);
                        (self.free_plist)(keys);
                        result
                    }
                    _ => (self.query_diagnostics)(self.client, key.as_ptr(), &mut node),
                };
                if code != 0 || node.is_null() {
                    if !node.is_null() {
                        (self.free_plist)(node);
                    }
                    return Err(format!("设备未提供数据（{code}）"));
                }
                let mut bytes = ptr::null_mut();
                let mut length = 0u32;
                (self.to_xml)(node, &mut bytes, &mut length);
                let result = if bytes.is_null() || length == 0 {
                    Err("设备返回空数据".into())
                } else if length > 8 * 1024 * 1024 {
                    Err("诊断数据超过读取上限".into())
                } else {
                    Value::from_reader(std::io::Cursor::new(std::slice::from_raw_parts(
                        bytes.cast::<u8>(),
                        length as usize,
                    )))
                    .map_err(|e| e.to_string())
                };
                if !bytes.is_null() {
                    (self.free_xml)(bytes.cast());
                }
                (self.free_plist)(node);
                result
            }
        }
    }
    impl Drop for Session {
        fn drop(&mut self) {
            unsafe {
                if !self.client.is_null() {
                    (self.free_client)(self.client);
                }
                if !self.device.is_null() {
                    (self.free_device)(self.device);
                }
            }
        }
    }
}
