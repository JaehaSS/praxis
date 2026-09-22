use super::*;

mod database;

#[test]
fn summarizes_completed_and_failed_turn_activity_without_counting_idle_gaps() {
    let events = [
        (0, r#"{"kind":"user","text":"first"}"#),
        (1, r#"{"kind":"user_expanded","text":"expanded"}"#),
        (3, r#"{"kind":"tool_use","name":"Read"}"#),
        (4, r#"{"kind":"tool_result","is_error":false}"#),
        (
            5,
            r#"{"kind":"result","is_error":false,"tokens_in":100,"tokens_out":10,"cost_usd":0.2}"#,
        ),
        (20, r#"{"kind":"user","text":"second"}"#),
        (21, r#"{"kind":"tool_use","name":"Bash"}"#),
        (
            25,
            r#"{"kind":"result","is_error":true,"tokens_in":50,"tokens_out":5,"cost_usd":0.1}"#,
        ),
    ];

    let metrics = summarize_events(events);

    assert_eq!(metrics.active_seconds, 10);
    assert_eq!(metrics.user_turns, 2);
    assert_eq!(metrics.completed_turns, 1);
    assert_eq!(metrics.failed_turns, 1);
    assert_eq!(metrics.tool_calls, 2);
    assert_eq!(metrics.tool_errors, 0);
    assert_eq!(metrics.tokens_in, 150);
    assert_eq!(metrics.tokens_out, 15);
    assert!((metrics.cost_usd - 0.3).abs() < f64::EPSILON);
}

#[test]
fn ignores_malformed_and_unfinished_activity_but_counts_tool_errors() {
    let events = [
        (1, "not json"),
        (2, r#"{"kind":"tool_result","is_error":true}"#),
        (10, r#"{"kind":"user","text":"unfinished"}"#),
    ];

    let metrics = summarize_events(events);

    assert_eq!(metrics.active_seconds, 0);
    assert_eq!(metrics.user_turns, 1);
    assert_eq!(metrics.completed_turns, 0);
    assert_eq!(metrics.failed_turns, 0);
    assert_eq!(metrics.tool_errors, 1);
}

#[test]
fn uses_the_latest_user_event_when_an_unexpected_second_turn_starts() {
    let events = [
        (10, r#"{"kind":"user","text":"stale"}"#),
        (15, r#"{"kind":"user","text":"latest"}"#),
        (20, r#"{"kind":"result","is_error":false}"#),
    ];

    let metrics = summarize_events(events);

    assert_eq!(metrics.active_seconds, 5);
    assert_eq!(metrics.user_turns, 2);
    assert_eq!(metrics.completed_turns, 1);
}

#[test]
fn combines_requested_and_resolved_model_snapshots_without_clearing_either() {
    let events = [
        (
            1,
            r#"{"kind":"model_snapshot","requested":"opus","source":"invocation"}"#,
        ),
        (
            2,
            r#"{"kind":"model_snapshot","resolved":"claude-opus-4-8","source":"claude_stream"}"#,
        ),
        (3, r#"{"kind":"model_snapshot","source":"invocation"}"#),
    ];

    let metrics = summarize_events(events);

    assert_eq!(metrics.requested_model.as_deref(), Some("opus"));
    assert_eq!(metrics.resolved_model.as_deref(), Some("claude-opus-4-8"));
}

#[test]
fn saturates_corrupt_extreme_timestamps_tokens_and_cost_instead_of_panicking() {
    let events = [
        (i64::MIN, r#"{"kind":"user","text":"extreme"}"#),
        (
            i64::MAX,
            r#"{"kind":"result","is_error":false,"tokens_in":9223372036854775807,"tokens_out":9223372036854775807,"cost_usd":1e308}"#,
        ),
        (
            i64::MAX,
            r#"{"kind":"result","is_error":false,"tokens_in":9223372036854775807,"tokens_out":9223372036854775807,"cost_usd":1e308}"#,
        ),
    ];

    let metrics = summarize_events(events);

    assert_eq!(metrics.active_seconds, i64::MAX);
    assert_eq!(metrics.tokens_in, i64::MAX);
    assert_eq!(metrics.tokens_out, i64::MAX);
    assert_eq!(metrics.cost_usd, f64::MAX);
}
