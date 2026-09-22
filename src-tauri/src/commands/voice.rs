//! 음성 입력(STT) 설정 커맨드.
//!
//! `commands.rs`에서 갈라져 나왔다 — 모든 기능이 한 파일 끝에 줄을 붙이는
//! 구조가 병행 worktree의 머지 충돌을 만들었다.


use tauri::{Manager, State};

use crate::voice;

use super::{AppState, pool_of};

/// 음성 설정 조회 — 미설정 키는 기본값(로컬 ohr, Alt+C/Alt+D)으로 채운다.
#[tauri::command]
pub async fn voice_settings_get(state: State<'_, AppState>) -> Result<voice::VoiceSettings, String> {
    let pool = pool_of(&state)?;
    Ok(voice::load_settings(&pool).await)
}

/// 음성 설정 저장 후 핫키를 재등록한다.
///
/// 등록 실패(다른 앱이 선점 등)는 Err 로 올린다 — 조용히 삼키면 사용자는 저장됐다고
/// 믿는데 핫키만 죽어 있는 상태가 된다. 저장 자체는 이미 끝났으므로 값은 남는다.
#[tauri::command]
pub async fn voice_settings_set(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    settings: voice::VoiceSettings,
) -> Result<(), String> {
    voice::validate(&settings)?;
    let pool = pool_of(&state)?;
    voice::save_settings(&pool, &settings).await?;

    let managed = app.state::<voice::VoiceManaged>();
    voice::unregister_shortcuts(&app, &managed.settings());
    managed.set_settings(settings.clone());
    voice::register_shortcuts(&app, &settings)
}

/// STT 엔드포인트 왕복 확인 — 0.5초 무음을 실제로 전사시켜 본다.
///
/// `/models` 로 확인하지 않는 이유는 그 경로를 노출하지 않는 호환 서버가 있어서다.
/// 전사 경로 자체를 때려야 모델명·인증까지 함께 검증된다. 무음이라 결과 텍스트는 비어도 정상.
#[tauri::command]
pub async fn voice_stt_test(settings: voice::VoiceSettings) -> Result<(), String> {
    let silence = voice::capture::encode_wav(&vec![0.0; 8_000], 16_000);
    voice::stt::transcribe(&settings, silence).await.map(|_| ())
}

/// 로컬 STT 서버 상태 — 설치 여부와 앱이 띄운 자식의 생존을 함께 준다.
#[tauri::command]
pub fn voice_server_status(app: tauri::AppHandle) -> voice::server::VoiceServerStatus {
    app.state::<voice::server::ServerManaged>().status()
}

/// 로컬 STT 서버 기동 — 사용자 클릭으로만 불린다(ADR 0103 개정).
///
/// 저장된 설정이 아니라 인자로 받은 설정을 쓴다. 설정 화면에서 주소를 고친 직후
/// 저장 없이 켜 보는 흐름이 자연스럽고, `voice_stt_test` 도 같은 규약이다.
#[tauri::command]
pub async fn voice_server_start(
    app: tauri::AppHandle,
    settings: voice::VoiceSettings,
) -> Result<voice::server::VoiceServerStatus, String> {
    app.state::<voice::server::ServerManaged>()
        .start(&settings)
        .await
}

/// 로컬 STT 서버 종료 — 실제 종료(최대 0.8초)를 기다린 뒤 답한다.
///
/// 동기 명령은 메인 스레드에서 돌아 그 시간만큼 UI가 멈추므로, 블로킹 풀로 넘긴다.
#[tauri::command]
pub async fn voice_server_stop(
    app: tauri::AppHandle,
) -> Result<voice::server::VoiceServerStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<voice::server::ServerManaged>().stop()
    })
    .await
    .map_err(|e| format!("서버 종료 작업이 중단됐습니다: {e}"))
}
