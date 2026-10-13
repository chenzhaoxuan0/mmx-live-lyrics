//! 消息总线：同一条消息同时推给 Tauri 前端（事件）和 OBS 浏览器源（WebSocket）。
use crate::config::Settings;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub struct Bus {
    app: AppHandle,
    ws_clients: Mutex<Vec<std::sync::mpsc::Sender<String>>>,
    seq: AtomicU64,
}

impl Bus {
    pub fn new(app: AppHandle) -> Arc<Self> {
        Arc::new(Self {
            app,
            ws_clients: Mutex::new(Vec::new()),
            seq: AtomicU64::new(0),
        })
    }

    pub fn add_client(&self, tx: std::sync::mpsc::Sender<String>) {
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
        let _ = self.app.emit("asr://status", &payload);
        self.broadcast(&payload.to_string());
    }

    /// 外观设置快照（OBS 页面没有 Tauri API，靠这条同步主题/字号）
    pub fn settings_snapshot(&self, s: &Settings) {
        let payload = serde_json::json!({
            "type": "settings",
            "theme": s.theme,
            "fontSize": s.font_size,
            "bgOpacity": s.bg_opacity,
        });
        let _ = self.app.emit("asr://settings", &payload);
        self.broadcast(&payload.to_string());
    }

    fn broadcast(&self, json: &str) {
        if let Ok(mut v) = self.ws_clients.lock() {
            v.retain(|c| c.send(json.to_string()).is_ok());
        }
    }
}
