//! Process birth receipts are append-only across resumed conversation turns.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db::{self, state};

#[tokio::test]
async fn resumed_turn_appends_and_preserves_process_identity_receipts() {
    let path = temp_root::dir().join(format!(
        "praxis-runner-process-receipt-{}.sqlite",
        std::process::id()
    ));
    let pool = db::init_pool(path.to_str().unwrap()).await.unwrap();
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        "/worktree",
        "conversation",
        Some("claude"),
        None,
        "conversation",
        1,
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, state::RUNNING, 2)
        .await
        .unwrap();
    db::record_task_process_start(&pool, task_id, 101, &"a".repeat(64), "conversation", 3)
        .await
        .unwrap();
    db::finish_running_task(&pool, task_id, state::AWAITING_REVIEW, 4, "completed", None)
        .await
        .unwrap();
    db::mark_running_from_review(&pool, task_id, 5)
        .await
        .unwrap();
    db::record_task_process_start(&pool, task_id, 202, &"b".repeat(64), "conversation", 6)
        .await
        .unwrap();

    let latest = db::task_process_receipt(&pool, task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(latest.pgid, 202);
    assert_eq!(latest.identity_hash, "b".repeat(64));
    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_process_receipts WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 2);
    assert!(
        sqlx::query("DELETE FROM task_process_receipts WHERE task_id = ?")
            .bind(task_id)
            .execute(&pool)
            .await
            .is_err()
    );
    db::finish_running_task(&pool, task_id, state::FAILED, 7, "failed", None)
        .await
        .unwrap();
    db::delete_task(&pool, task_id).await.unwrap();
    let retained: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_process_receipts WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(retained, 2);
    let _ = std::fs::remove_file(path);
}
