//! 本地 MDM 服务端（8 条路由）。
//!
//! 重写了随包的 `backend.ps1`，因此运行时不依赖 PowerShell。路由与语义逐条对齐：
//!
//! ```text
//! POST /A12/mdm/win_mdm.php              版本检查（固定 key/IV，±300 秒）
//! GET  /A12/mdm/api1.php?serialNumber=   授权
//! POST /A12/mdm/model.php                机型回显
//! POST /A12/mdm/backup.php               备份源（HKDF+HMAC 信封）
//! POST /A12/mdm/requestActivation.php    请求激活
//! POST /A12/mdm/amdeviceactivate.php     激活完成
//! POST /A12/mdm/alert.php                埋点
//! GET  /Erase27/<name>                   不透明载荷
//! ```
//!
//! # 与原始实现的两处刻意差异
//!
//! 1. **不做上游转发。** `backend.ps1` 在缓存缺失时会转发到 `gavaldev.com`；
//!    这里不隐式联网，`relay` 打开了也只会明确拒绝。
//! 2. **只绑回环。** 调试用桩不应该被局域网访问到。
//!
//! 路由逻辑（[`respond`]）是纯函数，可以在不开端口的情况下单测。

use crate::bypass::crypto::{b64_encode, Envelope, VersionEnvelope};
use crate::bypass::secrets::Secrets;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

/// 请求日志：(方法, 目标, 状态码)。
///
/// 起别名是为了让 `Server` 与 `handle_connection` 的签名可读，也避免 clippy
/// 报"类型过于复杂"。
pub type CallLog = Arc<Mutex<Vec<(String, String, u16)>>>;

pub const MDM_PREFIX: &str = "/A12/mdm/";
pub const ERASE_PREFIX: &str = "/Erase27/";

/// 随包载荷里的常量，不能改。
pub const BACKUP_SOURCE_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
pub const BACKUP_PASSWORD: &str = "90abbd003d76a97445502b14433ab6ab544df16a";

/// 版本检查的内层文档。用 raw string 避免转义与续行带来的歧义。
const VERSION_INNER: &str = r#"{"version":"1.0.0","id":"1","status":"ok","message":"","changelog":"","has_message":false}"#;

#[derive(Clone)]
pub struct Config {
    pub secrets: Secrets,
    pub template_dir: Option<PathBuf>,
    pub replay_dir: Option<PathBuf>,
    pub product_type: String,
    /// 允许转发到真实上游。默认关闭；打开后仍然会明确拒绝，而不是偷偷联网。
    pub relay: bool,
}

impl Config {
    pub fn new(secrets: Secrets) -> Self {
        Self {
            secrets,
            template_dir: None,
            replay_dir: None,
            product_type: "iPhone14,2".to_string(),
            relay: false,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

impl Response {
    fn json(status: u16, value: &Value) -> Self {
        Self {
            status,
            content_type: "application/json; charset=utf-8",
            body: value.to_string().into_bytes(),
        }
    }
    fn bytes(status: u16, body: Vec<u8>) -> Self {
        Self {
            status,
            content_type: "application/octet-stream",
            body,
        }
    }
}

/// 镜像 `backend.ps1` 的 `$safe = $key -replace '[^A-Za-z0-9._-]', '_'`。
pub fn safe_key(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn query_param(target: &str, name: &str) -> Option<String> {
    let query = target.split_once('?')?.1;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k.eq_ignore_ascii_case(name) {
            return Some(percent_decode(v));
        }
    }
    None
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or("");
            if let Ok(b) = u8::from_str_radix(hex, 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        if bytes[i] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[i]);
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

fn replay_candidates(dir: &Path, target: &str, prefix: &str) -> Vec<PathBuf> {
    let name = target
        .split_once('?')
        .map(|(p, _)| p)
        .unwrap_or(target)
        .rsplit('/')
        .next()
        .unwrap_or("");
    vec![
        dir.join(format!("{}.bin", safe_key(&format!("{prefix}{target}")))),
        dir.join(format!("{}.bin", safe_key(target))),
        dir.join(name),
    ]
}

fn read_replay(config: &Config, target: &str, prefix: &str) -> Option<Vec<u8>> {
    let dir = config.replay_dir.as_ref()?;
    for candidate in replay_candidates(dir, target, prefix) {
        if let Ok(data) = std::fs::read(&candidate) {
            return Some(data);
        }
    }
    None
}

fn collect_files(root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir)
            .map_err(|e| format!("读取 {} 失败：{e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("遍历目录失败：{e}"))?;
            let path = entry.path();
            let file_type = entry.file_type().map_err(|e| format!("读取类型失败：{e}"))?;
            if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() {
                let rel = path
                    .strip_prefix(root)
                    .map_err(|_| "相对路径计算失败".to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, path));
            }
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

/// 组装 `backup.php` 的明文：`{status, source_id, password, files}`。
pub fn build_backup_plaintext(template_dir: &Path) -> Result<String, String> {
    if !template_dir.is_dir() {
        return Err(format!("备份模板目录不存在：{}", template_dir.display()));
    }
    let files = collect_files(template_dir)?;
    if files.is_empty() {
        return Err("备份模板目录为空".into());
    }
    let mut map = serde_json::Map::new();
    for (rel, path) in files {
        let data = std::fs::read(&path)
            .map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
        map.insert(rel, Value::String(b64_encode(&data)));
    }
    let plain = json!({
        "status": "1",
        "source_id": BACKUP_SOURCE_ID,
        "password": BACKUP_PASSWORD,
        "files": Value::Object(map),
    });
    Ok(plain.to_string())
}

/// 纯路由：不碰网络，便于单测。
pub fn respond(
    config: &Config,
    method: &str,
    target: &str,
    headers: &BTreeMap<String, String>,
    body: &[u8],
) -> Response {
    let path = target.split_once('?').map(|(p, _)| p).unwrap_or(target);
    let lowered = path.to_ascii_lowercase();

    if path.starts_with(ERASE_PREFIX) {
        return match read_replay(config, target, "erase27") {
            Some(data) => Response::bytes(200, data),
            None => Response::json(
                404,
                &json!({"status":"404","error":"protection archive not available"}),
            ),
        };
    }

    if lowered.ends_with("/win_mdm.php") {
        let venv = VersionEnvelope::new(config.secrets.version_key, config.secrets.version_iv);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        return match venv.seal(VERSION_INNER, now) {
            Ok(value) => Response::json(200, &value),
            Err(e) => Response::json(500, &json!({"status":"500","error":e})),
        };
    }

    if lowered.ends_with("/api1.php") {
        let serial = query_param(target, "serialNumber").unwrap_or_default();
        return Response::json(
            200,
            &json!({"status":"200","message":"AUTHORIZED","serial":serial}),
        );
    }

    if lowered.ends_with("/model.php") {
        let text = String::from_utf8_lossy(body);
        let model = text
            .split('&')
            .find_map(|pair| {
                let (k, v) = pair.split_once('=')?;
                if k.eq_ignore_ascii_case("productType") {
                    Some(percent_decode(v))
                } else {
                    None
                }
            })
            .unwrap_or_else(|| config.product_type.clone());
        return Response::json(
            200,
            &json!({"status":"success","model":model,"productType":model}),
        );
    }

    if lowered.ends_with("/backup.php") {
        let nonce = headers
            .get("x-aldaz-nonce")
            .map(String::as_str)
            .unwrap_or("");
        if !nonce.is_empty() {
            if let Some(dir) = config.template_dir.as_deref() {
                let nonce_bytes = match hex_to_bytes(nonce) {
                    Ok(v) => v,
                    Err(e) => return Response::json(400, &json!({"status":"400","error":e})),
                };
                let plain = match build_backup_plaintext(dir) {
                    Ok(v) => v,
                    Err(e) => return Response::json(500, &json!({"status":"500","error":e})),
                };
                return match Envelope::seal(
                    &config.secrets.backup_secret,
                    &nonce_bytes,
                    plain.as_bytes(),
                    None,
                ) {
                    Ok(env) => Response::json(200, &env.to_json()),
                    Err(e) => Response::json(500, &json!({"status":"500","error":e})),
                };
            }
        }
        if let Some(data) = read_replay(config, target, "backup") {
            return Response::json(200, &serde_json::from_slice(&data).unwrap_or(Value::Null));
        }
        let message = if config.relay {
            "已禁用上游转发：请提供模板目录，或预先填充采集缓存"
        } else {
            "backup 请求需要 X-Aldaz-Nonce 与模板目录；本桩不做上游转发"
        };
        return Response::json(500, &json!({"status":"500","error":message}));
    }

    if lowered.ends_with("/requestactivation.php") {
        return Response::json(200, &json!({"status":"200","message":"OK"}));
    }
    if lowered.ends_with("/amdeviceactivate.php") {
        return Response::json(200, &json!({"status":"200","done":true,"message":"OK"}));
    }
    if lowered.ends_with("/alert.php") {
        return Response::json(200, &json!({"status":"200"}));
    }

    let _ = method;
    Response::json(404, &json!({"status":"404","error":"unknown endpoint"}))
}

fn hex_to_bytes(text: &str) -> Result<Vec<u8>, String> {
    let cleaned: String = text.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if !cleaned.len().is_multiple_of(2) || cleaned.is_empty() {
        return Err("X-Aldaz-Nonce 不是合法的十六进制串".into());
    }
    (0..cleaned.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&cleaned[i..i + 2], 16)
                .map_err(|_| "X-Aldaz-Nonce 含非法字符".to_string())
        })
        .collect()
}

// ------------------------------------------------------------------ 服务循环

pub struct Server {
    addr: SocketAddr,
    stop: Arc<AtomicBool>,
    calls: CallLog,
    join: Option<JoinHandle<()>>,
}

impl Server {
    /// 绑定回环并启动后台线程。端口传 0 表示由系统分配（测试用）。
    pub fn start(config: Config, port: u16) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", port))
            .map_err(|e| format!("监听 127.0.0.1:{port} 失败：{e}"))?;
        let addr = listener
            .local_addr()
            .map_err(|e| format!("取得本地地址失败：{e}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| format!("设置非阻塞失败：{e}"))?;

        let stop = Arc::new(AtomicBool::new(false));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let stop_flag = Arc::clone(&stop);
        let calls_ref = Arc::clone(&calls);

        let join = thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let _ = handle_connection(&mut stream, &config, &calls_ref);
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(20));
                    }
                    Err(_) => break,
                }
            }
        });

        Ok(Self {
            addr,
            stop,
            calls,
            join: Some(join),
        })
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
    pub fn calls(&self) -> Vec<(String, String, u16)> {
        self.calls.lock().map(|v| v.clone()).unwrap_or_default()
    }
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.join.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_connection(
    stream: &mut TcpStream,
    config: &Config,
    calls: &CallLog,
) -> Result<(), String> {
    // Windows 上 accept() 出来的套接字可能继承监听套接字的非阻塞模式；那样
    // read 会立刻返回 WouldBlock，被当成"连接提前关闭"。这里显式改回阻塞 +
    // 读超时，行为才确定。
    stream
        .set_nonblocking(false)
        .map_err(|e| format!("设置阻塞模式失败：{e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];

    let head_end = loop {
        match stream.read(&mut chunk) {
            Ok(0) => return Err("连接提前关闭".into()),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e) => return Err(format!("读取请求失败：{e}")),
        }
        if buf.len() > 1_048_576 {
            return Err("请求超过 1 MiB".into());
        }
        if let Some(pos) = find_sequence(&buf, b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buf[..pos]).to_string();
            let want = content_length(&head);
            if buf.len() >= pos + 4 + want {
                break pos;
            }
        }
    };

    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("GET").to_string();
    let target = parts.next().unwrap_or("/").to_string();

    let mut headers = BTreeMap::new();
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }

    let body_start = head_end + 4;
    let want = content_length(&head);
    let body = if want > 0 && body_start + want <= buf.len() {
        buf[body_start..body_start + want].to_vec()
    } else {
        Vec::new()
    };

    let response = respond(config, &method, &target, &headers, &body);

    let head_text = format!(
        "HTTP/1.1 {} OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.content_type,
        response.body.len()
    );
    stream
        .write_all(head_text.as_bytes())
        .and_then(|_| stream.write_all(&response.body))
        .and_then(|_| stream.flush())
        .map_err(|e| format!("写响应失败：{e}"))?;

    if let Ok(mut log) = calls.lock() {
        log.push((method, target, response.status));
    }
    Ok(())
}

fn find_sequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn content_length(head: &str) -> usize {
    for line in head.split("\r\n") {
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                return v.trim().parse().unwrap_or(0);
            }
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secrets() -> Secrets {
        Secrets {
            version_key: [9u8; 32],
            version_iv: [8u8; 16],
            backup_secret: [7u8; 32],
            offset: 0,
        }
    }

    /// 每个调用都要拿到**独立**目录：这些测试在同一进程里并行跑，共用一个路径
    /// 会互相 `remove_dir_all`，表现为随机失败。
    fn temp_template() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicUsize =
            std::sync::atomic::AtomicUsize::new(0);
        let seq = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("atk-tpl-{}-{seq}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("Info.plist"), b"bplist00-fake").unwrap();
        std::fs::write(dir.join("sub/blob.bin"), [0u8, 1, 2]).unwrap();
        dir
    }

    #[test]
    fn version_route_decrypts_with_the_same_keys() {
        let config = Config::new(secrets());
        let response = respond(&config, "GET", "/A12/mdm/win_mdm.php", &BTreeMap::new(), b"");
        assert_eq!(response.status, 200);
        let value: Value = serde_json::from_slice(&response.body).unwrap();
        let venv = VersionEnvelope::new(config.secrets.version_key, config.secrets.version_iv);
        let now = value.get("timestamp").and_then(Value::as_i64).unwrap();
        let inner = venv.open(&value, now).unwrap();
        let doc: Value =
            serde_json::from_str(inner.get("data").and_then(Value::as_str).unwrap()).unwrap();
        assert_eq!(doc.get("version").and_then(Value::as_str), Some("1.0.0"));
    }

    #[test]
    fn authorize_echoes_serial() {
        let config = Config::new(secrets());
        let response = respond(
            &config,
            "GET",
            "/A12/mdm/api1.php?serialNumber=DJN929M5XP",
            &BTreeMap::new(),
            b"",
        );
        let value: Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value.get("message").and_then(Value::as_str), Some("AUTHORIZED"));
        assert_eq!(value.get("serial").and_then(Value::as_str), Some("DJN929M5XP"));
    }

    #[test]
    fn model_uses_form_value() {
        let config = Config::new(secrets());
        let response = respond(
            &config,
            "POST",
            "/A12/mdm/model.php",
            &BTreeMap::new(),
            b"productType=iPhone15,2",
        );
        let value: Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(value.get("productType").and_then(Value::as_str), Some("iPhone15,2"));
    }

    #[test]
    fn backup_requires_nonce() {
        let mut config = Config::new(secrets());
        config.template_dir = Some(temp_template());
        let response = respond(&config, "POST", "/A12/mdm/backup.php", &BTreeMap::new(), b"{}");
        assert_eq!(response.status, 500);
        let value: Value = serde_json::from_slice(&response.body).unwrap();
        assert!(value
            .get("error")
            .and_then(Value::as_str)
            .unwrap()
            .contains("X-Aldaz-Nonce"));
    }

    #[test]
    fn backup_envelope_opens_with_nonce() {
        let template = temp_template();
        let mut config = Config::new(secrets());
        config.template_dir = Some(template);

        let nonce = [0x11u8; 16];
        let mut headers = BTreeMap::new();
        headers.insert("x-aldaz-nonce".to_string(), nonce.iter().map(|b| format!("{b:02x}")).collect());
        let response = respond(&config, "POST", "/A12/mdm/backup.php", &headers, b"{}");
        assert_eq!(response.status, 200);

        let value: Value = serde_json::from_slice(&response.body).unwrap();
        let envelope = Envelope::from_json(&value).unwrap();
        assert!(envelope.verify_mac(&config.secrets.backup_secret, &nonce));
        let plain = envelope.open(&config.secrets.backup_secret, &nonce).unwrap();
        let doc: Value = serde_json::from_slice(&plain).unwrap();
        assert_eq!(doc.get("status").and_then(Value::as_str), Some("1"));
        assert_eq!(doc.get("password").and_then(Value::as_str), Some(BACKUP_PASSWORD));
        let files = doc.get("files").and_then(Value::as_object).unwrap();
        assert_eq!(files.len(), 2);
        assert!(files.contains_key("Info.plist"));
        assert!(files.contains_key("sub/blob.bin"));
    }

    #[test]
    fn erase27_serves_cached_bytes() {
        let dir = std::env::temp_dir().join(format!("atk-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let name = "erase27_Erase27_gaval_ios27_disabled.enc.bin";
        std::fs::write(dir.join(name), b"G27E2payload").unwrap();

        let mut config = Config::new(secrets());
        config.replay_dir = Some(dir.clone());
        let response = respond(
            &config,
            "GET",
            &format!("/Erase27/{name}"),
            &BTreeMap::new(),
            b"",
        );
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"G27E2payload");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unknown_endpoint_is_404() {
        let config = Config::new(secrets());
        let response = respond(&config, "GET", "/A12/mdm/nope.php", &BTreeMap::new(), b"");
        assert_eq!(response.status, 404);
    }

    #[test]
    fn safe_key_matches_powershell_rule() {
        assert_eq!(safe_key("/A12/mdm/backup.php"), "_A12_mdm_backup.php");
        assert_eq!(safe_key("a b/c"), "a_b_c");
    }

    #[test]
    fn server_serves_over_loopback() {
        let config = Config::new(secrets());
        let server = Server::start(config, 0).expect("启动失败");
        let url = format!("{}/A12/mdm/api1.php?serialNumber=ABC", server.base_url());
        let response = ureq_free_get(&server.addr(), "/A12/mdm/api1.php?serialNumber=ABC");
        assert!(response.contains("AUTHORIZED"), "响应：{response}");
        assert!(response.contains("ABC"));
        let _ = url;
    }

    /// 极简 HTTP 客户端，避免为测试引入 reqwest。
    fn ureq_free_get(addr: &SocketAddr, target: &str) -> String {
        let mut stream = TcpStream::connect(addr).expect("连接失败");
        let request = format!("GET {target} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let mut out = Vec::new();
        let _ = stream.read_to_end(&mut out);
        String::from_utf8_lossy(&out).to_string()
    }
}
