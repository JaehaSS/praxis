//! 마이크 캡처와 인메모리 WAV 인코딩.
//!
//! 샘플레이트는 장치 기본값을 그대로 쓴다(DR-4). 16kHz 고정은 리샘플러를 요구하는데,
//! 리샘플링은 STT 서버가 이미 한다 — Task 0 에서 48kHz WAV 수용을 실측 확인했다.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use hound::{SampleFormat, WavSpec, WavWriter};
use std::io::Cursor;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 캡처 1회의 결과 — 샘플과 그때 쓰인 장치 샘플레이트.
pub struct Capture {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

/// 마이크를 열고 `stop` 신호 또는 `max_secs` 까지 녹음한다. **호출 스레드를 점유한다** —
/// cpal `Stream` 이 `!Send` 라 스트림을 만든 스레드가 끝까지 소유해야 하기 때문이다.
///
/// 멀티채널 장치는 채널 0만 취해 mono 로 만든다(설계 0043).
pub fn record_blocking(stop: Receiver<()>, max_secs: u64) -> Result<Capture, String> {
    let device = cpal::default_host()
        .default_input_device()
        .ok_or_else(|| mic_error("입력 장치를 찾을 수 없습니다"))?;
    let config = device
        .default_input_config()
        .map_err(|e| mic_error(&format!("입력 설정을 읽을 수 없습니다({e})")))?;
    let sample_rate = config.sample_rate().0;
    let channels = config.channels() as usize;

    let collected = Arc::new(Mutex::new(Vec::<f32>::new()));
    let sink = collected.clone();
    let on_error = |e| eprintln!("음성 캡처 스트림 오류: {e}");

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device.build_input_stream(
            &config.into(),
            move |data: &[f32], _: &_| push_mono(&sink, data, channels, |s| s),
            on_error,
            None,
        ),
        cpal::SampleFormat::I16 => device.build_input_stream(
            &config.into(),
            move |data: &[i16], _: &_| {
                push_mono(&sink, data, channels, |s| s as f32 / i16::MAX as f32)
            },
            on_error,
            None,
        ),
        cpal::SampleFormat::U16 => device.build_input_stream(
            &config.into(),
            move |data: &[u16], _: &_| {
                push_mono(&sink, data, channels, |s| {
                    (s as f32 - u16::MAX as f32 / 2.0) / (u16::MAX as f32 / 2.0)
                })
            },
            on_error,
            None,
        ),
        other => return Err(mic_error(&format!("지원하지 않는 샘플 형식입니다({other})"))),
    }
    .map_err(|e| mic_error(&format!("마이크를 열 수 없습니다({e})")))?;

    stream
        .play()
        .map_err(|e| mic_error(&format!("녹음을 시작할 수 없습니다({e})")))?;
    // 상한에 걸리면 거기까지를 전사한다 — 버리면 사용자가 말한 내용이 통째로 사라진다.
    let _ = stop.recv_timeout(Duration::from_secs(max_secs));
    drop(stream);

    let samples = collected.lock().map(|s| s.clone()).unwrap_or_default();
    Ok(Capture {
        samples,
        sample_rate,
    })
}

fn push_mono<T: Copy>(
    sink: &Arc<Mutex<Vec<f32>>>,
    data: &[T],
    channels: usize,
    to_f32: impl Fn(T) -> f32,
) {
    let Ok(mut buffer) = sink.lock() else { return };
    for frame in data.chunks(channels.max(1)) {
        if let Some(first) = frame.first() {
            buffer.push(to_f32(*first));
        }
    }
}

/// 마이크 실패는 대개 권한 문제라, 사용자가 바로 갈 수 있는 경로를 붙인다.
fn mic_error(detail: &str) -> String {
    format!("{detail} — 시스템 설정 > 개인정보 보호 및 보안 > 마이크를 확인하세요")
}

/// f32(-1.0..=1.0) 샘플을 mono PCM16 WAV 바이트로 만든다.
///
/// 범위를 벗어난 샘플은 `as i16` 이 랩어라운드시켜 부호가 뒤집히므로 clamp 로 포화시킨다.
pub fn encode_wav(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let spec = WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    let mut buffer = Cursor::new(Vec::<u8>::new());
    {
        // hound 는 finalize 시점에 헤더 길이를 되쓴다 — writer 를 스코프로 가둬 반드시 마감시킨다.
        let mut writer = WavWriter::new(&mut buffer, spec).expect("WAV 헤더 작성 실패");
        for sample in samples {
            let scaled = (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round();
            let _ = writer.write_sample(scaled as i16);
        }
        let _ = writer.finalize();
    }
    buffer.into_inner()
}
