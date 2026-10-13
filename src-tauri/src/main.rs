//! mmx-live-lyrics：直播实时歌词字幕
//!
//! 架构：cpal 采集（麦克风 / WASAPI loopback 系统声）
//!   → 能量 VAD 分段 → mmx speech_to_text API → Tauri 歌词窗口 + OBS 浏览器源
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod bus;
mod capture;
mod config;
mod dsp;
mod obs_server;
mod pipeline;
mod vad;
mod wav;

use crate::bus::Bus;
use crate::config::{Settings, WindowRect};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub bus: Arc<Bus>,
    /// 状态聚合器（双声源合成一个状态灯）
    pub agg: Arc<bus::StatusReport>,
    /// [KIND_MIC, KIND_SYSTEM] 两条独立管线
    pub pipelines: Mutex<[Option<pipeline::Pipeline>; 2]>,
}

fn main() {
    if std::env::args().any(|a| a == "--selftest") {
        selftest();
        return;
    }

    tauri::Builder::default()
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        let _ = toggle_run(app.clone());
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            cmd_get_settings,
            cmd_set_settings,
            cmd_list_devices,
            cmd_start,
            cmd_stop,
            cmd_run_toggle,
            cmd_test_api,
            cmd_save_window,
            cmd_obs_url
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let mut settings = config::load();

            // CLI 覆盖：--start（自动开始识别）、--source mic|system|both；或配置里的 auto_start
            let args: Vec<String> = std::env::args().collect();
            let autostart = args.iter().any(|a| a == "--start") || settings.auto_start;
            if let Some(src) = args
                .iter()
                .position(|a| a == "--source")
                .and_then(|i| args.get(i + 1))
            {
                match src.as_str() {
                    "mic" => {
                        settings.mic_enabled = true;
                        settings.system_enabled = false;
                    }
                    "system" => {
                        settings.mic_enabled = false;
                        settings.system_enabled = true;
                    }
                    "both" => {
                        settings.mic_enabled = true;
                        settings.system_enabled = true;
                    }
                    _ => {}
                }
            }

            // 恢复窗口位置和尺寸
            if let Some(r) = settings.window.as_ref() {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.set_position(tauri::PhysicalPosition::new(r.x, r.y));
                    let _ = win.set_size(tauri::PhysicalSize::new(r.width, r.height));
                }
            }

            let bus = Bus::new(handle);

            // OBS 浏览器源服务（与桌面窗口共用同一条总线）
            if settings.obs_enabled {
                obs_server::serve(settings.obs_port, bus.clone());
            }

            // 全局暂停热键
            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            if let Ok(sc) = settings
                .hotkey
                .parse::<tauri_plugin_global_shortcut::Shortcut>()
            {
                let _ = app.global_shortcut().register(sc);
            }

            bus.settings_snapshot(&settings);
            bus.status("idle", "");

            let agg = bus::StatusReport::new(bus.clone());
            app.manage(AppState {
                settings: Mutex::new(settings),
                bus,
                agg,
                pipelines: Mutex::new([None, None]),
            });

            if autostart {
                let h = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    if let Err(e) = start_enabled(&h) {
                        eprintln!("autostart failed: {e}");
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run mmx-live-lyrics");
}

/// 唯一的运行开关：运行中 → 停止全部；空闲（或已因致命错误停摆）→ 启动所有已启用声源。立即返回，绝不阻塞。
fn toggle_run(app: AppHandle) -> Result<bool, String> {
    if is_running(&app.state::<AppState>()) {
        stop_all(&app);
        Ok(false)
    } else {
        start_enabled(&app)?;
        Ok(true)
    }
}

fn is_running(state: &AppState) -> bool {
    state
        .pipelines
        .lock()
        .unwrap()
        .iter()
        .any(|p| p.as_ref().map(|x| x.is_active()).unwrap_or(false))
}

/// 启动单条声源管线（kind：bus::KIND_MIC / bus::KIND_SYSTEM）
fn start_kind(app: &AppHandle, kind: usize) -> Result<(), String> {
    let state = app.state::<AppState>();
    let s = state.settings.lock().unwrap().clone();
    match pipeline::start(state.bus.clone(), state.agg.clone(), kind, s) {
        Ok(p) => {
            state.pipelines.lock().unwrap()[kind] = Some(p);
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn stop_kind(app: &AppHandle, kind: usize) {
    let state = app.state::<AppState>();
    if let Some(p) = state.pipelines.lock().unwrap()[kind].take() {
        p.stop();
    }
    state.agg.clear(kind);
}

/// 启动所有已启用的声源（至少要有一个，否则视为无效操作）
fn start_enabled(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let s = state.settings.lock().unwrap().clone();
    if !s.mic_enabled && !s.system_enabled {
        state.bus.status("idle", "未启用任何声源，请在设置中开启");
        return Ok(());
    }
    let mut first_err = None;
    if s.mic_enabled {
        if let Err(e) = start_kind(app, bus::KIND_MIC) {
            state
                .agg
                .report(bus::KIND_MIC, "error", &e);
            first_err = Some(e);
        }
    }
    if s.system_enabled {
        if let Err(e) = start_kind(app, bus::KIND_SYSTEM) {
            state
                .agg
                .report(bus::KIND_SYSTEM, "error", &e);
            if first_err.is_none() {
                first_err = Some(e);
            }
        }
    }
    match first_err {
        // 两路全败才算启动失败（此时聚合状态已是 error）
        Some(e) if !is_running(&app.state::<AppState>()) => Err(e),
        _ => Ok(()),
    }
}

fn stop_all(app: &AppHandle) {
    stop_kind(app, bus::KIND_MIC);
    stop_kind(app, bus::KIND_SYSTEM);
}

/// 设置变化后的按源协调：停掉被关闭/设备变更的源，补启新开启的源（仅在整体运行中）
fn reconcile(app: &AppHandle, old: &Settings, new: &Settings) {
    let was_running = is_running(&app.state::<AppState>());
    if !was_running {
        return;
    }
    let shared_changed = old.language != new.language
        || old.segment_seconds != new.segment_seconds
        || old.base_url != new.base_url
        || old.api_key != new.api_key;
    // 先停：被关闭的源 / 设备或共享参数变化的源
    for (kind, (_was_on, now_on), dev_changed) in [
        (
            bus::KIND_MIC,
            (old.mic_enabled, new.mic_enabled),
            old.input_device != new.input_device,
        ),
        (
            bus::KIND_SYSTEM,
            (old.system_enabled, new.system_enabled),
            old.loopback_device != new.loopback_device,
        ),
    ] {
        let active = app
            .state::<AppState>()
            .pipelines
            .lock()
            .unwrap()[kind]
            .as_ref()
            .map(|p| p.is_active())
            .unwrap_or(false);
        if active && (!now_on || dev_changed || shared_changed) {
            stop_kind(app, kind);
        }
    }
    // 后启：仍在/新启用但未运行的源
    let s = app.state::<AppState>().settings.lock().unwrap().clone();
    for (kind, on) in [(bus::KIND_MIC, s.mic_enabled), (bus::KIND_SYSTEM, s.system_enabled)] {
        let active = app
            .state::<AppState>()
            .pipelines
            .lock()
            .unwrap()[kind]
            .as_ref()
            .map(|p| p.is_active())
            .unwrap_or(false);
        if on && !active {
            let _ = start_kind(app, kind);
        }
    }
}

#[tauri::command]
fn cmd_get_settings(state: State<AppState>) -> Result<serde_json::Value, String> {
    let s = state.settings.lock().unwrap().clone();
    let mut v = serde_json::to_value(&s).map_err(|e| e.to_string())?;
    let has_custom = s
        .api_key
        .as_deref()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);
    v["api_key"] = serde_json::json!("");
    v["api_key_set"] = serde_json::json!(has_custom);
    v["mmx_key_found"] = serde_json::json!(api::resolve_key(None).is_ok());
    v["running"] = serde_json::json!(is_running(&state));
    Ok(v)
}

#[tauri::command]
fn cmd_set_settings(
    state: State<AppState>,
    app: AppHandle,
    settings: Settings,
) -> Result<(), String> {
    let old = {
        let mut guard = state.settings.lock().unwrap();
        let old = guard.clone();
        // 前端用 "__keep__" 表示未改动自定义 Key：保持旧值不下发明文
        let mut incoming = settings;
        if incoming.api_key.as_deref() == Some("__keep__") {
            incoming.api_key = old.api_key.clone();
        } else if incoming.api_key.as_deref() == Some("") {
            incoming.api_key = None;
        }
        *guard = incoming.clone();
        config::save(&incoming)?;
        state.bus.settings_snapshot(&incoming);
        old
    };
    reconcile(&app, &old, &app.state::<AppState>().settings.lock().unwrap().clone());
    Ok(())
}

#[tauri::command]
fn cmd_list_devices() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "inputs": capture::list_input_devices(),
        "loopbacks": capture::list_loopback_devices(),
    }))
}

#[tauri::command]
fn cmd_start(app: AppHandle) -> Result<(), String> {
    if is_running(&app.state::<AppState>()) {
        return Ok(());
    }
    start_enabled(&app)
}

#[tauri::command]
fn cmd_stop(app: AppHandle) -> Result<(), String> {
    stop_all(&app);
    Ok(())
}

#[tauri::command]
fn cmd_run_toggle(app: AppHandle) -> Result<bool, String> {
    toggle_run(app)
}

#[tauri::command]
fn cmd_test_api(state: State<AppState>) -> Result<String, String> {
    let s = state.settings.lock().unwrap().clone();
    let key = api::resolve_key(s.api_key.as_deref())?;
    let client = api::MmxAsr::new(&s.base_url, key);
    // 0.4 秒 440Hz 正弦：验证链路（预期返回空文本或时长）
    let sine: Vec<f32> = (0..6400)
        .map(|i| (i as f32 / 16000.0 * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.3)
        .collect();
    match client.transcribe(&sine, "") {
        Ok(_) => Ok("连接成功，API Key 有效".into()),
        Err(e) => Err(e.message()),
    }
}

#[tauri::command]
fn cmd_save_window(state: State<AppState>, rect: WindowRect) -> Result<(), String> {
    let mut guard = state.settings.lock().unwrap();
    guard.window = Some(rect);
    config::save(&guard)
}

#[tauri::command]
fn cmd_obs_url(state: State<AppState>) -> Result<String, String> {
    let s = state.settings.lock().unwrap();
    if s.obs_enabled {
        Ok(format!("http://127.0.0.1:{}/", s.obs_port))
    } else {
        Ok(String::new())
    }
}

/// 端到端自检：wav 编码 → 重采样 → VAD 静音丢弃 → API 连通（debug 构建下可见输出）
fn selftest() {
    println!("== mmx-live-lyrics selftest ==");

    let sine: Vec<f32> = (0..16000)
        .map(|i| (i as f32 / 16000.0 * 440.0 * 2.0 * std::f32::consts::PI).sin() * 0.3)
        .collect();
    let wav = wav::encode(&sine);
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(&wav[8..12], b"WAVE");
    println!("[ok] wav encode: {} bytes for 1s sine", wav.len());

    let mut r = dsp::Resampler::new(48000, 16000);
    let input: Vec<f32> = (0..48000).map(|i| (i as f32 * 0.002).sin()).collect();
    let mut out = Vec::new();
    r.process(&input, &mut out);
    println!("[ok] resample 48k→16k: {} → {} samples", input.len(), out.len());
    assert_eq!(dsp::mix_to_mono(&vec![0.5f32; 8], 2).len(), 4);

    let mut seg = vad::Segmenter::new(2.0);
    // 纯静音 3 秒：不应出段（配额保护）
    let silence = vec![0.001f32; 48000];
    let mut got = false;
    for chunk in silence.chunks(1600) {
        if seg.feed(chunk).is_some() {
            got = true;
        }
    }
    assert!(!got, "纯静音不应出段（省配额）");
    println!("[ok] vad: 3s 纯静音未出段");

    let key = match api::resolve_key(None) {
        Ok(k) => {
            println!("[ok] mmx key: {}...", &k[..k.len().min(10)]);
            k
        }
        Err(e) => {
            println!("[FAIL] key: {e}");
            std::process::exit(2);
        }
    };
    let client = api::MmxAsr::new("https://api.minimaxi.com", key);
    match client.transcribe(&sine, "zh") {
        Ok(t) => println!("[ok] api: 200, text='{}'（正弦音为空属正常）", t),
        Err(e) => {
            println!("[FAIL] api: {}", e.message());
            std::process::exit(3);
        }
    }
    println!("== selftest PASS ==");
}
