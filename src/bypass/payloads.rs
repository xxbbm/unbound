//! 激活锁 payload 登记表与校验。
//!
//! 逆向结论：四套工具里的六个文件名其实只有 **三个唯一二进制**，
//! 且文件名的序号**既不等于 iOS 版本，也不单调** ——
//! `bypass3` 是 12.4，比 `bypass2`(13.3) 更旧。所以选择必须按 `minos`，
//! 身份必须按 SHA-256，**绝不能按文件名排序**。
//!
//! 表里的哈希取自重组后的字节，并已在两个工具包之间做过交叉比对。

use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// 一个已登记的 payload。
#[derive(Debug)]
pub struct Payload {
    pub id: &'static str,
    pub label: &'static str,
    /// Mach-O `LC_BUILD_VERSION` 里的 minos，不是设备兼容上限。
    pub minos: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    pub target_path: &'static str,
    /// 每种已知落盘形态：分片按顺序，或整包一个文件。
    pub layouts: &'static [&'static [&'static str]],
}

pub const CATALOG: &[Payload] = &[
    Payload {
        id: "activationd-minos12.4",
        label: "mobileactivationd 替换件（iOS 12.4 SDK 构建）",
        minos: "12.4.0",
        size: 2_261_712,
        sha256: "04aead666a4e8d2dea9b10a5ca1ee57ec35ac0ec1b3cca2a3b2fc92dd7e4d498",
        target_path: "/usr/libexec/mobileactivationd",
        layouts: &[
            &[
                "res/bypass3_1",
                "res/bypass3_2",
                "res/bypass3_3",
                "res/bypass3_4",
                "res/bypass3_5",
            ],
            &["Helper/bypass6"],
            &["Src/bypass6"],
        ],
    },
    Payload {
        id: "activationd-minos13.3",
        label: "mobileactivationd 替换件（iOS 13.3 SDK 构建）",
        minos: "13.3.0",
        size: 2_160_848,
        sha256: "2454be8c9efb99ee556902fe08cbd8693f263e2d3b9dd4e51e07db58bda20a39",
        target_path: "/usr/libexec/mobileactivationd",
        layouts: &[
            &[
                "res/bypass2_1",
                "res/bypass2_2",
                "res/bypass2_3",
                "res/bypass2_4",
                "res/bypass2_5",
            ],
            &["Helper/bypass1"],
            &["Src/bypass"],
        ],
    },
    Payload {
        id: "activationd-minos15.5",
        label: "mobileactivationd 替换件（iOS 15.5 SDK 构建）",
        minos: "15.5.0",
        size: 2_398_096,
        sha256: "edefe8f4183cc9a18cbb2d3fc89a64a028a622e59f1c7a820fbd738cc08d84d5",
        target_path: "/usr/libexec/mobileactivationd",
        layouts: &[&[
            "res/bypass4_1",
            "res/bypass4_2",
            "res/bypass4_3",
            "res/bypass4_4",
            "res/bypass4_5",
        ]],
    },
];

/// 校验通过的结果。
#[derive(Debug)]
pub struct Verified {
    pub payload: &'static Payload,
    pub layout: Vec<PathBuf>,
    pub digest: String,
    pub size: u64,
}

pub fn by_id(id: &str) -> Option<&'static Payload> {
    CATALOG.iter().find(|p| p.id == id)
}

fn parse_version(text: &str) -> (u32, u32, u32) {
    let mut parts = [0u32; 3];
    for (i, piece) in text.split('.').take(3).enumerate() {
        let digits: String = piece.chars().filter(char::is_ascii_digit).collect();
        parts[i] = digits.parse().unwrap_or(0);
    }
    (parts[0], parts[1], parts[2])
}

/// 选 minos 不高于设备系统版本的最新一个；都比设备新则取最旧的。
pub fn pick_for_ios(version: &str) -> Option<&'static Payload> {
    let want = parse_version(version);
    let mut sorted: Vec<&'static Payload> = CATALOG.iter().collect();
    sorted.sort_by_key(|p| parse_version(p.minos));
    let eligible: Vec<_> = sorted
        .iter()
        .copied()
        .filter(|p| parse_version(p.minos) <= want)
        .collect();
    eligible.last().copied().or_else(|| sorted.first().copied())
}

/// 在 `root` 下按顺序试每种形态，返回第一个哈希与体积都对得上的。
pub fn verify(root: &Path, payload: &'static Payload) -> Result<Verified, String> {
    let mut absent = 0usize;
    let mut mismatched: Option<String> = None;

    for layout in payload.layouts {
        let paths: Vec<PathBuf> = layout.iter().map(|rel| root.join(rel)).collect();
        if paths.iter().any(|p| !p.is_file()) {
            absent += 1;
            continue;
        }
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        for path in &paths {
            let data = std::fs::read(path)
                .map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
            hasher.update(&data);
            total += data.len() as u64;
        }
        let digest: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if digest == payload.sha256 && total == payload.size {
            return Ok(Verified {
                payload,
                layout: paths,
                digest,
                size: total,
            });
        }
        mismatched = Some(format!(
            "{}（期望 {} 字节 / {}，实际 {} 字节 / {}）",
            layout[0],
            payload.size,
            &payload.sha256[..16],
            total,
            &digest[..16]
        ));
    }

    if let Some(detail) = mismatched {
        return Err(format!("payload 校验失败：{detail}"));
    }
    Err(format!(
        "在 {} 下未找到 {} 的完整形态（{} 种形态均缺文件）",
        root.display(),
        payload.id,
        absent
    ))
}

/// 把校验通过的 payload 拼成一个文件。
pub fn assemble(root: &Path, payload: &'static Payload, out: &Path) -> Result<PathBuf, String> {
    let verified = verify(root, payload)?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建输出目录失败：{e}"))?;
    }
    let mut buf = Vec::with_capacity(verified.size as usize);
    for path in &verified.layout {
        let data = std::fs::read(path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
        buf.extend_from_slice(&data);
    }
    std::fs::write(out, &buf).map_err(|e| format!("写入 {} 失败：{e}", out.display()))?;
    Ok(out.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_ignores_file_name_order() {
        // bypass3 是 12.4，比 bypass2 的 13.3 更旧 —— 按名字选会挑错
        assert_eq!(pick_for_ios("12.4").unwrap().id, "activationd-minos12.4");
        assert_eq!(pick_for_ios("13.3").unwrap().id, "activationd-minos13.3");
        assert_eq!(pick_for_ios("15.5").unwrap().id, "activationd-minos15.5");
        assert_eq!(pick_for_ios("17.5.1").unwrap().id, "activationd-minos15.5");
        assert_eq!(pick_for_ios("9.3.5").unwrap().id, "activationd-minos12.4");
    }

    #[test]
    fn catalog_has_three_unique_entries() {
        assert_eq!(CATALOG.len(), 3);
        let mut ids: Vec<&str> = CATALOG.iter().map(|p| p.id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn by_id_and_unknown() {
        assert!(by_id("activationd-minos13.3").is_some());
        assert!(by_id("nope").is_none());
    }

    #[test]
    fn version_parsing_is_numeric() {
        assert_eq!(parse_version("16.7.8"), (16, 7, 8));
        assert_eq!(parse_version("17.5"), (17, 5, 0));
        assert_eq!(parse_version("garbage"), (0, 0, 0));
    }

    #[test]
    fn verify_reports_missing_root() {
        let dir = std::env::temp_dir().join("atk-payload-absent");
        let err = verify(&dir, by_id("activationd-minos12.4").unwrap()).unwrap_err();
        assert!(err.contains("未找到"));
    }

    #[test]
    fn verify_detects_tampering() {
        static COUNTER: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir()
            .join(format!("atk-payload-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(dir.join("Src")).unwrap();
        // 造一个内容错误的整包，必须被判为不匹配而不是"缺失"
        std::fs::write(dir.join("Src/bypass"), vec![0u8; 16]).unwrap();
        let err = verify(&dir, by_id("activationd-minos13.3").unwrap()).unwrap_err();
        assert!(err.contains("校验失败"), "实际: {err}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
