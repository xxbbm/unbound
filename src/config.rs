use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// 每个工具单独存储绝对路径；不搜索 PATH，避免误调用同名程序。
    pub executables: BTreeMap<String, String>,
    pub mdm_source: String,
    pub mdm_workspace: String,
}

impl Config {
    pub fn path() -> Result<PathBuf, String> {
        directories::ProjectDirs::from("local", "AppleToolkit", "AppleToolkit")
            .map(|p| p.config_dir().join("config.json"))
            .ok_or_else(|| "无法确定用户配置目录".into())
    }

    pub fn load() -> Result<Self, String> {
        let path = Self::path()?;
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("配置损坏 {}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("读取配置失败: {e}")),
        }
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::path()?;
        let parent = path.parent().ok_or("配置路径缺少父目录")?;
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        // 同目录临时文件。Windows 上 rename 不替换已有文件，使用带备份的替换。
        let temp = path.with_extension("tmp");
        std::fs::write(
            &temp,
            serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let backup = path.with_extension("bak");
        if path.exists() {
            std::fs::copy(&path, &backup).map_err(|e| e.to_string())?;
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        if let Err(e) = std::fs::rename(&temp, &path) {
            if backup.exists() {
                let _ = std::fs::copy(&backup, &path);
            }
            return Err(e.to_string());
        }
        Ok(())
    }
}

pub fn bundled_path(name: &str) -> PathBuf {
    let base = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("tools")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}
