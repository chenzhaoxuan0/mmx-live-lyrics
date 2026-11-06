//! 管线：采集线程 →（块通道）→ 引擎线程（混音/重采样/VAD）→（段通道）→ 工作线程（HTTP 上传）。
//!
//! 线程模型与 sub-title 一致：单向通道解耦，停止用 AtomicBool 哨兵。
//! 暂停期间采集不停（秒级恢复），引擎线程丢弃音频。
use crate::api::AsrClient;
use crate::bus::{Bus, StatusReport, KIND_SYSTEM};
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
    /// 采集线程句柄：停止时 join它，确保音频设备真正释放（否则"停完立刻再开"会撞上设备占用）
    capture: Option<std::thread::JoinHandle<()>>,
    /// 引擎线程句柄：采集线程结束后它自然退出（通道关闭），一并 join 保持整洁
    engine: Option<std::thread::JoinHandle<()>>,
}

impl Pipeline {
    /// 停止：置标志位后，**等待采集线程真正退出**（≤几百 ms），让设备立即可用。
    ///
    /// 只 join 采集/引擎线程——它们不做网络 I/O，退出有确定上界。
    /// 工作线程可能在途一次 HTTP（最长 20s），保持 detached，绝不 join，
    /// 否则会把主线程阻塞在 HTTP 超时上（"停止不跟手"的根因）。
    pub fn stop(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.active.store(false, Ordering::Relaxed);
        if let Some(h) = self.capture.take() {
            join_bounded(h, Duration::from_millis(2000));
        }
        if let Some(h) = self.engine.take() {
            join_bounded(h, Duration::from_millis(2000));
        }
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }
}

/// 带超时的 join：超时后放弃等待（线程仍在后台自行结束），绝不无限期阻塞调用方
fn join_bounded(h: std::thread::JoinHandle<()>, timeout: Duration) {
    let (tx, rx) = mpsc::channel::<()>();
    std::thread::spawn(move || {
        let _ = h.join();
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(timeout);
}

/// 剥掉句尾句读（。！？…等）：历史字幕按文本流排列，句号会打断阅读
fn strip_tail_punct(s: &str) -> &str {
    const TAIL: &[char] = &[
        '。', '．', '｡', '！', '!', '？', '?', '…', '‥', '.', '"', '”', '\'', '’', ' ', '　',
    ];
    s.trim_end_matches(TAIL)
}

/// 启动一条单源管线。kind：KIND_MIC（麦克风）/ KIND_SYSTEM（系统声音回录）。
/// 双声源 = 调用方各启动一条，互不干扰。
pub fn start(
    bus: Arc<Bus>,
    report: Arc<StatusReport>,
    kind: usize,
    settings: Settings,
) -> Result<Pipeline, String> {
    // 关键：先做所有可能失败的准备工作（Key / 客户端），失败就直接返回错误，
    // 绝不在"已经 spawn 了线程"之后才失败——否则采集线程会变成僵尸，
    // 一直占着麦克风设备，导致下一次启动失败（"一次性"体验）。
    let client = AsrClient::from_settings(&settings)?;

    let stop = Arc::new(AtomicBool::new(false));
    let active = Arc::new(AtomicBool::new(true));
    let (btx, brx) = mpsc::channel::<AudioBlock>();
    let (stx, srx) = mpsc::channel::<Vec<f32>>();

    // 1) 采集线程（设备失败 = 致命：停整条管线）
    let stop_c = stop.clone();
    let active_c = active.clone();
    let source = if kind == KIND_SYSTEM { "system" } else { "mic" }.to_string();
    let device = if kind == KIND_SYSTEM {
        settings.loopback_device.clone()
    } else {
        settings.input_device.clone()
    };
    let report_c = report.clone();
    let capture = std::thread::spawn(move || {
        // 采集线程里的 panic（如 cpal 内部线程异常）不能被静默吞掉：
        // 否则管线会永远显示"在跑"却没有任何音频（假运行）。
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            capture_loop(&source, device.as_deref(), btx, stop_c.clone())
        }));
        active_c.store(false, Ordering::Relaxed);
        stop_c.store(true, Ordering::Relaxed);
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(e)) => report_c.report(kind, "error", &e),
            Err(_) => report_c.report(kind, "error", "音频采集线程异常退出（设备可能被独占或已拔出）"),
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
    let language = if settings.language == "auto" {
        String::new()
    } else {
        settings.language.clone()
    };
    let stop_w = stop.clone();
    let active_w = active.clone();
    let bus_w = bus.clone();
    let report_w = report.clone();
    let worker = std::thread::spawn(move || {
        let mut auth_dead = false;
        for seg_pcm in srx {
            if stop_w.load(Ordering::Relaxed) {
                break;
            }
            if auth_dead || seg_pcm.is_empty() {
                continue;
            }
            report_w.report(kind, "sending", "");
            let mut attempt = 0u32;
            loop {
                match client.transcribe(&seg_pcm, &language) {
                    Ok(text) => {
                        // 停止后到达的迟到结果不再推送
                        if !stop_w.load(Ordering::Relaxed) {
                            report_w.report(kind, "ok", "");
                            if !text.is_empty() {
                                bus_w.text(strip_tail_punct(&text));
                            }
                        }
                        break;
                    }
                    Err(e) if e.is_auth() => {
                        active_w.store(false, Ordering::Relaxed);
                        stop_w.store(true, Ordering::Relaxed);
                        report_w.report(kind, "error", &e.message());
                        auth_dead = true;
                        break;
                    }
                    Err(e) => {
                        attempt += 1;
                        if attempt >= 3 || stop_w.load(Ordering::Relaxed) {
                            active_w.store(false, Ordering::Relaxed);
                            stop_w.store(true, Ordering::Relaxed);
                            report_w.report(kind, "error", &e.message());
                            break;
                        }
                        report_w.report(
                            kind,
                            "retry",
                            &format!("重试 {attempt}/2：{}", e.message()),
                        );
                        std::thread::sleep(Duration::from_millis(400 * attempt as u64));
                    }
                }
            }
        }
    });

    report.report(kind, "listening", "");
    // 工作线程保持 detached（可能在途 HTTP，不能 join）；采集/引擎线程交给 Pipeline.stop
    drop(worker);
    Ok(Pipeline {
        stop,
        active,
        capture: Some(capture),
        engine: Some(engine),
    })
}
