use praxis_lib::preview_bridge::{
    sha256_hex, PendingAction, PreviewBridge, RejectReason, ResultEnvelope, SessionRegistration,
    SubmitOutcome, MAX_RESULT_BYTES,
};

fn session() -> SessionRegistration {
    SessionRegistration::new(42, "designmode-42-1", "session-a", 7)
}

fn pending() -> PendingAction {
    PendingAction::new(42, "session-a", 7, "command-a")
}

fn result(body: String) -> ResultEnvelope {
    ResultEnvelope::new(
        42,
        "session-a",
        7,
        "command-a",
        sha256_hex(body.as_bytes()),
        body,
    )
}

fn json_payload(size: usize) -> String {
    format!("\"{}\"", "x".repeat(size - 2))
}

fn bridge_with_pending() -> PreviewBridge {
    let bridge = PreviewBridge::new();
    bridge.register(session()).unwrap();
    bridge.begin(pending()).unwrap();
    bridge
}

#[test]
fn result_accepts_exactly_512_kib_with_matching_sha() {
    let bridge = bridge_with_pending();
    let body = json_payload(MAX_RESULT_BYTES);
    assert_eq!(MAX_RESULT_BYTES, 512 * 1024);
    assert!(matches!(
        bridge.submit("designmode-42-1", result(body)),
        Ok(SubmitOutcome::Accepted { .. })
    ));

    let bridge = bridge_with_pending();
    let oversized = json_payload(MAX_RESULT_BYTES + 1);
    assert_eq!(
        bridge.submit("designmode-42-1", result(oversized)),
        Err(RejectReason::PayloadTooLarge)
    );

    let bridge = bridge_with_pending();
    let bad_sha = ResultEnvelope::new(42, "session-a", 7, "command-a", "00".repeat(32), "{}");
    assert_eq!(
        bridge.submit("designmode-42-1", bad_sha),
        Err(RejectReason::ShaMismatch)
    );
}

#[test]
fn wrong_identity_duplicate_and_stale_results_never_change_pending_state() {
    let bridge = bridge_with_pending();
    for (caller, reply, reason) in [
        (
            "other-webview",
            result("{}".into()),
            RejectReason::WebviewMismatch,
        ),
        (
            "designmode-42-1",
            ResultEnvelope::new(
                41,
                "session-a",
                7,
                "command-a",
                sha256_hex("{}".as_bytes()),
                "{}",
            ),
            RejectReason::TaskMismatch,
        ),
        (
            "designmode-42-1",
            ResultEnvelope::new(
                42,
                "wrong",
                7,
                "command-a",
                sha256_hex("{}".as_bytes()),
                "{}",
            ),
            RejectReason::SessionMismatch,
        ),
        (
            "designmode-42-1",
            ResultEnvelope::new(
                42,
                "session-a",
                6,
                "command-a",
                sha256_hex("{}".as_bytes()),
                "{}",
            ),
            RejectReason::GenerationMismatch,
        ),
        (
            "designmode-42-1",
            ResultEnvelope::new(
                42,
                "session-a",
                7,
                "wrong",
                sha256_hex("{}".as_bytes()),
                "{}",
            ),
            RejectReason::CommandMismatch,
        ),
    ] {
        assert_eq!(bridge.submit(caller, reply), Err(reason));
        assert!(bridge.has_pending(42));
    }

    assert!(matches!(
        bridge.submit("designmode-42-1", result("{}".into())),
        Ok(SubmitOutcome::Accepted { .. })
    ));
    assert_eq!(
        bridge.submit("designmode-42-1", result("{}".into())),
        Err(RejectReason::Duplicate)
    );
}

#[test]
fn navigation_and_close_clear_pending_and_fallback_assembly_within_contract() {
    let bridge = bridge_with_pending();
    bridge.begin_fallback_assembly(42, "command-a").unwrap();
    let started = std::time::Instant::now();
    bridge.on_navigation(42, 8);
    assert!(!bridge.has_pending(42));
    assert!(!bridge.has_fallback_assembly(42));

    bridge
        .register(SessionRegistration::new(
            42,
            "designmode-42-2",
            "session-b",
            8,
        ))
        .unwrap();
    bridge
        .begin(PendingAction::new(42, "session-b", 8, "command-b"))
        .unwrap();
    bridge.begin_fallback_assembly(42, "command-b").unwrap();
    bridge.close(42);
    assert!(bridge.is_empty(42));
    assert!(started.elapsed() < std::time::Duration::from_millis(250));
}
