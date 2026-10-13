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

/// 一套外观预设（内置主题的字段由前端硬编码，用户预设存这里）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookPreset {
    pub name: String,
    #[serde(default)]
    pub font_family: Option<String>,
    #[serde(default = "d_font_size")]
    pub font_size: f32,
    #[serde(default = "d_font_weight")]
    pub font_weight: u32,
    #[serde(default = "d_color1")]
    pub color1: String,
    #[serde(default = "d_color2")]
    pub color2: String,
    #[serde(default = "d_angle")]
    pub grad_angle: u32,
    #[serde(default)]
    pub outline_color: String,
    #[serde(default)]
    pub outline_width: f32,
    #[serde(default = "d_glow_color")]
    pub glow_color: String,
    #[serde(default = "d_glow_strength")]
    pub glow_strength: f32,
    #[serde(default = "d_bg_color")]
    pub bg_color: String,
    #[serde(default)]
    pub bg_opacity: f32,
    #[serde(default = "d_true")]
    pub show_prev: bool,
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
    // ---- 外观（全字段自定义，自动保存）----
    pub font_family: Option<String>,
    pub font_size: f32,
    pub font_weight: u32,
    /// 渐变起点色（与 color2 相同即为纯色）
    pub color1: String,
    pub color2: String,
    pub grad_angle: u32,
    pub outline_color: String,
    pub outline_width: f32,
    pub glow_color: String,
    pub glow_strength: f32,
    pub bg_color: String,
    pub bg_opacity: f32,
    pub show_prev: bool,
    pub presets: Vec<LookPreset>,
    /// 启动软件后自动开始识别（等效 --start）
    pub auto_start: bool,
    pub hotkey: String,
    pub window: Option<WindowRect>,
}

fn d_font_size() -> f32 { 72.0 }
fn d_font_weight() -> u32 { 900 }
fn d_color1() -> String { "#67e8f9".into() }
fn d_color2() -> String { "#f0abfc".into() }
fn d_angle() -> u32 { 100 }
fn d_glow_color() -> String { "#8b5cf6".into() }
fn d_glow_strength() -> f32 { 22.0 }
fn d_bg_color() -> String { "#080a12".into() }
fn d_true() -> bool { true }

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
            font_family: None,
            font_size: d_font_size(),
            font_weight: d_font_weight(),
            color1: d_color1(),
            color2: d_color2(),
            grad_angle: d_angle(),
            outline_color: "#000000".into(),
            outline_width: 0.0,
            glow_color: d_glow_color(),
            glow_strength: d_glow_strength(),
            bg_color: d_bg_color(),
            bg_opacity: 0.0,
            show_prev: true,
            presets: Vec::new(),
            auto_start: false,
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
