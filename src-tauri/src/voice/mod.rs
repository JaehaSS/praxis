//! 음성 입력(Plan 0043) — push-to-talk 커맨드·딕테이션.
//!
//! 글로벌 핫키 Pressed 로 캡처를 시작하고 Released 로 멈춘 뒤, 인메모리 WAV 를
//! OpenAI 호환 `/v1/audio/transcriptions` 로 보낸다. 프로바이더는 그 계약을 지키는
//! 서버 전부다 — 기본값은 로컬 ohr(macOS SpeechAnalyzer)이고, 핫키를 누르면 꺼져 있을 때
//! 앱이 띄운다.

pub mod capture;
pub mod server;
pub mod stt;

use serde::{Deserialize, Serialize};
use std::sync::mpsc::{self, Sender};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

/// STT 서버 기본 주소. ohr 의 기본 포트다 — 경로 B(mlx-qwen3-asr)는 8765.
pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434/v1";
/// ohr 은 모델명을 무시하지만 `/v1/models` 가 이 이름을 준다. 경로 B 는 `Qwen/Qwen3-ASR-1.7B` —
/// 그쪽 0.6B 는 "전송"을 "선송"으로 흘려 커맨드 매칭이 깨진다(1.7B 가 실측 하한).
pub const DEFAULT_MODEL: &str = "apple-speechanalyzer";
/// ohr 은 로케일 형식만 받는다 — `ko` 는 HTTP 500 이다. 경로 B 는 `ko`.
pub const DEFAULT_LANGUAGE: &str = "ko-KR";
// macOS Option 키는 Tauri 단축키 문법에서 "Alt"로 표기한다 — 설계 0043 의 Option+C/D 와 같은 키다.
// 표기만 다르고 물리 키는 하나이므로, 사용자에게 보이는 자리에서는 ⌥ 로 되돌려 준다(`src/lib/hotkey.ts`).
#[cfg(target_os = "macos")]
pub const DEFAULT_HOTKEY_COMMAND: &str = "Alt+C";
#[cfg(target_os = "macos")]
pub const DEFAULT_HOTKEY_DICTATION: &str = "Alt+D";

// Windows·Linux 는 Ctrl 을 하나 더 얹는다. push-to-talk 은 홀드 → 릴리스인데, 그 플랫폼에서
// Alt+문자를 홀드하면 메뉴 니모닉이 깨어나고 떼는 순간 메뉴바로 포커스가 간다 — 녹음의
// 시작과 끝이 정확히 그 패턴이라 정면으로 부딪힌다.
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_HOTKEY_COMMAND: &str = "Ctrl+Alt+C";
#[cfg(not(target_os = "macos"))]
pub const DEFAULT_HOTKEY_DICTATION: &str = "Ctrl+Alt+D";

/// 녹음 상한. 초과하면 자동으로 멈추고 거기까지를 전사한다(설계 0043 §Business Rules).
pub const MAX_RECORDING_SECS: u64 = 60;

pub const STATE_EVENT: &str = "voice://state";
pub const TRANSCRIPT_EVENT: &str = "voice://transcript";
pub const ERROR_EVENT: &str = "voice://error";

/// 설정 키 — 기존 관례대로 키당 문자열 1값(DR-2).
const KEY_BASE_URL: &str = "voice_stt_base_url";
const KEY_MODEL: &str = "voice_stt_model";
const KEY_API_KEY: &str = "voice_stt_api_key";
const KEY_LANGUAGE: &str = "voice_stt_language";
const KEY_HOTKEY_COMMAND: &str = "voice_hotkey_command";
const KEY_HOTKEY_DICTATION: &str = "voice_hotkey_dictation";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VoiceSettings {
    pub base_url: String,
    pub model: String,
    /// 빈 문자열이면 Authorization 헤더를 붙이지 않는다. ohr 는 키 없이 뜬다 — 값이 있으면
    /// 앱이 띄우는 ohr 에 `OHR_TOKEN` 으로 넘어가고 요청에도 같은 Bearer 가 붙는다.
    pub api_key: String,
    pub language: String,
    pub hotkey_command: String,
    pub hotkey_dictation: String,
}

impl Default for VoiceSettings {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_string(),
            model: DEFAULT_MODEL.to_string(),
            api_key: String::new(),
            language: DEFAULT_LANGUAGE.to_string(),
            hotkey_command: DEFAULT_HOTKEY_COMMAND.to_string(),
            hotkey_dictation: DEFAULT_HOTKEY_DICTATION.to_string(),
        }
    }
}

/// 빈 문자열 저장값은 미설정과 같게 본다 — 설정 화면에서 지운 칸이 기본값으로 돌아온다.
/// 단 api_key 는 예외다(비우는 것이 유효한 의도).
fn or_default(stored: Option<String>, default: &str) -> String {
    stored
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// 설정 6키를 읽어 조립한다. 조회 실패는 기본값으로 흡수한다 — 설정 DB 사고로
/// 음성이 통째로 죽는 것보다 기본값으로 도는 편이 낫다.
pub async fn load_settings(pool: &sqlx::SqlitePool) -> VoiceSettings {
    let get = |key: &'static str| async move { crate::db::get_setting(pool, key).await.ok().flatten() };
    VoiceSettings {
        base_url: or_default(get(KEY_BASE_URL).await, DEFAULT_BASE_URL),
        model: or_default(get(KEY_MODEL).await, DEFAULT_MODEL),
        api_key: get(KEY_API_KEY).await.unwrap_or_default(),
        language: or_default(get(KEY_LANGUAGE).await, DEFAULT_LANGUAGE),
        hotkey_command: or_default(get(KEY_HOTKEY_COMMAND).await, DEFAULT_HOTKEY_COMMAND),
        hotkey_dictation: or_default(get(KEY_HOTKEY_DICTATION).await, DEFAULT_HOTKEY_DICTATION),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VoiceMode {
    Command,
    Dictation,
}

/// 상태는 3개뿐이다. Transcribing 은 전사가 끝나 transcript/error 를 emit 한 뒤에야
/// Idle 로 돌아가는 **비-Idle** 상태라, 그 사이 핫키는 전부 무시된다.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Recording,
    Transcribing,
}

struct Recording {
    mode: VoiceMode,
    stop: Sender<()>,
}

#[derive(Default)]
struct Runtime {
    recording: Option<Recording>,
    transcribing: bool,
    settings: VoiceSettings,
}

impl Runtime {
    fn is_idle(&self) -> bool {
        self.recording.is_none() && !self.transcribing
    }
}

/// Tauri 관리 상태. 설정을 캐시해 두는 이유는 핫키 핸들러가 동기 컨텍스트라
/// 거기서 DB 를 읽을 수 없기 때문이다 — 저장 시점에 갱신한다.
#[derive(Default)]
pub struct VoiceManaged {
    inner: Mutex<Runtime>,
}

impl VoiceManaged {
    pub fn settings(&self) -> VoiceSettings {
        self.inner
            .lock()
            .map(|rt| rt.settings.clone())
            .unwrap_or_default()
    }

    pub fn set_settings(&self, settings: VoiceSettings) {
        if let Ok(mut rt) = self.inner.lock() {
            rt.settings = settings;
        }
    }
}

fn emit_state(app: &AppHandle, phase: Phase, mode: Option<VoiceMode>) {
    let _ = app.emit(STATE_EVENT, serde_json::json!({ "phase": phase, "mode": mode }));
}

fn emit_error(app: &AppHandle, message: String) {
    let _ = app.emit(ERROR_EVENT, serde_json::json!({ "message": message }));
}

/// 문자열 단축키를 파싱해 비교한다 — "Alt+C"와 "alt+c"처럼 표기가 달라도 같은 키다.
fn matches(spec: &str, shortcut: &Shortcut) -> bool {
    spec.parse::<Shortcut>()
        .map(|parsed| parsed == *shortcut)
        .unwrap_or(false)
}

/// 설정된 두 핫키를 등록한다.
///
/// 하나가 실패해도 나머지는 등록한다 — 한쪽이 다른 앱과 충돌했다고 멀쩡한 쪽까지
/// 죽이면 사용자는 쓸 수 있었을 기능마저 잃는다. 실패는 모아서 함께 올린다.
pub fn register_shortcuts(app: &AppHandle, settings: &VoiceSettings) -> Result<(), String> {
    let mut failures = Vec::new();
    for spec in [&settings.hotkey_command, &settings.hotkey_dictation] {
        if spec.trim().is_empty() {
            continue;
        }
        if let Err(error) = app.global_shortcut().register(spec.as_str()) {
            failures.push(format!("'{spec}' 등록 실패({error})"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} — 다른 앱이 쓰고 있는 조합일 수 있습니다",
            failures.join(", ")
        ))
    }
}

/// 이전 설정의 핫키를 해제한다. 실패는 무시한다 — 등록된 적 없는 키의 해제는 정상 경로다.
pub fn unregister_shortcuts(app: &AppHandle, settings: &VoiceSettings) {
    for spec in [&settings.hotkey_command, &settings.hotkey_dictation] {
        if !spec.trim().is_empty() {
            let _ = app.global_shortcut().unregister(spec.as_str());
        }
    }
}

/// 핫키 이벤트 진입점. 음성 핫키가 아니면 조용히 돌아간다(디스패처가 모든 키를 여기로 보낸다).
pub fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
    let managed = app.state::<VoiceManaged>();
    let settings = managed.settings();
    let mode = if matches(&settings.hotkey_command, shortcut) {
        VoiceMode::Command
    } else if matches(&settings.hotkey_dictation, shortcut) {
        VoiceMode::Dictation
    } else {
        return;
    };

    match state {
        ShortcutState::Pressed => start_recording(app, &managed, settings, mode),
        ShortcutState::Released => {
            // 녹음 종료 신호만 보낸다. 상태 전이는 캡처 스레드가 소유한다 —
            // 60초 자동 종료처럼 Released 가 오지 않는 경로와 한 곳에서 만나야
            // 어느 쪽이든 Idle 로 돌아온다.
            if let Ok(rt) = managed.inner.lock() {
                if let Some(active) = rt.recording.as_ref() {
                    if active.mode == mode {
                        let _ = active.stop.send(());
                    }
                }
            }
        }
    }
}

fn start_recording(
    app: &AppHandle,
    managed: &tauri::State<'_, VoiceManaged>,
    settings: VoiceSettings,
    mode: VoiceMode,
) {
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    {
        let Ok(mut rt) = managed.inner.lock() else {
            return;
        };
        // 한 모드가 녹음/전사 중이면 다른 핫키는 무시한다. 홀드 중 키 반복(auto-repeat)도 여기서 걸린다.
        if !rt.is_idle() {
            return;
        }
        rt.recording = Some(Recording {
            mode,
            stop: stop_tx,
        });
    }
    emit_state(app, Phase::Recording, Some(mode));

    let handle = app.clone();
    std::thread::spawn(move || {
        let captured = capture::record_blocking(stop_rx, MAX_RECORDING_SECS);
        // 녹음이 어떻게 끝났든 여기서 Recording 을 벗어난다.
        let managed = handle.state::<VoiceManaged>();
        let captured = match captured {
            Ok(captured) => captured,
            Err(message) => {
                if let Ok(mut rt) = managed.inner.lock() {
                    rt.recording = None;
                }
                emit_error(&handle, message);
                emit_state(&handle, Phase::Idle, None);
                return;
            }
        };
        if captured.samples.is_empty() {
            if let Ok(mut rt) = managed.inner.lock() {
                rt.recording = None;
            }
            emit_error(&handle, "녹음된 소리가 없습니다".to_string());
            emit_state(&handle, Phase::Idle, None);
            return;
        }

        let wav = capture::encode_wav(&captured.samples, captured.sample_rate);
        if let Ok(mut rt) = managed.inner.lock() {
            rt.recording = None;
            rt.transcribing = true;
        }
        emit_state(&handle, Phase::Transcribing, Some(mode));

        tauri::async_runtime::spawn(async move {
            // 서버가 꺼져 있으면 여기서 띄운다(최대 5초 대기, Transcribing 상태가 그 시간을 덮는다).
            let result = match handle
                .state::<server::ServerManaged>()
                .ensure_for_hotkey(&settings)
                .await
            {
                Ok(()) => stt::transcribe(&settings, wav).await,
                Err(message) => Err(message),
            };
            match result {
                Ok(text) if text.is_empty() => {
                    emit_error(&handle, "전사 결과가 비었습니다".to_string())
                }
                Ok(text) => {
                    let _ = handle.emit(
                        TRANSCRIPT_EVENT,
                        serde_json::json!({ "mode": mode, "text": text }),
                    );
                }
                Err(message) => emit_error(&handle, message),
            }
            if let Ok(mut rt) = handle.state::<VoiceManaged>().inner.lock() {
                rt.transcribing = false;
            }
            emit_state(&handle, Phase::Idle, None);
        });
    });
}

/// 저장 전 유효성 검사.
///
/// 두 핫키가 같으면 커맨드가 먼저 매칭되어 받아쓰기는 영영 잡히지 않는다.
/// 등록 자체는 성공하므로 조용히 반쪽이 되는 대신 저장을 거부한다.
pub fn validate(settings: &VoiceSettings) -> Result<(), String> {
    if settings.base_url.trim().is_empty() {
        return Err("서버 주소가 비었습니다".to_string());
    }
    let command = settings.hotkey_command.trim();
    let dictation = settings.hotkey_dictation.trim();

    // 해석 불가능한 표기는 여기서 막는다. 통과시키면 DB 에는 남고 등록만 실패하는데,
    // 그 실패는 이번 저장에서만 사용자에게 닿는다 — 다음 실행의 등록 실패는 stderr 로만
    // 새어 나가므로(`lib.rs` setup), 앱은 재시작될 때마다 이유 없이 핫키가 죽은 상태가 된다.
    for (label, spec) in [("커맨드", command), ("받아쓰기", dictation)] {
        if !spec.is_empty() && spec.parse::<Shortcut>().is_err() {
            return Err(format!("{label} 핫키 '{spec}' 를 해석할 수 없습니다"));
        }
    }

    // 표기가 달라도 조합이 같으면 같은 키다 — "Alt+Shift+C" 와 "Shift+Alt+C".
    // 파싱된 값끼리 비교해야 잡힌다(문자열 비교는 통과시킨다).
    if let (Ok(command), Ok(dictation)) = (command.parse::<Shortcut>(), dictation.parse::<Shortcut>())
    {
        if command == dictation {
            return Err("커맨드와 받아쓰기에 같은 핫키를 쓸 수 없습니다".to_string());
        }
    }
    Ok(())
}

/// 설정 6키를 저장한다.
pub async fn save_settings(pool: &sqlx::SqlitePool, settings: &VoiceSettings) -> Result<(), String> {
    let pairs = [
        (KEY_BASE_URL, settings.base_url.trim()),
        (KEY_MODEL, settings.model.trim()),
        (KEY_API_KEY, settings.api_key.trim()),
        (KEY_LANGUAGE, settings.language.trim()),
        (KEY_HOTKEY_COMMAND, settings.hotkey_command.trim()),
        (KEY_HOTKEY_DICTATION, settings.hotkey_dictation.trim()),
    ];
    for (key, value) in pairs {
        crate::db::set_setting(pool, key, value)
            .await
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}
