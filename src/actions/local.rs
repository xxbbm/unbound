//! Local operations never require a connected phone or a lockdown session.
use super::Task;
use plist::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Path, PathBuf},
};

pub fn is_local(id: &str) -> bool {
    matches!(id, "firmware-info" | "backup-info" | "backup-copy")
}
pub fn validate(t: &Task) -> Result<(), String> {
    if ["backup-info", "backup-copy"].contains(&t.id.as_str()) && !Path::new(&t.input).is_dir() {
        return Err("请选择本地备份文件夹".into());
    }
    if t.id == "backup-copy" {
        if t.output.is_empty() {
            return Err("请选择复制目标文件夹".into());
        }
        backup_folder(Path::new(&t.input))?;
    }
    Ok(())
}
fn plist(path: &Path) -> Result<Value, String> {
    if fs::metadata(path).map_err(|e| e.to_string())?.len() > 16 * 1024 * 1024 {
        return Err("备份元数据异常过大".into());
    }
    Value::from_file(path).map_err(|e| format!("无法读取 {}：{e}", path.display()))
}
fn backup_folder(path: &Path) -> Result<(), String> {
    for name in ["Info.plist", "Manifest.plist", "Status.plist"] {
        if !path.join(name).is_file() {
            return Err(format!(
                "请选中单个备份目录（缺少 {name}），不是 Backup 父目录"
            ));
        }
    }
    let status = plist(&path.join("Status.plist"))?;
    if status
        .as_dictionary()
        .and_then(|d| d.get("SnapshotState"))
        .and_then(Value::as_string)
        != Some("finished")
    {
        return Err("备份未完成或仍在写入，请先结束备份再复制".into());
    }
    Ok(())
}
pub fn backup_info(path: &Path) -> Result<String, String> {
    let mut summary = vec![];
    let folders: Vec<PathBuf> = if path.join("Info.plist").is_file() {
        vec![path.into()]
    } else {
        fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.join("Info.plist").is_file())
            .take(200)
            .collect()
    };
    if folders.is_empty() {
        return Err("此目录没有找到标准 Apple 本地备份".into());
    }
    for folder in &folders {
        let info = plist(&folder.join("Info.plist"))?;
        let d = info.as_dictionary().ok_or("Info.plist 不是字典")?;
        println!("\n备份目录：{}", folder.display());
        summary.push(format!("备份目录：{}", folder.display()));
        for (label, key) in [
            ("设备名称", "Device Name"),
            ("机型", "Product Type"),
            ("系统版本", "Product Version"),
            ("序列号", "Serial Number"),
            ("设备标识", "Target Identifier"),
            ("备份时间", "Last Backup Date"),
        ] {
            let text = d
                .get(key)
                .and_then(|v| match v {
                    Value::Date(d) => Some(d.to_xml_format()),
                    _ => crate::inspection::text(v),
                })
                .unwrap_or_else(|| "未提供".into());
            println!("{label}：{text}");
            summary.push(format!("{label}：{text}"));
        }
        if let Ok(v) = plist(&folder.join("Manifest.plist")) {
            summary.push(format!(
                "加密：{}",
                match v
                    .as_dictionary()
                    .and_then(|d| d.get("IsEncrypted"))
                    .and_then(Value::as_boolean)
                {
                    Some(true) => "是",
                    Some(false) => "否",
                    None => "未提供",
                }
            ));
            println!(
                "加密：{}",
                match v
                    .as_dictionary()
                    .and_then(|d| d.get("IsEncrypted"))
                    .and_then(Value::as_boolean)
                {
                    Some(true) => "是",
                    Some(false) => "否",
                    None => "未提供",
                }
            );
        }
        println!(
            "完成状态：{}",
            if backup_folder(folder).is_ok() {
                "已完成"
            } else {
                "未完成或无法确认"
            }
        );
        summary.push(format!(
            "完成状态：{}\n",
            if backup_folder(folder).is_ok() {
                "已完成"
            } else {
                "未完成或无法确认"
            }
        ));
    }
    Ok(format!(
        "已读取 {} 份本地备份信息；无需连接手机。\n{}",
        folders.len(),
        summary.join("\n")
    ))
}
fn scan(root: &Path, base: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for item in fs::read_dir(root).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let meta = fs::symlink_metadata(item.path()).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err("备份包含链接或重解析点，停止复制".into());
            }
        }
        if meta.file_type().is_symlink() {
            return Err("备份包含符号链接，停止复制".into());
        }
        if meta.is_dir() {
            scan(&item.path(), base, out)?;
        } else if meta.is_file() {
            out.push(
                item.path()
                    .strip_prefix(base)
                    .map_err(|e| e.to_string())?
                    .to_path_buf(),
            );
        } else {
            return Err("备份包含非常规文件，停止复制".into());
        }
    }
    Ok(())
}
fn hash(path: &Path) -> Result<[u8; 32], String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut b = [0; 65536];
    loop {
        let n = file.read(&mut b).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(h.finalize().into())
}
pub fn copy_backup(source: &Path, parent: &Path) -> Result<String, String> {
    backup_folder(source)?;
    let source = source.canonicalize().map_err(|e| e.to_string())?;
    let parent = parent
        .canonicalize()
        .map_err(|e| format!("请先创建目标文件夹：{e}"))?;
    if parent.starts_with(&source) {
        return Err("目标不能在原备份内部".into());
    }
    let name = source.file_name().ok_or("备份文件夹名称无效")?;
    let dest = parent.join(name);
    if dest.exists() {
        return Err("目标已有同名备份，不会覆盖或合并".into());
    }
    let partial = parent.join(format!(
        "{}.Unbound-{}-partial",
        name.to_string_lossy(),
        std::process::id()
    ));
    let mut files = vec![];
    scan(&source, &source, &mut files)?;
    fs::create_dir(&partial).map_err(|e| e.to_string())?;
    let result = (|| {
        let mut digests = vec![];
        for (index, relative) in files.iter().enumerate() {
            let from = source.join(relative);
            let to = partial.join(relative);
            fs::create_dir_all(to.parent().ok_or("目标无父目录")?).map_err(|e| e.to_string())?;
            let before = hash(&from)?;
            let mut input = File::open(&from).map_err(|e| e.to_string())?;
            let mut output = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&to)
                .map_err(|e| e.to_string())?;
            std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
            output.sync_all().map_err(|e| e.to_string())?;
            if hash(&to)? != before || hash(&from)? != before {
                return Err("复制期间源备份发生变化或校验失败".to_string());
            }
            digests.push(before);
            if index % 100 == 0 || index + 1 == files.len() {
                println!("[STEP] 备份复制并校验：{}/{}", index + 1, files.len());
            }
        }
        let mut after = vec![];
        scan(&source, &source, &mut after)?;
        let mut sorted = files.clone();
        sorted.sort();
        after.sort();
        if after != sorted {
            return Err("源备份文件列表在复制期间改变".into());
        }
        for (relative, digest) in files.iter().zip(digests) {
            if hash(&source.join(relative))? != digest {
                return Err("源备份在最终校验前改变".into());
            }
        }
        backup_folder(&source)?;
        if dest.exists() {
            return Err("目标备份已存在，停止提交".into());
        }
        fs::rename(&partial, &dest).map_err(|e| e.to_string())?;
        Ok(format!(
            "已复制并逐文件 SHA-256 校验 {} 个文件：{}。原备份保留，未更改 iTunes 系统路径。",
            files.len(),
            dest.display()
        ))
    })();
    result.map_err(|e| {
        format!(
            "{e}；未完成文件保留于 {}，不会作为完成备份使用。",
            partial.display()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copies_finished_backup_without_overwriting_or_touching_source() {
        let base = std::env::temp_dir().join(format!("unbound-copy-test-{}", std::process::id()));
        let source = base.join("source").join("0".repeat(40));
        let target = base.join("destination");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        for name in ["Info.plist", "Manifest.plist"] {
            Value::Dictionary(Default::default())
                .to_file_xml(source.join(name))
                .unwrap();
        }
        let mut status = plist::Dictionary::new();
        status.insert("SnapshotState".into(), Value::String("finished".into()));
        Value::Dictionary(status)
            .to_file_xml(source.join("Status.plist"))
            .unwrap();
        fs::write(source.join("test-data"), b"actual backup bytes").unwrap();
        assert!(copy_backup(&source, &source).is_err());
        assert!(copy_backup(&source, &target).is_ok());
        assert_eq!(
            hash(&source.join("test-data")).unwrap(),
            hash(&target.join("0".repeat(40)).join("test-data")).unwrap()
        );
        assert!(copy_backup(&source, &target).is_err());
        fs::write(source.join("Status.plist"), b"unfinished").unwrap();
        assert!(copy_backup(&source, &target).is_err());
        fs::remove_dir_all(base).unwrap();
    }
}
