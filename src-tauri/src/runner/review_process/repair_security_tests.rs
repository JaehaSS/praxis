use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use super::repair_test_support::{fixture, quarantined_receipt};
use super::{repair, ReviewOperation, ReviewPhase, ReviewProcessRepairStatus};
use crate::runner::process_identity::ProcessTerminationOutcome;

#[tokio::test]
async fn legacy_oversized_process_group_id_never_reaches_the_terminator() {
    let fixture = fixture("invalid-pgid").await;
    let receipt = quarantined_receipt(&fixture, 501).await;
    sqlx::query("DROP TRIGGER review_process_receipts_immutable")
        .execute(&fixture.pool)
        .await
        .unwrap();
    sqlx::query("UPDATE review_process_receipts SET pgid = ? WHERE id = ?")
        .bind(i64::from(i32::MAX) + 1)
        .bind(receipt.id)
        .execute(&fixture.pool)
        .await
        .unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();

    let result = repair::reconcile_with(&fixture.pool, receipt.id, 7, move |_, _| async move {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(ProcessTerminationOutcome::Absent)
    })
    .await
    .unwrap();

    assert_eq!(result.status, ReviewProcessRepairStatus::StillQuarantined);
    assert_eq!(result.reason.as_deref(), Some("invalid_process_group_id"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn quarantine_listing_is_ordered_and_withholds_raw_identity_and_detail() {
    let fixture = fixture("list").await;
    let verify = quarantined_receipt(&fixture, 502).await;
    let challenge_task_id = crate::db::insert_task(
        &fixture.pool,
        "/repo",
        "branch-2",
        "main",
        "/wt-2",
        "challenge review",
        None,
        None,
        "terminal",
        3,
    )
    .await
    .unwrap();
    let challenge = super::register(
        &fixture.pool,
        challenge_task_id,
        ReviewOperation::Challenge,
        ReviewPhase::Reviewer,
        503,
        &"b".repeat(64),
        4,
    )
    .await
    .unwrap();
    super::quarantine(
        &fixture.pool,
        challenge_task_id,
        ReviewOperation::Challenge,
        challenge.id,
        "raw internal database detail",
        5,
    )
    .await
    .unwrap();

    let quarantines = repair::list_quarantined(&fixture.pool).await.unwrap();
    let encoded = serde_json::to_string(&quarantines).unwrap();

    assert_eq!(
        quarantines
            .iter()
            .map(|item| item.receipt_id)
            .collect::<Vec<_>>(),
        vec![challenge.id, verify.id]
    );
    assert!(!encoded.contains(&"a".repeat(64)));
    assert!(!encoded.contains(&"b".repeat(64)));
    assert!(!encoded.contains("raw internal database detail"));
    assert!(encoded.contains("verification_uncertain"));
}

#[tokio::test]
async fn process_probe_error_stays_quarantined_without_losing_machine_reason() {
    let fixture = fixture("probe-error").await;
    let receipt = quarantined_receipt(&fixture, 504).await;

    let result = repair::reconcile_with(&fixture.pool, receipt.id, 8, |_, _| async {
        anyhow::bail!("injected process probe failure")
    })
    .await
    .unwrap();

    assert_eq!(result.status, ReviewProcessRepairStatus::StillQuarantined);
    assert_eq!(result.reason.as_deref(), Some("verification_uncertain"));
    let quarantines = repair::list_quarantined(&fixture.pool).await.unwrap();
    assert_eq!(quarantines[0].reason, "verification_uncertain");
    assert!(!serde_json::to_string(&quarantines)
        .unwrap()
        .contains("injected process probe failure"));
}
