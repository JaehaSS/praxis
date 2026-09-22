use super::test_pool;
use crate::today::store;
use crate::today::DayStatus;

#[tokio::test]
async fn add_assigns_increasing_positions_within_a_day() {
    let pool = test_pool().await;
    let a = store::add(&pool, "2026-08-03", "첫째", None, 100)
        .await
        .unwrap();
    let b = store::add(&pool, "2026-08-03", "둘째", None, 200)
        .await
        .unwrap();
    assert!(b.position > a.position);
}

#[tokio::test]
async fn positions_are_independent_per_day() {
    let pool = test_pool().await;
    store::add(&pool, "2026-08-03", "오늘 것", None, 100)
        .await
        .unwrap();
    let other = store::add(&pool, "2026-08-04", "내일 것", None, 100)
        .await
        .unwrap();
    assert_eq!(other.position, 0, "새 날은 0부터 다시 센다");
}

#[tokio::test]
async fn list_returns_only_the_requested_day_in_position_order() {
    let pool = test_pool().await;
    store::add(&pool, "2026-08-03", "A", None, 100)
        .await
        .unwrap();
    store::add(&pool, "2026-08-04", "B", None, 100)
        .await
        .unwrap();
    store::add(&pool, "2026-08-03", "C", None, 100)
        .await
        .unwrap();
    let items = store::list(&pool, "2026-08-03").await.unwrap();
    let titles: Vec<_> = items.iter().map(|i| i.title.as_str()).collect();
    assert_eq!(titles, vec!["A", "C"]);
}

#[tokio::test]
async fn set_status_done_stamps_done_at_and_reopening_clears_it() {
    let pool = test_pool().await;
    let item = store::add(&pool, "2026-08-03", "A", None, 100)
        .await
        .unwrap();
    let done = store::set_status(&pool, item.id, DayStatus::Done, 500)
        .await
        .unwrap();
    assert_eq!(done.status, "done");
    assert_eq!(done.done_at, Some(500));
    let reopened = store::set_status(&pool, item.id, DayStatus::Open, 600)
        .await
        .unwrap();
    assert_eq!(reopened.done_at, None, "되돌리면 완료 시각도 지운다");
}

#[tokio::test]
async fn reorder_applies_given_sequence_and_ignores_ids_from_other_days() {
    let pool = test_pool().await;
    let a = store::add(&pool, "2026-08-03", "A", None, 100)
        .await
        .unwrap();
    let b = store::add(&pool, "2026-08-03", "B", None, 100)
        .await
        .unwrap();
    let c = store::add(&pool, "2026-08-03", "C", None, 100)
        .await
        .unwrap();
    let intruder = store::add(&pool, "2026-08-04", "다른 날", None, 100)
        .await
        .unwrap();
    store::reorder(&pool, "2026-08-03", &[c.id, a.id, b.id, intruder.id], 200)
        .await
        .unwrap();
    let titles: Vec<_> = store::list(&pool, "2026-08-03")
        .await
        .unwrap()
        .iter()
        .map(|i| i.title.clone())
        .collect();
    assert_eq!(titles, vec!["C", "A", "B"]);
    // 다른 날 항목은 건드리지 않는다
    assert_eq!(store::list(&pool, "2026-08-04").await.unwrap().len(), 1);
}

#[tokio::test]
async fn update_changes_only_given_fields() {
    let pool = test_pool().await;
    let item = store::add(&pool, "2026-08-03", "원래 제목", Some("repo1"), 100)
        .await
        .unwrap();
    let updated = store::update(&pool, item.id, Some("새 제목"), None, None, 200)
        .await
        .unwrap();
    assert_eq!(updated.title, "새 제목");
    assert_eq!(
        updated.repo.as_deref(),
        Some("repo1"),
        "note/repo는 None이면 유지"
    );
}

#[tokio::test]
async fn remove_deletes_the_row() {
    let pool = test_pool().await;
    let item = store::add(&pool, "2026-08-03", "A", None, 100)
        .await
        .unwrap();
    store::remove(&pool, item.id).await.unwrap();
    assert!(store::list(&pool, "2026-08-03").await.unwrap().is_empty());
}

#[tokio::test]
async fn empty_title_is_rejected() {
    let pool = test_pool().await;
    assert!(store::add(&pool, "2026-08-03", "   ", None, 100)
        .await
        .is_err());
}

#[tokio::test]
async fn malformed_day_is_rejected_before_insert() {
    let pool = test_pool().await;
    assert!(store::add(&pool, "2026-13-99", "A", None, 100).await.is_err());
    assert!(store::list(&pool, "2026-13-99").await.unwrap().is_empty());
}
