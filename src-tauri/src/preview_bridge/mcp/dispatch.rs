//! 서버가 프리뷰 웹뷰에 보내는 명령과 그 실패. 실제 디스패처는 Tauri 쪽에 산다.

use std::time::Duration;

use serde_json::{json, Value};

use crate::preview_bridge::{validate_preview_probe_url, CancelReason, RejectReason};

/// 인자로 `timeout`을 받지 않는 툴의 데드라인(0058 D-12).
const DEFAULT_DEADLINE: Duration = Duration::from_secs(10);

/// `wait_for`의 데드라인은 툴 `timeout`보다 이만큼 길다 — 웹뷰가 스스로 끝내고 결과를
/// 올릴 틈을 준다. 상한은 에이전트 쪽 90초(`inject.rs`)보다 작아야 한다(0058 D-12).
const DEADLINE_MARGIN_MS: u64 = 2_000;
const MAX_DEADLINE: Duration = Duration::from_secs(62);

/// `timeout` 인자는 초 단위 정수다.
const WAIT_TIMEOUT_SECS: std::ops::RangeInclusive<u64> = 1..=60;
const DEFAULT_WAIT_SECS: u64 = 10;

/// 대기 문구 상한. 페이지 텍스트를 통째로 넣는 용법이 아니다.
const MAX_WAIT_CHARS: usize = 1_024;

/// `fill` 본문 상한. 넘는 텍스트는 웹뷰에 닿기 전에 거절한다.
const MAX_FILL_BYTES: usize = 65_536;

/// 키 이름은 `Enter`·`ArrowDown` 같은 `KeyboardEvent.key` 값이다.
const MAX_KEY_CHARS: usize = 32;

const INVALID_REF: (i64, &str) = (-32000, "invalid_argument: ref");
const INVALID_TIMEOUT: (i64, &str) = (-32000, "invalid_argument: timeout");
const INVALID_WAIT_TEXT: (i64, &str) = (-32000, "invalid_argument: text");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Navigate {
        url: String,
    },
    Snapshot,
    Click {
        r#ref: String,
    },
    Fill {
        r#ref: String,
        text: String,
    },
    PressKey {
        key: String,
        r#ref: Option<String>,
    },
    /// `text`·`gone` 중 적어도 하나는 있다 — 둘 다 없는 대기는 인자 검증에서 걸린다.
    WaitFor {
        text: Option<String>,
        gone: Option<String>,
        timeout_ms: u64,
    },
    Console {
        clear: bool,
    },
}

impl Command {
    pub fn op(&self) -> &'static str {
        match self {
            Command::Navigate { .. } => "navigate",
            Command::Snapshot => "snapshot",
            Command::Click { .. } => "click",
            Command::Fill { .. } => "fill",
            Command::PressKey { .. } => "press_key",
            Command::WaitFor { .. } => "wait_for",
            Command::Console { .. } => "console",
        }
    }

    /// 웹뷰 `__praxisPreviewExec.run`이 받는 전선 형식. 두 벌이 되면 하나가 먼저 낡는다.
    pub fn to_exec_json(&self) -> Value {
        match self {
            Command::Navigate { url } => json!({ "op": "navigate", "url": url }),
            Command::Snapshot => json!({ "op": "snapshot" }),
            Command::Click { r#ref } => json!({ "op": "click", "ref": r#ref }),
            Command::Fill { r#ref, text } => json!({ "op": "fill", "ref": r#ref, "text": text }),
            // `ref`가 없으면 웹뷰가 `document.activeElement`에 친다 — 키를 빼고 보낸다.
            Command::PressKey { key, r#ref: None } => json!({ "op": "press_key", "key": key }),
            Command::PressKey {
                key,
                r#ref: Some(r#ref),
            } => json!({ "op": "press_key", "key": key, "ref": r#ref }),
            Command::WaitFor {
                text,
                gone,
                timeout_ms,
            } => {
                // 없는 조건은 키째로 뺀다 — 웹뷰가 `null`을 빈 문자열로 읽을 여지를 남기지 않는다.
                let mut value = json!({ "op": "wait_for", "timeout_ms": timeout_ms });
                for (key, arg) in [("text", text), ("gone", gone)] {
                    if let Some(arg) = arg {
                        value[key] = Value::from(arg.as_str());
                    }
                }
                value
            }
            Command::Console { clear } => json!({ "op": "console", "clear": clear }),
        }
    }

    /// MCP 툴 인자를 명령으로 옮긴다. 인자 검증은 여기서 끝난다 — 웹뷰까지 내려간 뒤
    /// 실패하면 왕복 하나와 제어 표시 한 번을 헛되이 쓴다. 실패는 (JSON-RPC 코드, 메시지).
    pub fn from_tool_call(name: &str, arguments: &Value) -> Result<Command, (i64, &'static str)> {
        match name {
            "browser_snapshot" => Ok(Command::Snapshot),
            "browser_navigate" => Ok(Command::Navigate {
                url: url_arg(arguments)?,
            }),
            "browser_click" => Ok(Command::Click {
                r#ref: ref_arg(arguments)?,
            }),
            "browser_fill" => Ok(Command::Fill {
                r#ref: ref_arg(arguments)?,
                text: text_arg(arguments)?,
            }),
            "browser_press_key" => Ok(Command::PressKey {
                key: key_arg(arguments)?,
                r#ref: optional_ref_arg(arguments)?,
            }),
            "browser_wait_for" => wait_for_command(arguments),
            "browser_console" => Ok(Command::Console {
                clear: clear_arg(arguments)?,
            }),
            _ => Err((-32601, "unknown tool")),
        }
    }
}

/// 조건이 하나도 없으면 무엇을 기다릴지 모른다 — `text` 쪽 이름으로 거절한다.
fn wait_for_command(arguments: &Value) -> Result<Command, (i64, &'static str)> {
    let text = wait_arg(arguments, "text", INVALID_WAIT_TEXT)?;
    let gone = wait_arg(arguments, "gone", (-32000, "invalid_argument: gone"))?;
    if text.is_none() && gone.is_none() {
        return Err(INVALID_WAIT_TEXT);
    }
    Ok(Command::WaitFor {
        text,
        gone,
        timeout_ms: timeout_arg(arguments)?,
    })
}

fn wait_arg(
    arguments: &Value,
    key: &str,
    invalid: (i64, &'static str),
) -> Result<Option<String>, (i64, &'static str)> {
    match arguments.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .filter(|text| (1..=MAX_WAIT_CHARS).contains(&text.chars().count()))
            .map(|text| Some(text.to_string()))
            .ok_or(invalid),
    }
}

/// 초를 받아 밀리초로 돌려준다. 소수는 내림한다 — `1.9`는 1초지 2초가 아니다.
fn timeout_arg(arguments: &Value) -> Result<u64, (i64, &'static str)> {
    let Some(value) = arguments.get("timeout").filter(|value| !value.is_null()) else {
        return Ok(DEFAULT_WAIT_SECS * 1_000);
    };
    let secs = value
        .as_f64()
        .filter(|secs| secs.is_finite() && *secs >= 0.0)
        .map(|secs| secs.floor() as u64)
        .filter(|secs| WAIT_TIMEOUT_SECS.contains(secs))
        .ok_or(INVALID_TIMEOUT)?;
    Ok(secs * 1_000)
}

fn clear_arg(arguments: &Value) -> Result<bool, (i64, &'static str)> {
    match arguments.get("clear") {
        None | Some(Value::Null) => Ok(false),
        Some(value) => value.as_bool().ok_or((-32000, "invalid_argument: clear")),
    }
}

fn url_arg(arguments: &Value) -> Result<String, (i64, &'static str)> {
    let url = str_arg(arguments, "url")
        .and_then(|value| tauri::Url::parse(value).ok())
        .filter(is_loopback_http)
        .ok_or((-32000, "invalid_url"))?;
    Ok(url.to_string())
}

fn ref_arg(arguments: &Value) -> Result<String, (i64, &'static str)> {
    optional_ref_arg(arguments)?.ok_or(INVALID_REF)
}

fn optional_ref_arg(arguments: &Value) -> Result<Option<String>, (i64, &'static str)> {
    match arguments.get("ref") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .filter(|value| is_element_ref(value))
            .map(|value| Some(value.to_string()))
            .ok_or(INVALID_REF),
    }
}

fn text_arg(arguments: &Value) -> Result<String, (i64, &'static str)> {
    str_arg(arguments, "text")
        .filter(|text| text.len() <= MAX_FILL_BYTES)
        .map(str::to_string)
        .ok_or((-32000, "invalid_argument: text"))
}

fn key_arg(arguments: &Value) -> Result<String, (i64, &'static str)> {
    str_arg(arguments, "key")
        .filter(|key| (1..=MAX_KEY_CHARS).contains(&key.chars().count()))
        .map(str::to_string)
        .ok_or((-32000, "invalid_argument: key"))
}

fn str_arg<'a>(arguments: &'a Value, key: &str) -> Option<&'a str> {
    arguments.get(key)?.as_str()
}

/// 스냅샷이 발급한 `s<스냅샷>e<요소>` 꼴만 받는다. 정규식 크레이트를 들이지 않는다 —
/// 규칙이 이 한 줄이라 손으로 읽는 편이 짧다.
fn is_element_ref(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('s') else {
        return false;
    };
    let Some((snapshot, element)) = rest.split_once('e') else {
        return false;
    };
    is_digits(snapshot) && is_digits(element)
}

fn is_digits(value: &str) -> bool {
    !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DispatchError {
    NoPreview,
    NotControllableOrigin,
    Busy,
    TakenOver,
    /// 세대가 올라 대기자가 버려졌다 — 회수(`TakenOver`)와 달리 다시 걸면 된다.
    Stale,
    Timeout,
    InvalidUrl(String),
    /// 네비게이션 자체가 거절·롤백됐다 — URL이 잘못된 것과 구분한다.
    NavigationFailed(String),
    Bridge(RejectReason),
}

impl DispatchError {
    /// 에이전트가 그대로 읽는 오류 이름. 계획 0029 표와 같은 snake_case를 쓴다.
    pub fn as_str(&self) -> &'static str {
        match self {
            DispatchError::NoPreview => "no_preview",
            DispatchError::NotControllableOrigin => "not_controllable_origin",
            DispatchError::Busy => "busy",
            DispatchError::TakenOver => "taken_over",
            DispatchError::Stale => "stale",
            DispatchError::Timeout => "timeout",
            DispatchError::InvalidUrl(_) => "invalid_url",
            DispatchError::NavigationFailed(_) => "navigation_failed",
            DispatchError::Bridge(_) => "bridge_rejected",
        }
    }
}

impl From<RejectReason> for DispatchError {
    fn from(reason: RejectReason) -> Self {
        match reason {
            RejectReason::Busy => DispatchError::Busy,
            RejectReason::TakenOver => DispatchError::TakenOver,
            other => DispatchError::Bridge(other),
        }
    }
}

#[async_trait::async_trait]
pub trait Dispatcher: Send + Sync {
    async fn dispatch(&self, task_id: i64, cmd: Command) -> Result<String, DispatchError>;
}

/// 대기자가 버려진 이유를 오류 이름으로 옮긴다. 회수만 `TakenOver`이고 — 사용자가 몰고 있으니
/// 다시 걸면 안 된다 — 나머지(네비게이션·취소·이유 없음)는 다시 걸면 되는 `Stale`이다.
pub fn error_for_dropped_waiter(reason: Option<CancelReason>) -> DispatchError {
    match reason {
        Some(CancelReason::TakeOver) => DispatchError::TakenOver,
        Some(CancelReason::Navigation) | Some(CancelReason::Cancel) | None => DispatchError::Stale,
    }
}

/// 부등식 `에이전트 타임아웃 ≥ 서버 데드라인 ≥ 툴 timeout`을 지킨다(0058 D-12).
pub fn deadline_for(cmd: &Command) -> Duration {
    let Command::WaitFor { timeout_ms, .. } = cmd else {
        return DEFAULT_DEADLINE;
    };
    Duration::from_millis(timeout_ms + DEADLINE_MARGIN_MS).min(MAX_DEADLINE)
}

/// 인자로 들어온 URL 규칙 — 프로브와 같은 것을 쓴다(포트 필수, userinfo 금지).
pub fn is_loopback_http(url: &tauri::Url) -> bool {
    validate_preview_probe_url(url.as_str()).is_ok()
}

/// 지금 떠 있는 페이지가 제어 가능한 출처인지. 포트를 요구하지 않는다 — 80번으로 뜬
/// `http://localhost/`도 로컬 개발 서버이고, 이미 로드된 URL은 우리가 고를 수 없다.
pub fn is_loopback_origin(url: &tauri::Url) -> bool {
    url.scheme() == "http" && matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn element_ref_needs_digits_on_both_sides_of_e() {
        assert!(is_element_ref("s1e3"));
        assert!(is_element_ref("s12e345"));
        for denied in ["", "x1e3", "s1", "se3", "s1e", "s1e3e4", "S1E3", " s1e3"] {
            assert!(!is_element_ref(denied), "accepted bad ref: {denied}");
        }
    }

    #[test]
    fn press_key_omits_ref_when_the_focused_element_is_the_target() {
        let cmd = Command::from_tool_call("browser_press_key", &json!({ "key": "Enter" })).unwrap();
        assert_eq!(cmd.to_exec_json(), json!({ "op": "press_key", "key": "Enter" }));
    }

    #[test]
    fn oversized_fill_text_and_empty_key_are_invalid_arguments() {
        let text = "x".repeat(MAX_FILL_BYTES + 1);
        let fill = Command::from_tool_call("browser_fill", &json!({ "ref": "s1e3", "text": text }));
        let key = Command::from_tool_call("browser_press_key", &json!({ "key": "" }));
        assert_eq!(fill, Err((-32000, "invalid_argument: text")));
        assert_eq!(key, Err((-32000, "invalid_argument: key")));
    }

    #[test]
    fn wait_for_needs_a_condition_and_bounds_the_timeout() {
        let call = |arguments| Command::from_tool_call("browser_wait_for", &arguments);
        let default = call(json!({ "gone": "loading" }));
        assert_eq!(call(json!({ "timeout": 5 })), Err(INVALID_WAIT_TEXT));
        assert_eq!(call(json!({ "text": "x", "timeout": 61 })), Err(INVALID_TIMEOUT));
        assert_eq!(call(json!({ "gone": "x", "timeout": 0 })), Err(INVALID_TIMEOUT));
        assert!(matches!(default, Ok(Command::WaitFor { timeout_ms, .. }) if timeout_ms == 10_000));
        let clear = Command::from_tool_call("browser_console", &json!({ "clear": "yes" }));
        assert_eq!(clear, Err((-32000, "invalid_argument: clear")));
    }

    /// 에이전트(90초) ≥ 서버 데드라인 ≥ 툴 `timeout` — AC-10이 재는 부등식이다.
    #[test]
    fn wait_for_deadline_is_the_timeout_plus_two_seconds_under_the_agent_timeout() {
        let deadline = |secs: u64| {
            deadline_for(&Command::WaitFor {
                text: Some("done".into()),
                gone: None,
                timeout_ms: secs * 1_000,
            })
        };
        assert_eq!(deadline(30), Duration::from_secs(32));
        assert_eq!(deadline(60), Duration::from_secs(62));
        // `inject.rs`의 TOOL_TIMEOUT_SECS는 비공개다 — 그 값 90을 여기 리터럴로 둔다.
        assert!(MAX_DEADLINE < Duration::from_secs(90));
    }
}
