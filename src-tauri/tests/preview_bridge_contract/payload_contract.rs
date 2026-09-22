use praxis_lib::preview_bridge::{
    sha256_hex, PendingAction, PreviewBridge, RejectReason, ResultEnvelope, SessionRegistration,
    SubmitOutcome, MAX_RESULT_BYTES,
};

fn json_payload(size: usize) -> String {
    format!("\"{}\"", "x".repeat(size - 2))
}

fn envelope(payload: String) -> ResultEnvelope {
    ResultEnvelope::new(
        42,
        "session-a",
        7,
        "command-a",
        sha256_hex(payload.as_bytes()),
        payload,
    )
}

#[test]
fn result_body_is_valid_utf8_json_at_exact_payload_limit() {
    let payload = json_payload(MAX_RESULT_BYTES);
    let result = envelope(payload.clone());

    assert_eq!(payload.len(), 512 * 1024);
    assert!(serde_json::from_str::<serde_json::Value>(&payload).is_ok());
    assert_eq!(result.sha256, sha256_hex(payload.as_bytes()));
}

/// preview_agent.test.js의 벡터와 같은 값이다 — 한쪽만 바뀌면 여기서 걸린다.
#[test]
fn sha256_hex_matches_the_javascript_vectors() {
    assert_eq!(
        sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );
    assert_eq!(
        sha256_hex("한글 미리보기".as_bytes()),
        "a06b140bb82cd2207b0ae7dc6c9f98e63b604d8a70bbc029e68b50499fa5f043"
    );
}

#[test]
fn result_wire_contract_keeps_json_payload_as_utf8_not_byte_array() {
    let result = envelope(json_payload(32));
    let wire = serde_json::to_value(result).unwrap();

    assert!(
        wire["body"].is_string(),
        "remote IPC must receive final JSON payload text, never Vec<u8> numeric array"
    );
}

#[test]
fn payload_limit_is_checked_on_final_json_payload_bytes() {
    let exact = json_payload(MAX_RESULT_BYTES);
    let oversized = json_payload(MAX_RESULT_BYTES + 1);

    assert_eq!(exact.len(), MAX_RESULT_BYTES);
    assert_eq!(oversized.len(), MAX_RESULT_BYTES + 1);
    assert!(serde_json::from_str::<serde_json::Value>(&oversized).is_ok());

    let bridge = PreviewBridge::new();
    bridge
        .register(SessionRegistration::new(
            42,
            "designmode-42-1",
            "session-a",
            7,
        ))
        .unwrap();
    bridge
        .begin(PendingAction::new(42, "session-a", 7, "command-a"))
        .unwrap();
    assert!(matches!(
        bridge.submit("designmode-42-1", envelope(exact)),
        Ok(SubmitOutcome::Accepted { .. })
    ));

    let bridge = PreviewBridge::new();
    bridge
        .register(SessionRegistration::new(
            42,
            "designmode-42-1",
            "session-a",
            7,
        ))
        .unwrap();
    bridge
        .begin(PendingAction::new(42, "session-a", 7, "command-a"))
        .unwrap();
    assert_eq!(
        bridge.submit("designmode-42-1", envelope(oversized)),
        Err(RejectReason::PayloadTooLarge)
    );
}

#[test]
fn invalid_json_is_rejected_without_replacing_the_supplied_sha() {
    let bridge = PreviewBridge::new();
    bridge
        .register(SessionRegistration::new(
            42,
            "designmode-42-1",
            "session-a",
            7,
        ))
        .unwrap();
    bridge
        .begin(PendingAction::new(42, "session-a", 7, "command-a"))
        .unwrap();
    let invalid = ResultEnvelope::new(42, "session-a", 7, "command-a", "00".repeat(32), "not-json");

    assert_eq!(invalid.sha256, "00".repeat(32));
    assert_eq!(
        bridge.submit("designmode-42-1", invalid),
        Err(RejectReason::InvalidJson)
    );
}
