//! 能量 VAD 分段器（移植自 sub-title 的 sensevoice_engine.py 逻辑）。
//!
//! 攒段规则：
//!   - 攒够 segment_seconds → 出段
//!   - 缓冲超过 500ms 且检测到 300ms 连续静音 → 提前出段（句尾断句）
//!   - 段内完全没有语音能量（纯静音/纯噪音低于阈值）→ 丢弃不上传，省配额
//!   - 段长不足 100ms → 丢弃

pub const MIN_SEG: usize = 1600; // 100ms @16k
const FLUSH_MIN_BUF: usize = 8000; // 500ms @16k
const SILENCE_FLUSH: usize = 4800; // 300ms 连续静音 @16k

pub struct Segmenter {
    buf: Vec<f32>,
    segment_samples: usize,
    silence_threshold: f32,
    silence_run: usize,
    speech_samples: usize,
}

impl Segmenter {
    pub fn new(segment_seconds: f32) -> Self {
        Self {
            buf: Vec::new(),
            segment_samples: (segment_seconds.clamp(0.8, 8.0) * 16000.0) as usize,
            silence_threshold: 0.01,
            silence_run: 0,
            speech_samples: 0,
        }
    }

    /// 喂入 16k mono 块，返回一段完整的 16k 音频（无需出段时为 None）
    pub fn feed(&mut self, chunk: &[f32]) -> Option<Vec<f32>> {
        if chunk.is_empty() {
            return None;
        }
        self.buf.extend_from_slice(chunk);
        let energy = rms(chunk);
        if energy < self.silence_threshold {
            self.silence_run += chunk.len();
        } else {
            self.silence_run = 0;
            self.speech_samples += chunk.len();
        }
        let full = self.buf.len() >= self.segment_samples;
        let tail_silence = self.buf.len() > FLUSH_MIN_BUF && self.silence_run >= SILENCE_FLUSH;
        if (full || tail_silence) && self.buf.len() > MIN_SEG {
            return self.take();
        }
        None
    }

    /// 收尾：把残留缓冲出段
    pub fn flush(&mut self) -> Option<Vec<f32>> {
        if self.buf.len() > MIN_SEG {
            self.take()
        } else {
            self.reset();
            None
        }
    }

    pub fn reset(&mut self) {
        self.buf.clear();
        self.silence_run = 0;
        self.speech_samples = 0;
    }

    fn take(&mut self) -> Option<Vec<f32>> {
        let had_speech = self.speech_samples > 0;
        let seg = std::mem::take(&mut self.buf);
        self.silence_run = 0;
        self.speech_samples = 0;
        if had_speech {
            Some(seg)
        } else {
            None
        }
    }
}

fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        0.0
    } else {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
    }
}
