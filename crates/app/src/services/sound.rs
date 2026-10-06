//! Agent-run 结束提示音 —— 复刻 pi-web `hooks/useAudio.ts` 的 Web Audio 双音。
//!
//! pi-web 原始逻辑：两个 sine 振荡器，频率 C5 (523.25 Hz) 与 E5 (659.25 Hz)，
//! 相隔 0.18 s 触发；每个音的包络是 0 → 0.18 线性 20 ms 淡入，再指数衰减到
//! 0.001（45 ms 处截断）。这里在 Rust 侧合成同参数的 16-bit PCM WAV，Windows
//! 用 `PlaySoundW(SND_MEMORY)` 播放，避免依赖系统提示音（MessageBeep 的
//! MB_ICONASTERISK 音色与 pi-web 不一致）。
//!
//! 参考：`D:\github\---ai-tools---\pi-web\hooks\useAudio.ts`

/// 采样率（Hz）。Web Audio 默认 44100，这里同样取 44100。
const SAMPLE_RATE: u32 = 44_100;
/// 两个音的频率（Hz）：C5 与 E5。
const TONE_FREQS: [f64; 2] = [523.25, 659.25];
/// 第二个音相对第一个音的延迟（秒）。
const TONE_OFFSET: f64 = 0.18;
/// 线性淡入时长（秒）。
const ATTACK: f64 = 0.02;
/// 单个音从开始到衰减到静音的总时长（秒）。
const TONE_LEN: f64 = 0.45;
/// 峰值增益。
const PEAK_GAIN: f64 = 0.18;
/// 衰减终点增益（Web Audio 的 0.001，非 0，避免指数曲线除零）。
const TAIL_GAIN: f64 = 0.001;

/// 单个音的包络：0 → [`PEAK_GAIN`] 线性淡入，再指数衰减到 [`TAIL_GAIN`]。
fn envelope(t: f64) -> f64 {
    if t < 0.0 {
        return 0.0;
    }
    if t < ATTACK {
        return PEAK_GAIN * (t / ATTACK);
    }
    if t < TONE_LEN {
        // 与 exponentialRampToValueAtTime(TAIL_GAIN, TONE_LEN) 对齐：
        // t=ATTACK 时为 PEAK_GAIN，t=TONE_LEN 时为 TAIL_GAIN。
        let decay = (TONE_LEN - ATTACK).max(1e-9);
        let k = 1.0 - (t - ATTACK) / decay;
        return TAIL_GAIN * (PEAK_GAIN / TAIL_GAIN).powf(k);
    }
    0.0
}

/// 总时长（秒）：第二个音开始时刻 + 自身长度。
fn total_duration() -> f64 {
    TONE_OFFSET + TONE_LEN
}

/// 合成 pi-web 的双音提示音，返回 16-bit PCM mono WAV 字节。
pub fn done_chime_wav() -> Vec<u8> {
    let total = total_duration();
    let frames = (SAMPLE_RATE as f64 * total).ceil() as usize;
    let data_len = frames * 2; // mono / 16-bit

    let mut samples = Vec::with_capacity(frames);
    for n in 0..frames {
        let t = n as f64 / SAMPLE_RATE as f64;
        let mut v = 0.0;
        for (i, &freq) in TONE_FREQS.iter().enumerate() {
            let local = t - i as f64 * TONE_OFFSET;
            if local < 0.0 {
                continue;
            }
            v += envelope(local) * (2.0 * std::f64::consts::PI * freq * local).sin();
        }
        let pcm = (v.clamp(-1.0, 1.0) * i16::MAX as f64).round() as i16;
        samples.extend_from_slice(&pcm.to_le_bytes());
    }

    let mut wav = Vec::with_capacity(44 + data_len);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_len as u32).to_le_bytes());
    wav.extend_from_slice(&samples);
    wav
}

/// 播放 agent-run 结束提示音（pi-web 的双音 chime）；非 Windows 平台为 no-op。
pub fn play_notify_sound() {
    #[cfg(windows)]
    {
        use std::sync::OnceLock;
        // SND_MEMORY 下 pszSound 被解释为指向内存中 WAV 数据的指针；
        // SND_ASYNC 要求这块内存在播放结束前一直有效，所以只合成一次并常驻。
        const SND_ASYNC: u32 = 0x0001;
        const SND_NODEFAULT: u32 = 0x0002;
        const SND_MEMORY: u32 = 0x0004;
        static WAV: OnceLock<&'static [u8]> = OnceLock::new();
        let bytes = WAV.get_or_init(|| Box::leak(done_chime_wav().into_boxed_slice()));
        unsafe {
            #[link(name = "winmm")]
            unsafe extern "system" {
                fn PlaySoundW(
                    psz_sound: *const u16,
                    hmod: *mut core::ffi::c_void,
                    fdw_sound: u32,
                ) -> i32;
            }
            PlaySoundW(
                bytes.as_ptr() as *const u16,
                std::ptr::null_mut(),
                SND_MEMORY | SND_ASYNC | SND_NODEFAULT,
            );
        }
    }
    #[cfg(not(windows))]
    {
        // 其他平台暂无内置播放器，保持 no-op（与旧 MessageBeep 实现一致）。
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_is_valid() {
        let w = done_chime_wav();
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(&w[12..16], b"fmt ");
        assert_eq!(&w[36..40], b"data");
        // fmt: PCM(1) / mono(1) / 44100 / byte rate / block align / 16 bit
        assert_eq!(u16::from_le_bytes([w[20], w[21]]), 1);
        assert_eq!(u16::from_le_bytes([w[22], w[23]]), 1);
        assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], w[27]]), SAMPLE_RATE);
        assert_eq!(u16::from_le_bytes([w[34], w[35]]), 16);
        let data_len = u32::from_le_bytes([w[40], w[41], w[42], w[43]]) as usize;
        assert_eq!(w.len(), 44 + data_len);
        assert!(w.len() > 44);
    }

    #[test]
    fn duration_matches_pi_web() {
        // pi-web: 0.18s 偏移 + 0.45s 音长 = 0.63s
        assert!((total_duration() - 0.63).abs() < 1e-9);
        let w = done_chime_wav();
        let frames = (w.len() - 44) / 2;
        let secs = frames as f64 / SAMPLE_RATE as f64;
        assert!((secs - 0.63).abs() < 0.001, "got {secs}");
    }

    #[test]
    fn envelope_matches_web_audio() {
        assert!(envelope(0.0).abs() < 1e-12);
        assert!((envelope(ATTACK) - PEAK_GAIN).abs() < 1e-9);
        // 指数段在 TONE_LEN 处收敛到 TAIL_GAIN，之后 osc.stop 硬切到 0
        assert!((envelope(TONE_LEN - 1e-6) - TAIL_GAIN).abs() < 1e-6);
        assert_eq!(envelope(TONE_LEN), 0.0);
        assert_eq!(envelope(TONE_LEN + 0.1), 0.0);
        // 线性淡入中点
        assert!((envelope(ATTACK / 2.0) - PEAK_GAIN / 2.0).abs() < 1e-9);
        // 指数段单调下降
        let mut prev = f64::INFINITY;
        let mut t = ATTACK;
        while t < TONE_LEN {
            let e = envelope(t);
            assert!(e < prev, "envelope not decreasing at {t}");
            prev = e;
            t += 0.01;
        }
    }

    #[test]
    fn contains_both_partials() {
        // 第二音（659.25 Hz）在第一音之后启动，0.18~0.20s 区间不应静音
        let w = done_chime_wav();
        let pcm = |i: usize| -> f64 {
            let o = 44 + i * 2;
            i16::from_le_bytes([w[o], w[o + 1]]) as f64 / i16::MAX as f64
        };
        let at = |secs: f64| -> f64 {
            let n = (secs * SAMPLE_RATE as f64).round() as usize;
            (0..16).map(|k| pcm(n + k).abs()).fold(0.0, f64::max)
        };
        assert!(at(0.01) > 0.01, "first tone missing");
        assert!(at(0.19) > 0.01, "second tone missing");
    }
}

