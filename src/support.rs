//! 支持提示：什么时候弹、弹过没有、用户说过「不再提示」没有。
//!
//! 三件事分开：
//!
//! * **时机** —— 累计使用到一定时长才弹，不在刚打开就伸手。
//! * **本会话只弹一次** —— 关掉了就不再骚扰，除非重启。
//! * **永久不再提示** —— 用户明确点过，就写一个标记文件，之后永不再弹。

/// 累计使用多久后弹（秒）。20 分钟。
pub const AFTER_SECONDS: f32 = 20.0 * 60.0;

/// 标记文件位置。用项目自己的数据目录，不污染用户目录。
fn marker_path() -> Option<std::path::PathBuf> {
    directories::ProjectDirs::from("local", "AppleToolkit", "AppleToolkit")
        .map(|dirs| dirs.data_local_dir().join("support-dismissed"))
}

pub fn dismissed() -> bool {
    marker_path().map(|path| path.is_file()).unwrap_or(false)
}

/// 记下「不再提示」。写失败也不影响使用，只是下次还会弹一次。
pub fn set_dismissed() {
    let Some(path) = marker_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, b"1");
}

/// 是否该弹。
pub fn should_prompt(elapsed: f32, dismissed: bool, already_shown: bool) -> bool {
    !dismissed && !already_shown && elapsed >= AFTER_SECONDS
}

/// 把两张收款码的原图写到桌面，方便用手机从相册识别。
///
/// 程序里显示的是缩小后的贴图，截图再扫容易糊；原图是 1152 px，扫得稳。
pub fn save_images() -> Result<std::path::PathBuf, String> {
    let base = directories::UserDirs::new()
        .and_then(|dirs| dirs.desktop_dir().map(|dir| dir.to_path_buf()))
        .unwrap_or_else(std::env::temp_dir);
    let dir = base.join("Unbound-支持码");
    std::fs::create_dir_all(&dir).map_err(|e| format!("创建 {} 失败：{e}", dir.display()))?;
    for (name, bytes) in [
        ("微信赞赏码.jpg", crate::design::SUPPORT_REWARD),
        ("支付宝.jpg", crate::design::SUPPORT_ALIPAY),
    ] {
        let path = dir.join(name);
        std::fs::write(&path, bytes).map_err(|e| format!("写入 {} 失败：{e}", path.display()))?;
    }
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_only_after_the_threshold() {
        assert!(!should_prompt(AFTER_SECONDS - 1.0, false, false));
        assert!(!should_prompt(0.0, false, false));
        assert!(should_prompt(AFTER_SECONDS, false, false));
        assert!(should_prompt(AFTER_SECONDS * 3.0, false, false));
    }

    #[test]
    fn never_prompts_after_dismiss_or_once_shown() {
        assert!(!should_prompt(AFTER_SECONDS + 1.0, true, false));
        assert!(!should_prompt(AFTER_SECONDS + 1.0, false, true));
        assert!(!should_prompt(AFTER_SECONDS + 1.0, true, true));
    }
}
