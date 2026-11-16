//! 配置锁绕过的本地实现。
//!
//! 逻辑来自逆向 `iFineMDM` / `UnlockGo` / `Aiseesoft`，并已按
//! `apple-toolkit-windows-0.2/MDM-FINDINGS.md` 的 CIL 复核结论对齐 ——
//! 那份结论比早期报告更可靠，凡有冲突以它为准。
//!
//! # 路线由系统主版本决定，不是由越狱状态决定
//!
//! 原程序的 `MdmWorkflowRouter.SelectRoute` 按主版本分支，并**拒绝更高版本**：
//!
//! | 设备主版本 | 路线 | 备份来源 | 恢复器 | 完成标记 |
//! |---|---|---|---|---|
//! | < 27 | [`Route::LegacyGaval`] | `GavalLegacyBackup.zip` | 专用恢复器（空 argv，cwd 为 `任务目录/ikey`） | `IKEY MDM Successful.` |
//! | = 27 | [`Route::Mdm27`] | 保持原本地备份字节 | `idevicebackup2.exe`（带 `-s <源>`） | 无独立标记，用同一组正标记 |
//! | > 27 | [`Route::Unsupported`] | 不适用 | 不适用 | 不适用 |
//!
//! **这一点容易搞错**：`Gaval27ProtectionPlist.PatchInfo/PatchManifest` 属于
//! `Gaval27ProtectionBackup.BuildFromArchive` 的**防护包**逻辑（`/Erase27/` 那条线），
//! 不在 `BackupClient.RequestAsync → MdmWorkflow` 的 MDM 主链上。所以：
//!
//! * **Legacy 路线**会用当前设备信息改写其中的 plist（[`rekey`] 就是这段）。
//! * **Mdm27 路线保留原本地备份字节**，不混用防护包改写，也不新增 manifest 未引用的文件。
//!
//! 早期分析把这类改写泛化成 MDM 主流程，依据不足，已在本模块纠正。
//!
//! # 执行顺序（`MdmWorkflow.RunAsync`）
//!
//! ```text
//! 设备预检 -> 激活 -> 获取备份 -> 恢复 -> 等待重连 -> 读取最终状态 -> 上报厂商状态
//! ```
//!
//! **HTTP 200 不代表恢复完成。** 完成判定必须来自恢复器输出里的完成标记，
//! 并且最终以设备上的远程管理界面为准 —— `ActivationState == Activated`
//! 不能证明 MDM 服务器分配已解除。
//!
//! # 安全姿态
//!
//! * 不隐式联网：[`backend`] 只监听回环，也不做上游转发。
//! * 不猜设备：改写只覆盖**已存在**的键；缺值只记录，不填默认值。
//! * 不静默改写：[`rekey`] 返回逐键报告，调用方必须显示给用户。
//! * 破坏性：还原会覆盖设备资料，调用方必须先要求用户输入设备序列号确认。

pub mod activation;
pub mod backend;
pub mod crypto;
pub mod exec;
pub mod judge;
pub mod payloads;
pub mod pipeline;
pub mod rekey;
pub mod resources;
pub mod secrets;
pub mod ssh;

pub use crypto::{Envelope, VersionEnvelope};
pub use exec::{Outcome, Plan};
pub use judge::Verdict;
pub use payloads::Payload;
pub use pipeline::{PreparedCommand, Stage, StageKind, TaskLayout};
pub use secrets::Secrets;

/// 恢复完成标记。
///
/// **只找到这一个**：扫描 `iFineMDM.exe` 的全部 `#US` 字符串，`Successful` 相关只有
/// `IKEY MDM Successful.`。没有发现独立的现代路线标记（早前笔记里的
/// `Restore Successful.` 在本次扫描中不存在）。因此现代路线也用同一组正标记判定，
/// 详见 [`judge`]。
pub const MARKER_LEGACY: &str = "IKEY MDM Successful.";

/// 原版恢复器已知的异常退出码。
///
/// `0xC0000374`（堆损坏）只可在**明确完成标记**与最终设备核验都通过后继续，
/// 不能无条件忽略。
pub const KNOWN_FAULT_CODES: [i32; 1] = [0xC000_0374u32 as i32];

/// MDM 路线。判据是系统主版本，不是越狱状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// 主版本 < 27：四份内嵌资源 + 专用恢复器。
    LegacyGaval,
    /// 主版本 == 27：`MixIos27Restore`，备份字节保持原样。
    Mdm27,
    /// 主版本 > 27：原程序明确拒绝。
    Unsupported,
}

impl Route {
    pub fn label(self) -> &'static str {
        match self {
            Route::LegacyGaval => "旧路线（GavalLegacy，主版本 < 27）",
            Route::Mdm27 => "现代路线（MixIos27Restore，主版本 = 27）",
            Route::Unsupported => "不支持的版本（主版本 > 27）",
        }
    }
    /// 该路线是否需要按当前设备改写备份里的 plist。
    ///
    /// 现代路线**不需要** —— 它保留原本地备份字节。
    pub fn rewrites_device_identity(self) -> bool {
        matches!(self, Route::LegacyGaval)
    }
    /// 该路线的正标记。
    ///
    /// 两条路线用同一个标记 —— 扫描没有发现现代路线的独立标记。
    pub fn completion_marker(self) -> Option<&'static str> {
        match self {
            Route::LegacyGaval | Route::Mdm27 => Some(MARKER_LEGACY),
            Route::Unsupported => None,
        }
    }
    pub fn steps(self) -> &'static [&'static str] {
        match self {
            Route::LegacyGaval => LEGACY_STEPS,
            Route::Mdm27 => MDM27_STEPS,
            Route::Unsupported => &[],
        }
    }
}

const LEGACY_STEPS: &[&str] = &[
    "设备预检",
    "激活设备（ideviceactivation）",
    "取 GavalLegacyBackup.zip 并按当前设备改写其中 plist",
    "解出任务目录：备份放到 ikey/<UDID>，TEMP/TMP 指向本次任务目录",
    "以空 argv、cwd=任务目录/ikey 启动专用恢复器",
    "等待重连并读取最终状态",
];

const MDM27_STEPS: &[&str] = &[
    "设备预检",
    "激活设备（ideviceactivation）",
    "取服务端备份：保持原始字节，不做防护包改写",
    "校验必需文件 Info.plist / Manifest.plist / Manifest.db / Status.plist",
    "从 Manifest.plist 的 Lockdown 读出原设备标识（作为 -s）",
    "启动 idevicebackup2.exe restore -u <目标> -s <源> --system --settings --remove --reboot --password <口令> ./",
    "等待重连并读取最终状态",
];

/// 按系统主版本选路。`major` 是设备版本号的主版本位。
///
/// 与 `MdmWorkflowRouter.SelectRoute` 一致：>27 直接拒绝，不做"尽力而为"。
pub fn route(major: u32) -> Route {
    match major {
        0..=26 => Route::LegacyGaval,
        27 => Route::Mdm27,
        _ => Route::Unsupported,
    }
}

/// 恢复器输出是否给出了完成标记。
///
/// 只看输出内容，不看退出码 —— HTTP 200 或进程退 0 都不代表恢复完成。
/// 实现在 [`judge`]，这里只是薄封装，避免两处各写一份判定。
pub fn completion(route: Route, output: &str) -> Completion {
    match judge::judge(route, output).state {
        judge::State::Complete => Completion::Complete,
        judge::State::Incomplete => Completion::Incomplete,
        judge::State::Doubtful => Completion::Doubtful,
        judge::State::Unsupported => Completion::Unsupported,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Completion {
    Complete,
    Incomplete,
    /// 出现了完成标记，但输出里也有错误内容 —— 必须人工判断。
    Doubtful,
    Unsupported,
}

/// 退出码是否属于"仅在已确认完成时才可容忍"的那一类。
pub fn tolerated_fault(route: Route, code: i32, output: &str) -> bool {
    judge::tolerated_fault(route, code, output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_follows_major_version_not_jailbreak() {
        assert_eq!(route(16), Route::LegacyGaval);
        assert_eq!(route(26), Route::LegacyGaval);
        assert_eq!(route(27), Route::Mdm27);
        assert_eq!(route(28), Route::Unsupported);
        assert_eq!(route(30), Route::Unsupported);
    }

    #[test]
    fn only_legacy_rewrites_identity() {
        assert!(Route::LegacyGaval.rewrites_device_identity());
        assert!(!Route::Mdm27.rewrites_device_identity());
        assert!(!Route::Unsupported.rewrites_device_identity());
    }

    #[test]
    fn markers_are_recognized_but_errors_downgrade_to_doubtful() {
        assert_eq!(
            completion(Route::LegacyGaval, "… IKEY MDM Successful. …"),
            Completion::Complete
        );
        assert_eq!(
            completion(Route::Mdm27, "… IKEY MDM Successful. …"),
            Completion::Complete
        );
        // 有标记但同时有错误内容 → 可疑，不是完成
        assert_eq!(
            completion(Route::Mdm27, "IKEY MDM Successful. Error: timeout"),
            Completion::Doubtful
        );
        assert_eq!(completion(Route::Unsupported, "IKEY MDM Successful."), Completion::Unsupported);
    }

    #[test]
    fn http_200_alone_is_not_completion() {
        assert_eq!(completion(Route::Mdm27, "HTTP 200 OK"), Completion::Incomplete);
        assert_eq!(completion(Route::Mdm27, ""), Completion::Incomplete);
    }

    #[test]
    fn heap_corruption_needs_the_marker() {
        let code = KNOWN_FAULT_CODES[0];
        assert!(!tolerated_fault(Route::Mdm27, code, "Started"));
        assert!(tolerated_fault(Route::Mdm27, code, "IKEY MDM Successful."));
        assert!(!tolerated_fault(Route::Mdm27, 1, "IKEY MDM Successful."));
        // 有标记但有错误内容 → 可疑，不容忍
        assert!(!tolerated_fault(Route::Mdm27, code, "IKEY MDM Successful. Error"));
    }

    #[test]
    fn unsupported_has_no_steps() {
        assert!(Route::Unsupported.steps().is_empty());
        assert!(!Route::LegacyGaval.steps().is_empty());
        assert!(!Route::Mdm27.steps().is_empty());
    }
}
