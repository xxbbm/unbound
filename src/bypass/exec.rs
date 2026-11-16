//! 执行器：把阶段表真正跑起来。
//!
//! 分成两半，界线是"要不要碰设备"：
//!
//! * [`plan`] —— 纯本地：建任务目录、解资源、按路线决定是否改写 plist。
//!   不连设备、不改设备。
//! * [`apply`] —— 按 [`crate::bypass::pipeline::stages`] 走一遍。真正启动进程、
//!   等待重连的动作通过闭包注入，所以这一半可以用假实现完整测试。
//!
//! 这样安排的原因：设备侧行为**没有真机验证过**，把它挡在闭包后面，至少能保证
//! 编排逻辑、完成判定、失败即停这些不依赖硬件的行为是被测过的。
//!
//! # 失败即停
//!
//! 任一阶段出错立刻返回，不继续、不重试、不把 `Doubtful` 当成功。

use crate::bypass::{
    judge, pipeline, PreparedCommand, Route, StageKind, TaskLayout,
};
use std::path::{Path, PathBuf};

/// 一次绕过的计划。
#[derive(Clone, Debug)]
pub struct Plan {
    pub route: Route,
    pub layout: TaskLayout,
    pub command: Option<PreparedCommand>,
    /// 给用户看的过程说明。包含被跳过的键等"不完美"信息。
    pub notes: Vec<String>,
}

/// 执行结果。
#[derive(Clone, Debug)]
pub struct Outcome {
    pub route: Route,
    pub completed_stages: Vec<&'static str>,
    /// 恢复器原始输出的末尾片段，便于排查。
    pub tail: String,
    pub verdict: judge::Verdict,
    /// 过程备注（例如某条进度提示没出现）。**不影响成败判定**。
    pub notes: Vec<String>,
}

impl Outcome {
    pub fn is_success(&self) -> bool {
        self.verdict.is_complete()
    }
    pub fn summary(&self) -> String {
        let mut text = format!(
            "{}：{} 个阶段；{}",
            self.route.label(),
            self.completed_stages.len(),
            self.verdict.summary()
        );
        for note in &self.notes {
            text.push_str("\n  · ");
            text.push_str(note);
        }
        text
    }
}

/// 解 ZIP 到目标目录，拒绝越界路径。
///
/// 返回写出的文件数。符号链接与 `..` 一律拒绝 —— 原始工具的压缩包里出现过
/// 目录穿越，不能被它带出去。
pub fn extract_zip(archive: &Path, dest: &Path) -> Result<usize, String> {
    let file = std::fs::File::open(archive)
        .map_err(|e| format!("打开 {} 失败：{e}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| format!("读取压缩包 {} 失败：{e}", archive.display()))?;
    std::fs::create_dir_all(dest)
        .map_err(|e| format!("创建 {} 失败：{e}", dest.display()))?;

    let mut written = 0usize;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|e| format!("读取第 {index} 项失败：{e}"))?;
        let raw = entry.name().to_string();
        // ZipFile::enclosed_name 会拒绝绝对路径与 `..`
        let relative = match entry.enclosed_name() {
            Some(p) => p,
            None => return Err(format!("压缩包含越界路径：{raw}")),
        };
        let out = dest.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&out)
                .map_err(|e| format!("创建 {} 失败：{e}", out.display()))?;
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建 {} 失败：{e}", parent.display()))?;
        }
        let mut sink = std::fs::File::create(&out)
            .map_err(|e| format!("创建 {} 失败：{e}", out.display()))?;
        std::io::copy(&mut entry, &mut sink)
            .map_err(|e| format!("解出 {} 失败：{e}", out.display()))?;
        written += 1;
    }
    Ok(written)
}

/// 准备任务目录。
///
/// * `device` —— 目标设备身份，用于路线判定与 plist 改写。
/// * `workdir` —— 任务目录根。
/// * `kit` —— 旧路线的外部 kit。**传 `None` 就用内置资源**，用户不需要准备任何文件。
/// * `rekey` —— 是否按当前设备改写 plist。默认由路线决定；传 `Some(false)`
///   可以强制保留原始字节（用于排查"是不是改写引入的问题"）。
pub fn plan(
    device: &crate::mdm::Device,
    route: Route,
    workdir: &Path,
    kit: Option<&Path>,
    rekey: Option<bool>,
) -> Result<Plan, String> {
    if route == Route::Unsupported {
        return Err("原程序也拒绝该版本，不提供绕过".to_string());
    }

    let layout = TaskLayout::new(workdir, &device.udid);
    layout.create()?;

    let mut notes = Vec::new();

    match route {
        Route::LegacyGaval => {
            // 没有外部 kit 就用内置资源 —— 用户不该为了用功能去翻原工具。
            let kit_dir = match kit {
                Some(path) => path.to_path_buf(),
                None => {
                    let dir = workdir.join("kit");
                    let written = crate::bypass::resources::write_all(
                        &dir,
                        crate::bypass::resources::LEGACY,
                    )?;
                    notes.push(format!(
                        "已从内置资源放入 {written} 个文件（不需要原工具）"
                    ));
                    dir
                }
            };
            let installed = install_kit(&kit_dir, &layout)?;
            notes.push(format!(
                "装入工具 {} 个文件、备份 {} 个文件",
                installed.tools, installed.backup
            ));
            if !installed.info_plist {
                notes.push("警告：缺少 GavalLegacyInfo.plist，备份不会带正确的设备信息".into());
            }
            if !installed.manifest_plist {
                notes.push("警告：缺少 GavalLegacyManifest.plist，加密备份无法被识别".into());
            }

            // `layout.create()` 会把 backup 目录也建出来，所以不能只看目录存在。
            let has_backup = std::fs::read_dir(layout.backup())
                .map(|mut it| it.next().is_some())
                .unwrap_or(false);
            if !has_backup {
                return Err(format!(
                    "备份目录里没有文件：{}",
                    layout.backup().display()
                ));
            }
        }
        Route::Mdm27 => {
            // 备份直接落在任务根目录（cwd 就是那里）；缺了就放内置的那份。
            let missing = pipeline::missing_required_files(workdir);
            if !missing.is_empty() {
                let written = crate::bypass::resources::write_all(
                    workdir,
                    crate::bypass::resources::MODERN_BACKUP,
                )?;
                notes.push(format!(
                    "已从内置资源放入 {written} 个备份文件（不需要原工具）"
                ));
                let still_missing = pipeline::missing_required_files(workdir);
                if !still_missing.is_empty() {
                    return Err(format!(
                        "内置备份仍缺少文件：{}",
                        still_missing.join(" / ")
                    ));
                }
            }
            // 恢复组件同理
            let tool_root = modern_tool_root(workdir);
            if !tool_root.join(pipeline::MODERN_RESTORER).is_file() {
                let written = crate::bypass::resources::write_all(
                    &tool_root,
                    crate::bypass::resources::MODERN_TOOLS,
                )?;
                notes.push(format!(
                    "已从内置资源放入 {written} 个恢复组件（不需要原工具）"
                ));
            }
            notes.push(format!(
                "现代路线：备份留在任务根目录，不改写；恢复参数由 {} 生成",
                pipeline::MODERN_RESTORER
            ));
        }
        Route::Unsupported => unreachable!("已在函数开头拦下"),
    }

    let should_rekey = rekey.unwrap_or_else(|| route.rewrites_device_identity());
    if should_rekey {
        let reports = crate::bypass::rekey::rekey_backup_dir(&layout.backup(), device)?;
        for (file, report) in &reports {
            notes.push(format!("{file}：{}", report.summary()));
            for skipped in &report.skipped {
                // 被跳过的键意味着这份备份可能与该设备不匹配，必须让用户看到
                notes.push(format!("  {file} 跳过 {skipped}"));
            }
        }
    } else {
        notes.push("按路线要求保留原始备份字节，未改写设备身份".to_string());
    }

    Ok(Plan {
        route,
        layout,
        command: None,
        notes,
    })
}

/// 现代路线的工具目录：`tool/` → `tools/` → `MixIos27/`，都没有就默认 `tools/`。
///
/// 默认 `tools/` 而不是任务根，避免恢复器和备份文件混在一起。
fn modern_tool_root(workdir: &Path) -> PathBuf {
    for name in ["tool", "tools", "MixIos27"] {
        let path = workdir.join(name);
        if path.is_dir() {
            return path;
        }
    }
    workdir.join("tools")
}

/// 一件 kit 装进了任务目录多少东西。
#[derive(Clone, Copy, Debug, Default)]
pub struct KitInstalled {
    pub tools: usize,
    pub backup: usize,
    pub info_plist: bool,
    pub manifest_plist: bool,
}

/// 把 kit 装进任务目录。
///
/// kit 的布局（与内嵌资源一一对应）：
///
/// ```text
/// kit/
/// ├─ GavalLegacyTools.zip        （或已解出的 tools/）
/// ├─ GavalLegacyBackup.zip       （或已解出的 backup/）
/// ├─ GavalLegacyInfo.plist
/// └─ GavalLegacyManifest.plist
/// ```
///
/// 装配目标遵循原程序的目录约定：工具与恢复器进 `任务目录/ikey/`，备份进
/// `任务目录/ikey/<UDID>/`，两份模板 plist 也写进备份目录（备份压缩包里没有它们）。
pub fn install_kit(kit: &Path, layout: &TaskLayout) -> Result<KitInstalled, String> {
    if !kit.is_dir() {
        return Err(format!("kit 目录不存在：{}", kit.display()));
    }
    let mut installed = KitInstalled::default();

    // 工具
    let tools_dir = kit.join("tools");
    if tools_dir.is_dir() {
        installed.tools = copy_dir(&tools_dir, &layout.ikey())?;
    } else {
        let archive = kit.join(pipeline::LEGACY_TOOLS_ZIP);
        if archive.is_file() {
            installed.tools = extract_zip(&archive, &layout.ikey())?;
        }
    }

    // 备份
    let backup_dir = kit.join("backup");
    if backup_dir.is_dir() {
        installed.backup = copy_dir(&backup_dir, &layout.backup())?;
    } else {
        let archive = kit.join(pipeline::LEGACY_BACKUP_ZIP);
        if archive.is_file() {
            installed.backup = extract_zip(&archive, &layout.backup())?;
        }
    }

    // 两份模板 plist：备份包里没有，必须由 kit 提供
    for (source_name, target_name, flag) in [
        (pipeline::LEGACY_INFO_PLIST, "Info.plist", 0usize),
        (pipeline::LEGACY_MANIFEST_PLIST, "Manifest.plist", 1usize),
    ] {
        let source = kit.join(source_name);
        if !source.is_file() {
            continue;
        }
        let target = layout.backup().join(target_name);
        std::fs::copy(&source, &target)
            .map_err(|e| format!("复制 {source_name} 失败：{e}"))?;
        if flag == 0 {
            installed.info_plist = true;
        } else {
            installed.manifest_plist = true;
        }
    }

    Ok(installed)
}

fn copy_dir(from: &Path, to: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(to).map_err(|e| format!("创建 {} 失败：{e}", to.display()))?;
    let mut count = 0usize;
    let entries = std::fs::read_dir(from)
        .map_err(|e| format!("读取 {} 失败：{e}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("遍历 {} 失败：{e}", from.display()))?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| format!("读取类型失败：{e}"))?;
        if kind.is_dir() {
            count += copy_dir(&source, &target)?;
        } else if kind.is_file() {
            std::fs::create_dir_all(target.parent().unwrap_or(to))
                .map_err(|e| format!("创建目录失败：{e}"))?;
            std::fs::copy(&source, &target)
                .map_err(|e| format!("复制 {} 失败：{e}", source.display()))?;
            count += 1;
        }
    }
    Ok(count)
}

/// 装上恢复命令。现代路线必须显式给参数；旧路线用
/// [`crate::bypass::pipeline::legacy_restore`] 即可，因为它是空 argv。
impl Plan {
    pub fn with_command(mut self, command: PreparedCommand) -> Self {
        self.command = Some(command);
        self
    }
}

/// 按阶段表执行。
///
/// `run_restore` 与 `wait_device` 由调用方注入：真正的进程启动和设备重连探测
/// 属于设备侧行为，本函数只负责顺序、失败即停与完成判定。
pub fn apply<F, W>(
    plan: &Plan,
    mut run_restore: F,
    mut wait_device: W,
) -> Result<Outcome, String>
where
    F: FnMut(&PreparedCommand, u8) -> Result<String, String>,
    W: FnMut(u64) -> Result<(), String>,
{
    let stages = pipeline::stages(plan.route);
    let mut completed: Vec<&'static str> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let mut tail = String::new();

    for stage in stages {
        match stage.kind {
            StageKind::Preflight => {
                if plan.command.is_none() && matches!(plan.route, Route::Mdm27) {
                    return Err(format!(
                        "阶段 {}：现代路线需要显式恢复参数，尚未提供",
                        stage.id
                    ));
                }
            }
            StageKind::Restore { round } => {
                let command = plan.command.as_ref().ok_or_else(|| {
                    format!("阶段 {}：没有可用的恢复命令", stage.id)
                })?;
                // 命令本身的失败会从 run_restore 冒出来（非零退出码/超时/取消）。
                let output = run_restore(command, round)?;
                tail = tail_snippet(&output);
                // 进度提示只作参考：`idevicebackup2.exe` 不会打印厂商那两句
                // "First/Second restore complete"，所以**不能**因为它缺席就中止。
                if let Some(hint) = next_hint(plan.route, round) {
                    if !output.contains(hint) {
                        notes.push(format!(
                            "第 {round} 轮输出里没有进度提示「{hint}」（不影响判定）"
                        ));
                    }
                }
            }
            StageKind::WaitRestart { round } => {
                wait_device(u64::from(round) * 60)?;
            }
            StageKind::WaitDevice => {
                wait_device(120)?;
            }
            StageKind::Activate | StageKind::ReadState | StageKind::Report => {
                // 激活与最终状态由调用方在设备侧完成；这里只推进阶段。
            }
            StageKind::BuildBackup => {}
        }
        completed.push(stage.id);
    }

    let verdict = judge::judge(plan.route, &tail);
    Ok(Outcome {
        route: plan.route,
        completed_stages: completed,
        tail,
        verdict,
        notes,
    })
}

/// 第 `round` 轮恢复结束后应当出现的提示。
fn next_hint(route: Route, round: u8) -> Option<&'static str> {
    pipeline::stages(route).iter().find_map(|s| match s.kind {
        StageKind::WaitRestart { round: r } if r == round => s.hint,
        _ => None,
    })
}

/// 保留输出的末尾若干行，用于展示与判定。
fn tail_snippet(output: &str) -> String {
    const LIMIT: usize = 8000;
    if output.len() <= LIMIT {
        return output.to_string();
    }
    // 按字符边界截断，避免切坏 UTF-8
    let mut cut = output.len() - LIMIT;
    while cut < output.len() && !output.is_char_boundary(cut) {
        cut += 1;
    }
    output[cut..].to_string()
}

/// 用项目自带的受限运行器启动一个进程并收集双流输出。
///
/// 这是设备侧的唯一入口：固定 argv、不经过 shell、有超时、有大小上限。
pub fn run_process(command: &PreparedCommand, seconds: u64) -> Result<String, String> {
    use crate::runner::{self, Event, Options, Request};

    if !command.program.is_file() {
        return Err(format!("找不到程序：{}", command.program.display()));
    }
    let run = runner::start_with_options(
        Request {
            program: command.program.clone(),
            args: command.args.iter().map(std::ffi::OsString::from).collect(),
            timeout: std::time::Duration::from_secs(seconds),
        },
        Options {
            cwd: Some(command.cwd.clone()),
            new_process_group: false,
            ..Default::default()
        },
    );

    let mut stdout = String::new();
    let mut stderr = String::new();
    loop {
        match run.events.recv().map_err(|e| e.to_string())? {
            Event::Output { stderr: is_err, text } => {
                let out = if is_err { &mut stderr } else { &mut stdout };
                if out.len() + text.len() > 4 * 1024 * 1024 {
                    run.cancel();
                    return Err("恢复器输出超过大小限制".into());
                }
                out.push_str(&text);
            }
            Event::Error(e) => return Err(e),
            Event::Finished { code, cancelled, timed_out, dropped } => {
                if cancelled || timed_out || dropped > 0 {
                    return Err(format!(
                        "恢复器未正常结束（超时 {timed_out}，丢弃输出 {dropped}）：{}",
                        stderr.chars().take(1200).collect::<String>()
                    ));
                }
                // 退出码不参与完成判定；由 judge 看输出。
                let _ = code;
                return Ok(format!("{stdout}\n{stderr}"));
            }
            Event::Started(_) => (),
        }
    }
}

/// 默认的等待：只睡眠，不探测设备。
///
/// 真实实现应当在等待期间轮询设备是否重连（原程序有 `Waiting for device`
/// 状态）。这里不假装探测，由调用方注入更好的实现。
pub fn sleep_wait(seconds: u64) -> Result<(), String> {
    std::thread::sleep(std::time::Duration::from_secs(seconds.min(600)));
    Ok(())
}

// ---------------------------------------------------------------- 动作入口

/// 从 `Manifest.plist` 读出这份备份的**原设备**标识。
///
/// 现代路线不改写备份，靠 `-s <源UDID>` 指明来源，所以这个值必须恢复出来。
pub fn modern_source_udid(dir: &Path) -> Option<String> {
    let data = std::fs::read(dir.join("Manifest.plist")).ok()?;
    let value = plist::Value::from_reader(std::io::Cursor::new(&data)).ok()?;
    value
        .as_dictionary()?
        .get("Lockdown")?
        .as_dictionary()?
        .get("UniqueDeviceID")?
        .as_string()
        .map(str::to_string)
}

/// 界面动作的两个入口。
///
/// `worker_main` 只需要一行转调，所有判断都在这里，便于测试。
/// * `bypass-prepare`：只动本地任务目录，不碰设备。
/// * `bypass-apply`：启动恢复器，会改写设备资料。`password` 为服务端下发的备份口令
///   （来自任务的密码字段），现代路线必需。
pub fn handle(
    action: &str,
    device: &crate::mdm::Device,
    workdir: &str,
    password: &str,
) -> Result<String, String> {
    let wd = PathBuf::from(workdir.trim());
    if wd.as_os_str().is_empty() {
        return Err("请选择任务目录".into());
    }
    let major = major_version(&device.version)?;
    let route = crate::bypass::route(major);

    match action {
        "bypass-prepare" => {
            // kit 可以放在 任务目录/kit/ 下，也可以把内容直接摊在任务目录里。
            let nested = wd.join("kit");
            let kit = if nested.is_dir() {
                Some(nested)
            } else if wd.join(pipeline::LEGACY_BACKUP_ZIP).is_file()
                || wd.join(pipeline::LEGACY_TOOLS_ZIP).is_file()
                || wd.join(pipeline::LEGACY_INFO_PLIST).is_file()
            {
                Some(wd.clone())
            } else {
                None
            };
            let prepared = plan(device, route, &wd, kit.as_deref(), None)?;
            let mut text = format!("{}：已准备\n", route.label());
            for note in &prepared.notes {
                text.push_str("  · ");
                text.push_str(note);
                text.push('\n');
            }
            text.push_str("下一步：确认后执行「执行配置锁绕过」。");
            Ok(text)
        }
        "bypass-apply" => {
            // 用户可能直接点「开始」：先确保资源就位（幂等，已存在不会覆盖）。
            let ensured = plan(device, route, &wd, None, None)?;
            let layout = ensured.layout.clone();
            let mut notes = ensured.notes;
            let command = match route {
                Route::LegacyGaval => {
                    let program = layout.ikey().join(pipeline::LEGACY_RESTORER);
                    if !program.is_file() {
                        return Err(format!(
                            "找不到旧路线恢复器：{}",
                            program.display()
                        ));
                    }
                    pipeline::legacy_restore(&layout, program)
                }
                Route::Mdm27 => {
                    // 现代路线用 idevicebackup2.exe，参数顺序取自 CreateStartInfo。
                    let tool_root = modern_tool_root(&wd);
                    let program = tool_root.join(pipeline::MODERN_RESTORER);
                    if !program.is_file() {
                        return Err(format!(
                            "找不到 {}：{}",
                            pipeline::MODERN_RESTORER,
                            program.display()
                        ));
                    }
                    // 口令：没填就用内置备份自带的那一个，用户不需要知道它
                    let password = if password.trim().is_empty() {
                        notes.push("未填口令，使用内置备份的默认口令".to_string());
                        crate::bypass::backend::BACKUP_PASSWORD
                    } else {
                        password
                    };
                    let source = modern_source_udid(&wd).ok_or_else(|| {
                        "无法从 Manifest.plist 读出原设备标识，未开始恢复".to_string()
                    })?;
                    pipeline::modern_command(
                        &layout, &tool_root, &device.udid, &source, password,
                    )
                }
                Route::Unsupported => return Err("原程序也拒绝该版本，不提供绕过".into()),
            };

            let prepared = Plan {
                route,
                layout: layout.clone(),
                command: Some(command),
                notes,
            };
            let outcome = apply(
                &prepared,
                |cmd, round| {
                    // 第二轮通常更久：设备要再启动一次
                    let seconds = if round == 1 { 1800 } else { 2400 };
                    run_process(cmd, seconds)
                },
                sleep_wait,
            )?;

            if outcome.is_success() {
                let mut text = outcome.summary();
                text.push_str("\n最终仍需在设备上确认远程管理界面已消失。");
                Ok(text)
            } else {
                Err(format!("{}（恢复器输出末尾：{}）", outcome.summary(), outcome.tail))
            }
        }
        other => Err(format!("未知的解锁动作：{other}")),
    }
}

/// 取系统主版本。`27.0.1` → `27`。
pub fn major_version(version: &str) -> Result<u32, String> {
    version
        .split('.')
        .next()
        .unwrap_or("")
        .parse()
        .map_err(|_| format!("系统版本无效：{version}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bypass::Route;

    fn device() -> crate::mdm::Device {
        crate::mdm::Device {
            udid: "00008110-000A1C2E3F40001E".into(),
            serial: "M69PCG7LJX".into(),
            product: "iPhone14,2".into(),
            version: "17.5.1".into(),
            build: "21F90".into(),
            name: "t".into(),
            imei: String::new(),
            activation: String::new(),
            facts: std::collections::BTreeMap::new(),
        }
    }

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("atk-exec-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn plan_refuses_unsupported_version() {
        let dir = tmp("unsupported");
        let err = plan(&device(), Route::Unsupported, &dir, None, None).unwrap_err();
        assert!(err.contains("拒绝"));
    }

    #[test]
    fn plan_installs_embedded_legacy_resources() {
        // 没有外部 kit 也要能准备：内置资源负责把文件放好
        let dir = tmp("embedded-legacy");
        let prepared = plan(&device(), Route::LegacyGaval, &dir, None, None)
            .expect("没有外部 kit 也应能准备");
        assert!(
            prepared.notes.iter().any(|n| n.contains("内置资源")),
            "应说明用了内置资源：{:?}",
            prepared.notes
        );
        assert!(
            prepared.layout.ikey().join("MDM_SETUP.exe").is_file(),
            "内置工具包应解出恢复器"
        );
        let count = std::fs::read_dir(prepared.layout.backup())
            .map(|it| it.count())
            .unwrap_or(0);
        assert!(count > 0, "内置备份包应解出内容");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn plan_installs_embedded_modern_resources() {
        let dir = tmp("embedded-modern");
        let mut d = device();
        d.version = "27.0".into();
        let prepared = plan(&d, Route::Mdm27, &dir, None, None).expect("应能准备");
        assert!(
            prepared.notes.iter().any(|n| n.contains("内置资源")),
            "{:?}",
            prepared.notes
        );
        for name in pipeline::MODERN_REQUIRED_FILES {
            assert!(dir.join(name).is_file(), "{name} 应已放入任务根");
        }
        assert!(
            dir.join("tools").join(pipeline::MODERN_RESTORER).is_file(),
            "恢复器应已放入 tools/"
        );
        // 口令不必用户填写：内置备份自带一个
        assert!(!crate::bypass::backend::BACKUP_PASSWORD.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_progress_hint_is_noted_but_does_not_abort() {
        let layout = TaskLayout::new(tmp("hint"), &device().udid);
        layout.create().unwrap();
        let plan = Plan {
            route: Route::LegacyGaval,
            layout: layout.clone(),
            command: Some(pipeline::legacy_restore(&layout, "MDM_SETUP.exe")),
            notes: Vec::new(),
        };
        let mut rounds = Vec::new();
        let outcome = apply(
            &plan,
            |_, round| {
                rounds.push(round);
                // 故意不提进度提示，只给完成标记
                Ok("IKEY MDM Successful.".to_string())
            },
            |_| Ok(()),
        )
        .expect("提示缺席不应中止");
        assert_eq!(rounds, vec![1, 2], "两轮都要跑完");
        assert!(outcome.is_success());
        assert!(
            outcome.notes.iter().any(|n| n.contains("进度提示")),
            "应记录提示缺席：{:?}",
            outcome.notes
        );
        let _ = std::fs::remove_dir_all(layout.root());
    }

    #[test]
    fn apply_runs_two_rounds_and_judges_completion() {
        let layout = TaskLayout::new(tmp("tworounds"), &device().udid);
        layout.create().unwrap();
        let plan = Plan {
            route: Route::LegacyGaval,
            layout: layout.clone(),
            command: Some(pipeline::legacy_restore(&layout, "MDM_SETUP.exe")),
            notes: Vec::new(),
        };
        let mut rounds = Vec::new();
        let outcome = apply(
            &plan,
            |_, round| {
                rounds.push(round);
                Ok("done\nFirst restore complete, waiting for device restart\n\
                    Second restore complete, waiting for final restart\n\
                    IKEY MDM Successful."
                    .to_string())
            },
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(rounds, vec![1, 2]);
        assert!(outcome.is_success(), "{}", outcome.summary());
        assert!(outcome.completed_stages.contains(&"report"));
        let _ = std::fs::remove_dir_all(layout.root());
    }

    #[test]
    fn apply_does_not_treat_doubtful_as_success() {
        let layout = TaskLayout::new(tmp("doubtful"), &device().udid);
        layout.create().unwrap();
        let plan = Plan {
            route: Route::LegacyGaval,
            layout: layout.clone(),
            command: Some(pipeline::legacy_restore(&layout, "MDM_SETUP.exe")),
            notes: Vec::new(),
        };
        let outcome = apply(
            &plan,
            |_, _| {
                Ok("First restore complete, waiting for device restart\n\
                    Second restore complete, waiting for final restart\n\
                    IKEY MDM Successful.\nError: reconnect failed"
                    .to_string())
            },
            |_| Ok(()),
        )
        .unwrap();
        assert!(!outcome.is_success(), "可疑不能当成功");
        assert!(outcome.summary().contains("可疑"));
        let _ = std::fs::remove_dir_all(layout.root());
    }

    #[test]
    fn modern_route_without_command_fails_at_preflight() {
        let layout = TaskLayout::new(tmp("modern"), &device().udid);
        layout.create().unwrap();
        let plan = Plan {
            route: Route::Mdm27,
            layout: layout.clone(),
            command: None,
            notes: Vec::new(),
        };
        let err = apply(&plan, |_, _| Ok(String::new()), |_| Ok(())).unwrap_err();
        assert!(err.contains("需要显式恢复参数"), "实际：{err}");
        let _ = std::fs::remove_dir_all(layout.root());
    }

    #[test]
    fn tail_snippet_keeps_utf8_boundaries() {
        let text = "汉".repeat(5000);
        let tail = tail_snippet(&text);
        assert!(tail.len() <= 8000);
        assert!(tail.chars().all(|c| c == '汉'));
    }

    #[test]
    fn major_version_parses() {
        assert_eq!(major_version("27").unwrap(), 27);
        assert_eq!(major_version("26.5.1").unwrap(), 26);
        assert!(major_version("abc").is_err());
    }

    #[test]
    fn handle_rejects_unknown_action() {
        let err = handle("bypass-nope", &device(), "", "").unwrap_err();
        assert!(err.contains("任务目录") || err.contains("未知"), "实际：{err}");
    }

    #[test]
    fn modern_route_needs_no_external_components() {
        // 现代路线：内置备份与恢复组件都要就位，且口令有默认值。
        // 这里只验证"准备得齐"，不真的执行恢复器。
        let dir = tmp("modern-embedded");
        std::fs::create_dir_all(&dir).unwrap();
        let mut d = device();
        d.version = "27.0".into();
        let prepared = plan(&d, Route::Mdm27, &dir, None, None).expect("应能准备");
        assert!(prepared.notes.iter().any(|n| n.contains("内置资源")));
        let tool_root = dir.join("tools");
        assert!(tool_root.join(pipeline::MODERN_RESTORER).is_file());
        // 用内置口令能构造出完整命令（12 项）
        let command = pipeline::modern_command(
            &prepared.layout,
            &tool_root,
            &d.udid,
            &d.udid,
            crate::bypass::backend::BACKUP_PASSWORD,
        );
        assert_eq!(command.args.len(), 12);
        assert!(command.program.is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn handle_prepare_works_without_any_external_file() {
        let dir = tmp("prepare-embedded");
        std::fs::create_dir_all(&dir).unwrap();
        let text = handle("bypass-prepare", &device(), &dir.display().to_string(), "")
            .expect("不应要求用户准备文件");
        assert!(text.contains("内置资源"), "报告应说明用了内置资源：{text}");
        assert!(dir.join("ikey").join("MDM_SETUP.exe").is_file());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
