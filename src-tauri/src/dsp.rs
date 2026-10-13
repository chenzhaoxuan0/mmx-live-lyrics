//! 音频 DSP：声道混缩、抗混叠低通、线性插值重采样。
//!
//! 采集侧拿到的是设备混音格式（通常 48k 立体声 f32），
//! 这里统一处理到 16k mono 再进 VAD。

/// 多声道混缩为单声道（算术平均）
pub fn mix_to_mono(samples: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return samples.to_vec();
    }
    samples
        .chunks(channels)
        .map(|c| c.iter().sum::<f32>() / channels as f32)
        .collect()
}

/// RBJ 双二阶低通（butterworth Q），降采样前滤掉 8kHz 以上成分防混叠
pub struct LowPass {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    x1: f32,
    x2: f32,
    y1: f32,
    y2: f32,
}

impl LowPass {
    pub fn new(rate: u32, cutoff: f32) -> Self {
        let w0 = 2.0 * std::f32::consts::PI * cutoff / rate as f32;
        let (sin, cos) = (w0.sin(), w0.cos());
        let alpha = sin / (2.0 * std::f32::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        Self {
            b0: ((1.0 - cos) / 2.0) / a0,
            b1: (1.0 - cos) / a0,
            b2: ((1.0 - cos) / 2.0) / a0,
            a1: (-2.0 * cos) / a0,
            a2: (1.0 - alpha) / a0,
            x1: 0.0,
            x2: 0.0,
            y1: 0.0,
            y2: 0.0,
        }
    }

    pub fn run(&mut self, x: &[f32]) -> Vec<f32> {
        let mut y = vec![0.0f32; x.len()];
        for i in 0..x.len() {
            let yn = self.b0 * x[i] + self.b1 * self.x1 + self.b2 * self.x2
                - self.a1 * self.y1
                - self.a2 * self.y2;
            self.x2 = self.x1;
            self.x1 = x[i];
            self.y2 = self.y1;
            self.y1 = yn;
            y[i] = yn;
        }
        y
    }
}

/// 线性插值重采样（跨块保持相位）。
/// 输出位置落在输入采样坐标系 0, r, 2r, ...（r = in_rate/out_rate）。
pub struct Resampler {
    step: f32,
    rem: f32,
    prev: f32,
    bypass: bool,
}

impl Resampler {
    pub fn new(in_rate: u32, out_rate: u32) -> Self {
        Self {
            step: in_rate as f32 / out_rate as f32,
            rem: 0.0,
            prev: 0.0,
            bypass: in_rate == out_rate,
        }
    }

    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.bypass {
            out.extend_from_slice(input);
            return;
        }
        for &cur in input {
            while self.rem < 1.0 {
                out.push(self.prev + (cur - self.prev) * self.rem);
                self.rem += self.step;
            }
            self.rem -= 1.0;
            self.prev = cur;
        }
    }
}
