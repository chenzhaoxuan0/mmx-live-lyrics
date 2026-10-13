//! mmx speech_to_text 客户端。
//!
//! 契约（已用 curl 实测）：
//!   POST {base_url}/v1/speech_to_text
//!   Authorization: Bearer <key>
//!   multipart/form-data: model=asr-1.0, file=<wav>, [language], [response_format=json]
//!   200 -> {"text": "...", "duration": 4.2, "trace_id": "..."}
use serde_json::Value;
use std::time::Duration;

const BOUNDARY: &str = "----mmxlyrics1701a2b3c4d";

#[derive(Debug)]
pub enum ApiError {
    Auth(String),
    Server(String),
    Network(String),
}

impl ApiError {
    pub fn message(&self) -> String {
        match self {
            ApiError::Auth(m) => format!("鉴权失败：{m}"),
            ApiError::Server(m) => format!("服务端错误：{m}"),
            ApiError::Network(m) => format!("网络错误：{m}"),
        }
    }
    pub fn is_auth(&self) -> bool {
        matches!(self, ApiError::Auth(_))
    }
}

/// API Key 解析优先级：设置里的覆盖值 → ~/.mmx/config.json（mmx CLI 登录态）
pub fn resolve_key(override_key: Option<&str>) -> Result<String, String> {
    if let Some(k) = override_key {
        if !k.trim().is_empty() {
            return Ok(k.trim().to_string());
        }
    }
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .map_err(|_| "无法定位用户目录".to_string())?;
    let path = std::path::Path::new(&home).join(".mmx").join("config.json");
    let txt = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "未找到 {}（mmx CLI 未登录）；也可在设置里手动填写 API Key",
            path.display()
        )
    })?;
    let v: Value =
        serde_json::from_str(&txt).map_err(|_| "解析 ~/.mmx/config.json 失败".to_string())?;
    v.get("api_key")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "~/.mmx/config.json 中没有 api_key".to_string())
}

pub struct MmxAsr {
    agent: ureq::Agent,
    base_url: String,
    key: String,
}

impl MmxAsr {
    pub fn new(base_url: &str, key: String) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .build();
        Self {
            agent,
            base_url: base_url.trim_end_matches('/').to_string(),
            key,
        }
    }

    /// 上传一段 16k mono f32 音频，返回识别文本（可能为空串）
    pub fn transcribe(&self, pcm16k: &[f32], language: &str) -> Result<String, ApiError> {
        let url = format!("{}/v1/speech_to_text", self.base_url);
        let body = build_multipart(pcm16k, language);
        let resp = self
            .agent
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.key))
            .set(
                "Content-Type",
                &format!("multipart/form-data; boundary={BOUNDARY}"),
            )
            .send_bytes(&body);
        match resp {
            Ok(r) => {
                let txt = r
                    .into_string()
                    .map_err(|e| ApiError::Network(e.to_string()))?;
                let v: Value = serde_json::from_str(&txt)
                    .map_err(|e| ApiError::Network(format!("响应解析失败：{e}")))?;
                if let Some(code) = v.pointer("/base_resp/status_code").and_then(Value::as_i64) {
                    if code != 0 {
                        let msg = v
                            .pointer("/base_resp/status_msg")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        return Err(match code {
                            1001 | 1004 | 1013 | 1014 | 1026 | 1039 => {
                                ApiError::Auth(msg.to_string())
                            }
                            _ => ApiError::Server(format!("code={code} {msg}")),
                        });
                    }
                }
                Ok(v.get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string())
            }
            Err(ureq::Error::Status(code, r)) => {
                let body = r.into_string().unwrap_or_default();
                Err(match code {
                    401 | 403 => ApiError::Auth(format!("API Key 无效（HTTP {code}）")),
                    _ => ApiError::Server(format!("HTTP {code}：{}", truncate(&body, 200))),
                })
            }
            Err(e) => Err(ApiError::Network(e.to_string())),
        }
    }
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn build_multipart(pcm16k: &[f32], language: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(1024 + pcm16k.len() * 2);
    let field = |name: &str, value: &str, out: &mut Vec<u8>| {
        out.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
        out.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    };
    field("model", "asr-1.0", &mut out);
    if !language.is_empty() {
        field("language", language, &mut out);
    }
    out.extend_from_slice(format!("--{BOUNDARY}\r\n").as_bytes());
    out.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"seg.wav\"\r\n\
          Content-Type: audio/wav\r\n\r\n",
    );
    crate::wav::encode_into(pcm16k, &mut out);
    out.extend_from_slice(format!("\r\n--{BOUNDARY}--\r\n").as_bytes());
    out
}
