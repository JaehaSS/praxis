//! 음성 입력(Plan 0043) — WAV 인코딩과 STT 클라이언트.
//!
//! STT 서버는 dev-dependency 를 늘리지 않고 기존 axum 으로 세운다(DR-5).

use praxis_lib::voice::capture::encode_wav;
use praxis_lib::voice::server::{resolve_binary, ServerManaged};
use praxis_lib::voice::stt::transcribe;
use praxis_lib::voice::VoiceSettings;

#[test]
fn encode_wav_writes_valid_mono_pcm16() {
    let samples: Vec<f32> = vec![0.0, 0.5, -0.5, 1.0, -1.0];
    let wav = encode_wav(&samples, 48_000);

    let reader = hound::WavReader::new(std::io::Cursor::new(&wav)).unwrap();
    let spec = reader.spec();
    assert_eq!(
        (spec.channels, spec.sample_rate, spec.bits_per_sample),
        (1, 48_000, 16)
    );
    assert_eq!(reader.len(), 5);
}

/// f32 범위를 벗어난 샘플이 i16 랩어라운드로 부호가 뒤집히면 안 된다 — clamp 로 포화시킨다.
/// 스케일은 대칭(±i16::MAX)이라 하한은 i16::MIN 이 아니라 -32767 이다.
#[test]
fn encode_wav_clamps_out_of_range_samples() {
    let wav = encode_wav(&[2.0, -2.0], 16_000);
    let mut reader = hound::WavReader::new(std::io::Cursor::new(&wav)).unwrap();
    let samples: Vec<i16> = reader.samples::<i16>().map(|s| s.unwrap()).collect();
    assert_eq!(samples, vec![i16::MAX, -i16::MAX]);
}

/// 빈 캡처도 유효한 WAV 여야 한다 — 헤더가 깨지면 서버가 400 을 주고 원인이 흐려진다.
#[test]
fn encode_wav_handles_empty_input() {
    let wav = encode_wav(&[], 48_000);
    let reader = hound::WavReader::new(std::io::Cursor::new(&wav)).unwrap();
    assert_eq!(reader.len(), 0);
}

#[test]
fn validate_rejects_identical_hotkeys() {
    // 같은 핫키면 커맨드가 먼저 매칭되어 받아쓰기가 영영 잡히지 않는다.
    let settings = VoiceSettings {
        hotkey_command: "Alt+C".into(),
        hotkey_dictation: "alt+c".into(),
        ..VoiceSettings::default()
    };
    assert!(praxis_lib::voice::validate(&settings).is_err());
}

#[test]
fn validate_rejects_empty_base_url() {
    let settings = VoiceSettings {
        base_url: "   ".into(),
        ..VoiceSettings::default()
    };
    assert!(praxis_lib::voice::validate(&settings).is_err());
}

#[test]
fn validate_accepts_defaults() {
    assert!(praxis_lib::voice::validate(&VoiceSettings::default()).is_ok());
}

fn settings_for(base_url: String) -> VoiceSettings {
    VoiceSettings {
        base_url,
        model: "test-model".into(),
        api_key: String::new(),
        language: "ko".into(),
        ..VoiceSettings::default()
    }
}

/// 랜덤 포트 mock 서버 — `/v1/audio/transcriptions` 만 응답한다.
async fn spawn_mock(
    handler: axum::routing::MethodRouter,
) -> (String, tokio::task::JoinHandle<()>) {
    let app = axum::Router::new().route("/audio/transcriptions", handler);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    (format!("http://{address}"), server)
}

#[tokio::test]
async fn transcribe_returns_text_from_openai_compatible_response() {
    let (base, server) = spawn_mock(axum::routing::post(|| async {
        axum::Json(serde_json::json!({ "text": "  리뷰  " }))
    }))
    .await;

    let text = transcribe(&settings_for(base), vec![0u8; 16]).await.unwrap();
    // 앞뒤 공백은 라우터 정규화 이전에 털어낸다.
    assert_eq!(text, "리뷰");
    server.abort();
}

/// base_url 끝의 슬래시가 있든 없든 같은 경로를 친다 — 설정 칸에 붙여넣은 URL 이 흔히 슬래시로 끝난다.
#[tokio::test]
async fn transcribe_tolerates_trailing_slash_in_base_url() {
    let (base, server) = spawn_mock(axum::routing::post(|| async {
        axum::Json(serde_json::json!({ "text": "설정" }))
    }))
    .await;

    let text = transcribe(&settings_for(format!("{base}/")), vec![0u8; 16])
        .await
        .unwrap();
    assert_eq!(text, "설정");
    server.abort();
}

#[tokio::test]
async fn transcribe_maps_http_error_to_message() {
    let (base, server) = spawn_mock(axum::routing::post(|| async {
        (
            axum::http::StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({ "error": "no key" })),
        )
    }))
    .await;

    let err = transcribe(&settings_for(base), vec![0u8; 16])
        .await
        .unwrap_err();
    assert!(err.contains("401"), "상태코드가 문구에 남아야 한다: {err}");
    server.abort();
}

/// 실제 STT 서버와의 왕복. 서버가 있어야만 의미가 있으므로 기본 실행에서 뺀다.
///
/// 기본값이 경로 A(ohr·11434·ko-KR)라 그 서버를 띄우고 돌린다. 경로 B 는 설정을 직접 바꿔야 한다.
///
/// ```sh
/// ~/.praxis-stt/bin/ohr --serve --port 11434
/// cargo test --test voice_test -- --ignored
/// ```
#[tokio::test]
#[ignore = "로컬 STT 서버가 떠 있을 때만"]
async fn transcribe_round_trips_against_live_server() {
    let settings = VoiceSettings {
        api_key: std::env::var("VOICE_STT_API_KEY").unwrap_or_default(),
        ..VoiceSettings::default()
    };
    // 무음이라 결과 텍스트는 비어도 정상이다 — 검증 대상은 경로·인증·모델명이 맞물리는지다.
    let silence = encode_wav(&vec![0.0; 8_000], 16_000);
    transcribe(&settings, silence)
        .await
        .expect("라이브 STT 서버 왕복 실패");
}

/// 서버가 죽어 있을 때의 문구는 사용자가 원인을 바로 알아야 한다 — "STT 서버"가 들어간다.
#[tokio::test]
async fn transcribe_reports_connection_failure_with_hint() {
    // 포트 0 바인드 후 즉시 닫아 확실히 비어 있는 주소를 얻는다.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let dead = listener.local_addr().unwrap();
    drop(listener);

    let err = transcribe(&settings_for(format!("http://{dead}")), vec![0u8; 16])
        .await
        .unwrap_err();
    assert!(err.contains("STT 서버"), "안내 문구가 빠졌다: {err}");
}

/// 앱이 서버를 띄우고 전사까지 받아낸 뒤 끄는 왕복. `ohr` 가 깔려 있어야만 의미가 있다.
///
/// 포트는 기본값(11434)을 피한다 — 사용자가 손으로 띄워 둔 서버와 부딪히면
/// 테스트가 남의 프로세스를 검증하고 끝난다.
#[tokio::test]
#[ignore = "ohr 가 설치돼 있을 때만"]
async fn server_start_stop_round_trip() {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    if resolve_binary(home.as_deref()).is_none() {
        return;
    }
    let settings = VoiceSettings {
        base_url: "http://127.0.0.1:11491/v1".into(),
        language: "ko-KR".into(),
        ..VoiceSettings::default()
    };
    let manager = ServerManaged::default();
    const HEALTH: &str = "http://127.0.0.1:11491/health";

    let started = manager.start(&settings).await.expect("서버 기동 실패");
    assert!(started.running, "기동 후 running 이 아니다: {started:?}");
    assert_eq!(started.port, Some(11491));

    let silence = encode_wav(&vec![0.0; 8_000], 16_000);
    transcribe(&settings, silence)
        .await
        .expect("앱이 띄운 서버로의 전사 실패");

    // 상태 플래그가 아니라 포트를 본다 — running=false 는 자식을 놓았다는 뜻일 뿐이고,
    // 우리가 보장해야 하는 것은 프로세스가 실제로 죽어 포트를 놓았다는 사실이다.
    assert!(!manager.stop().running, "stop 직후 running 이 남았다");
    assert!(
        reqwest::get(HEALTH).await.is_err(),
        "중지 뒤에도 /health 가 응답한다 — 프로세스가 살아 있다"
    );

    // 중지 직후 재시작. stop 이 동기라 옛 프로세스가 포트를 붙들고 있을 틈이 없다.
    let restarted = manager.start(&settings).await.expect("재시작 실패");
    assert!(restarted.running, "재시작 후 running 이 아니다: {restarted:?}");
    assert_eq!(restarted.port, Some(11491));
    assert!(!manager.stop().running, "재시작분 stop 이 실패했다");
}

/// 핫키 경로의 지연 기동 왕복 — 꺼진 포트에 핫키가 오면 앱이 ohr 를 띄우고, 이미 떠 있으면
/// 손대지 않으며, 지역 없는 언어는 띄우기 전에 거른다. `ohr` 가 깔려 있어야만 의미가 있다.
#[tokio::test]
#[ignore = "ohr 가 설치돼 있을 때만"]
async fn hotkey_lazy_start_round_trip() {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    if resolve_binary(home.as_deref()).is_none() {
        return;
    }
    let settings = VoiceSettings {
        base_url: "http://127.0.0.1:11492/v1".into(),
        language: "ko-KR".into(),
        ..VoiceSettings::default()
    };
    let manager = ServerManaged::default();

    // 꺼져 있다 → 띄운다.
    manager
        .ensure_for_hotkey(&settings)
        .await
        .expect("핫키 지연 기동 실패");
    let first = manager.status();
    assert!(first.running, "지연 기동 뒤 running 이 아니다: {first:?}");

    let silence = encode_wav(&vec![0.0; 8_000], 16_000);
    transcribe(&settings, silence)
        .await
        .expect("지연 기동한 서버로의 전사 실패");

    // 떠 있다 → 같은 프로세스를 그대로 쓴다.
    manager
        .ensure_for_hotkey(&settings)
        .await
        .expect("두 번째 핫키가 실패했다");
    assert_eq!(manager.status().pid, first.pid, "떠 있는 서버를 두고 다시 띄웠다");
    assert!(!manager.stop().running, "stop 직후 running 이 남았다");

    // 지역 없는 언어는 띄우기 전에 거른다 — 띄워 봐야 ohr 가 500 을 준다.
    let bare = VoiceSettings {
        language: "ko".into(),
        ..settings
    };
    let err = manager.ensure_for_hotkey(&bare).await.unwrap_err();
    assert!(err.contains("ko-KR"), "언어 형식 안내가 빠졌다: {err}");
    assert!(!manager.status().running, "거른 뒤에 서버가 떠 있다");
}
