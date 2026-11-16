//! 独立发布包：组件嵌入当前 EXE，自动校验、释放到当前用户的应用缓存。
//! 不读取桌面的工具目录，不运行原商业程序的界面，不需要管理员权限。
#[cfg(any(not(target_os = "macos"), test))]
use sha2::{Digest, Sha256};
use std::path::PathBuf;
#[cfg(any(not(target_os = "macos"), test))]
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
    path::Path,
};
#[cfg(any(not(target_os = "macos"), test))]
const ARCHIVE: &[u8] = include_bytes!("../assets/runtime.zip");
#[cfg(any(not(target_os = "macos"), test))]
const INDEX: &str = include_str!("../assets/runtime-index.json");

#[cfg(any(not(target_os = "macos"), test))]
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
#[cfg(any(not(target_os = "macos"), test))]
fn verify(root: &Path, index: &BTreeMap<String, String>) -> Result<(), String> {
    for (name, hash) in index {
        let path = root.join(name);
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err("内置组件类型不正确".into());
        }
        let data = std::fs::read(path).map_err(|e| e.to_string())?;
        if digest(&data) != *hash {
            return Err(format!("内置组件校验失败: {name}"));
        }
    }
    Ok(())
}
#[cfg(target_os = "macos")]
pub fn ensure_runtime() -> Result<PathBuf, String> {
    mac_runtime()
}
#[cfg(not(target_os = "macos"))]
pub fn ensure_runtime() -> Result<PathBuf, String> {
    if !cfg!(windows) {
        return Err("此独立发布包内置的是Windows设备后端；macOS恢复后端尚未完成。".into());
    }
    let data = directories::ProjectDirs::from("local", "AppleToolkit", "AppleToolkit")
        .ok_or("无法定位应用缓存目录")?;
    ensure_at(&data.data_local_dir().join("runtime"))
}
#[cfg(any(not(target_os = "macos"), test))]
pub(crate) fn ensure_at(parent: &Path) -> Result<PathBuf, String> {
    let index: BTreeMap<String, String> = serde_json::from_str(INDEX).map_err(|e| e.to_string())?;
    let fingerprint = digest(INDEX.as_bytes());
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let root = parent.join(format!("0.7-{}", &fingerprint[..12]));
    if root.is_dir() && verify(&root, &index).is_ok() {
        return Ok(root);
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let staging = parent.join(format!("install-{}-{stamp}", std::process::id()));
    std::fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(Cursor::new(ARCHIVE)).map_err(|e| e.to_string())?;
    let mut files = 0;
    let mut total = 0u64;
    for n in 0..archive.len() {
        let mut item = archive.by_index(n).map_err(|e| e.to_string())?;
        if item.is_dir() {
            continue;
        }
        if item.unix_mode().is_some_and(|m| m & 0xf000 == 0xa000) {
            return Err("内置组件不允许符号链接".into());
        }
        let relative = item.enclosed_name().ok_or("内置组件路径无效")?;
        let name = relative.to_string_lossy().replace('\\', "/");
        let expected = index.get(&name).ok_or("内置组件清单不匹配")?;
        total = total.checked_add(item.size()).ok_or("内置组件长度溢出")?;
        if total > 256 * 1024 * 1024 {
            return Err("内置组件超过大小上限".into());
        }
        let path = staging.join(relative);
        std::fs::create_dir_all(path.parent().ok_or("组件路径无父目录")?)
            .map_err(|e| e.to_string())?;
        let declared = item.size();
        let mut bytes = Vec::new();
        item.by_ref()
            .take(declared + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 != declared || digest(&bytes) != *expected {
            return Err("内置组件内容不完整".into());
        }
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        files += 1;
    }
    if files != index.len() {
        return Err("内置组件数量不完整".into());
    }
    verify(&staging, &index)?;
    if root.exists() {
        // 保留损坏缓存供诊断，不覆盖或递归删除用户目录。
        if verify(&root, &index).is_ok() {
            return Ok(root);
        }
        std::fs::rename(&root, parent.join(format!("damaged-{stamp}")))
            .map_err(|e| e.to_string())?;
    }
    match std::fs::rename(&staging, &root) {
        Ok(()) => Ok(root),
        Err(_) if verify(&root, &index).is_ok() => Ok(root),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg(target_os = "macos")]
fn mac_runtime() -> Result<PathBuf, String> {
    let data = directories::ProjectDirs::from("local", "AppleToolkit", "AppleToolkit")
        .ok_or("无法确定应用目录")?;
    let root = data.data_local_dir().join("native-0.4");
    let normal = root.join("Tools/NormalMode");
    std::fs::create_dir_all(&normal).map_err(|e| e.to_string())?;
    for name in ["idevice_id", "ideviceinfo", "idevicepair"] {
        let source = crate::actions::resolve_program(&root, name, "", name);
        if !source.is_file() {
            return Err(format!(
                "macOS 缺少原生 {name}。请安装 libimobiledevice，或放入应用同级 tools/ 文件夹。"
            ));
        }
        let destination = normal.join(name);
        if !destination.exists() {
            std::os::unix::fs::symlink(source, destination).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(
        root.join("bundle-manifest.json"),
        b"{\"format\":2,\"platform\":\"macos\",\"native_tools\":true}",
    )
    .map_err(|e| e.to_string())?;
    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standalone_assets_install_without_vendor_directory() {
        let parent = std::env::temp_dir().join(format!("atk-runtime-test-{}", std::process::id()));
        let root = ensure_at(&parent).unwrap();
        assert!(root.join("Tools/NormalMode/idevice_id.exe").is_file());
        assert!(!root.join("Media/ffmpeg.exe").exists());
        assert!(!root.join("iFineMDM.exe").exists());
        let expected = root.clone();
        assert_eq!(ensure_at(&parent).unwrap(), expected);
        // 同时验证组件被篡改后不会继续使用。
        std::fs::write(root.join("Tools/NormalMode/idevice_id.exe"), b"corrupt").unwrap();
        let recovered = ensure_at(&parent).unwrap();
        assert!(
            std::fs::metadata(recovered.join("Tools/NormalMode/idevice_id.exe"))
                .unwrap()
                .len()
                > 1000
        );
    }
}
