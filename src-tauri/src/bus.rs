//! 消息总线：同一条消息同时推给 Tauri 前端（事件）和 OBS 浏览器源（WebSocket）。
use crate::config::Settings;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub struct Bus {
    app: AppHandle,
    ws_clients: Mutex<Vec<std::sync::mpsc::Sender<String>>>,
    seq: AtomicU64,
    /// 最近一次外观快照 / 状态，新连入的 OBS 客户端立即补发，避免错过启动期消息
    last_snapshot: Mutex<Option<String>>,
    last_status: Mutex<String>,
}

impl Bus {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            app,
            ws_clients: Mutex::new(Vec::new()),
            seq: AtomicU64::new(0),
            last_snapshot: Mutex::new(None),
            last_status: Mutex::new(String::new()),
        })
    }

    pub fn add_client(&self, tx: std::sync::mpsc::Sender<String>) {
        if let Some(s) = self.last_snapshot.lock().unwrap().clone() {
            let _ = tx.send(s);
        }
        let st = self.last_status.lock().unwrap().clone();
        if !st.is_empty() {
            let _ = tx.send(st);
        }
        self.ws_clients.lock().unwrap().push(tx);
    }

    pub fn text(&self, t: &str) {
        let n = self.seq.fetch_add(1, Ordering::Relaxed);
        let payload = serde_json::json!({ "type": "text", "text": t, "seq": n });
        let _ = self.app.emit("asr://text", &payload);
        self.broadcast(&payload.to_string());
    }

    /// state: idle | listening | sending | retry | ok | paused | error
    pub fn status(&self, state: &str, detail: &str) {
        let payload = serde_json::json!({ "type": "status", "state": state, "detail": detail });
        let json = payload.to_string();
        *self.last_status.lock().unwrap() = json.clone();
        let _ = self.app.emit("asr://status", &payload);
        self.broadcast(&json);
    }

    /// 外观设置快照（OBS 页面没有 Tauri API，靠这条同步全部外观）
    pub fn settings_snapshot(&self, s: &Settings) {
        let payload = serde_json::json!({
            "type": "settings",
            "fontFamily": s.font_family,
            "fontSize": s.font_size,
            "fontWeight": s.font_weight,
            "color1": s.color1,
            "color2": s.color2,
            "gradAngle": s.grad_angle,
            "outlineColor": s.outline_color,
            "outlineWidth": s.outline_width,
            "glowColor": s.glow_color,
            "glowStrength": s.glow_strength,
            "bgColor": s.bg_color,
            "bgOpacity": s.bg_opacity,
            "showPrev": s.show_prev,
            "histFontSize": s.hist_font_size,
            "histColor": s.hist_color,
            "histOpacity": s.hist_opacity,
        });
        let json = payload.to_string();
        *self.last_snapshot.lock().unwrap() = Some(json.clone());
        let _ = self.app.emit("asr://settings", &payload);
        self.broadcast(&json);
    }

    fn broadcast(&self, json: &str) {
        if let Ok(mut v) = self.ws_clients.lock() {
            v.retain(|c| c.send(json.to_string()).is_ok());
        }
    }
}

/// 双声源状态聚合：麦克风/系统各占一个槽位，合成一个对外状态。
/// 规则：任一源健康 → listening/sending/retry（附错误详情）；全部停摆且有错误 → error；
/// 槽位全空 → idle。这样"error"只会出现在彻底停止时，前端按钮语义不会混乱。
pub struct StatusReport {
    bus: Arc<Bus>,
    slots: Mutex<[Option<(String, String)>; 2]>, // (state, detail)
}

pub const KIND_MIC: usize = 0;
pub const KIND_SYSTEM: usize = 1;

impl StatusReport {
    pub fn new(bus: Arc<Bus>) -> Arc<Self> {
        Arc::new(Self {
            bus,
            slots: Mutex::new([None, None]),
        })
    }

    pub fn report(&self, kind: usize, state: &str, detail: &str) {
        self.slots.lock().unwrap()[kind.min(1)] = Some((state.to_string(), detail.to_string()));
        self.recompute();
    }

    /// 某声源停止后清除其槽位并重算
    pub fn clear(&self, kind: usize) {
        self.slots.lock().unwrap()[kind.min(1)] = None;
        self.recompute();
    }

    fn recompute(&self) {
        let slots = self.slots.lock().unwrap();
        let flat: Vec<&(String, String)> = slots.iter().flatten().collect();
        let (agg, detail) = if flat.is_empty() {
            ("idle".to_string(), String::new())
        } else {
            let healthy = flat.iter().filter(|(st, _)| st != "error" && st != "idle").count();
            let err = flat.iter().find(|(st, _)| st == "error");
            if healthy == 0 && err.is_some() {
                (err.unwrap().0.clone(), err.unwrap().1.clone())
            } else {
                let has_retry = flat.iter().any(|(st, _)| st == "retry");
                let has_send = flat.iter().any(|(st, _)| st == "sending");
                let state = if has_retry {
                    "retry"
                } else if has_send {
                    "sending"
                } else {
                    "listening"
                };
                // 一路出错但另一路还在跑：状态保持运行，附上错误详情
                let d = err
                    .or_else(|| flat.iter().find(|(st, _)| st == "retry"))
                    .map(|(_, d)| d.clone())
                    .unwrap_or_default();
                (state.to_string(), d)
            }
        };
        drop(slots);
        self.bus.status(&agg, &detail);
    }
}
