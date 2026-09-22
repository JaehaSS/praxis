use super::test_pool;
use crate::today::store;

/// `tasks` 행을 최소 컬럼만 채워 넣는다 (NOT NULL만 만족).
async fn insert_task(pool: &sqlx::SqlitePool, id: i64, state: &str) {
    sqlx::query(
        "INSERT INTO tasks (id, repo, branch, base, worktree_path, instruction, state, created_at, updated_at) \
         VALUES (?, '/r', 'b', 'main', '/w', 'i', ?, 0, 0)",
    )
    .bind(id)
    .bind(state)
    .execute(pool)
    .await
    .unwrap();
}

async fn bind_task(pool: &sqlx::SqlitePool, item_id: i64, task_id: i64) {
    store::link_task(pool, item_id, task_id, 0).await.unwrap();
}

#[tokio::test]
async fn done_task_flips_the_item_to_done() {
    let pool = test_pool().await;
    let item = store::add(&pool, "2026-08-03", "구현", Some("/r"), 100)
        .await
        .unwrap();
    insert_task(&pool, 42, "Done").await;
    bind_task(&pool, item.id, 42).await;

    store::reconcile_tasks(&pool, "2026-08-03", 500).await.unwrap();

    let after = store::get(&pool, item.id).await.unwrap();
    assert_eq!(after.status, "done");
    assert_eq!(after.done_at, Some(500));
}

#[tokio::test]
async fn running_task_leaves_the_item_open() {
    let pool = test_pool().await;
    let item = store::add(&pool, "2026-08-03", "구현", Some("/r"), 100)
        .await
        .unwrap();
    insert_task(&pool, 42, "Running").await;
    bind_task(&pool, item.id, 42).await;

    store::reconcile_tasks(&pool, "2026-08-03", 500).await.unwrap();

    assert_eq!(store::get(&pool, item.id).await.unwrap().status, "open");
}

#[tokio::test]
async fn failed_task_leaves_the_item_open_for_the_warning_badge() {
    let pool = test_pool().await;
    let item = store::add(&pool, "2026-08-03", "구현", Some("/r"), 100)
        .await
        .unwrap();
    insert_task(&pool, 42, "Failed").await;
    bind_task(&pool, item.id, 42).await;

    store::reconcile_tasks(&pool, "2026-08-03", 500).await.unwrap();

    // 실패는 자동 완료가 아니다 — 사람이 다시 판단해야 한다 (설계 0021 §6).
    assert_eq!(store::get(&pool, item.id).await.unwrap().status, "open");
}

#[tokio::test]
async fn manually_dropped_item_is_not_revived_by_a_done_task() {
    let pool = test_pool().await;
    let item = store::add(&pool, "2026-08-03", "구현", Some("/r"), 100)
        .await
        .unwrap();
    insert_task(&pool, 42, "Done").await;
    bind_task(&pool, item.id, 42).await;
    store::set_status(&pool, item.id, crate::today::DayStatus::Dropped, 200)
        .await
        .unwrap();

    store::reconcile_tasks(&pool, "2026-08-03", 500).await.unwrap();

    // reconcile 대상은 open만이다 — 사람이 내린 결정을 덮지 않는다.
    assert_eq!(store::get(&pool, item.id).await.unwrap().status, "dropped");
}

#[tokio::test]
async fn deleted_task_row_does_not_error() {
    let pool = test_pool().await;
    let item = store::add(&pool, "2026-08-03", "구현", Some("/r"), 100)
        .await
        .unwrap();
    bind_task(&pool, item.id, 999).await; // tasks에 없는 id

    store::reconcile_tasks(&pool, "2026-08-03", 500).await.unwrap();

    assert_eq!(store::get(&pool, item.id).await.unwrap().status, "open");
}

#[tokio::test]
async fn reconcile_only_touches_the_requested_day() {
    let pool = test_pool().await;
    let today = store::add(&pool, "2026-08-03", "오늘", Some("/r"), 100)
        .await
        .unwrap();
    let tomorrow = store::add(&pool, "2026-08-04", "내일", Some("/r"), 100)
        .await
        .unwrap();
    insert_task(&pool, 1, "Done").await;
    insert_task(&pool, 2, "Done").await;
    bind_task(&pool, today.id, 1).await;
    bind_task(&pool, tomorrow.id, 2).await;

    store::reconcile_tasks(&pool, "2026-08-03", 500).await.unwrap();

    assert_eq!(store::get(&pool, today.id).await.unwrap().status, "done");
    assert_eq!(store::get(&pool, tomorrow.id).await.unwrap().status, "open");
}
