//! ASR 客户端：可插拔的服务商适配层。
//!
//! 支持三类接口：
//!   mmx    —— MiniMax 原生：POST {base_url}/v1/speech_to_text（multipart，Bearer）
//!   openai —— OpenAI 兼容：POST {base_url}/audio/transcriptions（multipart，Bearer）
//!             兼容 Whisper 本地服务（faster-whisper-server / whisper.cpp server 等）
//!   custom —— 通用自定义：POST {base_url}{custom_path}，JSON 体（base64 音频），
//!             按 custom_text_path 从响应里取文本
//!
//! 三类都返回识别文本；错误统一映射成 Auth / Server / Network 三类。
use serde_json::Value;
use std::time::Duration;

const MMX_BOUNDARY: &str = "----mmxlyrics1701a2b3c4d";
const OPENAI_BOUNDARY: &str = "----mmxlyrics5d4c3b2a1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Mmx,
    OpenAi,
    Custom,
}

impl Provider {
    pub fn parse(s: &str) -> Provider {
        match s.trim().to_ascii_lowercase().as_str() {
            "openai" | "openai-compatible" | "whisper" | "兼容" => Provider::OpenAi,
            "custom" | "http" | "json" | "自定义" => Provider::Custom,
            _ => Provider::Mmx,
        }
    }
}

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

/// MMX CLI 登录态里的 Key（~/.mmx/config.json）
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

/// 按服务商取 Key：mmx 可回退到 CLI 登录态；其他服务商只用设置里填的（本地服务可留空）
pub fn provider_key(override_key: Option<&str>, provider: Provider) -> Result<Option<String>, String> {
    if let Some(k) = override_key {
        let k = k.trim();
        if !k.is_empty() {
            return Ok(Some(k.to_string()));
        }
    }
    match provider {
        Provider::Mmx => Ok(Some(resolve_key(None)?)),
        _ => Ok(None),
    }
}

pub struct AsrClient {
    agent: ureq::Agent,
    provider: Provider,
    base_url: String,
    key: Option<String>,
    model: String,
    custom_path: String,
    custom_text_path: String,
}

impl AsrClient {
    pub fn new(
        provider: Provider,
        base_url: &str,
        key: Option<String>,
        model: &str,
        custom_path: &str,
        custom_text_path: &str,
    ) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(8))
            .timeout(Duration::from_secs(20))
            .build();
        Self {
            agent,
            provider,
            base_url: base_url.trim_end_matches('/').to_string(),
            key,
            model: model.trim().to_string(),
            custom_path: custom_path.trim().to_string(),
            custom_text_path: custom_text_path.trim().to_string(),
        }
    }

    /// 按当前设置构造客户端。mmx 无可用 Key 时报错；其他服务商允许无 Key（本地服务）。
    pub fn from_settings(s: &crate::config::Settings) -> Result<Self, String> {
        let provider = Provider::parse(&s.provider);
        let key = provider_key(s.api_key.as_deref(), provider)?;
        Ok(Self::new(
            provider,
            &s.base_url,
            key,
            &s.model,
            &s.custom_path,
            &s.custom_text_path,
        ))
    }

    /// 上传一段 16k mono f32 音频，返回识别文本（可能为空串）
    pub fn transcribe(&self, pcm16k: &[f32], language: &str) -> Result<String, ApiError> {
        match self.provider {
            Provider::Mmx => self.transcribe_mmx(pcm16k, language),
            Provider::OpenAi => self.transcribe_openai(pcm16k, language),
            Provider::Custom => self.transcribe_custom(pcm16k, language),
        }
    }

    fn endpoint_mmx(&self) -> String {
        // base_url 默认就是主机根（https://api.minimaxi.com），接口固定挂在 /v1/speech_to_text
        let b = if self.base_url.ends_with("/v1") {
            self.base_url.clone()
        } else {
            format!("{}/v1", self.base_url)
        };
        format!("{}/speech_to_text", b)
    }

    fn endpoint_openai(&self) -> String {
        // 约定 base_url 形如 https://api.openai.com/v1；缺 /v1 时补上
        let b = if self.base_url.ends_with("/v1") {
            self.base_url.clone()
        } else if self.base_url.rsplit('/').next().map(|s| s.contains('.')).unwrap_or(false) {
            format!("{}/v1", self.base_url)
        } else {
            self.base_url.clone()
        };
        format!("{}/audio/transcriptions", b)
    }

    fn endpoint_custom(&self) -> String {
        let p = self.custom_path.trim();
        if p.is_empty() {
            self.base_url.clone()
        } else if p.starts_with('/') {
            format!("{}{}", self.base_url, p)
        } else {
            format!("{}/{}", self.base_url, p)
        }
    }

    fn auth_header(&self) -> Option<(&'static str, String)> {
        self.key
            .as_ref()
            .map(|k| ("Authorization", format!("Bearer {}", k)))
    }

    fn transcribe_mmx(&self, pcm16k: &[f32], language: &str) -> Result<String, ApiError> {
        let url = self.endpoint_mmx();
        let model = if self.model.is_empty() {
            "asr-1.0".to_string()
        } else {
            self.model.clone()
        };
        let body = build_multipart(pcm16k, language, &model, MMX_BOUNDARY);
        let mut req = self
            .agent
            .post(&url)
            .set("Content-Type", &format!("multipart/form-data; boundary={MMX_BOUNDARY}"));
        if let Some((k, v)) = self.auth_header() {
            req = req.set(k, &v);
        }
        let resp = req.send_bytes(&body);
        let v = read_json(resp)?;
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
        Ok(v
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string())
    }

    fn transcribe_openai(&self, pcm16k: &[f32], language: &str) -> Result<String, ApiError> {
        let url = self.endpoint_openai();
        let model = if self.model.is_empty() {
            "whisper-1".to_string()
        } else {
            self.model.clone()
        };
        let body = build_multipart(pcm16k, language, &model, OPENAI_BOUNDARY);
        let mut req = self
            .agent
            .post(&url)
            .set("Content-Type", &format!("multipart/form-data; boundary={OPENAI_BOUNDARY}"));
        if let Some((k, v)) = self.auth_header() {
            req = req.set(k, &v);
        }
        let resp = req.send_bytes(&body);
        let v = read_json(resp)?;
        Ok(v
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string())
    }

    fn transcribe_custom(&self, pcm16k: &[f32], language: &str) -> Result<String, ApiError> {
        let url = self.endpoint_custom();
        let wav = crate::wav::encode(pcm16k);
        let body = serde_json::json!({
            "audio": base64_encode(&wav),
            "audio_format": "wav",
            "language": language,
            "model": self.model,
        });
        let mut req = self.agent.post(&url).set("Content-Type", "application/json");
        if let Some((k, v)) = self.auth_header() {
            req = req.set(k, &v);
        }
        let resp = req.send_json(&body);
        let v = read_json(resp)?;
        let path = if self.custom_text_path.is_empty() {
            "text"
        } else {
            self.custom_text_path.as_str()
        };
        Ok(dig_text(&v, path).unwrap_or_default().trim().to_string())
    }
}

fn read_json(resp: Result<ureq::Response, ureq::Error>) -> Result<Value, ApiError> {
    match resp {
        Ok(r) => {
            let txt = r.into_string().map_err(|e| ApiError::Network(e.to_string()))?;
            serde_json::from_str(&txt)
                .map_err(|e| ApiError::Network(format!("响应解析失败：{e}")))
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

/// 按点号路径从 JSON 里取文本（支持 a.b、a.0.b）
fn dig_text(v: &Value, path: &str) -> Option<String> {
    let mut cur = v;
    for seg in path.split('.').filter(|s| !s.is_empty()) {
        cur = match cur {
            Value::Object(m) => m.get(seg)?,
            Value::Array(a) => a.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    match cur {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn truncate(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

fn build_multipart(pcm16k: &[f32], language: &str, model: &str, boundary: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(1024 + pcm16k.len() * 2);
    let field = |name: &str, value: &str, out: &mut Vec<u8>| {
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        out.extend_from_slice(
            format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n")
                .as_bytes(),
        );
    };
    if !model.is_empty() {
        field("model", model, &mut out);
    }
    if !language.is_empty() {
        field("language", language, &mut out);
    }
    out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
    out.extend_from_slice(
        b"Content-Disposition: form-data; name=\"file\"; filename=\"seg.wav\"\r\n\
          Content-Type: audio/wav\r\n\r\n",
    );
    crate::wav::encode_into(pcm16k, &mut out);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

/// 极简 base64（避免为一个小功能引入额外依赖）
fn base64_encode(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(T[((n >> 18) & 63) as usize] as char);
        out.push(T[((n >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            T[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_parsing() {
        assert_eq!(Provider::parse("MMX"), Provider::Mmx);
        assert_eq!(Provider::parse("openai"), Provider::OpenAi);
        assert_eq!(Provider::parse("Custom"), Provider::Custom);
        assert_eq!(Provider::parse("乱七八糟"), Provider::Mmx);
    }

    #[test]
    fn endpoints() {
        let mmx = AsrClient::new(Provider::Mmx, "https://api.minimaxi.com", None, "", "", "");
        assert_eq!(mmx.endpoint_mmx(), "https://api.minimaxi.com/v1/speech_to_text");
        let oa = AsrClient::new(Provider::OpenAi, "https://api.openai.com/v1", None, "", "", "");
        assert_eq!(oa.endpoint_openai(), "https://api.openai.com/v1/audio/transcriptions");
        let oa2 = AsrClient::new(Provider::OpenAi, "http://127.0.0.1:8000", None, "", "", "");
        assert_eq!(oa2.endpoint_openai(), "http://127.0.0.1:8000/v1/audio/transcriptions");
        let cu = AsrClient::new(Provider::Custom, "http://x.local", None, "", "/asr", "");
        assert_eq!(cu.endpoint_custom(), "http://x.local/asr");
    }

    #[test]
    fn dig_text_paths() {
        let v = serde_json::json!({"text":"hi","result":{"text":"yo"},"list":[{"t":"z"}]});
        assert_eq!(dig_text(&v, "text").as_deref(), Some("hi"));
        assert_eq!(dig_text(&v, "result.text").as_deref(), Some("yo"));
        assert_eq!(dig_text(&v, "list.0.t").as_deref(), Some("z"));
        assert_eq!(dig_text(&v, "nope"), None);
    }

    #[test]
    fn base64_roundtrip_shape() {
        assert_eq!(base64_encode(b"a"), "YQ==");
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
    }
}
