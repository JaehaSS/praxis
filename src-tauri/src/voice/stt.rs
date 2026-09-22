//! OpenAI 호환 전사 클라이언트.
//!
//! 별도 trait 을 두지 않는다 — 계약이 곧 `POST {base_url}/audio/transcriptions` 다.
//! mlx-qwen3-asr serve, whisper 계열 서버, OpenAI 클라우드가 모두 같은 자리에 꽂힌다.

use super::VoiceSettings;
use std::time::Duration;

/// 전사 요청 상한. 이 상한이 없으면 응답을 멈춘 서버 하나가 상태를 Transcribing 에
/// 영구히 묶어 핫키 전체를 먹통으로 만든다. 60초 녹음을 로컬 1.7B 로 전사하는 데
/// 드는 시간을 여유 있게 덮는 값이다.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// WAV 를 전사해 텍스트를 돌려준다. 실패는 전부 사용자에게 그대로 보일 한국어 문구다.
pub async fn transcribe(cfg: &VoiceSettings, wav: Vec<u8>) -> Result<String, String> {
    let part = reqwest::multipart::Part::bytes(wav)
        .file_name("audio.wav")
        .mime_str("audio/wav")
        .map_err(|e| format!("오디오 첨부 실패: {e}"))?;
    let mut form = reqwest::multipart::Form::new()
        .part("file", part)
        .text("model", cfg.model.clone());
    if !cfg.language.trim().is_empty() {
        form = form.text("language", cfg.language.clone());
    }

    let url = format!(
        "{}/audio/transcriptions",
        cfg.base_url.trim().trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|e| format!("HTTP 클라이언트를 만들 수 없습니다: {e}"))?;
    let mut request = client.post(url).multipart(form);
    if !cfg.api_key.trim().is_empty() {
        request = request.bearer_auth(cfg.api_key.trim());
    }

    let response = request
        .send()
        .await
        .map_err(|e| format!("STT 서버에 연결할 수 없습니다 — 설정의 주소를 확인하세요: {e}"))?;
    let status = response.status();
    if !status.is_success() {
        // 서버 본문에 실제 사유가 담기는 경우가 많다(모델명 오타·키 누락). 있으면 함께 보여준다.
        let body = response.text().await.unwrap_or_default();
        let detail = body.trim();
        return Err(if detail.is_empty() {
            format!("STT 서버 오류: HTTP {status}")
        } else {
            format!("STT 서버 오류: HTTP {status} — {detail}")
        });
    }

    let payload: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("STT 응답을 해석할 수 없습니다: {e}"))?;
    Ok(payload["text"].as_str().unwrap_or_default().trim().to_string())
}
