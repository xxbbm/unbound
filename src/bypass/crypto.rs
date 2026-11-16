//! 与厂商服务端字节级对齐的两套信封。
//!
//! 逆向自 `iFineMDM` 的 `backend.ps1`，并用厂商自己的 PowerShell 函数做过互通
//! 验证：派生密钥、密文、MAC 全部逐字节相同。
//!
//! # 两套方案
//!
//! * **备份信封**：`HKDF-SHA256 派生 -> AES-256-CBC -> HMAC-SHA256(iv||ct)`，
//!   随机 16 字节 nonce 走请求头 `X-Aldaz-Nonce`。
//! * **版本信封**：固定 key + 固定 IV 的 AES-CBC，整体 Base64，无 MAC；
//!   明文里带 `timestamp`，校验 ±300 秒。
//!
//! # 最容易写错的一处
//!
//! HKDF 里 **nonce 是盐（HMAC 的 key）、密钥是 IKM（HMAC 的 data）**。这符合
//! RFC 5869 Extract，真正的坑是把两者对调 —— HMAC 对操作数不对称，写反会得到
//! 一把"看起来正常"的 32 字节密钥，然后 MAC 校验失败且没有诊断信息。
//! 另外这里没有独立的 `info` 参数，标签本身就是 info。

use aes::cipher::{
    generic_array::{typenum::U16, GenericArray},
    BlockDecrypt, BlockEncrypt, KeyInit,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const LABEL_ENC: &[u8] = b"aldaz-resp-enc";
pub const LABEL_MAC: &[u8] = b"aldaz-resp-mac";
pub const IV_LEN: usize = 16;
pub const MAC_LEN: usize = 32;
pub const VERSION_SKEW_SECONDS: i64 = 300;

// ---------------------------------------------------------------- base64

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b1 = *chunk.get(1).unwrap_or(&0);
        let b2 = *chunk.get(2).unwrap_or(&0);
        let n = ((chunk[0] as u32) << 16) | ((b1 as u32) << 8) | b2 as u32;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

pub fn b64_decode(text: &str) -> Result<Vec<u8>, String> {
    let mut acc: u32 = 0;
    let mut bits = 0u32;
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for ch in text.bytes() {
        let val = match ch {
            b'A'..=b'Z' => ch - b'A',
            b'a'..=b'z' => ch - b'a' + 26,
            b'0'..=b'9' => ch - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\r' | b'\n' | b' ' | b'\t' => continue,
            _ => return Err("Base64 含非法字符".into()),
        } as u32;
        acc = (acc << 6) | val;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xFF) as u8);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- 填充

pub fn pkcs7_pad(data: &[u8], block: usize) -> Vec<u8> {
    let pad = block - (data.len() % block);
    let mut out = Vec::with_capacity(data.len() + pad);
    out.extend_from_slice(data);
    out.extend(std::iter::repeat_n(pad as u8, pad));
    out
}

pub fn pkcs7_unpad(data: &[u8], block: usize) -> Result<Vec<u8>, String> {
    if data.is_empty() || !data.len().is_multiple_of(block) {
        return Err("密文长度不是块大小的整数倍".into());
    }
    let pad = *data.last().unwrap() as usize;
    if pad == 0 || pad > block || pad > data.len() {
        return Err("PKCS#7 填充无效".into());
    }
    if !data[data.len() - pad..].iter().all(|b| *b as usize == pad) {
        return Err("PKCS#7 填充无效".into());
    }
    Ok(data[..data.len() - pad].to_vec())
}

// ---------------------------------------------------------------- HMAC / HKDF

/// HMAC-SHA256。用 sha2 手写，避免再引一套哈希实现。
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        let mut h = Sha256::new();
        h.update(key);
        let d = h.finalize();
        k[..32].copy_from_slice(&d);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    let outer_hash = outer.finalize();

    let mut out = [0u8; 32];
    out.copy_from_slice(&outer_hash);
    out
}

/// 厂商 KDF：`prk = HMAC(key=nonce, data=secret)`，再单块 Expand over `label || 0x01`。
pub fn hkdf_like(secret: &[u8], nonce: &[u8], label: &[u8]) -> [u8; 32] {
    let prk = hmac_sha256(nonce, secret);
    let mut info = Vec::with_capacity(label.len() + 1);
    info.extend_from_slice(label);
    info.push(1);
    hmac_sha256(&prk, &info)
}

/// MAC 覆盖 `iv || ct`（先加密后认证）。
pub fn mac_of(secret: &[u8], nonce: &[u8], iv: &[u8], ct: &[u8]) -> [u8; 32] {
    let key = hkdf_like(secret, nonce, LABEL_MAC);
    let mut data = Vec::with_capacity(iv.len() + ct.len());
    data.extend_from_slice(iv);
    data.extend_from_slice(ct);
    hmac_sha256(&key, &data)
}

// ---------------------------------------------------------------- AES-CBC

fn aes_cbc(key: &[u8], iv: &[u8], data: &[u8], encrypt: bool) -> Result<Vec<u8>, String> {
    if key.len() != 32 {
        return Err("AES 密钥必须是 32 字节".into());
    }
    if iv.len() != IV_LEN {
        return Err("IV 必须是 16 字节".into());
    }
    if data.is_empty() || !data.len().is_multiple_of(16) {
        return Err("数据长度必须是 16 的正整数倍".into());
    }
    let cipher = aes::Aes256::new_from_slice(key).map_err(|e| format!("AES 密钥无效: {e}"))?;
    let mut prev = [0u8; 16];
    prev.copy_from_slice(iv);

    let mut out = Vec::with_capacity(data.len());
    for chunk in data.as_chunks::<16>().0 {
        let mut block = [0u8; 16];
        if encrypt {
            for i in 0..16 {
                block[i] = chunk[i] ^ prev[i];
            }
        } else {
            block.copy_from_slice(chunk);
        }

        let mut generic: GenericArray<u8, U16> = GenericArray::clone_from_slice(&block);
        if encrypt {
            cipher.encrypt_block(&mut generic);
        } else {
            cipher.decrypt_block(&mut generic);
        }

        let mut result_block = [0u8; 16];
        result_block.copy_from_slice(generic.as_slice());
        if encrypt {
            out.extend_from_slice(&result_block);
            prev = result_block;
        } else {
            for i in 0..16 {
                out.push(result_block[i] ^ prev[i]);
            }
            prev.copy_from_slice(chunk);
        }
    }
    Ok(out)
}

fn random_bytes(len: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; len];
    getrandom::getrandom(&mut buf).map_err(|e| format!("取随机数失败: {e}"))?;
    Ok(buf)
}

// ---------------------------------------------------------------- 备份信封

/// `backup.php` 的响应体：`{"iv":b64,"ct":b64,"mac":b64}`。
#[derive(Clone, Debug)]
pub struct Envelope {
    pub iv: [u8; IV_LEN],
    pub ct: Vec<u8>,
    pub mac: [u8; MAC_LEN],
}

impl Envelope {
    pub fn seal(
        secret: &[u8],
        nonce: &[u8],
        plaintext: &[u8],
        iv: Option<[u8; IV_LEN]>,
    ) -> Result<Self, String> {
        let iv = match iv {
            Some(v) => v,
            None => {
                let raw = random_bytes(IV_LEN)?;
                let mut v = [0u8; IV_LEN];
                v.copy_from_slice(&raw);
                v
            }
        };
        let key = hkdf_like(secret, nonce, LABEL_ENC);
        let ct = aes_cbc(&key, &iv, &pkcs7_pad(plaintext, 16), true)?;
        let mac = mac_of(secret, nonce, &iv, &ct);
        Ok(Self { iv, ct, mac })
    }

    pub fn from_json(value: &Value) -> Result<Self, String> {
        let field = |name: &str| -> Result<Vec<u8>, String> {
            value
                .get(name)
                .and_then(Value::as_str)
                .ok_or_else(|| format!("信封缺少字段 {name}"))
                .and_then(b64_decode)
        };
        let iv_bytes = field("iv")?;
        if iv_bytes.len() != IV_LEN {
            return Err("信封 iv 长度不是 16".into());
        }
        let mut iv = [0u8; IV_LEN];
        iv.copy_from_slice(&iv_bytes);

        let ct = field("ct")?;
        if ct.is_empty() || ct.len() % 16 != 0 {
            return Err("信封 ct 长度无效".into());
        }
        let mac_bytes = field("mac")?;
        if mac_bytes.len() != MAC_LEN {
            return Err("信封 mac 长度不是 32".into());
        }
        let mut mac = [0u8; MAC_LEN];
        mac.copy_from_slice(&mac_bytes);
        Ok(Self { iv, ct, mac })
    }

    pub fn to_json(&self) -> Value {
        json!({
            "iv": b64_encode(&self.iv),
            "ct": b64_encode(&self.ct),
            "mac": b64_encode(&self.mac),
        })
    }

    pub fn verify_mac(&self, secret: &[u8], nonce: &[u8]) -> bool {
        let expected = mac_of(secret, nonce, &self.iv, &self.ct);
        // 定长比较，不做提前返回
        expected
            .iter()
            .zip(self.mac.iter())
            .fold(0u8, |acc, (a, b)| acc | (a ^ b))
            == 0
    }

    /// 先验 MAC，再解密，最后去填充。顺序不能颠倒。
    pub fn open(&self, secret: &[u8], nonce: &[u8]) -> Result<Vec<u8>, String> {
        if !self.verify_mac(secret, nonce) {
            return Err("信封 MAC 校验失败：密钥、nonce 或内容不匹配".into());
        }
        let key = hkdf_like(secret, nonce, LABEL_ENC);
        let padded = aes_cbc(&key, &self.iv, &self.ct, false)?;
        pkcs7_unpad(&padded, 16)
    }
}

// ---------------------------------------------------------------- 版本信封

/// `win_mdm.php` 的响应：固定 key/IV，无 MAC，`timestamp` 在密文内部。
#[derive(Clone, Debug)]
pub struct VersionEnvelope {
    pub version_key: [u8; 32],
    pub version_iv: [u8; IV_LEN],
}

impl VersionEnvelope {
    pub fn new(version_key: [u8; 32], version_iv: [u8; IV_LEN]) -> Self {
        Self {
            version_key,
            version_iv,
        }
    }

    /// 生成 `{"timestamp":ts,"data":"<密文Base64>"}`。
    ///
    /// 内层文档是**字符串**，其引号需要被转义 —— 厂商用 `Replace('"','\"')`，
    /// 这里用 `json!` 得到同样结果，且对含反斜杠的内容也正确。
    pub fn seal(&self, inner: &str, timestamp: i64) -> Result<Value, String> {
        let plain = serde_json::to_vec(&json!({ "timestamp": timestamp, "data": inner }))
            .map_err(|e| format!("序列化版本明文失败: {e}"))?;
        let ct = aes_cbc(
            &self.version_key,
            &self.version_iv,
            &pkcs7_pad(&plain, 16),
            true,
        )?;
        Ok(json!({ "timestamp": timestamp, "data": b64_encode(&ct) }))
    }

    /// 解密并校验时效。`now` 由调用方给，便于测试。
    pub fn open(&self, value: &Value, now: i64) -> Result<Value, String> {
        let data = value
            .get("data")
            .and_then(Value::as_str)
            .ok_or("版本信封缺少 data")?;
        let ct = b64_decode(data)?;
        let padded = aes_cbc(&self.version_key, &self.version_iv, &ct, false)?;
        let plain = pkcs7_unpad(&padded, 16)?;
        let inner: Value = serde_json::from_slice(&plain)
            .map_err(|e| format!("版本密文不是 JSON: {e}"))?;
        let ts = inner
            .get("timestamp")
            .and_then(Value::as_i64)
            .ok_or("版本明文缺少 timestamp")?;
        let skew = (now - ts).abs();
        if skew > VERSION_SKEW_SECONDS {
            return Err(format!(
                "版本响应已过期 {skew} 秒（上限 {VERSION_SKEW_SECONDS} 秒）"
            ));
        }
        Ok(inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secret() -> Vec<u8> {
        (0..32u8).collect()
    }
    fn nonce() -> Vec<u8> {
        (100..116u8).collect()
    }

    #[test]
    fn hmac_matches_rfc4231_case1() {
        // RFC 4231 Test Case 1
        let out = hmac_sha256(&[0x0b; 20], b"Hi There");
        let hex: String = out.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(
            hex,
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn base64_round_trip() {
        for len in [0usize, 1, 2, 3, 4, 15, 16, 17] {
            let data: Vec<u8> = (0..len).map(|i| i as u8).collect();
            let encoded = b64_encode(&data);
            assert_eq!(b64_decode(&encoded).unwrap(), data);
        }
        assert_eq!(b64_encode(b"ab"), "YWI=");
        assert_eq!(b64_encode(b"abc"), "YWJj");
    }

    #[test]
    fn pkcs7_round_trip_and_rejects_bad_padding() {
        for len in [0usize, 1, 15, 16, 17] {
            let data: Vec<u8> = (0..len).map(|i| i as u8).collect();
            let padded = pkcs7_pad(&data, 16);
            assert_eq!(padded.len() % 16, 0);
            assert_eq!(pkcs7_unpad(&padded, 16).unwrap(), data);
        }
        assert!(pkcs7_unpad(&[0u8; 15], 16).is_err());
        assert!(pkcs7_unpad(&[0u8; 15].iter().copied().chain([5u8]).collect::<Vec<_>>(), 16).is_err());
    }

    #[test]
    fn hkdf_roles_are_salt_is_nonce() {
        // 对调后必须不同，否则说明参数顺序写反了
        let a = hkdf_like(&secret(), &nonce(), LABEL_ENC);
        let b = hkdf_like(&nonce(), &secret(), LABEL_ENC);
        assert_ne!(a, b);
        assert_ne!(
            hkdf_like(&secret(), &nonce(), LABEL_ENC),
            hkdf_like(&secret(), &nonce(), LABEL_MAC)
        );
    }

    #[test]
    fn envelope_seal_open_round_trip() {
        let iv = [7u8; IV_LEN];
        let env = Envelope::seal(&secret(), &nonce(), b"{\"status\":\"1\"}", Some(iv)).unwrap();
        assert_eq!(env.open(&secret(), &nonce()).unwrap(), b"{\"status\":\"1\"}");
        assert!(env.verify_mac(&secret(), &nonce()));
    }

    #[test]
    fn envelope_rejects_tampering_and_wrong_nonce() {
        let iv = [7u8; IV_LEN];
        let env = Envelope::seal(&secret(), &nonce(), b"hello", Some(iv)).unwrap();

        let mut bad = env.clone();
        bad.ct[0] ^= 0x01;
        assert!(bad.open(&secret(), &nonce()).is_err());

        let mut bad_mac = env.clone();
        bad_mac.mac[0] ^= 0x01;
        assert!(!bad_mac.verify_mac(&secret(), &nonce()));

        assert!(env.open(&secret(), &[0u8; 16]).is_err());
    }

    #[test]
    fn envelope_json_round_trip() {
        let env = Envelope::seal(&secret(), &nonce(), b"payload", Some([1u8; IV_LEN])).unwrap();
        let again = Envelope::from_json(&env.to_json()).unwrap();
        assert_eq!(again.open(&secret(), &nonce()).unwrap(), b"payload");
    }

    #[test]
    fn version_envelope_round_trip_and_skew() {
        let venv = VersionEnvelope::new([3u8; 32], [4u8; IV_LEN]);
        let inner = r#"{"version":"1.0.0","status":"ok"}"#;
        let sealed = venv.seal(inner, 1_700_000_000).unwrap();
        let opened = venv.open(&sealed, 1_700_000_000 + 299).unwrap();
        assert_eq!(opened.get("data").and_then(Value::as_str).unwrap(), inner);
        assert!(venv.open(&sealed, 1_700_000_000 + 301).is_err());
    }
}
