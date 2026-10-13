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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub bus: Arc<Bus>,
    pub paused: Arc<AtomicBool>,
    pub pipeline: Mutex<Option<pipeline::Pipeline>>,
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
                        let _ = toggle_pause(app.clone());
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
            cmd_pause_toggle,
            cmd_test_api,
            cmd_save_window,
            cmd_obs_url
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            let mut settings = config::load();

            // CLI 覆盖：--start（自动开始识别）、--source mic|system；或配置里的 auto_start
            let args: Vec<String> = std::env::args().collect();
            let autostart = args.iter().any(|a| a == "--start") || settings.auto_start;
            if let Some(src) = args
                .iter()
                .position(|a| a == "--source")
                .and_then(|i| args.get(i + 1))
            {
                if src == "mic" || src == "system" {
                    settings.source = src.clone();
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

            app.manage(AppState {
                settings: Mutex::new(settings),
                bus,
                paused: Arc::new(AtomicBool::new(false)),
                pipeline: Mutex::new(None),
            });

            if autostart {
                let h = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    if let Err(e) = restart_pipeline(&h) {
                        eprintln!("autostart failed: {e}");
                    }
                });
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("failed to run mmx-live-lyrics");
}

fn toggle_pause(app: AppHandle) -> Result<bool, String> {
    let state = app.state::<AppState>();
    let now = !state.paused.load(Ordering::Relaxed);
    state.paused.store(now, Ordering::Relaxed);
    let running = state.pipeline.lock().unwrap().is_some();
    state.bus.status(
        match (running, now) {
            (false, _) => "idle",
            (true, true) => "paused",
            (true, false) => "listening",
        },
        "",
    );
    Ok(now)
}

fn restart_pipeline(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if let Some(p) = state.pipeline.lock().unwrap().take() {
        p.stop();
    }
    let s = state.settings.lock().unwrap().clone();
    match pipeline::start(state.bus.clone(), state.paused.clone(), s) {
        Ok(p) => {
            *state.pipeline.lock().unwrap() = Some(p);
            Ok(())
        }
        Err(e) => {
            state.bus.status("error", &e);
            Err(e)
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
    v["running"] = serde_json::json!(state.pipeline.lock().unwrap().is_some());
    v["paused"] = serde_json::json!(state.paused.load(Ordering::Relaxed));
    Ok(v)
}

#[tauri::command]
fn cmd_set_settings(
    state: State<AppState>,
    app: AppHandle,
    settings: Settings,
) -> Result<(), String> {
    let restart_needed = {
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
        old.source != incoming.source
            || old.input_device != incoming.input_device
            || old.loopback_device != incoming.loopback_device
            || old.segment_seconds != incoming.segment_seconds
            || old.language != incoming.language
            || old.base_url != incoming.base_url
            || old.api_key != incoming.api_key
    };
    if restart_needed && state.pipeline.lock().unwrap().is_some() {
        restart_pipeline(&app)?;
    }
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
    let state = app.state::<AppState>();
    if state.pipeline.lock().unwrap().is_some() {
        return Ok(());
    }
    restart_pipeline(&app)
}

#[tauri::command]
fn cmd_stop(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    if let Some(p) = state.pipeline.lock().unwrap().take() {
        p.stop();
    }
    state.bus.status("idle", "");
    Ok(())
}

#[tauri::command]
fn cmd_pause_toggle(app: AppHandle) -> Result<bool, String> {
    toggle_pause(app)
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
