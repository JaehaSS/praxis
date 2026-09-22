const COMMANDS_SOURCE: &str = include_str!("../commands.rs");

#[test]
fn conversation_entry_points_pin_their_input_origins() {
    assert_origin(
        "async fn spawn_task_agent_inner(",
        "pub async fn task_create(",
        "ConversationInputOrigin::InitialTask",
    );
    assert_origin(
        "pub async fn convo_send(",
        "pub async fn annotations_resend(",
        "ConversationInputOrigin::UserMessage",
    );
    assert_origin(
        "pub async fn annotations_resend(",
        "pub async fn convo_interrupt(",
        "ConversationInputOrigin::AnnotationResend",
    );
    assert_origin(
        "pub(crate) async fn remote_review_retry(",
        "pub(crate) async fn approve_pending_task(",
        "ConversationInputOrigin::RemoteReviewRetry",
    );
}

#[test]
fn task_write_persists_followup_before_forwarding_stdin() {
    let source = function_source("pub async fn task_write(", "pub async fn task_pty_replay(");
    let persisted = source
        .find("record_followup_before_forward")
        .expect("task_write must persist the follow-up");
    let forwarded = source
        .find("session.write")
        .expect("task_write must forward to the active session");

    assert!(persisted < forwarded);
}

#[test]
fn conversation_provenance_waits_for_a_tool_free_result() {
    let source = function_source(
        "async fn start_convo_turn(",
        "/// 대화 모드(Phase 2) 한 턴",
    )
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ");
    let pending = source
        .find("begin_conversation_provenance")
        .expect("conversation must begin incomplete");
    let tools = source
        .find("ConvoEvent::ToolResult")
        .expect("tool results must taint provenance");
    let codex_other = source
        .find("Other if vendor == crate::convo::Vendor::Codex")
        .expect("untrackable Codex events must taint provenance");
    let complete = source
        .find("complete_conversation_provenance")
        .expect("tool-free successful result must complete provenance");
    let question = source
        .find("ConvoEvent::Interaction")
        .expect("question answers have no implicit analysis consent");

    assert!(pending < tools && tools < complete);
    assert!(pending < codex_other && codex_other < complete);
    assert!(pending < question && question < complete);
}

#[test]
fn local_composer_marks_a_failed_pty_write_not_delivered() {
    let source = function_source(
        "pub async fn knowledge_vault_local_composer_send(",
        "/// 작업 PTY 스크롤백 replay",
    );
    let receipt = source
        .find("write_with_receipt")
        .expect("PTY write receipt boundary");
    let write = source.find("session.write(bytes)").expect("PTY write result");

    assert!(receipt < write);
}

fn assert_origin(start: &str, end: &str, expected: &str) {
    let source = function_source(start, end);
    assert!(
        source.contains(expected),
        "{start} must use {expected}, source: {source}"
    );
}

fn function_source(start: &str, end: &str) -> &'static str {
    let start_index = COMMANDS_SOURCE.find(start).expect("function start");
    let tail = &COMMANDS_SOURCE[start_index..];
    let end_index = tail.find(end).expect("next function");
    &tail[..end_index]
}
