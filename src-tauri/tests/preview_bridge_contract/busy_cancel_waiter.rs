use praxis_lib::preview_bridge::{
    sha256_hex, CancelReason, PendingAction, PreviewBridge, RejectReason, ResultEnvelope,
    SessionRegistration,
};
use tokio::sync::oneshot::error::TryRecvError;

fn session() -> SessionRegistration {
    SessionRegistration::new(42, "designmode-42-1", "session-a", 7)
}

fn pending(cmd: &str) -> PendingAction {
    PendingAction::new(42, "session-a", 7, cmd)
}

fn result(cmd: &str, body: &str) -> ResultEnvelope {
    ResultEnvelope::new(42, "session-a", 7, cmd, sha256_hex(body.as_bytes()), body)
}

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

#[test]
fn second_begin_while_pending_is_busy() {
    let bridge = PreviewBridge::new();
    bridge.register(session()).unwrap();
    let _rx = bridge.begin(pending(A)).unwrap();
    assert_eq!(bridge.begin(pending(B)).err(), Some(RejectReason::Busy));
}

/// AC-11 후반: 거절당한 두 번째 명령이 첫 명령의 대기자를 건드리지 않는다.
#[tokio::test]
async fn busy_rejection_leaves_the_first_waiter_intact() {
    let bridge = PreviewBridge::new();
    bridge.register(session()).unwrap();
    let rx = bridge.begin(pending(A)).unwrap();
    assert_eq!(bridge.begin(pending(B)).err(), Some(RejectReason::Busy));

    bridge
        .submit("designmode-42-1", result(A, "{\"ok\":true}"))
        .unwrap();

    assert_eq!(rx.await.unwrap(), "{\"ok\":true}");
}

/// 다른 명령 id로 온 취소는 진행 중인 명령을 죽이지 못한다.
#[test]
fn cancel_of_another_command_id_is_a_no_op() {
    let bridge = PreviewBridge::new();
    bridge.register(session()).unwrap();
    let _rx = bridge.begin(pending(A)).unwrap();

    bridge.cancel(42, B);

    assert!(bridge.has_pending(42));
    assert!(bridge.submit("designmode-42-1", result(A, "{}")).is_ok());
}

#[test]
fn begin_is_allowed_again_after_cancel() {
    let bridge = PreviewBridge::new();
    bridge.register(session()).unwrap();
    let _rx = bridge.begin(pending(A)).unwrap();
    bridge.cancel(42, A);

    assert!(bridge.begin(pending(B)).is_ok());
}

#[test]
fn cancel_clears_pending_and_late_submit_is_no_pending() {
    let bridge = PreviewBridge::new();
    bridge.register(session()).unwrap();
    let _rx = bridge.begin(pending(A)).unwrap();
    bridge.cancel(42, A);
    assert!(!bridge.has_pending(42));
    assert_eq!(
        bridge.submit("designmode-42-1", result(A, "{}")).err(),
        Some(RejectReason::NoPending)
    );
}

#[tokio::test]
async fn submit_delivers_body_to_waiter() {
    let bridge = PreviewBridge::new();
    bridge.register(session()).unwrap();
    let rx = bridge.begin(pending(A)).unwrap();
    bridge
        .submit("designmode-42-1", result(A, "{\"ok\":true}"))
        .unwrap();
    assert_eq!(rx.await.unwrap(), "{\"ok\":true}");
}

#[test]
fn take_over_rejects_begin_until_release() {
    let bridge = PreviewBridge::new();
    bridge.register(session()).unwrap();
    bridge.take_over(42);
    assert_eq!(
        bridge.begin(pending(A)).err(),
        Some(RejectReason::TakenOver)
    );
    bridge.release(42);
    assert!(bridge.begin(pending(A)).is_ok());
}

#[test]
fn take_over_cancels_in_flight_command() {
    let bridge = PreviewBridge::new();
    bridge.register(session()).unwrap();
    let mut rx = bridge.begin(pending(A)).unwrap();
    bridge.take_over(42);
    assert!(!bridge.has_pending(42));
    // sender dropped → 대기자가 취소를 안다
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Closed)));
    assert_eq!(bridge.take_cancel_reason(42), Some(CancelReason::TakeOver));
}

#[test]
fn navigation_generation_bump_drops_waiter() {
    let bridge = PreviewBridge::new();
    bridge.register(session()).unwrap();
    let mut rx = bridge.begin(pending(A)).unwrap();
    bridge.on_navigation(42, 8);
    assert!(matches!(rx.try_recv(), Err(TryRecvError::Closed)));
    assert_eq!(
        bridge.take_cancel_reason(42),
        Some(CancelReason::Navigation)
    );
}

/// 이유는 한 번만 읽힌다 — 다음 명령이 지난 취소를 자기 것으로 오해하면 안 된다.
#[test]
fn cancel_reason_is_consumed_by_the_first_reader() {
    let bridge = PreviewBridge::new();
    bridge.register(session()).unwrap();
    let _rx = bridge.begin(pending(A)).unwrap();
    bridge.cancel(42, A);

    assert_eq!(bridge.take_cancel_reason(42), Some(CancelReason::Cancel));
    assert_eq!(bridge.take_cancel_reason(42), None);

    // 새 명령은 지난 취소를 물려받지 않는다.
    let _rx = bridge.begin(pending(A)).unwrap();
    assert_eq!(bridge.take_cancel_reason(42), None);
}
