//! 内置资源：两条 iOS 路线上需要的全部文件。
//!
//! # 为什么要把这些打进 exe
//!
//! 这些文件原本散落在原始工具里（`iFineMDM.exe` 的内嵌资源和它旁边的
//! `MixIos27\`、`capture\backup_data\`）。如果让用户自己去挖，那这个软件就失去
//! 了存在的意义 —— **一个工具的价值就在于不用去翻别的工具**。
//!
//! 所以这里全部用 `include_bytes!` 编进程序，运行时写到任务目录即可。
//!
//! | 组 | 内容 | 体积 |
//! |---|---|---|
//! | [`LEGACY`] | 旧路线的工具包、备份包、两份模板 plist | 约 34.4 MB |
//! | [`MODERN_TOOLS`] | `idevicebackup2.exe` 及依赖 dll | 约 4.4 MB |
//! | [`MODERN_BACKUP`] | 现代路线用的 8 文件备份模板 | 约 45 KB |
//!
//! 现代路线的备份口令也在原工具里写死（见
//! [`crate::bypass::backend::BACKUP_PASSWORD`]），所以用户同样不需要知道它。

use std::path::Path;

/// 旧路线：四份内嵌资源，文件名与 `pipeline` 里的常量一一对应。
pub const LEGACY: &[(&str, &[u8])] = &[
    (
        "GavalLegacyTools.zip",
        include_bytes!("../../assets/legacy/GavalLegacyTools.zip"),
    ),
    (
        "GavalLegacyBackup.zip",
        include_bytes!("../../assets/legacy/GavalLegacyBackup.zip"),
    ),
    (
        "GavalLegacyInfo.plist",
        include_bytes!("../../assets/legacy/GavalLegacyInfo.plist"),
    ),
    (
        "GavalLegacyManifest.plist",
        include_bytes!("../../assets/legacy/GavalLegacyManifest.plist"),
    ),
];

/// 现代路线：恢复器与它的依赖。少一个都跑不起来，所以整组一起放。
pub const MODERN_TOOLS: &[(&str, &[u8])] = &[
    (
        "idevicebackup2.exe",
        include_bytes!("../../assets/mix27/tools/idevicebackup2.exe"),
    ),
    (
        "imobiledevice.dll",
        include_bytes!("../../assets/mix27/tools/imobiledevice.dll"),
    ),
    (
        "libcrypto-1_1-x64.dll",
        include_bytes!("../../assets/mix27/tools/libcrypto-1_1-x64.dll"),
    ),
    (
        "libssl-1_1-x64.dll",
        include_bytes!("../../assets/mix27/tools/libssl-1_1-x64.dll"),
    ),
    (
        "plist.dll",
        include_bytes!("../../assets/mix27/tools/plist.dll"),
    ),
    (
        "usbmuxd.dll",
        include_bytes!("../../assets/mix27/tools/usbmuxd.dll"),
    ),
    (
        "VCRUNTIME140.dll",
        include_bytes!("../../assets/mix27/tools/VCRUNTIME140.dll"),
    ),
];

/// 现代路线：服务端那份备份的本地模板，8 个文件（含 4 个哈希命名的 blob）。
pub const MODERN_BACKUP: &[(&str, &[u8])] = &[
    (
        "Info.plist",
        include_bytes!("../../assets/mix27/backup/Info.plist"),
    ),
    (
        "Manifest.plist",
        include_bytes!("../../assets/mix27/backup/Manifest.plist"),
    ),
    (
        "Manifest.db",
        include_bytes!("../../assets/mix27/backup/Manifest.db"),
    ),
    (
        "Status.plist",
        include_bytes!("../../assets/mix27/backup/Status.plist"),
    ),
    (
        "0d/0dc926a1810f7aee4e8f38793ed788701f93bf9d",
        include_bytes!("../../assets/mix27/backup/0d/0dc926a1810f7aee4e8f38793ed788701f93bf9d"),
    ),
    (
        "1e/1e6c0783f9b33d00b152067a0661c8fc8841073f",
        include_bytes!("../../assets/mix27/backup/1e/1e6c0783f9b33d00b152067a0661c8fc8841073f"),
    ),
    (
        "80/80fa782261261e54bb73c0253e5b6beefb6a2f39",
        include_bytes!("../../assets/mix27/backup/80/80fa782261261e54bb73c0253e5b6beefb6a2f39"),
    ),
    (
        "db/dbf354b95ce3e067684b5c4b66d19d5c8cac4d80",
        include_bytes!("../../assets/mix27/backup/db/dbf354b95ce3e067684b5c4b66d19d5c8cac4d80"),
    ),
];

/// 激活锁路线：上传通道用到的三个工具。
///
/// 原件用 OpenSSH + sshpass，但随包实际带的是 PuTTY 系的 PLINK/PSCP，用 `-pw`
/// 给口令，功能等价且不需要额外安装。
pub const SSH_TOOLS: &[(&str, &[u8])] = &[
    (
        "tools/PLINK.EXE",
        include_bytes!("../../assets/ssh/tools/PLINK.EXE"),
    ),
    (
        "tools/PSCP.EXE",
        include_bytes!("../../assets/ssh/tools/PSCP.EXE"),
    ),
    (
        "tools/iproxy.exe",
        include_bytes!("../../assets/ssh/tools/iproxy.exe"),
    ),
];

/// 激活锁路线：`mobileactivationd` 替换件的分片。
///
/// 15 个文件其实是 3 个唯一二进制（按 iOS 版本分），名字里的序号**不等于**系统
/// 版本，所以选择一律按 `minos` 而不是文件名。
macro_rules! ssh_chunks {
    ($($name:literal),* $(,)?) => {
        &[ $( (concat!("res/", $name), include_bytes!(concat!("../../assets/ssh/res/", $name))), )* ]
    };
}

pub const SSH_PAYLOAD: &[(&str, &[u8])] = ssh_chunks![
    "bypass2_1", "bypass2_2", "bypass2_3", "bypass2_4", "bypass2_5",
    "bypass3_1", "bypass3_2", "bypass3_3", "bypass3_4", "bypass3_5",
    "bypass4_1", "bypass4_2", "bypass4_3", "bypass4_4", "bypass4_5",
];

/// 安卓侧：`adb.exe` 与它的两个依赖。
///
/// 放在这里统一管理，首次运行时由
/// [`crate::android::ensure_tools`] 解到用户数据目录，之后 `find_program` 会优先用它。
pub const ANDROID_TOOLS: &[(&str, &[u8])] = &[
    (
        "adb.exe",
        include_bytes!("../../assets/android/tools/adb.exe"),
    ),
    (
        "AdbWinApi.dll",
        include_bytes!("../../assets/android/tools/AdbWinApi.dll"),
    ),
    (
        "AdbWinUsbApi.dll",
        include_bytes!("../../assets/android/tools/AdbWinUsbApi.dll"),
    ),
];

/// 把一组内置文件写到 `dir` 下（保持相对路径，自动建子目录）。///
/// 已存在的文件**不覆盖** —— 用户可能自己改过，覆盖会让他摸不着头脑。
pub fn write_all(dir: &Path, files: &[(&str, &[u8])]) -> Result<usize, String> {
    let mut written = 0usize;
    for (name, bytes) in files {
        let path = dir.join(name);
        if path.is_file() {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建 {} 失败：{e}", parent.display()))?;
        }
        std::fs::write(&path, bytes)
            .map_err(|e| format!("写入 {} 失败：{e}", path.display()))?;
        written += 1;
    }
    Ok(written)
}

/// 内置资源总体积，用于在报告里说明程序为什么这么大。
pub fn embedded_bytes() -> usize {
    let sum = |files: &[(&str, &[u8])]| files.iter().map(|(_, b)| b.len()).sum::<usize>();
    sum(LEGACY) + sum(MODERN_TOOLS) + sum(MODERN_BACKUP) + sum(SSH_TOOLS) + sum(SSH_PAYLOAD)
        + sum(ANDROID_TOOLS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_group_has_the_four_expected_files() {
        let names: Vec<&str> = LEGACY.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            names,
            vec![
                "GavalLegacyTools.zip",
                "GavalLegacyBackup.zip",
                "GavalLegacyInfo.plist",
                "GavalLegacyManifest.plist"
            ]
        );
    }

    #[test]
    fn modern_tools_include_the_restorer_and_dependencies() {
        let names: Vec<&str> = MODERN_TOOLS.iter().map(|(n, _)| *n).collect();
        assert!(names.contains(&"idevicebackup2.exe"));
        // 少一个 dll 都跑不起来，所以依赖必须整组在
        for required in [
            "imobiledevice.dll",
            "libcrypto-1_1-x64.dll",
            "libssl-1_1-x64.dll",
            "plist.dll",
            "usbmuxd.dll",
            "VCRUNTIME140.dll",
        ] {
            assert!(names.contains(&required), "{required} 缺失");
        }
    }

    #[test]
    fn modern_backup_has_the_four_required_plists_and_db() {
        let names: Vec<&str> = MODERN_BACKUP.iter().map(|(n, _)| *n).collect();
        for required in ["Info.plist", "Manifest.plist", "Manifest.db", "Status.plist"] {
            assert!(names.contains(&required), "{required} 缺失");
        }
        // 4 个哈希命名的 blob 也必须在，否则恢复会缺内容
        assert_eq!(names.iter().filter(|n| n.contains('/')).count(), 4);
    }

    #[test]
    fn embedded_payloads_are_not_empty_and_well_formed() {
        for (name, bytes) in LEGACY {
            assert!(!bytes.is_empty(), "{name} 是空的");
        }
        // 两个 zip 要能认出 ZIP 头
        let tools = LEGACY
            .iter()
            .find(|(n, _)| *n == "GavalLegacyTools.zip")
            .map(|(_, b)| *b)
            .unwrap();
        assert_eq!(&tools[..2], b"PK");
        // 两份 plist 是 XML
        for name in ["GavalLegacyInfo.plist", "GavalLegacyManifest.plist"] {
            let bytes = LEGACY.iter().find(|(n, _)| *n == name).map(|(_, b)| *b).unwrap();
            assert!(bytes.starts_with(b"<?xml"), "{name} 不是 XML");
        }
        let exe = MODERN_TOOLS
            .iter()
            .find(|(n, _)| *n == "idevicebackup2.exe")
            .map(|(_, b)| *b)
            .unwrap();
        assert_eq!(&exe[..2], b"MZ", "恢复器不是 PE");
    }

    #[test]
    fn write_all_is_idempotent_and_does_not_overwrite() {
        let dir = std::env::temp_dir().join(format!("atk-embed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let first = write_all(&dir, MODERN_BACKUP).unwrap();
        assert_eq!(first, MODERN_BACKUP.len());
        assert!(dir.join("Manifest.db").is_file());
        assert!(dir.join("1e").is_dir(), "子目录要建出来");

        // 用户改过的文件不能被覆盖
        std::fs::write(dir.join("Info.plist"), b"custom").unwrap();
        let second = write_all(&dir, MODERN_BACKUP).unwrap();
        assert_eq!(second, 0);
        assert_eq!(std::fs::read(dir.join("Info.plist")).unwrap(), b"custom");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn embedded_total_is_reported() {
        // 旧路线 ~34 MB，现代工具 ~4.4 MB
        assert!(embedded_bytes() > 30 * 1024 * 1024);
        assert!(embedded_bytes() < 80 * 1024 * 1024);
    }

    #[test]
    fn android_tools_are_a_usable_adb() {
        let names: Vec<&str> = ANDROID_TOOLS.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["adb.exe", "AdbWinApi.dll", "AdbWinUsbApi.dll"]);

        let adb = ANDROID_TOOLS
            .iter()
            .find(|(n, _)| *n == "adb.exe")
            .map(|(_, b)| *b)
            .unwrap();
        assert_eq!(&adb[..2], b"MZ", "adb.exe 不是 PE");
        assert!(adb.len() > 1_000_000, "adb.exe 体积不对");

        // 两个 dll 也必须是 PE，否则 adb 起来会缺依赖
        for name in ["AdbWinApi.dll", "AdbWinUsbApi.dll"] {
            let bytes = ANDROID_TOOLS
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, b)| *b)
                .unwrap();
            assert_eq!(&bytes[..2], b"MZ", "{name} 不是 PE");
        }

        // 写得出来，且幂等
        let dir = std::env::temp_dir().join(format!("atk-adb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(write_all(&dir, ANDROID_TOOLS).unwrap(), 3);
        assert!(dir.join("adb.exe").is_file());
        assert_eq!(write_all(&dir, ANDROID_TOOLS).unwrap(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
