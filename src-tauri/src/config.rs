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
    /// 麦克风开关：默认关闭（默认开着容易把环境音也识别进去）
    #[serde(alias = "mic_enabled")]
    pub mic_enabled: bool,
    /// 麦克风默认值版本：升级时按新默认值刷新一次旧配置（见 config::load）
    /// 缺字段 = 0 = "从未迁移过"，必须走一次迁移才能拿到新默认值
    #[serde(default)]
    pub mic_version: u32,
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
    /// 当前选中的外观预设：builtin:<id> / user:<序号> / custom（无预设）
    /// 空串 = 从未记录（迁移时按当前外观反推，见 config::load）
    #[serde(default, alias = "look_preset")]
    pub look_preset: String,
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
    "#ffffff".into()
}
fn d_color2() -> String {
    "#ffffff".into()
}
fn d_angle() -> u32 {
    100
}
fn d_outline_width() -> f32 {
    0.0
}
fn d_glow_color() -> String {
    "#000000".into()
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
pub const LOOK_VERSION: u32 = 3;
fn d_look_version() -> u32 {
    LOOK_VERSION
}
/// 麦克风开关默认值版本。改动麦克风默认值时 +1，
/// 旧配置会在下次启动时被刷新成新默认值（只刷一次）。
pub const MIC_VERSION: u32 = 1;
fn d_mic_version() -> u32 {
    MIC_VERSION
}
fn d_hist_size() -> f32 {
    40.0
}
fn d_hist_color() -> String {
    "#ffffff".into()
}
fn d_hist_opacity() -> f32 {
    0.55
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mic_enabled: false,
            mic_version: d_mic_version(),
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
            look_preset: "builtin:mono".into(),
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

/// 外观是否等于内置的「纯白黑描边」预设
fn looks_like_mono(s: &Settings) -> bool {
    s.font_family.is_none()
        && s.font_size == 72.0
        && s.font_weight == 900
        && s.color1 == "#ffffff"
        && s.color2 == "#ffffff"
        && s.grad_angle == 100
        && s.outline_color == "#000000"
        && s.outline_width == 0.0
        && s.glow_color == "#000000"
        && s.glow_strength == 0.0
        && s.bg_color == "#080a12"
        && s.bg_opacity == 0.2
        && s.show_prev
        && s.hist_font_size == 40.0
        && s.hist_color == "#ffffff"
        && s.hist_opacity == 0.55
}

/// 外观是否整组还等于旧默认值（极光配色）：整组未改过才允许整体刷新
fn looks_like_old_default(s: &Settings) -> bool {
    s.font_family.is_none()
        && s.font_size == 72.0
        && s.font_weight == 900
        && s.color1 == "#67e8f9"
        && s.color2 == "#f0abfc"
        && s.grad_angle == 100
        && s.outline_color == "#000000"
        && s.outline_width == 0.0
        && s.glow_color == "#8b5cf6"
        && s.glow_strength == 0.0
        && s.bg_color == "#080a12"
        && s.bg_opacity == 0.2
        && s.show_prev
        && s.hist_font_size == 40.0
        && s.hist_color == "#ffffff"
        && s.hist_opacity == 0.45
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
        migrate_look(&mut s);
        let _ = save(&s);
    }
    // 麦克风默认值迁移：旧默认是「麦克风 + 电脑音频双开」，升级后麦克风默认关闭。
    // 只在缺版本号时刷一次，刷完写入版本号，之后用户自己开的就是自己的，不再被动。
    if s.mic_version < MIC_VERSION {
        migrate_mic(&mut s);
        let _ = save(&s);
    }
    s
}

/// 外观默认值迁移：旧配置按新默认值刷新一次。
///
/// 原则：只动"还等于旧默认值"的字段，用户自己调过的值一律保留。
/// 迁移后必定带上 look_preset（当前生效的预设），前端据此回显下拉框。
fn migrate_look(s: &mut Settings) {
    if s.look_version >= LOOK_VERSION {
        return;
    }
    // 老版本内置预设留下的描边/光晕值：3.5/10（纯白黑描边）、22/28（极光等）一律归零
    if s.outline_width == 3.5 {
        s.outline_width = d_outline_width();
    }
    if s.glow_strength == 22.0 || s.glow_strength == 28.0 || s.glow_strength == 10.0 {
        s.glow_strength = d_glow_strength();
    }
    // v1 的旧默认是背景全透明；v2 起 0 是合法的手动值，不再覆盖
    if s.look_version < 2 && s.bg_opacity == 0.0 {
        s.bg_opacity = d_bg_opacity();
    }
    // 还没有预设记录（老配置）→ 按当前外观反推
    if s.look_preset.is_empty() {
        if looks_like_mono(s) {
            s.look_preset = "builtin:mono".into();
        } else if looks_like_old_default(s) {
            // 整组还是旧默认（极光配色）→ 刷成新默认（纯白黑描边）
            s.color1 = d_color1();
            s.color2 = d_color2();
            s.glow_color = d_glow_color();
            s.hist_opacity = d_hist_opacity();
            s.look_preset = "builtin:mono".into();
        } else {
            s.look_preset = "custom".into();
        }
    }
    s.look_version = LOOK_VERSION;
}

/// 麦克风默认值迁移：旧配置里「麦克风开启」只是旧默认值留下的，
/// 升级后统一刷成关闭；用户之后自己打开的不会再被刷掉。
fn migrate_mic(s: &mut Settings) {
    if s.mic_version >= MIC_VERSION {
        return;
    }
    s.mic_enabled = false;
    s.mic_version = MIC_VERSION;
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

    /// 默认外观 = 内置「纯白黑描边」：描边宽度与光晕强度都是 0px
    #[test]
    fn default_look_is_mono_with_zero_outline_and_glow() {
        let s = Settings::default();
        assert_eq!(s.outline_width, 0.0, "默认描边宽度必须是 0px");
        assert_eq!(s.glow_strength, 0.0, "默认光晕强度必须是 0px");
        assert_eq!(s.color1, "#ffffff");
        assert_eq!(s.color2, "#ffffff");
        assert_eq!(s.glow_color, "#000000");
        assert_eq!(s.look_preset, "builtin:mono");
        assert!(looks_like_mono(&s), "默认外观必须能被识别为内置 mono 预设");
    }

    /// 旧配置（缺 lookPreset、仍是极光配色）→ 迁移成新的默认外观并记上 mono 预设
    #[test]
    fn old_aurora_defaults_migrate_to_mono() {
        let mut s = Settings::default();
        s.look_version = 0;
        s.look_preset = String::new();
        s.color1 = "#67e8f9".into();
        s.color2 = "#f0abfc".into();
        s.glow_color = "#8b5cf6".into();
        s.hist_opacity = 0.45;
        assert!(looks_like_old_default(&s));
        // 模拟迁移里对"还等于旧默认值"的字段刷新
        s.color1 = d_color1();
        s.color2 = d_color2();
        s.glow_color = d_glow_color();
        s.hist_opacity = d_hist_opacity();
        assert!(looks_like_mono(&s));
    }

    /// 老版本内置预设留下的描边/光晕值（3.5 / 10 / 22 / 28）一律归零
    #[test]
    fn legacy_outline_and_glow_values_are_zeroed() {
        for legacy in [3.5f32, 0.0] {
            let mut s = Settings::default();
            s.look_version = 1;
            s.outline_width = legacy;
            migrate_look(&mut s);
            assert_eq!(s.outline_width, 0.0, "描边宽度 {legacy} 必须归零");
        }
        for legacy in [10.0f32, 22.0, 28.0, 0.0] {
            let mut s = Settings::default();
            s.look_version = 1;
            s.glow_strength = legacy;
            migrate_look(&mut s);
            assert_eq!(s.glow_strength, 0.0, "光晕强度 {legacy} 必须归零");
        }
    }

    /// 旧配置（极光配色、未记录预设）→ 迁移成 mono 并带上 lookPreset
    #[test]
    fn legacy_config_migrates_to_mono_preset() {
        let mut s = Settings::default();
        s.look_version = 1;
        s.look_preset = String::new();
        s.color1 = "#67e8f9".into();
        s.color2 = "#f0abfc".into();
        s.glow_color = "#8b5cf6".into();
        s.hist_opacity = 0.45;
        migrate_look(&mut s);
        assert_eq!(s.look_preset, "builtin:mono");
        assert_eq!(s.color1, "#ffffff");
        assert_eq!(s.color2, "#ffffff");
        assert_eq!(s.glow_color, "#000000");
        assert_eq!(s.outline_width, 0.0);
        assert_eq!(s.glow_strength, 0.0);
        assert_eq!(s.look_version, LOOK_VERSION);
    }

    /// 用户自己调过的外观必须原样保留，只补上 lookPreset 标记
    #[test]
    fn user_customised_look_is_preserved_by_migration() {
        let mut s = Settings::default();
        s.look_version = 1;
        s.look_preset = String::new();
        s.font_size = 100.0;
        s.outline_width = 4.0;
        s.glow_strength = 18.0;
        s.color1 = "#123456".into();
        migrate_look(&mut s);
        assert_eq!(s.font_size, 100.0);
        assert_eq!(s.outline_width, 4.0);
        assert_eq!(s.glow_strength, 18.0);
        assert_eq!(s.color1, "#123456");
        assert_eq!(s.look_preset, "custom");
    }

    /// 用户机器上的真实旧配置（lookVersion=2 + 纯白黑描边外观）→ 迁移后仍是 mono
    #[test]
    fn real_world_v2_config_migrates_to_mono_preset() {
        let raw = r##"{
            "color1": "#ffffff", "color2": "#ffffff", "gradAngle": 100,
            "outlineColor": "#000000", "outlineWidth": 0.0,
            "glowColor": "#000000", "glowStrength": 0.0,
            "bgColor": "#080a12", "bgOpacity": 0.2, "lookVersion": 2,
            "showPrev": true, "histFontSize": 40.0, "histColor": "#ffffff",
            "histOpacity": 0.55, "fontSize": 72.0, "fontWeight": 900
        }"##;
        let mut s: Settings = serde_json::from_str(raw).unwrap();
        assert!(s.look_preset.is_empty(), "老配置没有 lookPreset 记录");
        migrate_look(&mut s);
        assert_eq!(s.look_preset, "builtin:mono");
        assert_eq!(s.outline_width, 0.0);
        assert_eq!(s.glow_strength, 0.0);
        assert_eq!(s.look_version, LOOK_VERSION);
    }

    /// 已经是最新版本的用户配置不该被迁移动到
    #[test]
    fn current_version_config_is_not_migrated() {
        let mut s = Settings::default();
        s.outline_width = 5.0;
        s.glow_strength = 20.0;
        let before = s.clone();
        migrate_look(&mut s);
        assert_eq!(s.outline_width, before.outline_width);
        assert_eq!(s.glow_strength, before.glow_strength);
    }

    /// lookPreset 要能读写（camelCase），旧配置缺它时按空串处理
    #[test]
    fn look_preset_round_trips() {
        let raw = r#"{"lookPreset":"builtin:mono"}"#;
        let s: Settings = serde_json::from_str(raw).unwrap();
        assert_eq!(s.look_preset, "builtin:mono");
        let out = serde_json::to_string(&s).unwrap();
        assert!(out.contains("lookPreset"));
        // 缺字段 = 从未记录
        let s2: Settings = serde_json::from_str("{}").unwrap();
        assert!(s2.look_preset.is_empty());
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

    /// 麦克风默认关闭，电脑音频默认保持开启
    #[test]
    fn mic_is_off_by_default() {
        let s = Settings::default();
        assert!(!s.mic_enabled, "麦克风识别默认必须是关闭");
        assert!(s.system_enabled, "电脑音频默认保持开启");
        assert_eq!(s.mic_version, MIC_VERSION);
    }

    /// 旧配置（没有 micVersion）升级后麦克风刷成关闭
    #[test]
    fn legacy_config_mic_is_flipped_to_off() {
        let raw = r#"{"micEnabled":true,"systemEnabled":true,"lookVersion":3}"#;
        let mut s: Settings = serde_json::from_str(raw).unwrap();
        assert_eq!(s.mic_version, 0, "老配置没有 micVersion 记录");
        assert!(s.mic_enabled);
        migrate_mic(&mut s);
        assert!(!s.mic_enabled, "迁移后麦克风必须关闭");
        assert_eq!(s.mic_version, MIC_VERSION);
    }

    /// 迁移只跑一次：用户之后自己打开的麦克风不会再被刷掉
    #[test]
    fn mic_choice_is_kept_after_migration() {
        let mut s = Settings::default();
        s.mic_enabled = true;
        migrate_mic(&mut s);
        assert!(s.mic_enabled, "迁移过一次后，用户自己开的麦克风必须保留");
    }

    /// 端到端：旧配置文件启动一次，麦克风应被刷成关闭并落盘；
    /// 用户之后自己打开的麦克风不会在下次启动时被动掉。
    #[test]
    fn load_migrates_legacy_mic_to_off_and_keeps_user_choice() {
        let dir = std::env::temp_dir().join(format!("mmx-mic-e2e-{}", std::process::id()));
        let cfg_dir = dir.join("mmx-live-lyrics");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        let cfg = cfg_dir.join("config.json");
        let old_appdata = std::env::var("APPDATA").ok();
        std::env::set_var("APPDATA", &dir);

        // 旧配置：麦克风开着（那是旧默认值留下的），且没有 micVersion。
        // 设 MMX_TEST_SEED_CONFIG 可换成一个真实配置文件来复跑同一条路径。
        let seed_body = match std::env::var("MMX_TEST_SEED_CONFIG") {
            Ok(p) => std::fs::read_to_string(&p).unwrap(),
            Err(_) => {
                r#"{"micEnabled":true,"systemEnabled":true,"lookVersion":3,"lookPreset":"builtin:mono"}"#
                    .to_string()
            }
        };
        std::fs::write(&cfg, seed_body).unwrap();

        let s = load();
        assert!(!s.mic_enabled, "旧配置启动后麦克风必须关闭");
        assert_eq!(s.mic_version, MIC_VERSION);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&cfg).unwrap()).unwrap();
        assert_eq!(saved["micEnabled"], serde_json::json!(false), "落盘必须写成关闭");
        assert_eq!(saved["micVersion"], serde_json::json!(MIC_VERSION));
        if std::env::var("MMX_TEST_SEED_CONFIG").is_ok() {
            println!(
                "--- 迁移后落盘 ---\n{}",
                std::fs::read_to_string(&cfg).unwrap()
            );
        }

        // 再启动一次：状态稳定，不会来回跳
        assert!(!load().mic_enabled, "第二次启动仍应是关闭");

        // 用户自己把麦克风打开并保存 → 下次启动保留
        let mut on = s.clone();
        on.mic_enabled = true;
        save(&on).unwrap();
        assert!(
            load().mic_enabled,
            "用户自己打开的麦克风必须保留，不能再被刷掉"
        );

        if let Some(v) = old_appdata {
            std::env::set_var("APPDATA", v);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// micVersion 要能读写（camelCase），旧配置缺它时按 0 处理
    #[test]
    fn mic_version_round_trips() {
        let raw = r#"{"micVersion":1}"#;
        let s: Settings = serde_json::from_str(raw).unwrap();
        assert_eq!(s.mic_version, 1);
        let out = serde_json::to_string(&s).unwrap();
        assert!(out.contains("micVersion"));
        let s2: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s2.mic_version, 0, "缺字段 = 从未记录");
    }
}
