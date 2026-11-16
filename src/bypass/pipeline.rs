//! 任务目录布局、恢复命令与阶段表。
//!
//! 每条结论都标了来源：
//!
//! * **本模块扫描**：`iFineMDM.exe` 的字符串证据（`ikey`、`owned_temporary_files`、
//!   `MDM_SETUP.exe`、`GavalLegacy*.plist`、`First/Second restore complete`、
//!   `Waiting for device` 等）。
//! * **FINDINGS**：`apple-toolkit-windows-0.2/MDM-FINDINGS.md` 的 CIL 复核结论，
//!   用于我扫不到的部分（例如恢复器 argv 的具体构成）。
//!
//! 两者冲突时以 FINDINGS 为准 —— 那是读方法体得到的，比字符串强。

use crate::bypass::Route;
use std::path::{Path, PathBuf};

/// 旧路线的嵌入式资源名（扫描所得）。
pub const LEGACY_TOOLS_ZIP: &str = "GavalLegacyTools.zip";
pub const LEGACY_BACKUP_ZIP: &str = "GavalLegacyBackup.zip";
pub const LEGACY_INFO_PLIST: &str = "GavalLegacyInfo.plist";
pub const LEGACY_MANIFEST_PLIST: &str = "GavalLegacyManifest.plist";
pub const LEGACY_RESTORER: &str = "MDM_SETUP.exe";

/// 任务目录里的子目录名（扫描所得）。
pub const IKEY_DIR: &str = "ikey";
pub const OWNED_TEMP_DIR: &str = "owned_temporary_files";

// ---------------------------------------------------------------- 现代路线
//
// 全部取自 `IFineMDMWindows.Workflow.MixIos27PreparedRestore.CreateStartInfo`
// （IL 287 B / 109 条，file_offset 0x16EAC）。它不是自定义恢复器 —— 就是
// `idevicebackup2.exe`，参数顺序如下，数组长度在建数组时写死为 12。

pub const MODERN_RESTORER: &str = "idevicebackup2.exe";

/// 恢复开关，顺序即 argv 顺序。
pub const MODERN_SWITCHES: [&str; 4] = ["--system", "--settings", "--remove", "--reboot"];

/// 服务端备份必须包含的文件；缺一个就报"服务器备份缺少必需文件"。
pub const MODERN_REQUIRED_FILES: [&str; 4] =
    ["Info.plist", "Manifest.plist", "Manifest.db", "Status.plist"];

/// 临时目录前缀，以及它的合法性正则 `\Aifinemdm-mix27-[a-f0-9]{32}\z`
/// —— 清理前要先验名字，避免删到别处。
pub const MODERN_TEMP_PREFIX: &str = "ifinemdm-mix27-";

/// 组装现代路线的 argv（12 项）。
///
/// ```text
/// restore -u <目标UDID> -s <源UDID> --system --settings --remove --reboot --password <口令> ./
/// ```
///
/// `./` 是备份目录 —— 因为 `WorkingDirectory` 就是任务临时目录，备份文件直接落在那里。
/// `-s` 是**源**设备标识（服务端那份备份原来的机器），为空会被判"无效原设备标识"。
pub fn modern_argv(target_udid: &str, source_udid: &str, password: &str) -> Vec<String> {
    let mut argv = Vec::with_capacity(12);
    argv.push("restore".to_string());
    argv.push("-u".to_string());
    argv.push(target_udid.to_string());
    argv.push("-s".to_string());
    argv.push(source_udid.to_string());
    for switch in MODERN_SWITCHES {
        argv.push(switch.to_string());
    }
    argv.push("--password".to_string());
    argv.push(password.to_string());
    argv.push("./".to_string());
    argv
}

/// 检查服务端备份是否包含必需文件。返回缺失项。
pub fn missing_required_files(dir: &std::path::Path) -> Vec<&'static str> {
    MODERN_REQUIRED_FILES
        .iter()
        .copied()
        .filter(|name| {
            let path = dir.join(name);
            !path.is_file()
        })
        .collect()
}

// ---------------------------------------------------------------- 阶段表

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StageKind {
    Preflight,
    BuildBackup,
    WaitDevice,
    Restore { round: u8 },
    WaitRestart { round: u8 },
    Activate,
    ReadState,
    Report,
}

#[derive(Clone, Copy, Debug)]
pub struct Stage {
    pub id: &'static str,
    pub kind: StageKind,
    /// 该阶段应观察到的输出提示；用于展示进度，不参与完成判定。
    pub hint: Option<&'static str>,
}

const fn stage(id: &'static str, kind: StageKind, hint: Option<&'static str>) -> Stage {
    Stage { id, kind, hint }
}

const LEGACY_STAGES: &[Stage] = &[
    stage("preflight", StageKind::Preflight, None),
    stage("build-backup", StageKind::BuildBackup, Some("buildBackup")),
    stage("wait-device", StageKind::WaitDevice, Some("Waiting for device")),
    stage("restore-1", StageKind::Restore { round: 1 }, None),
    stage(
        "wait-restart-1",
        StageKind::WaitRestart { round: 1 },
        Some("First restore complete, waiting for device restart"),
    ),
    stage("restore-2", StageKind::Restore { round: 2 }, None),
    stage(
        "wait-restart-2",
        StageKind::WaitRestart { round: 2 },
        Some("Second restore complete, waiting for final restart"),
    ),
    stage("activate", StageKind::Activate, Some("Apple activation complete")),
    stage("read-state", StageKind::ReadState, None),
    stage("report", StageKind::Report, None),
];

const MDM27_STAGES: &[Stage] = &[
    stage("preflight", StageKind::Preflight, None),
    stage("build-backup", StageKind::BuildBackup, None),
    stage("wait-device", StageKind::WaitDevice, Some("Waiting for device")),
    stage("restore-1", StageKind::Restore { round: 1 }, None),
    stage(
        "wait-restart-1",
        StageKind::WaitRestart { round: 1 },
        Some("First restore complete, waiting for device restart"),
    ),
    stage("restore-2", StageKind::Restore { round: 2 }, None),
    stage(
        "wait-restart-2",
        StageKind::WaitRestart { round: 2 },
        Some("Second restore complete, waiting for final restart"),
    ),
    stage("activate", StageKind::Activate, Some("Apple activation complete")),
    stage("read-state", StageKind::ReadState, None),
    stage("report", StageKind::Report, None),
];

/// 该路线的阶段表。不支持时为空。
pub fn stages(route: Route) -> &'static [Stage] {
    match route {
        Route::LegacyGaval => LEGACY_STAGES,
        Route::Mdm27 => MDM27_STAGES,
        Route::Unsupported => &[],
    }
}

/// 恢复轮数。
///
/// **这是推断**：`First restore complete` / `Second restore complete` 两条提示说明
/// 存在两次恢复，但我没有从方法体里读出分支条件。若实际只有一轮，改这里即可 ——
/// 阶段表是从这个数生成的语义，不要在两个地方各写一份。
pub fn restore_rounds() -> u8 {
    2
}

// ---------------------------------------------------------------- 目录布局

/// 一次任务的工作目录。
#[derive(Clone, Debug)]
pub struct TaskLayout {
    root: PathBuf,
    udid: String,
}

impl TaskLayout {
    pub fn new(root: impl Into<PathBuf>, udid: &str) -> Self {
        Self {
            root: root.into(),
            udid: udid.to_string(),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `任务目录/ikey` —— 也是旧路线恢复器的工作目录。
    pub fn ikey(&self) -> PathBuf {
        self.root.join(IKEY_DIR)
    }

    /// `任务目录/ikey/<UDID>` —— 备份落点。
    pub fn backup(&self) -> PathBuf {
        self.ikey().join(&self.udid)
    }

    /// `任务目录/owned_temporary_files`
    pub fn owned_temp(&self) -> PathBuf {
        self.root.join(OWNED_TEMP_DIR)
    }

    /// TEMP / TMP 指向本次任务目录，避免污染系统临时目录。
    pub fn env(&self) -> Vec<(String, String)> {
        vec![
            ("TEMP".to_string(), self.root.display().to_string()),
            ("TMP".to_string(), self.root.display().to_string()),
        ]
    }

    /// 建出所需目录。已存在不算错。
    pub fn create(&self) -> Result<(), String> {
        for dir in [self.ikey(), self.backup(), self.owned_temp()] {
            std::fs::create_dir_all(&dir)
                .map_err(|e| format!("创建 {} 失败：{e}", dir.display()))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------- 恢复命令

/// 一条准备好的启动命令。`argv` 为空是合法情形（旧路线）。
#[derive(Clone, Debug)]
pub struct PreparedCommand {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
    /// 有值时调用方必须经 argv 传入；**不要写进日志**。
    pub password: Option<String>,
}

/// 旧路线恢复器：**空 argv**，cwd 为 `任务目录/ikey`。
///
/// 来源：FINDINGS 第 8 条（`LegacyMdmProcessRunner.CreateStartInfo`）。
/// 这条很反直觉 —— 不给参数，全靠 cwd 与目录约定。照抄比猜参数安全。
pub fn legacy_restore(layout: &TaskLayout, program: impl Into<PathBuf>) -> PreparedCommand {
    PreparedCommand {
        program: program.into(),
        args: Vec::new(),
        cwd: layout.ikey(),
        env: layout.env(),
        password: None,
    }
}

/// 现代路线恢复器：参数、密码、cwd 由调用方给出。
///
/// 本模块没有还原 `MixIos27PreparedRestore.CreateStartInfo` 的全部内容，
/// 所以这里**不做推断**：缺参数就报错，由调用方补。
pub fn modern_restore(
    layout: &TaskLayout,
    program: impl Into<PathBuf>,
    args: Vec<String>,
    password: Option<String>,
) -> Result<PreparedCommand, String> {
    if args.is_empty() {
        return Err("现代路线恢复器参数未提供；该路线的 argv 未完整还原".into());
    }
    Ok(PreparedCommand {
        program: program.into(),
        args,
        cwd: layout.root().to_path_buf(),
        env: layout.env(),
        password,
    })
}

/// 现代路线恢复器命令。
///
/// 与旧路线相反：这里有明确参数，cwd 是**任务根目录**（备份文件就落在那里），
/// 并把 PATH / TEMP / TMP 都指向本次任务，避免污染系统环境。
pub fn modern_command(
    layout: &TaskLayout,
    tool_root: &Path,
    target_udid: &str,
    source_udid: &str,
    password: &str,
) -> PreparedCommand {
    let mut env = layout.env();
    // 原程序: PATH = ToolRoot + ";" + GetFolderPath(37)，37 即 System 目录
    let system = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".to_string());
    env.push((
        "PATH".to_string(),
        format!(
            "{};{system}\\System32",
            tool_root.display()
        ),
    ));
    PreparedCommand {
        program: tool_root.join(MODERN_RESTORER),
        args: modern_argv(target_udid, source_udid, password),
        cwd: layout.root().to_path_buf(),
        env,
        password: Some(password.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout() -> TaskLayout {
        TaskLayout::new("C:/tmp/task1", "00008110-000A1C2E3F40001E")
    }

    #[test]
    fn modern_argv_matches_the_recovered_shape() {
        let argv = modern_argv("TGT", "SRC", "PW");
        assert_eq!(argv.len(), 12, "CreateStartInfo 建数组时长度写死为 12");
        assert_eq!(argv[0], "restore");
        assert_eq!(argv[1], "-u");
        assert_eq!(argv[2], "TGT");
        assert_eq!(argv[3], "-s");
        assert_eq!(argv[4], "SRC");
        assert_eq!(
            &argv[5..9],
            &["--system", "--settings", "--remove", "--reboot"],
            "开关顺序不能变"
        );
        assert_eq!(argv[9], "--password");
        assert_eq!(argv[10], "PW");
        assert_eq!(argv[11], "./", "备份目录相对任务根目录");
    }

    #[test]
    fn modern_command_uses_tool_root_and_task_cwd() {
        let layout = layout();
        let cmd = modern_command(
            &layout,
            std::path::Path::new("C:/tools/mix27"),
            "TGT",
            "SRC",
            "PW",
        );
        assert!(cmd.program.to_string_lossy().ends_with(MODERN_RESTORER));
        assert_eq!(cmd.cwd, layout.root());
        // TEMP / TMP / PATH 三项都要设
        let keys: Vec<&str> = cmd.env.iter().map(|(k, _)| k.as_str()).collect();
        assert!(keys.contains(&"TEMP"));
        assert!(keys.contains(&"TMP"));
        assert!(keys.contains(&"PATH"));
        let path = cmd
            .env
            .iter()
            .find(|(k, _)| k == "PATH")
            .map(|(_, v)| v.clone())
            .unwrap();
        assert!(path.contains("mix27"), "PATH 应包含工具目录：{path}");
        assert_eq!(cmd.password.as_deref(), Some("PW"));
    }

    #[test]
    fn missing_required_files_are_reported() {
        let dir = std::env::temp_dir().join(format!("atk-modern-req-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(missing_required_files(&dir).len(), 4);
        for name in MODERN_REQUIRED_FILES {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        assert!(missing_required_files(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stages_exist_for_supported_routes() {
        assert_eq!(stages(Route::LegacyGaval).len(), LEGACY_STAGES.len());
        assert_eq!(stages(Route::Mdm27).len(), MDM27_STAGES.len());
        assert!(stages(Route::Unsupported).is_empty());
    }

    #[test]
    fn stage_ids_are_unique_and_ordered() {
        for route in [Route::LegacyGaval, Route::Mdm27] {
            let ids: Vec<&str> = stages(route).iter().map(|s| s.id).collect();
            let mut sorted = ids.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(sorted.len(), ids.len(), "阶段 id 重复：{ids:?}");
            assert_eq!(ids.first(), Some(&"preflight"));
            assert_eq!(ids.last(), Some(&"report"));
        }
    }

    #[test]
    fn both_routes_restore_twice() {
        for route in [Route::LegacyGaval, Route::Mdm27] {
            let restores: Vec<u8> = stages(route)
                .iter()
                .filter_map(|s| match s.kind {
                    StageKind::Restore { round } => Some(round),
                    _ => None,
                })
                .collect();
            assert_eq!(restores, vec![1, 2]);
        }
        assert_eq!(restore_rounds(), 2);
    }

    #[test]
    fn wait_stages_carry_the_recovered_hints() {
        let waits: Vec<&str> = stages(Route::Mdm27)
            .iter()
            .filter_map(|s| s.hint)
            .collect();
        assert!(waits.contains(&"Waiting for device"));
        assert!(waits.contains(&"First restore complete, waiting for device restart"));
        assert!(waits.contains(&"Second restore complete, waiting for final restart"));
    }

    #[test]
    fn layout_paths_match_the_recovered_names() {
        let l = layout();
        assert!(l.ikey().ends_with("ikey"));
        assert!(l.backup().ends_with("00008110-000A1C2E3F40001E"));
        assert_eq!(l.backup().parent(), Some(l.ikey().as_path()));
        assert!(l.owned_temp().ends_with("owned_temporary_files"));
    }

    #[test]
    fn temp_points_at_the_task_directory() {
        let l = layout();
        let env = l.env();
        assert_eq!(env.len(), 2);
        for (key, value) in env {
            assert!(key == "TEMP" || key == "TMP");
            assert_eq!(value, l.root().display().to_string());
        }
    }

    #[test]
    fn legacy_command_has_empty_args_and_cwd_ikey() {
        let l = layout();
        let cmd = legacy_restore(&l, "C:/tools/MDM_SETUP.exe");
        assert!(cmd.args.is_empty());
        assert_eq!(cmd.cwd, l.ikey());
        assert!(cmd.password.is_none());
    }

    #[test]
    fn modern_command_refuses_without_args() {
        let l = layout();
        assert!(modern_restore(&l, "C:/r/restore.exe", Vec::new(), None).is_err());
        let cmd = modern_restore(
            &l,
            "C:/r/restore.exe",
            vec!["-u".into(), "UDID".into()],
            Some("pw".into()),
        )
        .unwrap();
        assert_eq!(cmd.args.len(), 2);
        assert!(cmd.password.is_some());
    }
}
