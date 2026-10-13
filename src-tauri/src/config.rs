//! 配置持久化：%APPDATA%\mmx-live-lyrics\config.json
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowRect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 声源："mic"（麦克风）| "system"（系统声音回录）
    pub source: String,
    pub input_device: Option<String>,
    pub loopback_device: Option<String>,
    /// BCP-47 语言提示，空串 = 自动检测
    pub language: String,
    /// API Key 覆盖；None/空 = 复用 ~/.mmx/config.json（mmx CLI 登录态）
    pub api_key: Option<String>,
    pub base_url: String,
    /// VAD 段长（秒），达到即强制上传
    pub segment_seconds: f32,
    pub obs_enabled: bool,
    pub obs_port: u16,
    pub theme: String,
    pub font_size: f32,
    pub bg_opacity: f32,
    pub hotkey: String,
    pub window: Option<WindowRect>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            source: "mic".into(),
            input_device: None,
            loopback_device: None,
            language: "zh".into(),
            api_key: None,
            base_url: "https://api.minimaxi.com".into(),
            segment_seconds: 2.0,
            obs_enabled: true,
            obs_port: 39871,
            theme: "aurora".into(),
            font_size: 72.0,
            bg_opacity: 0.0,
            hotkey: "Ctrl+Alt+L".into(),
            window: None,
        }
    }
}

pub fn config_path() -> PathBuf {
    let base = std::env::var("APPDATA")
        .ok()
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var("USERPROFILE")
                .ok()
                .map(|h| PathBuf::from(h).join("AppData").join("Roaming"))
        })
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("mmx-live-lyrics").join("config.json")
}

pub fn load() -> Settings {
    std::fs::read_to_string(config_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(s: &Settings) -> Result<(), String> {
    let p = config_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(&p, json).map_err(|e| e.to_string())
}
