//! Tauri 디스패처가 웹뷰 없이도 지켜야 하는 순수 규칙 — origin·데드라인·이벤트·스크립트.

use std::time::Duration;

use praxis_lib::preview_bridge::mcp::dispatch::error_for_dropped_waiter;
use praxis_lib::preview_bridge::mcp::{deadline_for, is_loopback_http, Command, DispatchError};
use praxis_lib::preview_bridge::CancelReason;
use praxis_lib::preview_control::{exec_script, ControlEvent};
use serde_json::{json, Value};

fn url(value: &str) -> tauri::Url {
    tauri::Url::parse(value).expect("test url")
}

#[test]
fn loopback_rule_accepts_localhost_with_port_and_rejects_https_and_external() {
    assert!(is_loopback_http(&url("http://localhost:3000/app")));
    assert!(is_loopback_http(&url("http://127.0.0.1:5173/")));
    assert!(!is_loopback_http(&url("https://localhost:3000/")));
    assert!(!is_loopback_http(&url("http://example.com/")));
}

#[test]
fn deadline_is_ten_seconds_for_every_command_without_a_timeout_argument() {
    let commands = [
        Command::Snapshot,
        Command::Navigate {
            url: "http://localhost:3000/".into(),
        },
        Command::Click {
            r#ref: "s1e3".into(),
        },
        Command::Fill {
            r#ref: "s1e3".into(),
            text: "hi".into(),
        },
        Command::PressKey {
            key: "Enter".into(),
            r#ref: None,
        },
    ];
    for cmd in commands {
        assert_eq!(deadline_for(&cmd), Duration::from_secs(10), "{:?}", cmd);
    }
    assert_eq!(
        deadline_for(&Command::Console { clear: false }),
        Duration::from_secs(10)
    );
}

/// 서버 데드라인은 툴 `timeout`보다 2초 길고 62초를 넘지 않는다(0058 D-12 · AC-10).
#[test]
fn wait_for_deadline_is_the_tool_timeout_plus_two_seconds() {
    let wait = |timeout_ms: u64| {
        deadline_for(&Command::WaitFor {
            text: Some("done".into()),
            gone: None,
            timeout_ms,
        })
    };
    assert_eq!(wait(30_000), Duration::from_secs(32));
    assert_eq!(wait(60_000), Duration::from_secs(62));
    assert_eq!(wait(10_000), Duration::from_secs(12));
}

/// 전선 형식은 exec.js와 나눠 가진 계약이다 — 여기가 그 표의 Rust 쪽 사본이다.
#[test]
fn action_commands_serialize_to_the_exec_wire_format() {
    assert_eq!(
        Command::Click {
            r#ref: "s1e3".into()
        }
        .to_exec_json(),
        json!({ "op": "click", "ref": "s1e3" })
    );
    assert_eq!(
        Command::Fill {
            r#ref: "s1e3".into(),
            text: "hello".into()
        }
        .to_exec_json(),
        json!({ "op": "fill", "ref": "s1e3", "text": "hello" })
    );
    assert_eq!(
        Command::PressKey {
            key: "Enter".into(),
            r#ref: Some("s1e3".into())
        }
        .to_exec_json(),
        json!({ "op": "press_key", "key": "Enter", "ref": "s1e3" })
    );
    // ref가 없으면 키만 간다 — 웹뷰가 포커스된 요소를 고른다.
    assert_eq!(
        Command::PressKey {
            key: "Tab".into(),
            r#ref: None
        }
        .to_exec_json(),
        json!({ "op": "press_key", "key": "Tab" })
    );
    // 없는 조건은 키째로 빠진다.
    assert_eq!(
        Command::WaitFor {
            text: None,
            gone: Some("로딩".into()),
            timeout_ms: 30_000
        }
        .to_exec_json(),
        json!({ "op": "wait_for", "gone": "로딩", "timeout_ms": 30_000 })
    );
    assert_eq!(
        Command::WaitFor {
            text: Some("완료".into()),
            gone: None,
            timeout_ms: 10_000
        }
        .to_exec_json(),
        json!({ "op": "wait_for", "text": "완료", "timeout_ms": 10_000 })
    );
    // 콘솔은 `clear`를 언제나 싣는다 — 웹뷰가 기본값을 따로 알 필요가 없다.
    assert_eq!(
        Command::Console { clear: true }.to_exec_json(),
        json!({ "op": "console", "clear": true })
    );
}

#[test]
fn control_event_payload_serializes_snake_case() {
    let event = ControlEvent {
        task_id: 42,
        active: true,
        op: "snapshot".into(),
        target: None,
        changed: None,
        url: "http://localhost:3000/".into(),
        controllable: true,
    };
    let value = serde_json::to_value(&event).expect("serialize");
    assert_eq!(
        value,
        json!({
            "task_id": 42,
            "active": true,
            "op": "snapshot",
            "target": Value::Null,
            "changed": Value::Null,
            "url": "http://localhost:3000/",
            "controllable": true
        })
    );
}

#[test]
fn exec_script_escapes_and_targets_exec_run() {
    let script = exec_script(7, "se\"ss", 3, "cmd-1", &Command::Snapshot);
    assert!(script.contains("__praxisPreviewExec.run("));
    assert!(script.contains(
        r#"if (!/^http:\/\/(localhost|127\.0\.0\.1)(:\d+)?$/.test(location.origin)) return;"#
    ));
    assert!(script.contains(r#"{"op":"snapshot"}"#));
    assert!(script.contains(r#""sessionId":"se\"ss""#));
    assert!(!script.contains("se\"ss"));
    assert!(script.contains(r#""commandId":"#));
}

#[test]
fn navigation_failure_is_named_apart_from_an_invalid_url() {
    assert_eq!(
        DispatchError::NavigationFailed("NAVIGATION_CANCELLED: Busy".into()).as_str(),
        "navigation_failed"
    );
    assert_eq!(
        DispatchError::InvalidUrl("x".into()).as_str(),
        "invalid_url"
    );
}

#[test]
fn a_dropped_waiter_is_a_take_over_only_when_the_user_took_over() {
    assert_eq!(
        error_for_dropped_waiter(Some(CancelReason::TakeOver)),
        DispatchError::TakenOver
    );
    assert_eq!(
        error_for_dropped_waiter(Some(CancelReason::Navigation)),
        DispatchError::Stale
    );
    assert_eq!(
        error_for_dropped_waiter(Some(CancelReason::Cancel)),
        DispatchError::Stale
    );
    assert_eq!(error_for_dropped_waiter(None), DispatchError::Stale);
}
