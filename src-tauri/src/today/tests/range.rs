//! 범위 조회 — 인사이트 계획 캘린더가 달 단위로 부른다 (설계 0023).

use super::test_pool;
use crate::today::store;

/// `tests/reconcile.rs`와 같은 최소 컬럼 삽입.
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

#[tokio::test]
async fn range_includes_both_boundaries() {
    let pool = test_pool().await;
    store::add(&pool, "2026-08-01", "첫날", None, 100).await.unwrap();
    store::add(&pool, "2026-08-15", "중간", None, 100).await.unwrap();
    store::add(&pool, "2026-08-31", "끝날", None, 100).await.unwrap();

    let items = store::range(&pool, "2026-08-01", "2026-08-31").await.unwrap();

    // 캘린더가 달의 첫날·마지막날을 그대로 넘기므로 경계가 빠지면 그 날이 통째로 사라진다.
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].title, "첫날");
    assert_eq!(items[2].title, "끝날");
}

#[tokio::test]
async fn range_excludes_days_outside_the_window() {
    let pool = test_pool().await;
    store::add(&pool, "2026-07-31", "전달", None, 100).await.unwrap();
    store::add(&pool, "2026-08-10", "이달", None, 100).await.unwrap();
    store::add(&pool, "2026-09-01", "다음달", None, 100).await.unwrap();

    let items = store::range(&pool, "2026-08-01", "2026-08-31").await.unwrap();

    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title, "이달");
}

#[tokio::test]
async fn range_orders_by_day_then_position() {
    let pool = test_pool().await;
    // 늦은 날을 먼저 넣어 삽입 순서와 정렬이 다르게 만든다.
    store::add(&pool, "2026-08-05", "5일-첫째", None, 100).await.unwrap();
    store::add(&pool, "2026-08-05", "5일-둘째", None, 200).await.unwrap();
    store::add(&pool, "2026-08-03", "3일-첫째", None, 300).await.unwrap();

    let items = store::range(&pool, "2026-08-01", "2026-08-31").await.unwrap();

    let titles: Vec<&str> = items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, ["3일-첫째", "5일-첫째", "5일-둘째"]);
}

#[tokio::test]
async fn range_returns_empty_for_a_month_with_no_items() {
    let pool = test_pool().await;
    store::add(&pool, "2026-08-10", "이달", None, 100).await.unwrap();

    let items = store::range(&pool, "2026-09-01", "2026-09-30").await.unwrap();

    assert!(items.is_empty());
}

#[tokio::test]
async fn reconcile_range_flips_done_tasks_across_multiple_days() {
    let pool = test_pool().await;
    let first = store::add(&pool, "2026-08-03", "3일", Some("/r"), 100).await.unwrap();
    let second = store::add(&pool, "2026-08-20", "20일", Some("/r"), 100).await.unwrap();
    insert_task(&pool, 1, "Done").await;
    insert_task(&pool, 2, "Done").await;
    store::link_task(&pool, first.id, 1, 0).await.unwrap();
    store::link_task(&pool, second.id, 2, 0).await.unwrap();

    store::reconcile_tasks_range(&pool, "2026-08-01", "2026-08-31", 500)
        .await
        .unwrap();

    assert_eq!(store::get(&pool, first.id).await.unwrap().status, "done");
    assert_eq!(store::get(&pool, second.id).await.unwrap().status, "done");
}

#[tokio::test]
async fn reconcile_range_does_not_touch_days_outside_the_window() {
    let pool = test_pool().await;
    let inside = store::add(&pool, "2026-08-10", "이달", Some("/r"), 100).await.unwrap();
    let outside = store::add(&pool, "2026-09-01", "다음달", Some("/r"), 100).await.unwrap();
    insert_task(&pool, 1, "Done").await;
    insert_task(&pool, 2, "Done").await;
    store::link_task(&pool, inside.id, 1, 0).await.unwrap();
    store::link_task(&pool, outside.id, 2, 0).await.unwrap();

    store::reconcile_tasks_range(&pool, "2026-08-01", "2026-08-31", 500)
        .await
        .unwrap();

    assert_eq!(store::get(&pool, inside.id).await.unwrap().status, "done");
    assert_eq!(store::get(&pool, outside.id).await.unwrap().status, "open");
}

#[tokio::test]
async fn reconcile_range_does_not_revive_a_dropped_item() {
    let pool = test_pool().await;
    let item = store::add(&pool, "2026-08-10", "접은 것", Some("/r"), 100).await.unwrap();
    insert_task(&pool, 1, "Done").await;
    store::link_task(&pool, item.id, 1, 0).await.unwrap();
    store::set_status(&pool, item.id, crate::today::DayStatus::Dropped, 200)
        .await
        .unwrap();

    store::reconcile_tasks_range(&pool, "2026-08-01", "2026-08-31", 500)
        .await
        .unwrap();

    // 단일 날짜판과 같은 규약 — 사람이 내린 결정을 덮지 않는다.
    assert_eq!(store::get(&pool, item.id).await.unwrap().status, "dropped");
}
