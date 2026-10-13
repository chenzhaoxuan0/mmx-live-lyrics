//! 管线：采集线程 →（块通道）→ 引擎线程（混音/重采样/VAD）→（段通道）→ 工作线程（HTTP 上传）。
//!
//! 线程模型与 sub-title 一致：单向通道解耦，停止用 AtomicBool 哨兵。
//! 暂停期间采集不停（秒级恢复），引擎线程丢弃音频。
use crate::api::{resolve_key, MmxAsr};
use crate::bus::Bus;
use crate::capture::{capture_loop, AudioBlock};
use crate::config::Settings;
use crate::dsp::{mix_to_mono, LowPass, Resampler};
use crate::vad::Segmenter;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

pub struct Pipeline {
    stop: Arc<AtomicBool>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Pipeline {
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

pub fn start(bus: Arc<Bus>, paused: Arc<AtomicBool>, settings: Settings) -> Result<Pipeline, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let (btx, brx) = mpsc::channel::<AudioBlock>();
    let (stx, srx) = mpsc::channel::<Vec<f32>>();

    // 1) 采集线程
    let stop_c = stop.clone();
    let source = settings.source.clone();
    let device = if source == "system" {
        settings.loopback_device.clone()
    } else {
        settings.input_device.clone()
    };
    let bus_c = bus.clone();
    let capture = std::thread::spawn(move || {
        if let Err(e) = capture_loop(&source, device.as_deref(), btx, stop_c) {
            bus_c.status("error", &e);
        }
    });

    // 2) 引擎线程：混音 → 低通 → 重采样到 16k → VAD 分段
    let stop_e = stop.clone();
    let paused_e = paused.clone();
    let seg_seconds = settings.segment_seconds;
    let engine = std::thread::spawn(move || {
        let mut seg = Segmenter::new(seg_seconds);
        let mut machine: Option<(u32, usize, LowPass, Resampler)> = None;
        for block in brx {
            if stop_e.load(Ordering::Relaxed) {
                break;
            }
            let m = machine.get_or_insert_with(|| {
                (
                    block.rate,
                    block.channels,
                    LowPass::new(block.rate, 7200.0),
                    Resampler::new(block.rate, 16000),
                )
            });
            if m.0 != block.rate || m.1 != block.channels {
                // 设备热切换了格式：重置换算状态，避免采样的错位
                *m = (
                    block.rate,
                    block.channels,
                    LowPass::new(block.rate, 7200.0),
                    Resampler::new(block.rate, 16000),
                );
                seg.reset();
            }
            if paused_e.load(Ordering::Relaxed) {
                seg.reset();
                continue;
            }
            let mono = mix_to_mono(&block.samples, block.channels);
            let filtered = if block.rate > 16000 {
                m.2.run(&mono)
            } else {
                mono
            };
            let mut out16 = Vec::with_capacity(filtered.len() / 3 + 8);
            m.3.process(&filtered, &mut out16);
            if let Some(seg_pcm) = seg.feed(&out16) {
                if stx.send(seg_pcm).is_err() {
                    break;
                }
            }
        }
        if let Some(seg_pcm) = seg.flush() {
            let _ = stx.send(seg_pcm);
        }
    });

    // 3) 工作线程：上传 + 推送（认证错误不重试，网络/服务端错误重试 2 次）
    let key = resolve_key(settings.api_key.as_deref())?;
    let client = MmxAsr::new(&settings.base_url, key);
    let language = if settings.language == "auto" {
        String::new()
    } else {
        settings.language.clone()
    };
    let stop_w = stop.clone();
    let bus_w = bus.clone();
    let worker = std::thread::spawn(move || {
        let mut auth_dead = false;
        for seg_pcm in srx {
            if stop_w.load(Ordering::Relaxed) {
                break;
            }
            if auth_dead || seg_pcm.is_empty() {
                continue;
            }
            bus_w.status("sending", "");
            let mut attempt = 0u32;
            loop {
                match client.transcribe(&seg_pcm, &language) {
                    Ok(text) => {
                        bus_w.status("ok", "");
                        if !text.is_empty() {
                            bus_w.text(&text);
                        }
                        break;
                    }
                    Err(e) if e.is_auth() => {
                        bus_w.status("error", &e.message());
                        auth_dead = true;
                        break;
                    }
                    Err(e) => {
                        attempt += 1;
                        if attempt >= 3 {
                            bus_w.status("error", &e.message());
                            break;
                        }
                        bus_w.status("retry", &format!("重试 {attempt}/2：{}", e.message()));
                        std::thread::sleep(Duration::from_millis(400 * attempt as u64));
                    }
                }
            }
        }
    });

    let state_name = if paused.load(Ordering::Relaxed) {
        "paused"
    } else {
        "listening"
    };
    bus.status(state_name, "");
    Ok(Pipeline {
        stop,
        threads: vec![capture, engine, worker],
    })
}
