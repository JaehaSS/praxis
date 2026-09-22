#![cfg(unix)]

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::convo::{run_turn, ConvoEvent, Vendor};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU32, Ordering};

static NEXT_STUB: AtomicU32 = AtomicU32::new(0);

fn run_stub(lines: &[&str]) -> Vec<ConvoEvent> {
    let id = NEXT_STUB.fetch_add(1, Ordering::SeqCst);
    let directory =
        temp_root::dir().join(format!("praxis-subagent-guard-{}-{id}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let binary = directory.join("claude-stub");
    let quoted_lines = lines
        .iter()
        .map(|line| format!("'{line}'"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut file = std::fs::File::create(&binary).unwrap();
    write!(file, "#!/bin/sh\nprintf '%s\\n' {quoted_lines}\n").unwrap();
    let mut permissions = file.metadata().unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&binary, permissions).unwrap();

    let mut events = Vec::new();
    run_turn(
        directory.to_str().unwrap(),
        "구현해줘",
        None,
        5,
        Vendor::Claude,
        binary.to_str().unwrap(),
        None,
        |_| {},
        |event| events.push(event),
    )
    .unwrap();
    std::fs::remove_dir_all(directory).ok();
    events
}

fn base_lines() -> [&'static str; 3] {
    [
        r#"{"type":"system","subtype":"init","session_id":"session"}"#,
        r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"agent-tool","name":"Agent","input":{"description":"구현 Worker"}}]}}"#,
        r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"agent-tool","content":"Async agent launched successfully. agentId: internal","is_error":false}]}}"#,
    ]
}

#[test]
fn unfinished_async_agent_turn_is_not_reported_as_success() {
    let mut lines = base_lines().to_vec();
    lines.push(
        r#"{"type":"result","subtype":"success","is_error":false,"result":"나중에 검증하겠습니다","session_id":"session"}"#,
    );
    let events = run_stub(&lines);

    assert!(!events.iter().any(|event| matches!(
        event,
        ConvoEvent::ToolResult { summary, .. } if summary.contains("agentId")
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ConvoEvent::Result {
            is_error: true,
            text,
            ..
        } if text.contains("완료 회수하기 전에 턴을 종료")
    )));
}

#[test]
fn completion_notification_allows_success_and_finishes_worker() {
    let mut lines = base_lines().to_vec();
    lines.push(
        r#"{"type":"user","message":{"role":"user","content":"<task-notification>\n<tool-use-id>agent-tool</tool-use-id>\n<status>completed</status>\n</task-notification>"}}"#,
    );
    lines.push(
        r#"{"type":"result","subtype":"success","is_error":false,"result":"검증 완료","session_id":"session"}"#,
    );
    let events = run_stub(&lines);

    assert!(events.iter().any(|event| matches!(
        event,
        ConvoEvent::ToolResult {
            is_error: false,
            tool_use_id: Some(id),
            ..
        } if id == "agent-tool"
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        ConvoEvent::Result {
            is_error: false,
            ..
        }
    )));
}
