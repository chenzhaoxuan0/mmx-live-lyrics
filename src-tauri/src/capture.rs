//! 音频采集（cpal / WASAPI）。
//!
//! 麦克风 = capture 设备的 input 流；
//! 系统声音 = render（输出）设备的 input 流，cpal 会自动加 AUDCLNT_STREAMFLAGS_LOOPBACK。
//! 两者都请求设备混音格式（f32），声道/采样率由引擎线程统一换算到 16k mono。
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

    // 混音格式必然受支持；强制 f32（WASAPI 共享模式原生就是 float）
    let supported = if system {
        device
            .default_output_config()
            .map_err(|e| format!("读取输出设备格式失败：{e}"))?
    } else {
        device
            .default_input_config()
            .map_err(|e| format!("读取麦克风格式失败：{e}"))?
    };
    let cfg = cpal::SupportedStreamConfig::new(
        supported.channels(),
        supported.sample_rate(),
        supported.buffer_size().clone(),
        cpal::SampleFormat::F32,
    );
    let rate = cfg.sample_rate().0;
    let channels = cfg.channels() as usize;

    let tx2 = tx.clone();
    let stream = device
        .build_input_stream(
            &cfg.into(),
            move |data: &[f32], _| {
                let _ = tx2.send(AudioBlock {
                    rate,
                    channels,
                    samples: data.to_vec(),
                });
            },
            |err| eprintln!("audio stream error: {err}"),
            None,
        )
        .map_err(|e| format!("打开音频流失败：{e}（设备可能被独占占用）"))?;
    stream
        .play()
        .map_err(|e| format!("启动音频流失败：{e}"))?;

    while !stop.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
    drop(stream);
    Ok(())
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
