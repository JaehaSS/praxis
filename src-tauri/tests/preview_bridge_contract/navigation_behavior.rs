use praxis_lib::preview_bridge::{PendingAction, PreviewBridge, SessionRegistration};

#[test]
fn agent_navigation_preserves_takeover_and_pending_actions() {
    use praxis_lib::preview_bridge::RejectReason;
    let bridge = PreviewBridge::new();
    bridge
        .register(SessionRegistration::new(42, "preview", "session", 7))
        .unwrap();
    bridge.take_over(42);
    assert_eq!(
        bridge.prepare_agent_navigation(42),
        Err(RejectReason::TakenOver)
    );
    assert_eq!(bridge.session_of(42).unwrap().1, 7);
    assert!(bridge.is_taken_over(42));
    bridge.release(42);
    let mut waiter = bridge
        .begin(PendingAction::new(42, "session", 7, "action"))
        .unwrap();
    assert_eq!(bridge.prepare_agent_navigation(42), Err(RejectReason::Busy));
    assert!(bridge.has_pending(42));
    assert_eq!(bridge.session_of(42).unwrap().1, 7);
    assert_eq!(
        waiter.try_recv(),
        Err(tokio::sync::oneshot::error::TryRecvError::Empty)
    );
    bridge.cancel(42, "action");
    assert_eq!(bridge.prepare_agent_navigation(42), Ok(8));
    assert_eq!(bridge.observe_navigation(42), Ok(8));
    assert!(bridge.prepare_agent_navigation(99).is_err());
}

#[test]
fn prepared_navigation_is_cancelled_then_spontaneous_navigation_invalidates_new_pending() {
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
    bridge.begin_fallback_assembly(42, "command-a").unwrap();

    let generation = bridge.prepare_navigation(42).unwrap();
    assert_eq!(generation, 8);
    assert!(!bridge.has_pending(42));
    assert!(!bridge.has_fallback_assembly(42));
    bridge.cancel_prepared_navigation(42).unwrap();
    bridge
        .begin(PendingAction::new(42, "session-a", 8, "command-b"))
        .unwrap();

    let observed = bridge.observe_navigation(42).unwrap();
    assert_eq!(observed, 9);
    assert!(!bridge.has_pending(42));
    bridge
        .begin(PendingAction::new(42, "session-a", 9, "command-c"))
        .unwrap();
}
