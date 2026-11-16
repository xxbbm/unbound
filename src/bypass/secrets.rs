//! 从 `iFineMDM.exe` 里读取 `IFM1` 密钥容器。
//!
//! 逆向结论（已逐字节复核）：`CompatibilitySecrets` 没有任何字段，它用
//! `ReadPart(offset,len)` 从一个内嵌的 **84 字节** 资源里取三把密钥，
//! 且 `ReadPart` 在流长度不等于 84 时直接抛异常。
//!
//! ```text
//! [0..3]   "IFM1"          魔数
//! [4..35]  VersionKey      32 B
//! [36..51] VersionIv       16 B
//! [52..83] BackupSecret    32 B
//! ```
//!
//! 该资源在被分析的构建里位于文件偏移 `0x726DC`。这里按魔数搜索并做结构校验，
//! 而不是写死偏移 —— 换版本时偏移会变。
//!
//! **不要把密钥写进日志或界面。** 只暴露长度、前 4 字节和指纹。

use sha2::{Digest, Sha256};
use std::path::Path;

pub const MAGIC: &[u8; 4] = b"IFM1";
pub const CONTAINER_LEN: usize = 84;

#[derive(Clone)]
pub struct Secrets {
    pub version_key: [u8; 32],
    pub version_iv: [u8; 16],
    pub backup_secret: [u8; 32],
    /// 容器在文件中的偏移，便于排查"读到的是不是那一份"。
    pub offset: usize,
}

impl Secrets {
    /// 从整份镜像里搜索容器。
    pub fn read_from_image(path: &Path) -> Result<Self, String> {
        let blob = std::fs::read(path)
            .map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
        Self::find_in(&blob).ok_or_else(|| {
            "未找到有效的 IFM1 容器；该文件可能是不同的构建".to_string()
        })
    }

    /// 在任意字节流里搜索并解析。
    pub fn find_in(blob: &[u8]) -> Option<Self> {
        if blob.len() < CONTAINER_LEN {
            return None;
        }
        let mut start = 0usize;
        while start + CONTAINER_LEN <= blob.len() {
            let idx = blob[start..]
                .windows(MAGIC.len())
                .position(|w| w == MAGIC)?;
            let pos = start + idx;
            if pos + CONTAINER_LEN <= blob.len() {
                let slice = &blob[pos..pos + CONTAINER_LEN];
                if let Ok(secrets) = Self::from_container(slice, pos) {
                    return Some(secrets);
                }
            }
            start = pos + 1;
        }
        None
    }

    /// 解析一段必须是 84 字节的容器。
    pub fn from_container(slice: &[u8], offset: usize) -> Result<Self, String> {
        if slice.len() != CONTAINER_LEN {
            return Err(format!(
                "IFM1 容器必须是 {CONTAINER_LEN} 字节，实际 {}",
                slice.len()
            ));
        }
        if &slice[..4] != MAGIC {
            return Err("IFM1 魔数不匹配".into());
        }
        let mut version_key = [0u8; 32];
        version_key.copy_from_slice(&slice[4..36]);
        let mut version_iv = [0u8; 16];
        version_iv.copy_from_slice(&slice[36..52]);
        let mut backup_secret = [0u8; 32];
        backup_secret.copy_from_slice(&slice[52..84]);

        // 全零意味着匹配到的是噪声而不是资源本身。
        for (name, part) in [
            ("VersionKey", version_key.as_slice()),
            ("VersionIv", version_iv.as_slice()),
            ("BackupSecret", backup_secret.as_slice()),
        ] {
            if part.iter().all(|b| *b == 0) {
                return Err(format!("{name} 是全零，疑似误匹配"));
            }
        }
        if distinct_bytes(&version_key) <= 8 || distinct_bytes(&backup_secret) <= 8 {
            return Err("密钥熵过低，疑似误匹配".into());
        }
        Ok(Self {
            version_key,
            version_iv,
            backup_secret,
            offset,
        })
    }

    /// 每把密钥的前 4 字节十六进制。足以核对来源，且不能用来还原密钥。
    pub fn prefixes(&self) -> [String; 3] {
        [
            hex4(&self.version_key),
            hex4(&self.version_iv),
            hex4(&self.backup_secret),
        ]
    }

    /// 每把密钥的 SHA-256 前 8 位十六进制。
    pub fn fingerprints(&self) -> [String; 3] {
        [
            fingerprint(&self.version_key),
            fingerprint(&self.version_iv),
            fingerprint(&self.backup_secret),
        ]
    }

    /// 供界面/日志使用的一句话摘要，不含密钥本体。
    pub fn summary(&self) -> String {
        let p = self.prefixes();
        format!(
            "IFM1 @ 0x{:X}：version_key {}… / version_iv {}… / backup_secret {}…",
            self.offset, p[0], p[1], p[2]
        )
    }
}

fn distinct_bytes(data: &[u8]) -> usize {
    let mut seen = [false; 256];
    let mut n = 0;
    for b in data {
        if !seen[*b as usize] {
            seen[*b as usize] = true;
            n += 1;
        }
    }
    n
}

fn hex4(data: &[u8]) -> String {
    data.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

fn fingerprint(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    digest.iter().take(4).map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn container() -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(MAGIC);
        v.extend((0..32u8).map(|i| i.wrapping_mul(7).wrapping_add(1)));
        v.extend((0..16u8).map(|i| i.wrapping_mul(13).wrapping_add(3)));
        v.extend((0..32u8).map(|i| i.wrapping_mul(11).wrapping_add(5)));
        v
    }

    #[test]
    fn parses_a_valid_container() {
        let data = container();
        let s = Secrets::from_container(&data, 0x726dc).unwrap();
        assert_eq!(s.offset, 0x726dc);
        assert_eq!(s.prefixes()[0].len(), 8);
    }

    #[test]
    fn rejects_wrong_length_and_magic() {
        assert!(Secrets::from_container(&[0u8; 10], 0).is_err());
        let mut bad = container();
        bad[0] = b'X';
        assert!(Secrets::from_container(&bad, 0).is_err());
    }

    #[test]
    fn rejects_zeroed_payload() {
        let mut data = container();
        for b in data[4..36].iter_mut() {
            *b = 0;
        }
        assert!(Secrets::from_container(&data, 0).is_err());
    }

    #[test]
    fn finds_container_inside_noise() {
        let mut blob = vec![0xAAu8; 200];
        blob.extend_from_slice(&container());
        blob.extend_from_slice(&[0xBBu8; 50]);
        let found = Secrets::find_in(&blob).expect("应当找到");
        assert_eq!(found.offset, 200);
    }

    #[test]
    fn ignores_magic_that_is_not_a_container() {
        let mut blob = b"IFM1".to_vec();
        blob.extend_from_slice(&[0u8; 200]);
        assert!(Secrets::find_in(&blob).is_none());
    }

    #[test]
    fn summary_does_not_leak_full_keys() {
        let s = Secrets::from_container(&container(), 0).unwrap();
        let text = s.summary();
        assert!(text.contains("IFM1"));
        let full_hex: String = s.version_key.iter().map(|b| format!("{b:02x}")).collect();
        assert!(!text.contains(&full_hex));
    }
}
