//! 16k/mono/PCM16 WAV 编码（纯标准库，无依赖）

/// 编码为完整 WAV 文件字节
pub fn encode(pcm16k_mono: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(44 + pcm16k_mono.len() * 2);
    encode_into(pcm16k_mono, &mut out);
    out
}

/// 追加编码到已有缓冲（multipart 里直接拼 body）
pub fn encode_into(pcm: &[f32], out: &mut Vec<u8>) {
    const SR: u32 = 16000;
    let data_len = (pcm.len() * 2) as u32;
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SR.to_le_bytes());
    out.extend_from_slice(&(SR * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.reserve(pcm.len() * 2);
    for &f in pcm {
        let v = (f.clamp(-1.0, 1.0) * 32767.0) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
}
