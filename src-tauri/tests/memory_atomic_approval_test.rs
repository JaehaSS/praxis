#[path = "support/temp_root.rs"]
mod temp_root;

#[path = "support/memory_atomic_approval.rs"]
mod approval_support;

use approval_support::{count_where, ApprovalFixture};
use praxis_lib::memory::{self, knowledge_status};

#[tokio::test]
async fn candidate_approval_and_lost_response_retry_are_idempotent() {
    let fixture = ApprovalFixture::candidate("idempotent").await;
    let first = memory::confirm_and_approve(&fixture.pool, fixture.memory_id, 1, 200)
        .await
        .unwrap();
    assert_eq!(first.version, 1);
    assert!(!first.already_approved);

    let retry = memory::confirm_and_approve(&fixture.pool, fixture.memory_id, 1, 201)
        .await
        .unwrap();
    assert_eq!(retry.receipt_id, first.receipt_id);
    assert!(retry.already_approved);
    assert_approval_counts(&fixture, 1, 1, 1, 1).await;
    fixture.cleanup().await;
}

#[tokio::test]
async fn every_human_review_state_can_use_the_atomic_contract() {
    for (label, status) in [
        ("candidate", knowledge_status::CANDIDATE),
        ("pending", knowledge_status::PENDING_REVIEW),
        ("stale", knowledge_status::STALE),
        ("legacy", knowledge_status::LEGACY_UNVERIFIED),
    ] {
        let fixture = ApprovalFixture::candidate(label).await;
        fixture.set_status(status).await;
        memory::confirm_and_approve(&fixture.pool, fixture.memory_id, 1, 200)
            .await
            .unwrap();
        let review_events = if status == knowledge_status::PENDING_REVIEW {
            0
        } else {
            1
        };
        assert_approval_counts(&fixture, 1, review_events, 1, 1).await;
        fixture.cleanup().await;
    }
}

#[tokio::test]
async fn concurrent_approval_creates_one_receipt_and_one_confirmation() {
    let fixture = ApprovalFixture::candidate("concurrent").await;
    let left_pool = fixture.pool.clone();
    let right_pool = fixture.pool.clone();
    let id = fixture.memory_id;
    let (left, right) = tokio::join!(
        memory::confirm_and_approve(&left_pool, id, 1, 200),
        memory::confirm_and_approve(&right_pool, id, 1, 201)
    );
    let left = left.unwrap();
    let right = right.unwrap();

    assert_eq!(left.receipt_id, right.receipt_id);
    assert_ne!(left.already_approved, right.already_approved);
    assert_approval_counts(&fixture, 1, 1, 1, 1).await;
    fixture.cleanup().await;
}

async fn assert_approval_counts(
    fixture: &ApprovalFixture,
    confirmations: i64,
    review_events: i64,
    approved_events: i64,
    receipts: i64,
) {
    let status: String = sqlx::query_scalar("SELECT status FROM memories WHERE id = ?")
        .bind(fixture.memory_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap();
    assert_eq!(status, knowledge_status::VERIFIED);
    assert_eq!(
        count_where(
            &fixture.pool,
            "SELECT COUNT(*) FROM memory_evidence \
             WHERE memory_id = ? AND kind = 'user_confirmation'",
            fixture.memory_id,
        )
        .await,
        confirmations
    );
    assert_eq!(
        event_count(fixture, "review_submitted").await,
        review_events
    );
    assert_eq!(event_count(fixture, "approved").await, approved_events);
    assert_eq!(
        count_where(
            &fixture.pool,
            "SELECT COUNT(*) FROM memory_approval_receipts WHERE memory_id = ?",
            fixture.memory_id,
        )
        .await,
        receipts
    );
    assert_receipt_links_confirmation(fixture).await;
}

async fn assert_receipt_links_confirmation(fixture: &ApprovalFixture) {
    let check_ids_json: String = sqlx::query_scalar(
        "SELECT source_check_ids_json FROM memory_approval_receipts WHERE memory_id = ?",
    )
    .bind(fixture.memory_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    let check_ids: Vec<i64> = serde_json::from_str(&check_ids_json).unwrap();
    let confirmation_check_id: i64 = sqlx::query_scalar(
        "SELECT checks.id FROM memory_evidence_checks checks \
         JOIN memory_evidence evidence ON evidence.id = checks.evidence_id \
         WHERE checks.memory_id = ? AND evidence.kind = 'user_confirmation'",
    )
    .bind(fixture.memory_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(check_ids, vec![confirmation_check_id]);
}

async fn event_count(fixture: &ApprovalFixture, action: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM memory_events WHERE memory_id = ? AND action = ?")
        .bind(fixture.memory_id)
        .bind(action)
        .fetch_one(&fixture.pool)
        .await
        .unwrap()
}
