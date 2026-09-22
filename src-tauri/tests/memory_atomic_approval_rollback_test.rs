#[path = "support/temp_root.rs"]
mod temp_root;

#[path = "support/memory_atomic_approval.rs"]
mod approval_support;

use approval_support::{count_where, ApprovalFixture};
use praxis_lib::memory::{self, knowledge_status, ConfirmApprovalFailureKind};

#[tokio::test]
async fn receipt_failure_rolls_back_every_approval_mutation() {
    let fixture = ApprovalFixture::candidate("rollback").await;
    sqlx::query(
        "CREATE TRIGGER abort_atomic_approval_receipt \
         BEFORE INSERT ON memory_approval_receipts BEGIN \
         SELECT RAISE(ABORT, 'forced atomic approval failure'); END",
    )
    .execute(&fixture.pool)
    .await
    .unwrap();

    let error = memory::confirm_and_approve(&fixture.pool, fixture.memory_id, 1, 200)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ConfirmApprovalFailureKind::Storage);
    assert_unchanged(&fixture, knowledge_status::CANDIDATE, 0).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn invalid_existing_evidence_blocks_without_fresh_confirmation() {
    let fixture = ApprovalFixture::candidate("invalid").await;
    memory::add_user_confirmation(&fixture.pool, fixture.memory_id, 100, Some(150))
        .await
        .unwrap();

    let error = memory::confirm_and_approve(&fixture.pool, fixture.memory_id, 1, 200)
        .await
        .unwrap_err();
    assert_eq!(error.kind(), ConfirmApprovalFailureKind::Invalid);
    assert_unchanged(&fixture, knowledge_status::CANDIDATE, 1).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn version_or_state_conflict_does_not_mutate_the_candidate() {
    let fixture = ApprovalFixture::candidate("conflict").await;
    let version_error = memory::confirm_and_approve(&fixture.pool, fixture.memory_id, 2, 200)
        .await
        .unwrap_err();
    assert_eq!(version_error.kind(), ConfirmApprovalFailureKind::Conflict);
    fixture.set_status(memory::knowledge_status::ARCHIVED).await;
    let state_error = memory::confirm_and_approve(&fixture.pool, fixture.memory_id, 1, 201)
        .await
        .unwrap_err();
    assert_eq!(state_error.kind(), ConfirmApprovalFailureKind::Conflict);
    assert_unchanged(&fixture, knowledge_status::ARCHIVED, 0).await;
    fixture.cleanup().await;
}

async fn assert_unchanged(fixture: &ApprovalFixture, expected_status: &str, evidence_count: i64) {
    let status: String = sqlx::query_scalar("SELECT status FROM memories WHERE id = ?")
        .bind(fixture.memory_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(status, expected_status);
    assert_eq!(
        count_where(
            &fixture.pool,
            "SELECT COUNT(*) FROM memory_evidence WHERE memory_id = ?",
            fixture.memory_id,
        )
        .await,
        evidence_count
    );
    for sql in [
        "SELECT COUNT(*) FROM memory_evidence_checks WHERE memory_id = ?",
        "SELECT COUNT(*) FROM memory_approval_receipts WHERE memory_id = ?",
        "SELECT COUNT(*) FROM memory_events WHERE memory_id = ? \
         AND action IN ('review_submitted', 'approved')",
    ] {
        assert_eq!(count_where(&fixture.pool, sql, fixture.memory_id).await, 0);
    }
}
