//! 激活阶段：Apple Albert 端点与步骤顺序。
//!
//! 端点与提示语全部来自本次对 `iFineMDM.exe` 的字符串扫描：
//!
//! ```text
//! https://albert.apple.com/deviceservices/deviceActivation
//! https://albert.apple.com/deviceservices/drmHandshake
//! https://albert.apple.com/deviceservices/phoneHome
//! Starting activation... / Running Apple DRM handshake
//! Requesting ActivationRecord from Albert... / Re-activating with Albert...
//! Apple activation complete
//! ```
//!
//! # 本模块只给"计划"，不发请求
//!
//! 原程序用 `imobiledevice.dll` / `CoreFoundation.dll` / `MobileDevice.dll` 走私有
//! API 完成 DRM 握手与 ActivationRecord 交换。这套流程**没有**在本模块里重写：
//! 既没还原每一步的报文，也没在真机上验证过。
//!
//! 所以这里提供两样东西：
//!
//! 1. [`plan`] —— 步骤与端点对照，供界面展示与测试；
//! 2. [`ideviceactivation_argv`] —— 用随包 CLI 走同一端点的等价命令，
//!    参数已用原文件帮助核对（来源：FINDINGS 第 6 条）。
//!
//! 真正发 HTTP 的实现属于另一个决定：那会引入真实的 Apple 端交互，需要单独的
//! 错误处理与真机验证。

pub const DEVICE_ACTIVATION: &str = "https://albert.apple.com/deviceservices/deviceActivation";
pub const DRM_HANDSHAKE: &str = "https://albert.apple.com/deviceservices/drmHandshake";
pub const PHONE_HOME: &str = "https://albert.apple.com/deviceservices/phoneHome";

/// 设备端 CLI 名（扫描所得）。
pub const ACTIVATION_CLI: &str = "ideviceactivation.exe";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    /// 日志/界面上的提示语，取自原程序字符串。
    pub label: &'static str,
    pub endpoint: Option<&'static str>,
}

const STEPS: &[Step] = &[
    Step {
        label: "Starting activation...",
        endpoint: None,
    },
    Step {
        label: "Running Apple DRM handshake",
        endpoint: Some(DRM_HANDSHAKE),
    },
    Step {
        label: "Requesting ActivationRecord from Albert...",
        endpoint: Some(PHONE_HOME),
    },
    Step {
        label: "Re-activating with Albert...",
        endpoint: Some(DEVICE_ACTIVATION),
    },
    Step {
        label: "Apple activation complete",
        endpoint: None,
    },
];

/// 激活步骤表。
pub fn plan() -> &'static [Step] {
    STEPS
}

/// 最后一步才应当出现完成提示。
pub fn is_done(output: &str) -> bool {
    output.contains("Apple activation complete")
}

/// 等价 CLI 命令（等价性未经真机验证；失败即停）。
///
/// ```text
/// ideviceactivation activate -u <UDID> -b -s <deviceActivation>
/// ```
pub fn ideviceactivation_argv(udid: &str) -> Vec<String> {
    vec![
        "activate".to_string(),
        "-u".to_string(),
        udid.to_string(),
        "-b".to_string(),
        "-s".to_string(),
        DEVICE_ACTIVATION.to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_starts_and_ends_with_the_recovered_labels() {
        let steps = plan();
        assert_eq!(steps.first().unwrap().label, "Starting activation...");
        assert_eq!(steps.last().unwrap().label, "Apple activation complete");
    }

    #[test]
    fn handshake_precedes_activation_record() {
        let steps = plan();
        let drm = steps
            .iter()
            .position(|s| s.endpoint == Some(DRM_HANDSHAKE))
            .expect("应含 DRM 握手");
        let record = steps
            .iter()
            .position(|s| s.endpoint == Some(PHONE_HOME))
            .expect("应含 ActivationRecord 请求");
        let act = steps
            .iter()
            .position(|s| s.endpoint == Some(DEVICE_ACTIVATION))
            .expect("应含设备激活");
        assert!(drm < record && record < act);
    }

    #[test]
    fn all_endpoints_are_https_albert() {
        for step in plan() {
            if let Some(endpoint) = step.endpoint {
                assert!(endpoint.starts_with("https://albert.apple.com/deviceservices/"));
            }
        }
    }

    #[test]
    fn done_requires_the_final_label() {
        assert!(!is_done("Starting activation..."));
        assert!(!is_done("Running Apple DRM handshake"));
        assert!(is_done("... Apple activation complete"));
    }

    #[test]
    fn cli_argv_matches_the_documented_form() {
        let argv = ideviceactivation_argv("00008110-000A1C2E3F40001E");
        assert_eq!(argv[0], "activate");
        assert_eq!(argv[1], "-u");
        assert_eq!(argv[2], "00008110-000A1C2E3F40001E");
        assert!(argv.contains(&"-b".to_string()));
        assert_eq!(argv.last().unwrap(), DEVICE_ACTIVATION);
    }
}
