//! 配置持久化：%APPDATA%\mmx-live-lyrics\config.json
//!
//! 键名规范：对外 JSON（前端 IPC / OBS 页面 / 配置文件）统一使用 camelCase，
//! 与前端 ui/index.html 的字段名一一对应。旧的 snake_case 键通过 alias 继续可读，
//! 读入后按 camelCase 写回，实现无感迁移。
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
#[serde(rename_all = "camelCase")]
pub struct LookPreset {
    pub name: String,
    #[serde(default, alias = "font_family")]
    pub font_family: Option<String>,
    #[serde(default = "d_font_size", alias = "font_size")]
    pub font_size: f32,
    #[serde(default = "d_font_weight", alias = "font_weight")]
    pub font_weight: u32,
    #[serde(default = "d_color1")]
    pub color1: String,
    #[serde(default = "d_color2")]
    pub color2: String,
    #[serde(default = "d_angle", alias = "grad_angle")]
    pub grad_angle: u32,
    #[serde(default, alias = "outline_color")]
    pub outline_color: String,
    #[serde(default = "d_outline_width", alias = "outline_width")]
    pub outline_width: f32,
    #[serde(default, alias = "glow_color")]
    pub glow_color: String,
    #[serde(default = "d_glow_strength", alias = "glow_strength")]
    pub glow_strength: f32,
    #[serde(default, alias = "bg_color")]
    pub bg_color: String,
    #[serde(default = "d_bg_opacity", alias = "bg_opacity")]
    pub bg_opacity: f32,
    #[serde(default = "d_true", alias = "show_prev")]
    pub show_prev: bool,
    #[serde(default = "d_hist_size", alias = "hist_font_size")]
    pub hist_font_size: f32,
    #[serde(default = "d_hist_color", alias = "hist_color")]
    pub hist_color: String,
    #[serde(default = "d_hist_opacity", alias = "hist_opacity")]
    pub hist_opacity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    /// 双声源独立开关：默认两者同时识别
    #[serde(alias = "mic_enabled")]
    pub mic_enabled: bool,
    #[serde(alias = "system_enabled")]
    pub system_enabled: bool,
    #[serde(alias = "input_device")]
    pub input_device: Option<String>,
    #[serde(alias = "loopback_device")]
    pub loopback_device: Option<String>,
    /// BCP-47 语言提示，空串 = 自动检测
    pub language: String,
    /// API Key 覆盖；None/空 = 复用 ~/.mmx/config.json（mmx CLI 登录态）
    #[serde(alias = "api_key")]
    pub api_key: Option<String>,
    /// 服务商接口地址
    #[serde(alias = "base_url")]
    pub base_url: String,
    /// VAD 段长（秒），达到即强制上传
    #[serde(alias = "segment_seconds")]
    pub segment_seconds: f32,
    /// 服务商：mmx | openai | custom
    pub provider: String,
    /// 模型名（各服务商不同）
    pub model: String,
    /// custom 模式的接口路径（相对 base_url）
    #[serde(alias = "custom_path")]
    pub custom_path: String,
    /// custom 模式下响应里取文本的字段路径（点号分隔，如 result.text）
    #[serde(alias = "custom_text_path")]
    pub custom_text_path: String,
    #[serde(alias = "obs_enabled")]
    pub obs_enabled: bool,
    #[serde(alias = "obs_port")]
    pub obs_port: u16,
    // ---- 外观（全字段自定义，自动保存）----
    #[serde(alias = "font_family")]
    pub font_family: Option<String>,
    #[serde(alias = "font_size")]
    pub font_size: f32,
    #[serde(alias = "font_weight")]
    pub font_weight: u32,
    /// 渐变起点色（与 color2 相同即为纯色）
    pub color1: String,
    pub color2: String,
    #[serde(alias = "grad_angle")]
    pub grad_angle: u32,
    #[serde(alias = "outline_color")]
    pub outline_color: String,
    #[serde(alias = "outline_width")]
    pub outline_width: f32,
    #[serde(alias = "glow_color")]
    pub glow_color: String,
    #[serde(alias = "glow_strength")]
    pub glow_strength: f32,
    #[serde(alias = "bg_color")]
    pub bg_color: String,
    #[serde(alias = "bg_opacity")]
    pub bg_opacity: f32,
    /// 外观默认值版本：升级时按新默认值刷新一次旧配置（见 config::load）
    /// 缺字段 = 0 = "从未迁移过"，必须走一次迁移才能拿到新默认值
    #[serde(default)]
    pub look_version: u32,
    #[serde(default = "d_true", alias = "show_prev")]
    pub show_prev: bool,
    #[serde(alias = "hist_font_size")]
    pub hist_font_size: f32,
    #[serde(alias = "hist_color")]
    pub hist_color: String,
    #[serde(alias = "hist_opacity")]
    pub hist_opacity: f32,
    pub presets: Vec<LookPreset>,
    /// 启动软件后自动开始识别（等效 --start）
    #[serde(alias = "auto_start")]
    pub auto_start: bool,
    pub hotkey: String,
    pub window: Option<WindowRect>,
}
fn d_font_size() -> f32 {
    72.0
}
fn d_font_weight() -> u32 {
    900
}
fn d_color1() -> String {
    "#67e8f9".into()
}
fn d_color2() -> String {
    "#f0abfc".into()
}
fn d_angle() -> u32 {
    100
}
fn d_outline_width() -> f32 {
    0.0
}
fn d_glow_color() -> String {
    "#8b5cf6".into()
}
fn d_glow_strength() -> f32 {
    0.0
}
fn d_bg_color() -> String {
    "#080a12".into()
}
fn d_bg_opacity() -> f32 {
    0.2
}
fn d_true() -> bool {
    true
}
/// 当前外观默认值版本。改动默认外观（背景/描边/光晕）时 +1，
/// 旧配置会在下次启动时被刷新成新默认值。
pub const LOOK_VERSION: u32 = 2;
fn d_look_version() -> u32 {
    LOOK_VERSION
}
fn d_hist_size() -> f32 {
    40.0
}
fn d_hist_color() -> String {
    "#ffffff".into()
}
fn d_hist_opacity() -> f32 {
    0.45
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mic_enabled: true,
            system_enabled: true,
            input_device: None,
            loopback_device: None,
            language: "zh".into(),
            api_key: None,
            base_url: "https://api.minimaxi.com".into(),
            segment_seconds: 2.0,
            provider: "mmx".into(),
            model: "asr-1.0".into(),
            custom_path: "/v1/speech_to_text".into(),
            custom_text_path: "text".into(),
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
            bg_opacity: d_bg_opacity(),
            look_version: d_look_version(),
            show_prev: true,
            hist_font_size: d_hist_size(),
            hist_color: d_hist_color(),
            hist_opacity: d_hist_opacity(),
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
    let raw = std::fs::read_to_string(config_path()).ok();
    let mut s: Settings = raw
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    // 旧格式（单 source 字段 / 全 snake_case）迁移到新键名，立即回写
    if let Some(raw) = &raw {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(raw) {
            let legacy = v.get("mic_enabled").is_none()
                && v.get("micEnabled").is_none()
                && v.get("source").is_none();
            let snake_left = v
                .as_object()
                .map(|o| {
                    o.keys()
                        .any(|k| k.contains('_') && *k != "hotkey")
                })
                .unwrap_or(false);
            if legacy || snake_left {
                let _ = save(&s);
            }
        }
    }
    // 外观默认值迁移：camelCase 修复前，前端的外观改动根本存不进磁盘，
    // 旧配置里的外观值全是旧默认值而非用户的选择，所以按新默认值刷新一次零损失；
    // 刷新后写入版本号，之后用户改的就是自己的，不再被动。
    if s.look_version < LOOK_VERSION {
        // 只覆盖"还等于旧默认值"的字段，用户自己调过的值一律保留
        if s.bg_opacity == 0.0 {
            s.bg_opacity = d_bg_opacity();
        }
        if s.glow_strength == 22.0 {
            s.glow_strength = d_glow_strength();
        }
        s.look_version = LOOK_VERSION;
        let _ = save(&s);
    }
    s
}

pub fn save(s: &Settings) -> Result<(), String> {
    let p = config_path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(&p, json).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 根因回归：前端发 camelCase，Rust 侧必须原样收到（否则声源开关写不进去）
    #[test]
    fn camel_case_from_frontend_is_honoured() {
        let raw = r#"{"micEnabled":false,"systemEnabled":true,"segmentSeconds":3.5,"fontSize":88}"#;
        let s: Settings = serde_json::from_str(raw).unwrap();
        assert!(!s.mic_enabled, "micEnabled=false 必须解析为 mic_enabled=false");
        assert!(s.system_enabled);
        assert_eq!(s.segment_seconds, 3.5);
        assert_eq!(s.font_size, 88.0);
    }

    /// 旧配置文件（snake_case）必须仍能读出来，且读出后能按 camelCase 写回
    #[test]
    fn legacy_snake_case_config_still_loads() {
        let raw = r#"{"mic_enabled":false,"system_enabled":false,"api_key":"sk-x","font_size":64}"#;
        let s: Settings = serde_json::from_str(raw).unwrap();
        assert!(!s.mic_enabled);
        assert_eq!(s.api_key.as_deref(), Some("sk-x"));
        assert_eq!(s.font_size, 64.0);
        let out = serde_json::to_string(&s).unwrap();
        assert!(out.contains("micEnabled"));
        assert!(out.contains("apiKey"));
    }

    /// 预设字段同样要走 camelCase
    #[test]
    fn preset_fields_are_camel_case() {
        let raw = r#"{"presets":[{"name":"p","fontSize":100,"showPrev":false,"histOpacity":0.2}]}"#;
        let s: Settings = serde_json::from_str(raw).unwrap();
        let p = &s.presets[0];
        assert_eq!(p.name, "p");
        assert_eq!(p.font_size, 100.0);
        assert!(!p.show_prev);
        assert_eq!(p.hist_opacity, 0.2);
    }
}
