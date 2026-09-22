use super::repair_test_support::{fixture, quarantined_receipt};
use super::{repair, ReviewOperation, ReviewProcessRepairError, ReviewProcessState};
use crate::runner::process_identity::ProcessTerminationOutcome;

#[tokio::test]
async fn failed_resolution_audit_rolls_back_lease_deletion() {
    let fixture = fixture("resolve-rollback").await;
    let receipt = quarantined_receipt(&fixture, 601).await;
    reject_event(&fixture.pool, "review_process_operator_resolved").await;

    let result = repair::reconcile_with(&fixture.pool, receipt.id, 8, |_, _| async {
        Ok(ProcessTerminationOutcome::Absent)
    })
    .await;

    assert!(matches!(result, Err(ReviewProcessRepairError::Internal(_))));
    let lease = super::lease(&fixture.pool, fixture.task_id, ReviewOperation::Verify)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.state, ReviewProcessState::Quarantined);
    assert_eq!(lease.updated_at, 3);
}

#[tokio::test]
async fn failed_defer_audit_rolls_back_reason_and_timestamp() {
    let fixture = fixture("defer-rollback").await;
    let receipt = quarantined_receipt(&fixture, 602).await;
    reject_event(&fixture.pool, "review_process_repair_deferred").await;

    let result = repair::reconcile_with(&fixture.pool, receipt.id, 9, |_, _| async {
        Ok(ProcessTerminationOutcome::IdentityMismatch)
    })
    .await;

    assert!(matches!(result, Err(ReviewProcessRepairError::Internal(_))));
    let row: (Option<String>, i64) =
        sqlx::query_as("SELECT reason, updated_at FROM review_process_leases WHERE receipt_id = ?")
            .bind(receipt.id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(row, (None, 3));
}

async fn reject_event(pool: &sqlx::SqlitePool, kind: &str) {
    sqlx::query(&format!(
        "CREATE TRIGGER reject_repair_event BEFORE INSERT ON runner_events \
         WHEN NEW.kind = '{kind}' BEGIN SELECT RAISE(ABORT, 'audit rejected'); END"
    ))
    .execute(pool)
    .await
    .unwrap();
}
