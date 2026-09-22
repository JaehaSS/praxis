use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use super::repair_test_support::{fixture, latest_event, quarantined_receipt, Fixture};
use super::{repair, ReviewOperation, ReviewPhase, ReviewProcessRepairStatus, ReviewProcessState};
use crate::runner::process_identity::ProcessTerminationOutcome;

#[tokio::test]
async fn absent_operator_repair_resolves_quarantine_and_releases_fence() {
    let fixture = fixture("absent").await;
    let receipt = quarantined_receipt(&fixture, 401).await;

    let result = repair::reconcile_with(&fixture.pool, receipt.id, 5, |_, _| async {
        Ok(ProcessTerminationOutcome::Absent)
    })
    .await
    .unwrap();

    assert_eq!(result.status, ReviewProcessRepairStatus::ResolvedAbsent);
    assert!(
        super::lease(&fixture.pool, fixture.task_id, ReviewOperation::Verify)
            .await
            .unwrap()
            .is_none()
    );
    super::assert_task_unfenced(&fixture.pool, fixture.task_id)
        .await
        .unwrap();
    assert_eq!(
        latest_event(&fixture.pool, fixture.task_id).await,
        (
            "review_process_operator_resolved".to_string(),
            format!(
                "review process receipt {}; outcome=resolved_absent",
                receipt.id
            )
        )
    );
}

#[tokio::test]
async fn identity_mismatch_keeps_quarantine_and_updates_audit_atomically() {
    let fixture = fixture("mismatch").await;
    let receipt = quarantined_receipt(&fixture, 402).await;

    let result = repair::reconcile_with(&fixture.pool, receipt.id, 6, |_, _| async {
        Ok(ProcessTerminationOutcome::IdentityMismatch)
    })
    .await
    .unwrap();

    assert_eq!(result.status, ReviewProcessRepairStatus::StillQuarantined);
    assert_eq!(result.reason.as_deref(), Some("identity_mismatch"));
    let lease = super::lease(&fixture.pool, fixture.task_id, ReviewOperation::Verify)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.state, ReviewProcessState::Quarantined);
    assert_eq!(lease.updated_at, 6);
    assert_eq!(
        latest_event(&fixture.pool, fixture.task_id).await,
        (
            "review_process_repair_deferred".to_string(),
            format!(
                "review process receipt {}; reason=identity_mismatch",
                receipt.id
            )
        )
    );
    let listed = repair::list_quarantined(&fixture.pool).await.unwrap();
    assert_eq!(listed[0].reason, "identity_mismatch");
}

#[tokio::test]
async fn confirmed_termination_resolves_with_a_distinct_operator_outcome() {
    let fixture = fixture("terminated").await;
    let receipt = quarantined_receipt(&fixture, 406).await;

    let result = repair::reconcile_with(&fixture.pool, receipt.id, 6, |_, _| async {
        Ok(ProcessTerminationOutcome::Terminated)
    })
    .await
    .unwrap();

    assert_eq!(result.status, ReviewProcessRepairStatus::ResolvedTerminated);
    assert_eq!(
        latest_event(&fixture.pool, fixture.task_id).await.1,
        format!(
            "review process receipt {}; outcome=resolved_terminated",
            receipt.id
        )
    );
}

#[tokio::test]
async fn active_and_stale_receipts_never_reach_the_process_terminator() {
    let fixture = fixture("stale").await;
    let first = super::register(
        &fixture.pool,
        fixture.task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        403,
        &"a".repeat(64),
        2,
    )
    .await
    .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(matches!(
        repair_with_counter(&fixture, first.id, &calls).await,
        Err(super::ReviewProcessRepairError::NotQuarantined)
    ));
    assert!(super::resolve(
        &fixture.pool,
        fixture.task_id,
        ReviewOperation::Verify,
        first.id,
        3,
        "review_process_completed",
    )
    .await
    .unwrap());
    let second = quarantined_receipt(&fixture, 404).await;
    assert!(matches!(
        repair_with_counter(&fixture, first.id, &calls).await,
        Err(super::ReviewProcessRepairError::StaleReceipt)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        super::lease(&fixture.pool, fixture.task_id, ReviewOperation::Verify)
            .await
            .unwrap()
            .unwrap()
            .receipt_id,
        second.id
    );
}

#[tokio::test]
async fn resolved_receipt_retry_is_idempotent_when_no_new_lease_exists() {
    let fixture = fixture("idempotent").await;
    let receipt = quarantined_receipt(&fixture, 405).await;
    let absent = |_: i64, _: String| async { Ok(ProcessTerminationOutcome::Absent) };
    repair::reconcile_with(&fixture.pool, receipt.id, 5, absent)
        .await
        .unwrap();

    let result = repair::reconcile_with(&fixture.pool, receipt.id, 6, |_, _| async {
        panic!("idempotent retry must not inspect the OS")
    })
    .await
    .unwrap();

    assert_eq!(result.status, ReviewProcessRepairStatus::AlreadyResolved);
}

async fn repair_with_counter(
    fixture: &Fixture,
    receipt_id: i64,
    calls: &Arc<AtomicUsize>,
) -> Result<super::ReviewProcessRepairResult, super::ReviewProcessRepairError> {
    let calls = calls.clone();
    repair::reconcile_with(&fixture.pool, receipt_id, 9, move |_, _| async move {
        calls.fetch_add(1, Ordering::SeqCst);
        Ok(ProcessTerminationOutcome::Absent)
    })
    .await
}
