//! 判定恢复流程是否真的完成。
//!
//! 判据来自 `iFineMDM.exe` 里的字符串（本次逆向的证据）：
//!
//! * 正标记：`IKEY MDM Successful.`
//! * 负标记 / 错误判定正则：
//!   ```text
//!   (?i)\b(?:error|fatal|failed|failure|exception|cannot|can't|could not|unable to|unsuccessful)\b|
//!   (?i)\bnot\s+(?:complete[ds]?|successful|restored)\b|
//!   ```
//!
//! # 与正则的差距（必须说清楚）
//!
//! 厂商用的是**带词边界的正则**。这里退化成小写子串匹配，不引入 `regex` 依赖。
//! 后果：可能出现误报（例如路径里含 `error`），但**不会出现漏报** —— 也就是说
//! 偏向"判定为可疑"，而不是"判定为成功"。可疑要人去核，误判成功会让人以为已经解除。
//!
//! # 为什么不能只看退出码
//!
//! 原程序在恢复器返回 `0xC0000374`（堆损坏）时仍可能成功，取决于完成标记是否出现；
//! 反过来 HTTP 200 也不代表恢复完成。所以判定只依赖**输出内容**。

use crate::bypass::Route;

/// 恢复器输出里的正标记。
pub const POSITIVE_MARKERS: &[&str] = &["IKEY MDM Successful."];

/// 错误关键词（对应厂商正则的第一组分支）。
pub const ERROR_WORDS: &[&str] = &[
    "error",
    "fatal",
    "failed",
    "failure",
    "exception",
    "cannot",
    "can't",
    "could not",
    "unable to",
    "unsuccessful",
];

/// 否定短语（对应厂商正则的第二组分支）。
///
/// 已含空格的完整短语，避免把 `not complete` 拆成 `not` + `complete` 后误伤。
pub const NEGATIVE_PHRASES: &[&str] = &[
    "not complete",
    "not completed",
    "not successful",
    "not restored",
];

/// 流程内部的阶段提示。用于观察进度，**不作为完成判据**。
pub const PROGRESS_HINTS: &[&str] = &[
    "First restore complete, waiting for device restart",
    "Second restore complete, waiting for final restart",
    "Waiting for device",
    "waiting for a restore",
    "Apple activation complete",
    "Device workflow complete",
    "Flow complete",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// 有正标记且没有错误词。
    Complete,
    /// 没有正标记。
    Incomplete,
    /// 有正标记，但同时出现错误词或被否定短语命中 —— 需要人工判断。
    Doubtful,
    /// 该路线没有可用标记（例如版本不受支持）。
    Unsupported,
}

#[derive(Clone, Debug)]
pub struct Verdict {
    pub state: State,
    pub marker: Option<String>,
    pub hits: Vec<String>,
}

impl Verdict {
    pub fn is_complete(&self) -> bool {
        self.state == State::Complete
    }
    pub fn summary(&self) -> String {
        match self.state {
            State::Complete => format!(
                "已完成（标记：{}）",
                self.marker.as_deref().unwrap_or("")
            ),
            State::Incomplete => "未检测到完成标记".to_string(),
            State::Doubtful => format!(
                "可疑：出现完成标记但输出含错误内容（{}）",
                self.hits.join(" / ")
            ),
            State::Unsupported => "该路线不支持配置锁处理".to_string(),
        }
    }
}

/// 判定一份恢复器输出。
pub fn judge(route: Route, output: &str) -> Verdict {
    if route == Route::Unsupported {
        return Verdict {
            state: State::Unsupported,
            marker: None,
            hits: Vec::new(),
        };
    }
    let haystack = output.to_lowercase();

    let marker = POSITIVE_MARKERS
        .iter()
        .find(|m| output.contains(**m))
        .map(|m| (*m).to_string());

    let mut hits = Vec::new();
    for word in ERROR_WORDS {
        if haystack.contains(word) {
            hits.push((*word).to_string());
        }
    }
    for phrase in NEGATIVE_PHRASES {
        if haystack.contains(phrase) {
            hits.push((*phrase).to_string());
        }
    }

    let state = match (&marker, hits.is_empty()) {
        (None, _) => State::Incomplete,
        (Some(_), true) => State::Complete,
        (Some(_), false) => State::Doubtful,
    };

    Verdict {
        state,
        marker,
        hits,
    }
}

/// 退出码是否属于"仅在已确认完成时才可容忍"的那一类。
pub fn tolerated_fault(route: Route, code: i32, output: &str) -> bool {
    crate::bypass::KNOWN_FAULT_CODES.contains(&code) && judge(route, output).is_complete()
}

/// 流程进度提示是否出现（只用于展示进度，不参与完成判定）。
pub fn saw_progress(output: &str, hint: &str) -> bool {
    output.contains(hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_alone_is_complete() {
        let v = judge(Route::LegacyGaval, "… IKEY MDM Successful. …");
        assert_eq!(v.state, State::Complete);
        assert!(v.is_complete());
    }

    #[test]
    fn http_200_is_not_completion() {
        let v = judge(Route::Mdm27, "HTTP/1.1 200 OK");
        assert_eq!(v.state, State::Incomplete);
    }

    #[test]
    fn marker_with_error_is_doubtful_not_complete() {
        let v = judge(
            Route::LegacyGaval,
            "IKEY MDM Successful.\nError: device did not reconnect",
        );
        assert_eq!(v.state, State::Doubtful);
        assert!(!v.is_complete());
        assert!(v.hits.iter().any(|h| h == "error"));
    }

    #[test]
    fn negative_phrase_detected() {
        let v = judge(
            Route::LegacyGaval,
            "IKEY MDM Successful. restore not completed",
        );
        assert_eq!(v.state, State::Doubtful);
        assert!(v.hits.iter().any(|h| h.contains("not completed")));
    }

    #[test]
    fn unsupported_route_is_unsupported() {
        let v = judge(Route::Unsupported, "IKEY MDM Successful.");
        assert_eq!(v.state, State::Unsupported);
    }

    #[test]
    fn heap_corruption_needs_completion() {
        let code = crate::bypass::KNOWN_FAULT_CODES[0];
        assert!(!tolerated_fault(Route::LegacyGaval, code, "starting"));
        assert!(tolerated_fault(Route::LegacyGaval, code, "IKEY MDM Successful."));
    }

    #[test]
    fn two_restore_progress_is_recognized() {
        let output = "First restore complete, waiting for device restart\n\
                      Second restore complete, waiting for final restart";
        assert!(saw_progress(output, PROGRESS_HINTS[0]));
        assert!(saw_progress(output, PROGRESS_HINTS[1]));
        // 进度提示不构成完成
        assert_eq!(judge(Route::Mdm27, output).state, State::Incomplete);
    }

    #[test]
    fn summary_never_mentions_success_when_doubtful() {
        let v = judge(Route::LegacyGaval, "IKEY MDM Successful. fatal error");
        assert!(v.summary().starts_with("可疑"));
    }
}
