//! 音频采集（cpal / WASAPI）。
//!
//! 麦克风 = capture 设备的 input 流；
//! 系统声音 = render（输出）设备的 input 流，cpal 会自动加 AUDCLNT_STREAMFLAGS_LOOPBACK。
//! 两者都按设备**原生**采样格式开流，再统一转成 f32；声道/采样率由引擎线程换算到 16k mono。
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::time::Duration;

pub struct AudioBlock {
    pub rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>,
}

pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

pub fn list_loopback_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.output_devices()
        .map(|it| it.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// 采集循环：阻塞直到 stop 置位。音频块通过 tx 发给引擎线程。
pub fn capture_loop(
    source: &str,
    device_name: Option<&str>,
    tx: Sender<AudioBlock>,
    stop: std::sync::Arc<AtomicBool>,
) -> Result<(), String> {
    let host = cpal::default_host();
    let system = source == "system";
    let device = match (system, device_name) {
        (true, Some(name)) => find_device(&host, true, name)?,
        (true, None) => host
            .default_output_device()
            .ok_or("找不到默认输出设备（无法回录系统声音）")?,
        (false, Some(name)) => find_device(&host, false, name)?,
        (false, None) => host.default_input_device().ok_or("找不到默认麦克风")?,
    };
    let dev_name = device.name().unwrap_or_else(|_| "(未知设备)".to_string());

    // 按设备原生格式开流：混音格式必然受支持，但未必是 f32（老设备常见 i16）
    let supported = if system {
        device
            .default_output_config()
            .map_err(|e| format!("读取输出设备格式失败：{e}"))?
    } else {
        device
            .default_input_config()
            .map_err(|e| format!("读取麦克风格式失败：{e}"))?
    };
    let rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let format = supported.sample_format();
    let buffer = supported.buffer_size().clone();

    let tx2 = tx.clone();
    let stream = build_stream(
        &device,
        supported.channels(),
        supported.sample_rate(),
        buffer,
        format,
        rate,
        channels,
        tx2,
    )
    .map_err(|e| format!("打开音频流失败：{e}（设备「{dev_name}」可能被独占占用或已失效）"))?;
    stream
        .play()
        .map_err(|e| format!("启动音频流失败：{e}"))?;

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
    // 先释放设备（drop 会终止 WASAPI 采集线程），让下一次启动能立刻重开同一设备
    drop(stream);
    Ok(())
}

/// 按设备原生采样格式建流；不同格式的回调分别转成 f32 后再送进引擎线程
#[allow(clippy::too_many_arguments)]
fn build_stream(
    device: &cpal::Device,
    ch: u16,
    rate: cpal::SampleRate,
    buffer: cpal::SupportedBufferSize,
    format: cpal::SampleFormat,
    rate_hz: u32,
    channels: usize,
    tx: Sender<AudioBlock>,
) -> Result<cpal::Stream, String> {
    match format {
        cpal::SampleFormat::F32 => {
            let cfg = cpal::SupportedStreamConfig::new(ch, rate, buffer, cpal::SampleFormat::F32);
            device
                .build_input_stream(
                    &cfg.into(),
                    move |data: &[f32], _| {
                        let _ = tx.send(AudioBlock {
                            rate: rate_hz,
                            channels,
                            samples: data.to_vec(),
                        });
                    },
                    |e| eprintln!("audio stream error: {e}"),
                    None,
                )
                .map_err(|e| e.to_string())
        }
        cpal::SampleFormat::I16 => {
            let cfg = cpal::SupportedStreamConfig::new(ch, rate, buffer, cpal::SampleFormat::I16);
            device
                .build_input_stream(
                    &cfg.into(),
                    move |data: &[i16], _| {
                        let _ = tx.send(AudioBlock {
                            rate: rate_hz,
                            channels,
                            samples: data.iter().map(|s| cpal::Sample::to_sample::<f32>(*s)).collect(),
                        });
                    },
                    |e| eprintln!("audio stream error: {e}"),
                    None,
                )
                .map_err(|e| e.to_string())
        }
        cpal::SampleFormat::I32 => {
            let cfg = cpal::SupportedStreamConfig::new(ch, rate, buffer, cpal::SampleFormat::I32);
            device
                .build_input_stream(
                    &cfg.into(),
                    move |data: &[i32], _| {
                        let _ = tx.send(AudioBlock {
                            rate: rate_hz,
                            channels,
                            samples: data.iter().map(|s| cpal::Sample::to_sample::<f32>(*s)).collect(),
                        });
                    },
                    |e| eprintln!("audio stream error: {e}"),
                    None,
                )
                .map_err(|e| e.to_string())
        }
        cpal::SampleFormat::I64 => {
            let cfg = cpal::SupportedStreamConfig::new(ch, rate, buffer, cpal::SampleFormat::I64);
            device
                .build_input_stream(
                    &cfg.into(),
                    move |data: &[i64], _| {
                        let _ = tx.send(AudioBlock {
                            rate: rate_hz,
                            channels,
                            samples: data.iter().map(|s| cpal::Sample::to_sample::<f32>(*s)).collect(),
                        });
                    },
                    |e| eprintln!("audio stream error: {e}"),
                    None,
                )
                .map_err(|e| e.to_string())
        }
        cpal::SampleFormat::U8 => {
            let cfg = cpal::SupportedStreamConfig::new(ch, rate, buffer, cpal::SampleFormat::U8);
            device
                .build_input_stream(
                    &cfg.into(),
                    move |data: &[u8], _| {
                        let _ = tx.send(AudioBlock {
                            rate: rate_hz,
                            channels,
                            samples: data.iter().map(|s| cpal::Sample::to_sample::<f32>(*s)).collect(),
                        });
                    },
                    |e| eprintln!("audio stream error: {e}"),
                    None,
                )
                .map_err(|e| e.to_string())
        }
        _ => {
            // 兜底：设备报告了未知格式（U16/U32/U64 等），退回 f32 尝试
            let cfg = cpal::SupportedStreamConfig::new(ch, rate, buffer, cpal::SampleFormat::F32);
            device
                .build_input_stream(
                    &cfg.into(),
                    move |data: &[f32], _| {
                        let _ = tx.send(AudioBlock {
                            rate: rate_hz,
                            channels,
                            samples: data.to_vec(),
                        });
                    },
                    |e| eprintln!("audio stream error: {e}"),
                    None,
                )
                .map_err(|e| e.to_string())
        }
    }
}

fn find_device(
    host: &cpal::Host,
    output: bool,
    name: &str,
) -> Result<cpal::Device, String> {
    let devices = if output {
        host.output_devices()
    } else {
        host.input_devices()
    };
    devices
        .map_err(|e| format!("枚举音频设备失败：{e}"))?
        .find(|d| d.name().map(|n| n == name).unwrap_or(false))
        .ok_or_else(|| format!("找不到设备「{name}」，可能已被拔出"))
}
