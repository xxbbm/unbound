//! 按当前设备改写备份里的 plist。
//!
//! 对应 `iFineMDM` 的 `Gaval27ProtectionPlist.PatchInfo` / `PatchManifest` /
//! `PatchManifestDictionary`（共 15 个方法，自带手写 bplist 读写器）。
//!
//! # 它属于哪条线（容易搞错，先说清楚）
//!
//! 据 `MDM-FINDINGS.md` 的 CIL 复核：这两个方法由
//! `Gaval27ProtectionBackup.BuildFromArchive` 调用，属于**防护包**逻辑
//! （`/Erase27/` 那条线），**不在** `BackupClient.RequestAsync → MdmWorkflow`
//! 的 MDM 主链上。因此：
//!
//! | 路线 | 是否用本模块 |
//! |---|---|
//! | `Route::LegacyGaval`（主版本 < 27） | **用**：`LegacyMdmBackup.Build` 会把两份 plist 以当前设备信息改写 |
//! | `Route::Mdm27`（主版本 = 27） | **不用**：保留原本地备份字节，不混用防护包改写 |
//!
//! 早期分析把这些改写泛化成 MDM 主流程，依据不足。现代路线若套用本模块，
//! 等于把防护包的假设硬塞进主链，可能让一份本来可用的备份变得不可用。
//!
//! # 为什么不新增文件
//!
//! 向备份里塞 `manifest` 不引用的新文件，还原时会被直接忽略。所以这里只改值。
//!
//! # 三条纪律
//!
//! 1. 只覆盖**已存在**的键；缺键只记录，不凭空创建。
//! 2. 只有 `IMEI` 允许在无值时置空字符串 —— 厂商仅对它做 `String.Empty` 替换。
//!    `DeviceName` 之类无值时保持原样，避免把用户数据清掉。
//! 3. 保持容器格式：`bplist00` 仍是二进制，XML 仍是 XML。`PatchInfo` 用
//!    `bplist00` 做闸门，改成 XML 会失败。

use crate::mdm::Device;
use plist::{Dictionary, Value};
use std::path::Path;

pub const INFO_PLIST_KEYS: [&str; 8] = [
    "Target Identifier",
    "Unique Identifier",
    "Serial Number",
    "Product Type",
    "Product Version",
    "Build Version",
    "Device Name",
    "Display Name",
];

pub const MANIFEST_LOCKDOWN_KEYS: [&str; 7] = [
    "UniqueDeviceID",
    "SerialNumber",
    "ProductType",
    "ProductVersion",
    "BuildVersion",
    "DeviceName",
    "IMEI",
];

/// 逐键改写结果。调用方应把它显示给用户，而不是静默丢弃。
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub patched: Vec<String>,
    pub skipped: Vec<String>,
}

impl Report {
    pub fn summary(&self) -> String {
        format!(
            "改写 {} 项，跳过 {} 项",
            self.patched.len(),
            self.skipped.len()
        )
    }
    pub fn is_complete(&self) -> bool {
        self.skipped.is_empty()
    }
    fn patched(mut self, key: &str) -> Self {
        self.patched.push(key.to_string());
        self
    }
    fn skipped(mut self, key: &str, why: &str) -> Self {
        self.skipped.push(format!("{key}（{why}）"));
        self
    }
}

/// 模板里的占位符。
///
/// 从真实 `GavalLegacyInfo.plist` 里读出来的（内嵌资源，2,917 B）。7 个字段是
/// `__GAVAL_*__` 令牌，只有 `Unique Identifier` / `Target Identifier` 是具体 UDID。
///
/// 意思很明确：这两个 plist 是**逐机填充的模板**，不是可直接用的备份元数据。
pub const TOKENS: &[&str] = &[
    "__GAVAL_Devicename__",
    "__GAVAL_ProductType__",
    "__GAVAL_IOSVersion__",
    "__GAVAL_BuildVersion__",
    "__GAVAL_SerialNumber__",
    "__GAVAL_IMEI__",
];

fn token_value<'a>(token: &str, device: &'a Device) -> Option<&'a str> {
    match token {
        "__GAVAL_Devicename__" => Some(device.name.as_str()),
        "__GAVAL_ProductType__" => Some(device.product.as_str()),
        "__GAVAL_IOSVersion__" => Some(device.version.as_str()),
        "__GAVAL_BuildVersion__" => Some(device.build.as_str()),
        "__GAVAL_SerialNumber__" => Some(device.serial.as_str()),
        "__GAVAL_IMEI__" => Some(device.imei.as_str()),
        _ => None,
    }
}

/// 递归替换任意深度的令牌，返回 (替换次数, 命中的令牌)。
///
/// 为什么整树扫一遍：令牌可能出现在我没列进 `INFO_PLIST_KEYS` /
/// `MANIFEST_LOCKDOWN_KEYS` 的字段里（例如嵌套 `Applications` 段）。只改已知键会
/// 漏掉它们，留下 `__GAVAL_*__` 这种明显不像设备数据的值。
pub fn replace_tokens(value: &mut Value, device: &Device) -> (usize, Vec<String>) {
    fn walk(v: &mut Value, device: &Device, count: &mut usize, seen: &mut Vec<String>) {
        match v {
            Value::String(text) => {
                for token in super::rekey::TOKENS.iter().copied() {
                    if !text.contains(token) {
                        continue;
                    }
                    if let Some(replacement) = super::rekey::token_value(token, device) {
                        *text = text.replace(token, replacement);
                        *count += 1;
                        seen.push(token.to_string());
                    }
                }
            }
            Value::Dictionary(dict) => {
                for (_, child) in dict.iter_mut() {
                    walk(child, device, count, seen);
                }
            }
            Value::Array(items) => {
                for child in items.iter_mut() {
                    walk(child, device, count, seen);
                }
            }
            _ => {}
        }
    }
    let mut count = 0usize;
    let mut seen: Vec<String> = Vec::new();
    walk(value, device, &mut count, &mut seen);
    seen.sort_unstable();
    seen.dedup();
    (count, seen)
}

fn is_binary(data: &[u8]) -> bool {
    data.starts_with(b"bplist")
}

fn read_plist(data: &[u8]) -> Result<Value, String> {
    Value::from_reader(std::io::Cursor::new(data)).map_err(|e| format!("plist 解析失败：{e}"))
}

fn write_plist(value: &Value, binary: bool) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    if binary {
        plist::to_writer_binary(&mut buf, value)
    } else {
        plist::to_writer_xml(&mut buf, value)
    }
    .map_err(|e| format!("plist 序列化失败：{e}"))?;
    Ok(buf)
}

/// 按 (键, 期望值) 改写一个字典。`blankable` 里的键在无值时置空。
fn apply(
    dict: &mut Dictionary,
    wanted: &[(&str, Option<String>)],
    blankable: &[&str],
) -> Report {
    let mut report = Report::default();
    for (key, value) in wanted {
        // 注意：迭代 &[(&str, ..)] 得到的是 &(&str, ..)，key 的类型是 &&str。
        let key: &str = key;
        if !dict.contains_key(key) {
            report = report.skipped(key, "缺失");
            continue;
        }
        match value {
            Some(v) => {
                dict.insert(key.to_string(), Value::String(v.clone()));
                report = report.patched(key);
            }
            None => {
                if blankable.contains(&key) {
                    dict.insert(key.to_string(), Value::String(String::new()));
                    report = report.patched(&format!("{key}=空"));
                } else {
                    report = report.skipped(key, "无值");
                }
            }
        }
    }
    report
}

fn non_empty(value: &str) -> Option<String> {
    if value.trim().is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

/// 重新签发 `Info.plist`（空格键）。
pub fn patch_info(data: &[u8], device: &Device) -> Result<(Vec<u8>, Report), String> {
    let binary = is_binary(data);
    let mut value = read_plist(data)?;
    let dict = value
        .as_dictionary_mut()
        .ok_or("Info.plist 根节点不是字典")?;
    let wanted: Vec<(&str, Option<String>)> = vec![
        ("Target Identifier", non_empty(&device.udid)),
        ("Unique Identifier", non_empty(&device.udid)),
        ("Serial Number", non_empty(&device.serial)),
        ("Product Type", non_empty(&device.product)),
        ("Product Version", non_empty(&device.version)),
        ("Build Version", non_empty(&device.build)),
        ("Device Name", non_empty(&device.name)),
        ("Display Name", non_empty(&device.name)),
    ];
    let report = apply(dict, &wanted, &[]);
    // 再整树替换一次令牌：模板里除上面 8 个键之外还可能散落占位符。
    let (token_hits, tokens) = replace_tokens(&mut value, device);
    let mut report = report;
    if token_hits > 0 {
        report
            .patched
            .push(format!("令牌×{token_hits}（{}）", tokens.join(", ")));
    }
    Ok((write_plist(&value, binary)?, report))
}

/// 重新签发 `Manifest.plist` 的 `Lockdown` 段（紧凑键）。
pub fn patch_manifest(data: &[u8], device: &Device) -> Result<(Vec<u8>, Report), String> {
    let binary = is_binary(data);
    let mut value = read_plist(data)?;
    let root = value
        .as_dictionary_mut()
        .ok_or("Manifest.plist 根节点不是字典")?;
    let lockdown = match root.get_mut("Lockdown").and_then(Value::as_dictionary_mut) {
        Some(dict) => dict,
        None => {
            // 没有该段时只报告，不凭空造一个。
            let mut report = Report::default();
            report = report.skipped("Lockdown", "缺失");
            return Ok((write_plist(&value, binary)?, report));
        }
    };
    let wanted: Vec<(&str, Option<String>)> = vec![
        ("UniqueDeviceID", non_empty(&device.udid)),
        ("SerialNumber", non_empty(&device.serial)),
        ("ProductType", non_empty(&device.product)),
        ("ProductVersion", non_empty(&device.version)),
        ("BuildVersion", non_empty(&device.build)),
        ("DeviceName", non_empty(&device.name)),
        ("IMEI", non_empty(&device.imei)),
    ];
    // 仅 IMEI 允许置空，与厂商 PatchManifestDictionary 一致。
    let report = apply(lockdown, &wanted, &["IMEI"]);
    let (token_hits, tokens) = replace_tokens(&mut value, device);
    let mut report = report;
    if token_hits > 0 {
        report
            .patched
            .push(format!("令牌×{token_hits}（{}）", tokens.join(", ")));
    }
    Ok((write_plist(&value, binary)?, report))
}

/// 对一个已落盘的备份目录执行改写。返回每个文件的报告。
///
/// **只用于 `Route::LegacyGaval`（主版本 < 27）。** Mdm27 路线应保留原始字节，
/// 调用前请先用 [`crate::bypass::Route::rewrites_device_identity`] 判断。
pub fn rekey_backup_dir(dir: &Path, device: &Device) -> Result<Vec<(String, Report)>, String> {
    if !dir.is_dir() {
        return Err(format!("备份目录不存在：{}", dir.display()));
    }
    let mut out = Vec::new();
    for (name, patcher) in [
        ("Info.plist", patch_info as fn(&[u8], &Device) -> Result<(Vec<u8>, Report), String>),
        ("Manifest.plist", patch_manifest as fn(&[u8], &Device) -> Result<(Vec<u8>, Report), String>),
    ] {
        let path = dir.join(name);
        if !path.is_file() {
            let mut report = Report::default();
            report = report.skipped(name, "文件缺失");
            out.push((name.to_string(), report));
            continue;
        }
        let data = std::fs::read(&path).map_err(|e| format!("读取 {name} 失败：{e}"))?;
        let (patched, report) = patcher(&data, device)?;
        std::fs::write(&path, &patched).map_err(|e| format!("写回 {name} 失败：{e}"))?;
        out.push((name.to_string(), report));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn device() -> Device {
        Device {
            udid: "00008110-000A1C2E3F40001E".into(),
            serial: "F17DGJ2R0DY1".into(),
            product: "iPhone10,3".into(),
            version: "16.7.8".into(),
            build: "20H343".into(),
            name: "测试机".into(),
            imei: String::new(),
            activation: String::new(),
            facts: BTreeMap::new(),
        }
    }

    fn sample_info() -> Vec<u8> {
        let mut dict = Dictionary::new();
        dict.insert("Device Name".into(), Value::String("AppleDevice".into()));
        dict.insert("Product Version".into(), Value::String("12.0".into()));
        dict.insert("Unique Identifier".into(), Value::String("a".repeat(40)));
        dict.insert("Build Version".into(), Value::String("16A366".into()));
        dict.insert("Serial Number".into(), Value::String("AAAAAAAAAAAA".into()));
        dict.insert(
            "iTunes Files".into(),
            Value::Dictionary(Dictionary::new()),
        );
        let mut buf = Vec::new();
        plist::to_writer_binary(&mut buf, &Value::Dictionary(dict)).unwrap();
        buf
    }

    fn sample_manifest() -> Vec<u8> {
        let mut lockdown = Dictionary::new();
        lockdown.insert("UniqueDeviceID".into(), Value::String("a".repeat(40)));
        lockdown.insert("SerialNumber".into(), Value::String("AAAAAAAAAAAA".into()));
        lockdown.insert("DeviceName".into(), Value::String("AppleDevice".into()));
        let mut root = Dictionary::new();
        root.insert("Version".into(), Value::String("10.0".into()));
        root.insert("IsEncrypted".into(), Value::Boolean(true));
        root.insert("Lockdown".into(), Value::Dictionary(lockdown));
        let mut buf = Vec::new();
        plist::to_writer_binary(&mut buf, &Value::Dictionary(root)).unwrap();
        buf
    }

    #[test]
    fn info_is_rekeyed_and_format_preserved() {
        let (out, report) = patch_info(&sample_info(), &device()).unwrap();
        assert!(out.starts_with(b"bplist"));
        let value = read_plist(&out).unwrap();
        let dict = value.as_dictionary().unwrap();
        assert_eq!(
            dict.get("Unique Identifier").and_then(Value::as_string),
            Some("00008110-000A1C2E3F40001E")
        );
        assert_eq!(
            dict.get("Serial Number").and_then(Value::as_string),
            Some("F17DGJ2R0DY1")
        );
        assert!(report.patched.iter().any(|k| k == "Unique Identifier"));
    }

    #[test]
    fn info_keeps_unrelated_keys() {
        let (out, _) = patch_info(&sample_info(), &device()).unwrap();
        let value = read_plist(&out).unwrap();
        assert!(value.as_dictionary().unwrap().contains_key("iTunes Files"));
    }

    #[test]
    fn info_never_invents_absent_keys() {
        let mut dict = Dictionary::new();
        dict.insert("Unique Identifier".into(), Value::String("x".into()));
        let mut buf = Vec::new();
        plist::to_writer_binary(&mut buf, &Value::Dictionary(dict)).unwrap();
        let (out, report) = patch_info(&buf, &device()).unwrap();
        let value = read_plist(&out).unwrap();
        assert_eq!(value.as_dictionary().unwrap().len(), 1);
        assert!(report.skipped.iter().any(|k| k.starts_with("Target Identifier")));
    }

    #[test]
    fn manifest_lockdown_is_rekeyed() {
        let (out, report) = patch_manifest(&sample_manifest(), &device()).unwrap();
        let value = read_plist(&out).unwrap();
        let lockdown = value
            .as_dictionary()
            .unwrap()
            .get("Lockdown")
            .and_then(Value::as_dictionary)
            .unwrap();
        assert_eq!(
            lockdown.get("UniqueDeviceID").and_then(Value::as_string),
            Some("00008110-000A1C2E3F40001E")
        );
        assert!(report.patched.iter().any(|k| k == "UniqueDeviceID"));
    }

    #[test]
    fn manifest_keeps_name_only_when_it_is_unknown() {
        // 名字未知（空）时，DeviceName 必须原样保留，不能被清空
        let mut unknown = device();
        unknown.name = String::new();
        unknown.imei = String::new();
        let (out, report) = patch_manifest(&sample_manifest(), &unknown).unwrap();
        let value = read_plist(&out).unwrap();
        let lockdown = value
            .as_dictionary()
            .unwrap()
            .get("Lockdown")
            .and_then(Value::as_dictionary)
            .unwrap();
        assert_eq!(
            lockdown.get("DeviceName").and_then(Value::as_string),
            Some("AppleDevice"),
            "未知的名字不应覆盖原值"
        );
        assert!(report.skipped.iter().any(|k| k.starts_with("DeviceName")));
        // IMEI 键在这份模板里不存在 → 只报告，不凭空创建
        assert!(report.skipped.iter().any(|k| k.starts_with("IMEI")));
        assert!(!lockdown.contains_key("IMEI"));

        // 名字已知时则应当写入
        let (out2, _) = patch_manifest(&sample_manifest(), &device()).unwrap();
        let value2 = read_plist(&out2).unwrap();
        let lockdown2 = value2
            .as_dictionary()
            .unwrap()
            .get("Lockdown")
            .and_then(Value::as_dictionary)
            .unwrap();
        assert_eq!(
            lockdown2.get("DeviceName").and_then(Value::as_string),
            Some("测试机")
        );
    }

    #[test]
    fn manifest_without_lockdown_is_reported() {
        let mut root = Dictionary::new();
        root.insert("Version".into(), Value::String("10.0".into()));
        let mut buf = Vec::new();
        plist::to_writer_binary(&mut buf, &Value::Dictionary(root)).unwrap();
        let (out, report) = patch_manifest(&buf, &device()).unwrap();
        assert!(report.skipped.iter().any(|k| k.starts_with("Lockdown")));
        let value = read_plist(&out).unwrap();
        assert_eq!(value.as_dictionary().unwrap().len(), 1);
    }

    #[test]
    fn xml_format_is_preserved() {
        let mut dict = Dictionary::new();
        dict.insert("Unique Identifier".into(), Value::String("x".into()));
        let mut buf = Vec::new();
        plist::to_writer_xml(&mut buf, &Value::Dictionary(dict)).unwrap();
        let (out, _) = patch_info(&buf, &device()).unwrap();
        assert!(out.starts_with(b"<?xml"));
    }

    #[test]
    fn rekey_backup_dir_writes_back() {
        let dir = std::env::temp_dir().join(format!("atk-rekey-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("Info.plist"), sample_info()).unwrap();
        std::fs::write(dir.join("Manifest.plist"), sample_manifest()).unwrap();

        let reports = rekey_backup_dir(&dir, &device()).unwrap();
        assert_eq!(reports.len(), 2);
        let info = std::fs::read(dir.join("Info.plist")).unwrap();
        assert!(info.starts_with(b"bplist"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
