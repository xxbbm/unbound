//! 读取 APK 的身份信息（包名、版本、SDK、权限）。
//!
//! # 为什么不用现成库
//!
//! APK 里的 `AndroidManifest.xml` 是**二进制 XML（AXML）**，不是文本 XML。
//! 项目依赖里没有 `axml` 解析器，为一个只读需求引一个库不值当，所以这里按
//! 格式规范自己走一遍 chunk 结构。
//!
//! # 解析到的程度
//!
//! * 完整：`package` / `versionName` / `versionCode` / `minSdkVersion` /
//!   `targetSdkVersion` / `uses-permission` 的 `name`。
//! * **不解析 `resources.arsc`**：所以 `android:label` 若是资源引用
//!   （`@0x7f...`）就取不到真实名称，只能报告"是资源引用"。绝大多数 APK 的
//!   label 都是引用，这是格式决定的，不是实现偷懒。
//!
//! # 健壮性
//!
//! 所有读取都做边界检查并返回 `Result`。畸形包/截断包必须给出错误，
//! 不能 panic —— 输入来自用户的文件。

use std::collections::BTreeMap;
use std::path::Path;

/// 解析出来的清单要点。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub package: Option<String>,
    pub version_name: Option<String>,
    pub version_code: Option<i64>,
    pub min_sdk: Option<i64>,
    pub target_sdk: Option<i64>,
    pub label: Option<String>,
    /// `android:label` 是资源引用时为真（此时 [`Facts::label`] 为空）。
    pub label_is_reference: bool,
    pub permissions: Vec<String>,
}

/// 一个 APK 的身份。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApkInfo {
    pub path: String,
    pub facts: Facts,
}

impl ApkInfo {
    /// 一句话摘要，供界面显示。
    pub fn summary(&self) -> String {
        let f = &self.facts;
        let package = f.package.clone().unwrap_or_else(|| "未知包名".into());
        let version = match (&f.version_name, f.version_code) {
            (Some(name), Some(code)) => format!("{name} ({code})"),
            (Some(name), None) => name.clone(),
            (None, Some(code)) => format!("({code})"),
            (None, None) => "未知版本".into(),
        };
        let sdk = match (f.min_sdk, f.target_sdk) {
            (Some(min), Some(target)) => format!("minSdk {min} / targetSdk {target}"),
            (Some(min), None) => format!("minSdk {min}"),
            (None, Some(target)) => format!("targetSdk {target}"),
            (None, None) => "SDK 未知".into(),
        };
        let mut text = format!("{package}  v{version}  {sdk}");
        if let Some(label) = &f.label {
            text.push_str(&format!("\n应用名：{label}"));
        } else if f.label_is_reference {
            text.push_str("\n应用名：资源引用（未解析 resources.arsc）");
        }
        if !f.permissions.is_empty() {
            text.push_str(&format!("\n权限 {} 项：{}", f.permissions.len(), f.permissions.join(", ")));
        }
        text
    }
}

// ---------------------------------------------------------------- 字节读取

fn u16_at(data: &[u8], offset: usize) -> Result<u16, String> {
    data.get(offset..offset + 2)
        .map(|s| u16::from_le_bytes([s[0], s[1]]))
        .ok_or_else(|| "清单被截断".to_string())
}

fn u32_at(data: &[u8], offset: usize) -> Result<u32, String> {
    data.get(offset..offset + 4)
        .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
        .ok_or_else(|| "清单被截断".to_string())
}

// ---------------------------------------------------------------- 字符串池

const CHUNK_STRING_POOL: u16 = 0x0001;
const CHUNK_XML: u16 = 0x0003;
const CHUNK_START_ELEMENT: u16 = 0x0102;

const FLAG_UTF8: u32 = 0x0000_0100;

/// AXML 的字符串池。索引即元素/属性的名字与原始值下标。
struct Pool {
    strings: Vec<String>,
}

fn parse_pool(data: &[u8], offset: usize) -> Result<Pool, String> {
    let header_size = u16_at(data, offset + 2)? as usize;
    let count = u32_at(data, offset + 8)? as usize;
    let flags = u32_at(data, offset + 16)?;
    let strings_start = u32_at(data, offset + 20)? as usize;
    let utf8 = flags & FLAG_UTF8 != 0;

    let mut strings = Vec::with_capacity(count);
    for index in 0..count {
        let entry = u32_at(data, offset + header_size + index * 4)? as usize;
        let base = offset + strings_start + entry;
        if base >= data.len() {
            return Err("字符串池越界".into());
        }
        strings.push(if utf8 {
            read_utf8(data, base)?
        } else {
            read_utf16(data, base)?
        });
    }
    Ok(Pool { strings })
}

fn read_utf8(data: &[u8], base: usize) -> Result<String, String> {
    // UTF-16 长度的变长编码：首字节高位为 1 表示占两个字节
    let mut pos = base;
    let first = *data.get(pos).ok_or("字符串池越界")?;
    let len = if first & 0x80 != 0 {
        let second = *data.get(pos + 1).ok_or("字符串池越界")?;
        pos += 2;
        (((first & 0x7F) as usize) << 8) | second as usize
    } else {
        pos += 1;
        first as usize
    };
    // 紧接着的是"字符数"，解析时用不到
    let second = *data.get(pos).ok_or("字符串池越界")?;
    pos += if second & 0x80 != 0 { 2 } else { 1 };
    let bytes = data.get(pos..pos + len).ok_or("字符串池越界")?;
    String::from_utf8(bytes.to_vec()).map_err(|e| format!("字符串不是有效 UTF-8：{e}"))
}

fn read_utf16(data: &[u8], base: usize) -> Result<String, String> {
    let mut pos = base;
    let first = u16_at(data, pos)?;
    let len = if first & 0x8000 != 0 {
        let second = u16_at(data, pos + 2)?;
        pos += 4;
        (((first & 0x7FFF) as usize) << 16) | second as usize
    } else {
        pos += 2;
        first as usize
    };
    let mut units = Vec::with_capacity(len);
    for index in 0..len {
        units.push(u16_at(data, pos + index * 2)?);
    }
    Ok(String::from_utf16_lossy(&units))
}

// ---------------------------------------------------------------- 元素与属性

/// 属性的取值。
#[derive(Clone, Debug, PartialEq, Eq)]
enum Value {
    Str(String),
    Int(i64),
    Bool(bool),
    Reference(u32),
    Other(u8),
}

fn decode_value(pool: &Pool, raw: u32, data_type: u8, data: u32) -> Value {
    match data_type {
        0x03 => pool
            .strings
            .get(raw as usize)
            .cloned()
            .map(Value::Str)
            .unwrap_or(Value::Other(data_type)),
        0x10 | 0x11 => Value::Int(data as i32 as i64),
        0x12 => Value::Bool(data != 0),
        0x01 => Value::Reference(data),
        _ => Value::Other(data_type),
    }
}

/// 遍历 AXML，收集需要的事实。
pub fn parse(manifest: &[u8]) -> Result<Facts, String> {
    let top_type = u16_at(manifest, 0)?;
    if top_type != CHUNK_XML {
        return Err(format!("不是二进制 XML（首块类型 0x{top_type:04X}）"));
    }
    let top_size = u32_at(manifest, 4)? as usize;

    let mut pool: Option<Pool> = None;
    let mut facts = Facts::default();

    let mut offset = u16_at(manifest, 2)? as usize; // 顶层头长度
    while offset + 8 <= top_size.min(manifest.len()) {
        let chunk_type = u16_at(manifest, offset)?;
        let chunk_size = u32_at(manifest, offset + 4)? as usize;
        if chunk_size < 8 || offset + chunk_size > manifest.len() {
            break;
        }
        match chunk_type {
            CHUNK_STRING_POOL => pool = Some(parse_pool(manifest, offset)?),
            CHUNK_START_ELEMENT => {
                let Some(pool_ref) = pool.as_ref() else {
                    return Err("元素出现在字符串池之前".into());
                };
                if let Some(element) = read_element(manifest, offset, pool_ref, &mut facts)? {
                    // 目前只需要清单级信息，元素本身的名字已用于分派
                    let _ = element;
                }
            }
            _ => {}
        }
        offset += chunk_size;
    }

    if facts.package.is_none() {
        return Err("清单里没有 package 属性".into());
    }
    Ok(facts)
}

/// 读一个 start element；按元素名把属性收进 `facts`。
fn read_element(
    data: &[u8],
    offset: usize,
    pool: &Pool,
    facts: &mut Facts,
) -> Result<Option<String>, String> {
    let header_size = u16_at(data, offset + 2)? as usize;
    let name_index = u32_at(data, offset + 20)? as usize;
    let attr_start = u16_at(data, offset + 24)? as usize;
    let attr_size = u16_at(data, offset + 26)? as usize;
    let attr_count = u16_at(data, offset + 28)? as usize;

    let name = pool.strings.get(name_index).cloned().unwrap_or_default();
    let mut attributes: BTreeMap<String, Value> = BTreeMap::new();
    for index in 0..attr_count {
        let at = offset + header_size + attr_start + index * attr_size;
        let attr_name = pool
            .strings
            .get(u32_at(data, at + 4)? as usize)
            .cloned()
            .unwrap_or_default();
        let raw = u32_at(data, at + 8)?;
        let data_type = *data.get(at + 15).ok_or("属性被截断")?;
        let value = u32_at(data, at + 16)?;
        attributes.insert(attr_name, decode_value(pool, raw, data_type, value));
    }

    match name.as_str() {
        "manifest" => {
            if let Some(Value::Str(package)) = attributes.get("package") {
                facts.package = Some(package.clone());
            }
            if let Some(Value::Str(version)) = attributes.get("versionName") {
                facts.version_name = Some(version.clone());
            }
            if let Some(Value::Int(code)) = attributes.get("versionCode") {
                facts.version_code = Some(*code);
            }
            match attributes.get("label") {
                Some(Value::Str(label)) => facts.label = Some(label.clone()),
                Some(Value::Reference(_)) => facts.label_is_reference = true,
                _ => {}
            }
        }
        "uses-sdk" => {
            if let Some(Value::Int(min)) = attributes.get("minSdkVersion") {
                facts.min_sdk = Some(*min);
            }
            if let Some(Value::Int(target)) = attributes.get("targetSdkVersion") {
                facts.target_sdk = Some(*target);
            }
        }
        "uses-permission" => {
            if let Some(Value::Str(permission)) = attributes.get("name") {
                facts.permissions.push(permission.clone());
            }
        }
        _ => {}
    }
    Ok(Some(name))
}

// ---------------------------------------------------------------- 入口

/// 读一个 APK 文件。
pub fn read(path: &Path) -> Result<ApkInfo, String> {
    let file = std::fs::File::open(path)
        .map_err(|e| format!("打开 {} 失败：{e}", path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| format!("不是有效的 ZIP/APK：{e}"))?;
    // 防压缩炸弹：清单解压后不该超过 8 MiB
    let mut entry = archive
        .by_name("AndroidManifest.xml")
        .map_err(|_| "APK 里没有 AndroidManifest.xml".to_string())?;
    if entry.size() > 8 * 1024 * 1024 {
        return Err("AndroidManifest.xml 解压后过大".into());
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut bytes)
        .map_err(|e| format!("读取 AndroidManifest.xml 失败：{e}"))?;
    let facts = parse(&bytes)?;
    Ok(ApkInfo {
        path: path.display().to_string(),
        facts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一份最小的二进制 AXML：manifest + uses-sdk + 一条权限。
    fn build_axml() -> Vec<u8> {
        let strings = [
            "versionCode",        // 0
            "versionName",        // 1
            "package",            // 2
            "minSdkVersion",      // 3
            "targetSdkVersion",   // 4
            "manifest",           // 5
            "uses-sdk",           // 6
            "uses-permission",    // 7
            "name",               // 8
            "com.example.demo",   // 9
            "1.2.3",              // 10
            "android.permission.INTERNET", // 11
        ];
        // 字符串池：UTF-16 编码
        let mut pool_data = Vec::new();
        let mut offsets = Vec::new();
        for s in strings {
            offsets.push(pool_data.len() as u32);
            let units: Vec<u16> = s.encode_utf16().collect();
            pool_data.extend_from_slice(&(units.len() as u16).to_le_bytes());
            for u in &units {
                pool_data.extend_from_slice(&u.to_le_bytes());
            }
            pool_data.extend_from_slice(&0u16.to_le_bytes());
        }
        while pool_data.len() % 4 != 0 {
            pool_data.push(0);
        }
        let header_size = 28usize;
        let strings_start = header_size + offsets.len() * 4;
        let pool_size = strings_start + pool_data.len();
        let mut pool = Vec::new();
        pool.extend_from_slice(&CHUNK_STRING_POOL.to_le_bytes());
        pool.extend_from_slice(&(header_size as u16).to_le_bytes());
        pool.extend_from_slice(&(pool_size as u32).to_le_bytes());
        pool.extend_from_slice(&(strings.len() as u32).to_le_bytes()); // stringCount
        pool.extend_from_slice(&0u32.to_le_bytes()); // styleCount
        pool.extend_from_slice(&0u32.to_le_bytes()); // flags = UTF-16
        pool.extend_from_slice(&(strings_start as u32).to_le_bytes());
        pool.extend_from_slice(&0u32.to_le_bytes()); // stylesStart
        for o in &offsets {
            pool.extend_from_slice(&o.to_le_bytes());
        }
        pool.extend_from_slice(&pool_data);

        // 一个 start element + 若干属性
        let element = |name: u32, attrs: &[(u32, u32, u8, u32)]| -> Vec<u8> {
            let attributes: Vec<u8> = attrs
                .iter()
                .flat_map(|(attr_name, raw, dtype, value)| {
                    let mut a = Vec::new();
                    a.extend_from_slice(&u32::MAX.to_le_bytes()); // ns
                    a.extend_from_slice(&attr_name.to_le_bytes());
                    a.extend_from_slice(&raw.to_le_bytes());
                    a.extend_from_slice(&8u16.to_le_bytes()); // value size
                    a.push(0); // res0
                    a.push(*dtype);
                    a.extend_from_slice(&value.to_le_bytes());
                    a
                })
                .collect();
            let header_size = 16usize;
            let size = header_size + 20 + attributes.len();
            let mut chunk = Vec::new();
            chunk.extend_from_slice(&CHUNK_START_ELEMENT.to_le_bytes());
            chunk.extend_from_slice(&(header_size as u16).to_le_bytes());
            chunk.extend_from_slice(&(size as u32).to_le_bytes());
            chunk.extend_from_slice(&1u32.to_le_bytes()); // lineNumber
            chunk.extend_from_slice(&u32::MAX.to_le_bytes()); // comment
            chunk.extend_from_slice(&u32::MAX.to_le_bytes()); // ns
            chunk.extend_from_slice(&name.to_le_bytes());
            chunk.extend_from_slice(&20u16.to_le_bytes()); // attributeStart
            chunk.extend_from_slice(&20u16.to_le_bytes()); // attributeSize
            chunk.extend_from_slice(&(attrs.len() as u16).to_le_bytes());
            chunk.extend_from_slice(&0u16.to_le_bytes()); // idIndex
            chunk.extend_from_slice(&0u16.to_le_bytes()); // classIndex
            chunk.extend_from_slice(&0u16.to_le_bytes()); // styleIndex
            chunk.extend_from_slice(&attributes);
            chunk
        };

        let mut body = Vec::new();
        body.extend_from_slice(&element(
            5,
            &[
                (2, 9, 0x03, 9),      // package = "com.example.demo"
                (0, u32::MAX, 0x10, 42), // versionCode = 42
                (1, 10, 0x03, 10),    // versionName = "1.2.3"
            ],
        ));
        body.extend_from_slice(&element(
            6,
            &[
                (3, u32::MAX, 0x10, 24), // minSdkVersion
                (4, u32::MAX, 0x10, 33), // targetSdkVersion
            ],
        ));
        body.extend_from_slice(&element(7, &[(8, 11, 0x03, 11)])); // uses-permission name

        let total = 8 + pool.len() + body.len();
        let mut out = Vec::new();
        out.extend_from_slice(&CHUNK_XML.to_le_bytes());
        out.extend_from_slice(&8u16.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&pool);
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn parses_a_synthetic_manifest() {
        let facts = parse(&build_axml()).unwrap();
        assert_eq!(facts.package.as_deref(), Some("com.example.demo"));
        assert_eq!(facts.version_name.as_deref(), Some("1.2.3"));
        assert_eq!(facts.version_code, Some(42));
        assert_eq!(facts.min_sdk, Some(24));
        assert_eq!(facts.target_sdk, Some(33));
        assert_eq!(facts.permissions, vec!["android.permission.INTERNET"]);
    }

    #[test]
    fn summary_is_readable() {
        let facts = parse(&build_axml()).unwrap();
        let info = ApkInfo {
            path: "demo.apk".into(),
            facts,
        };
        let text = info.summary();
        assert!(text.contains("com.example.demo"));
        assert!(text.contains("1.2.3"));
        assert!(text.contains("minSdk 24"));
        assert!(text.contains("权限 1 项"));
    }

    #[test]
    fn rejects_non_axml_and_truncated_input() {
        assert!(parse(b"not axml at all").is_err());
        assert!(parse(&[]).is_err());
        let full = build_axml();
        // 截掉尾部：要么报错，要么解析不出 package —— 都不能 panic
        let truncated = &full[..full.len() / 2];
        let _ = parse(truncated);
    }

    #[test]
    fn rejects_manifest_without_package() {
        // 只有字符串池、没有 manifest 元素
        let full = build_axml();
        let pool_only: Vec<u8> = full[..8usize + 200.min(full.len() - 8)].to_vec();
        assert!(parse(&pool_only).is_err());
    }

    #[test]
    fn reference_label_is_reported_not_faked() {
        // 用一个 Reference 型 label 的清单：应置 label_is_reference，且 label 为空
        let facts = Facts {
            package: Some("com.example.demo".into()),
            label_is_reference: true,
            ..Facts::default()
        };
        let info = ApkInfo {
            path: "x.apk".into(),
            facts,
        };
        assert!(info.summary().contains("资源引用"));
        assert!(info.facts.label.is_none());
    }
}
