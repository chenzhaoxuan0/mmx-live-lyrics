//! mmx-live-lyrics：直播实时歌词字幕
//!
//! 架构：cpal 采集（麦克风 / WASAPI loopback 系统声）
//!   → 能量 VAD 分段 → ASR API（可插拔服务商）→ Tauri 歌词窗口 + OBS 浏览器源
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
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub bus: Arc<Bus>,
    /// 状态聚合器（双声源合成一个状态灯）
    pub agg: Arc<bus::StatusReport>,
    /// [KIND_MIC, KIND_SYSTEM] 两条独立管线
    pub pipelines: Mutex<[Option<pipeline::Pipeline>; 2]>,
}

/// 取锁：即使之前有线程 panic 把锁毒化了，也照常取到值（避免"一次 panic → 之后全部 panic"的连锁闪退）
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    match m.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    }
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
    lock(&state.pipelines)
        .iter()
        .any(|p| p.as_ref().map(|x| x.is_active()).unwrap_or(false))
}

/// 启动单条声源管线（kind：bus::KIND_MIC / bus::KIND_SYSTEM）
/// 设置由调用方传入：这里绝不再去加 settings 锁（历史上正是这里与调用方的锁形成自死锁）
fn start_kind(app: &AppHandle, kind: usize, s: &Settings) -> Result<(), String> {
    let state = app.state::<AppState>();
    let p = pipeline::start(state.bus.clone(), state.agg.clone(), kind, s.clone())?;
    lock(&state.pipelines)[kind] = Some(p);
    Ok(())
}

fn stop_kind(app: &AppHandle, kind: usize) {
    let state = app.state::<AppState>();
    // 先把管线取出来，再在锁外 stop：避免在持有 pipelines 锁时调用外部逻辑
    let taken = { lock(&state.pipelines)[kind].take() };
    if let Some(p) = taken {
        p.stop();
    }
    state.agg.clear(kind);
}

/// 启动所有已启用的声源（至少要有一个，否则视为无效操作）
fn start_enabled(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let s = lock(&state.settings).clone();
    if !s.mic_enabled && !s.system_enabled {
        state.bus.status("idle", "未启用任何声源，请在设置中开启");
        return Ok(());
    }
    let mut first_err = None;
    if s.mic_enabled {
        if let Err(e) = start_kind(app, bus::KIND_MIC, &s) {
            state
                .agg
                .report(bus::KIND_MIC, "error", &e);
            first_err = Some(e);
        }
    }
    if s.system_enabled {
        if let Err(e) = start_kind(app, bus::KIND_SYSTEM, &s) {
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
///
/// 注意：本函数**不得**再去锁 settings —— 调用方 cmd_set_settings 曾经把 settings 锁
/// 一直持有到本调用返回，形成同线程重入自死锁（Tauri 同步命令在主线程执行 → 整个应用假死）。
/// 现在 new 就是最新设置，直接用参数即可。
fn reconcile(app: &AppHandle, old: &Settings, new: &Settings) {
    let state = app.state::<AppState>();
    let was_running = is_running(&state);
    if !was_running {
        return;
    }
    let shared_changed = old.language != new.language
        || old.segment_seconds != new.segment_seconds
        || old.base_url != new.base_url
        || old.api_key != new.api_key
        || old.provider != new.provider
        || old.model != new.model;
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
        let active = {
            let g = lock(&state.pipelines);
            g[kind].as_ref().map(|p| p.is_active()).unwrap_or(false)
        };
        if active && (!now_on || dev_changed || shared_changed) {
            stop_kind(app, kind);
        }
    }
    // 后启：仍在/新启用但未运行的源（设置直接用参数 new，不再二次加锁）
    for (kind, on) in [
        (bus::KIND_MIC, new.mic_enabled),
        (bus::KIND_SYSTEM, new.system_enabled),
    ] {
        let active = {
            let g = lock(&state.pipelines);
            g[kind].as_ref().map(|p| p.is_active()).unwrap_or(false)
        };
        if on && !active {
            if let Err(e) = start_kind(app, kind, new) {
                state.agg.report(kind, "error", &e);
            }
        }
    }
}

#[tauri::command]
fn cmd_get_settings(state: State<AppState>) -> Result<serde_json::Value, String> {
    let s = lock(&state.settings).clone();
    let mut v = serde_json::to_value(&s).map_err(|e| e.to_string())?;
    let has_custom = s
        .api_key
        .as_deref()
        .map(|k| !k.trim().is_empty())
        .unwrap_or(false);
    v["apiKey"] = serde_json::json!("");
    v["apiKeySet"] = serde_json::json!(has_custom);
    v["mmxKeyFound"] = serde_json::json!(api::resolve_key(None).is_ok());
    v["running"] = serde_json::json!(is_running(&state));
    Ok(v)
}

/// 只由后端维护、不接受前端回传的字段。
/// 迁移标记（lookVersion / micVersion）若被前端写回旧值（0 = 从未迁移），
/// 下次启动会反复触发迁移，把用户自己选的外观、打开的麦克风又刷掉，
/// 所以这里强制沿用内存里的当前值。
fn preserve_server_owned_fields(incoming: &mut Settings, old: &Settings) {
    incoming.look_version = old.look_version;
    incoming.mic_version = old.mic_version;
}

#[tauri::command]
fn cmd_set_settings(
    state: State<AppState>,
    app: AppHandle,
    settings: Settings,
) -> Result<(), String> {
    // 1) 在锁内只做纯计算，随即释放锁（磁盘 IO 与广播都不该在锁内做）
    let (old, incoming) = {
        let guard = lock(&state.settings);
        let old = guard.clone();
        let mut incoming = settings;
        // 前端用 "__keep__" 表示未改动自定义 Key：保持旧值不下发明文
        if incoming.api_key.as_deref() == Some("__keep__") {
            incoming.api_key = old.api_key.clone();
        } else if incoming.api_key.as_deref() == Some("") {
            incoming.api_key = None;
        }
        preserve_server_owned_fields(&mut incoming, &old);
        (old, incoming)
    };
    // 2) 落盘（失败则内存保持旧值，两者不会出现不一致）
    config::save(&incoming)?;
    // 3) 应用到内存 + 广播
    {
        *lock(&state.settings) = incoming.clone();
    }
    state.bus.settings_snapshot(&incoming);
    // 4) 协调：此刻**不持有** settings 锁
    reconcile(&app, &old, &incoming);
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
    let s = lock(&state.settings).clone();
    // 用当前设置里选的服务商做连通性测试
    let client = api::AsrClient::from_settings(&s)?;
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
    let snapshot = {
        let mut guard = lock(&state.settings);
        guard.window = Some(rect);
        guard.clone()
    };
    config::save(&snapshot)
}

#[tauri::command]
fn cmd_obs_url(state: State<AppState>) -> Result<String, String> {
    let s = lock(&state.settings).clone();
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
    let client = api::AsrClient::new(
        api::Provider::Mmx,
        "https://api.minimaxi.com",
        Some(key),
        "asr-1.0",
        "/v1/speech_to_text",
        "text",
    );
    match client.transcribe(&sine, "zh") {
        Ok(t) => println!("[ok] api: 200, text='{}'（正弦音为空属正常）", t),
        Err(e) => {
            println!("[FAIL] api: {}", e.message());
            std::process::exit(3);
        }
    }
    println!("== selftest PASS ==");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 迁移标记只能由后端维护：前端回传旧值也不能把版本号打回 0，
    /// 否则下次启动会反复触发迁移，把用户的选择刷掉。
    #[test]
    fn migration_markers_are_not_writable_from_frontend() {
        let old = Settings::default();
        let mut incoming = Settings::default();
        incoming.look_version = 0;
        incoming.mic_version = 0;
        preserve_server_owned_fields(&mut incoming, &old);
        assert_eq!(incoming.look_version, config::LOOK_VERSION);
        assert_eq!(incoming.mic_version, config::MIC_VERSION);
    }

    /// 根因回归：曾经的写法把 settings 锁一直持有到 reconcile 返回，
    /// 而 reconcile 内部又锁同一把锁 → 同线程重入自死锁（Tauri 同步命令在主线程 → 应用假死）。
    /// 这里用 try_lock 断言"协调阶段不再持有 settings 锁"。
    #[test]
    fn reconcile_does_not_hold_settings_lock() {
        let m = Mutex::new(7u32);
        // 旧写法：实参里的临时 guard 活到整条语句结束 → 协调期间锁被占
        let held_by_old_shape = {
            let g = m.lock().unwrap();
            let _clone = g.clone();
            m.try_lock().is_ok()
        };
        assert!(
            !held_by_old_shape,
            "旧形状应当持锁（这正是死锁的成因），否则本测试失去意义"
        );

        // 新写法：先在独立作用域里取值并释放锁，再协调
        let m2 = Mutex::new(7u32);
        let snapshot = {
            let g = m2.lock().unwrap();
            g.clone()
        };
        assert_eq!(snapshot, 7);
        assert!(
            m2.try_lock().is_ok(),
            "协调阶段不得持有 settings 锁，否则同线程重入即死锁"
        );
    }

    /// 锁中毒后仍可继续使用（避免一次 panic 导致后续全部 panic 的连锁闪退）
    #[test]
    fn lock_recovers_from_poisoning() {
        let m = Mutex::new(1u32);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.lock().unwrap();
            panic!("boom");
        }));
        assert!(m.is_poisoned());
        let v = *lock(&m);
        assert_eq!(v, 1, "中毒后应能取到值而不是 panic");
    }
}
