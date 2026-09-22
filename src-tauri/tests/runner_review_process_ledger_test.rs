#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db;
use praxis_lib::runner::review_process::{self, ReviewOperation, ReviewPhase, ReviewProcessState};

#[tokio::test]
async fn immutable_receipts_support_one_active_lease_per_review_operation() {
    let fixture = fixture("leases").await;
    let verify = review_process::register(
        &fixture.pool,
        fixture.task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        101,
        &"a".repeat(64),
        2,
    )
    .await
    .unwrap();
    let challenge = review_process::register(
        &fixture.pool,
        fixture.task_id,
        ReviewOperation::Challenge,
        ReviewPhase::Reviewer,
        102,
        &"b".repeat(64),
        3,
    )
    .await
    .unwrap();

    assert_ne!(verify.id, challenge.id);
    assert!(review_process::register(
        &fixture.pool,
        fixture.task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyTest,
        103,
        &"c".repeat(64),
        4,
    )
    .await
    .is_err());
    assert!(
        sqlx::query("UPDATE review_process_receipts SET pgid = 999 WHERE id = ?")
            .bind(verify.id)
            .execute(&fixture.pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM review_process_receipts WHERE id = ?")
            .bind(verify.id)
            .execute(&fixture.pool)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn quarantined_lease_is_retained_and_fences_task_mutation() {
    let fixture = fixture("quarantine").await;
    let receipt = register_verify(&fixture, 301, 2).await;
    review_process::quarantine(
        &fixture.pool,
        fixture.task_id,
        ReviewOperation::Verify,
        receipt.id,
        "identity mismatch",
        3,
    )
    .await
    .unwrap();

    let lease = review_process::lease(&fixture.pool, fixture.task_id, ReviewOperation::Verify)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.state, ReviewProcessState::Quarantined);
    assert_eq!(lease.detail.as_deref(), Some("identity mismatch"));
    assert!(
        review_process::assert_task_unfenced(&fixture.pool, fixture.task_id)
            .await
            .is_err()
    );
    assert!(db::delete_task(&fixture.pool, fixture.task_id)
        .await
        .is_err());
}

struct Fixture {
    pool: sqlx::SqlitePool,
    task_id: i64,
}

async fn fixture(label: &str) -> Fixture {
    let path = temp_root::dir().join(format!(
        "praxis-review-ledger-{label}-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = db::init_pool(path.to_str().unwrap()).await.unwrap();
    review_process::migrate(&pool).await.unwrap();
    let task_id = db::insert_task(
        &pool, "/repo", "branch", "main", "/wt", "review", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    Fixture { pool, task_id }
}

async fn register_verify(
    fixture: &Fixture,
    pgid: i64,
    now: i64,
) -> review_process::ReviewProcessReceipt {
    review_process::register(
        &fixture.pool,
        fixture.task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        pgid,
        &"d".repeat(64),
        now,
    )
    .await
    .unwrap()
}
