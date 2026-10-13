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
    /// 是否仍在有效运行：致命错误（设备/鉴权/连续网络失败）会置 false，此时管线虽有线程残留但已停摆
    active: Arc<AtomicBool>,
}

impl Pipeline {
    /// 立即返回，不 join 任何线程：
    ///   - 采集/引擎线程在 ≤100ms 内感知标志位自行退出（音频即刻停）
    ///   - 工作线程可能在途一次 HTTP（最长 20s），会在后台自然收尾且不再推送文本
    /// join 版本会把主线程阻塞在 HTTP 超时上，是"停止不跟手"的根因，绝不能恢复。
    pub fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
        self.active.store(false, Ordering::Relaxed);
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}

/// 剥掉句尾句读（。！？…等）：历史字幕按文本流排列，句号会打断阅读
fn strip_tail_punct(s: &str) -> &str {
    const TAIL: &[char] = &[
        '。', '．', '｡', '！', '!', '？', '?', '…', '‥', '.', '"', '”', '\'', '’', ' ', '　',
    ];
    s.trim_end_matches(TAIL)
}

pub fn start(bus: Arc<Bus>, settings: Settings) -> Result<Pipeline, String> {
    let stop = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicBool::new(true));
    let (btx, brx) = mpsc::channel::<AudioBlock>();
    let (stx, srx) = mpsc::channel::<Vec<f32>>();

    // 1) 采集线程（设备失败 = 致命：停整条管线）
    let stop_c = stop.clone();
    let active_c = active.clone();
    let source = settings.source.clone();
    let device = if source == "system" {
        settings.loopback_device.clone()
    } else {
        settings.input_device.clone()
    };
    let bus_c = bus.clone();
    let capture = std::thread::spawn(move || {
        if let Err(e) = capture_loop(&source, device.as_deref(), btx, stop_c.clone()) {
            active_c.store(false, Ordering::Relaxed);
            stop_c.store(true, Ordering::Relaxed);
            bus_c.status("error", &e);
        }
    });

    // 2) 引擎线程：混音 → 低通 → 重采样到 16k → VAD 分段
    let stop_e = stop.clone();
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

    // 3) 工作线程：上传 + 推送（鉴权错误不重试；网络/服务端错误重试 2 次；
    //    鉴权失败 / 连续失败 = 致命：停整条管线，避免按钮陷入"假运行"）
    let key = resolve_key(settings.api_key.as_deref())?;
    let client = MmxAsr::new(&settings.base_url, key);
    let language = if settings.language == "auto" {
        String::new()
    } else {
        settings.language.clone()
    };
    let stop_w = stop.clone();
    let active_w = active.clone();
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
                        // 停止后到达的迟到结果不再推送
                        if !stop_w.load(Ordering::Relaxed) {
                            bus_w.status("ok", "");
                            if !text.is_empty() {
                                bus_w.text(strip_tail_punct(&text));
                            }
                        }
                        break;
                    }
                    Err(e) if e.is_auth() => {
                        active_w.store(false, Ordering::Relaxed);
                        stop_w.store(true, Ordering::Relaxed);
                        bus_w.status("error", &e.message());
                        auth_dead = true;
                        break;
                    }
                    Err(e) => {
                        attempt += 1;
                        if attempt >= 3 || stop_w.load(Ordering::Relaxed) {
                            active_w.store(false, Ordering::Relaxed);
                            stop_w.store(true, Ordering::Relaxed);
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

    bus.status("listening", "");
    // JoinHandle 一经 drop 线程即脱离（detached，不阻塞）；停止只靠 stop 标志广播
    drop(capture);
    drop(engine);
    drop(worker);
    Ok(Pipeline { stop, active })
}
