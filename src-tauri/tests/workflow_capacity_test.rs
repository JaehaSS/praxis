#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db::{self, state};
use praxis_lib::runner::{
    capacity::RunnerCapacity, queue::QueueWorker, worktree_lock::WorktreeLocks,
};

#[test]
fn clones_share_one_budget_and_release_only_when_the_owner_drops() {
    let capacity = RunnerCapacity::new(2);
    let clone = capacity.clone();
    let first = capacity.try_acquire().unwrap();
    let second = clone.try_acquire().unwrap();
    assert!(capacity.try_acquire().is_err());
    assert_eq!(clone.available(), 0);
    drop(first);
    assert_eq!(capacity.available(), 1);
    drop(second);
    assert_eq!(capacity.available(), 2);
}

#[tokio::test]
async fn queue_and_resumed_conversation_obey_the_same_external_capacity() {
    let root = temp_root::dir().join("workflow-runner-capacity");
    std::fs::create_dir_all(&root).unwrap();
    let pool = db::init_pool(root.join("runner.sqlite").to_str().unwrap())
        .await
        .unwrap();
    let capacity = RunnerCapacity::new(1);
    let worker =
        QueueWorker::with_capacity(pool.clone(), capacity.clone(), WorktreeLocks::default());
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        "/wt",
        "task",
        None,
        None,
        "conversation",
        1,
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, state::AWAITING_REVIEW, 2)
        .await
        .unwrap();
    let permit = capacity.try_acquire().unwrap();
    assert!(worker.lease_next(3).await.unwrap().is_none());
    assert!(worker
        .resume_conversation(task_id, "continue".into(), 3)
        .await
        .unwrap_err()
        .contains("슬롯"));
    assert_eq!(
        db::get_task(&pool, task_id).await.unwrap().unwrap().state,
        state::AWAITING_REVIEW
    );
    assert!(processless_output(&pool, task_id).await);
    assert_eq!(worker.capacity().available(), 0);
    drop(permit);
    assert_eq!(worker.capacity().available(), 1);
    // Use a fresh queue task to demonstrate the shared slot is returned without
    // starting a real vendor or leaving a background turn in this test.
    db::update_state(&pool, task_id, state::QUEUED, 4)
        .await
        .unwrap();
    let lease = worker.lease_next(5).await.unwrap().unwrap();
    assert_eq!(capacity.available(), 0);
    drop(lease);
    assert_eq!(capacity.available(), 1);
    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

async fn processless_output(pool: &sqlx::SqlitePool, task_id: i64) -> bool {
    db::list_task_output_after(pool, 0, 100)
        .await
        .unwrap()
        .iter()
        .all(|item| item.task_id != task_id)
}
