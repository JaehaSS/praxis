//! 백로그 레인 — `day = 'backlog'` 센티넬 (플랜 0054).

use super::test_pool;
use crate::today::{carry, day, store, suggest, DayStatus};

const TODAY: &str = "2026-08-27";

#[test]
fn backlog_is_a_valid_lane_key_but_not_a_valid_day() {
    // 날짜를 받아야 하는 자리(마감·캘린더)는 `validate`를 그대로 쓴다 — 백로그가 새어
    // 들어가는 것을 이 비대칭이 막는다.
    assert!(day::validate(day::BACKLOG).is_err());
    assert!(day::validate_key(day::BACKLOG).is_ok());
    assert!(day::validate_key(TODAY).is_ok());
    assert!(day::validate_key("backlogg").is_err());
    assert!(day::is_backlog(day::BACKLOG));
    assert!(!day::is_backlog(TODAY));
}

#[tokio::test]
async fn backlog_stays_out_of_date_ranges() {
    let pool = test_pool().await;
    store::add(&pool, day::BACKLOG, "언젠가", None, 100)
        .await
        .unwrap();
    store::add(&pool, "2026-08-01", "그날 한 일", None, 100)
        .await
        .unwrap();

    let range = store::range(&pool, "2000-01-01", "2999-12-31").await.unwrap();

    assert_eq!(range.len(), 1, "백로그는 어떤 날짜 범위에도 들지 않는다");
    assert_eq!(range[0].title, "그날 한 일");
}

#[tokio::test]
async fn carry_forward_never_pulls_the_backlog() {
    let pool = test_pool().await;
    store::add(&pool, day::BACKLOG, "언젠가", None, 100)
        .await
        .unwrap();
    store::add(&pool, "2026-08-01", "지난 일", None, 100)
        .await
        .unwrap();

    let moved = carry::carry_forward(&pool, TODAY, 500).await.unwrap();

    assert_eq!(moved, 1, "지난 날의 항목만 따라온다");
    assert_eq!(store::list(&pool, day::BACKLOG).await.unwrap().len(), 1);
    let today = store::list(&pool, TODAY).await.unwrap();
    assert_eq!(today.len(), 1);
    assert_eq!(today[0].title, "지난 일");
}

#[tokio::test]
async fn move_to_appends_at_the_end_and_clears_the_carry_mark() {
    let pool = test_pool().await;
    let first = store::add(&pool, TODAY, "먼저", None, 100).await.unwrap();
    let second = store::add(&pool, TODAY, "나중", None, 100).await.unwrap();
    store::add(&pool, day::BACKLOG, "이미 있던 것", None, 100)
        .await
        .unwrap();

    let moved = store::move_to(&pool, second.id, day::BACKLOG, 500)
        .await
        .unwrap();

    assert_eq!(moved.day, day::BACKLOG);
    assert_eq!(moved.position, 1, "백로그의 끝에 붙는다");
    assert_eq!(moved.carried_from, None);
    // 옮긴 것이지 복제한 것이 아니다.
    let today = store::list(&pool, TODAY).await.unwrap();
    assert_eq!(today.len(), 1);
    assert_eq!(today[0].id, first.id);
}

#[tokio::test]
async fn move_back_to_today_drops_the_carry_mark() {
    let pool = test_pool().await;
    store::add(&pool, "2026-08-01", "밀린 일", None, 100)
        .await
        .unwrap();
    carry::carry_forward(&pool, TODAY, 200).await.unwrap();
    let carried = store::list(&pool, TODAY).await.unwrap().remove(0);
    assert_eq!(carried.carried_from.as_deref(), Some("2026-08-01"));

    store::move_to(&pool, carried.id, day::BACKLOG, 300)
        .await
        .unwrap();
    let pulled = store::move_to(&pool, carried.id, TODAY, 400).await.unwrap();

    // 손으로 옮긴 것은 이월이 아니다 — 백로그를 거쳐 돌아온 항목은 새 시작이다.
    assert_eq!(pulled.day, TODAY);
    assert_eq!(pulled.carried_from, None);
}

#[tokio::test]
async fn move_keeps_the_task_link() {
    let pool = test_pool().await;
    let item = store::add(&pool, TODAY, "착수한 일", None, 100)
        .await
        .unwrap();
    store::link_task(&pool, item.id, 42, 200).await.unwrap();

    let moved = store::move_to(&pool, item.id, day::BACKLOG, 300)
        .await
        .unwrap();

    // 새 테이블이 아니라 같은 행을 옮기는 이유가 이것이다 (플랜 0054 DR-1).
    assert_eq!(moved.task_id, Some(42));
}

#[tokio::test]
async fn move_rejects_finished_items() {
    let pool = test_pool().await;
    let done = store::add(&pool, TODAY, "끝난 일", None, 100).await.unwrap();
    store::set_status(&pool, done.id, DayStatus::Done, 200)
        .await
        .unwrap();
    let dropped = store::add(&pool, TODAY, "접은 일", None, 100).await.unwrap();
    store::set_status(&pool, dropped.id, DayStatus::Dropped, 200)
        .await
        .unwrap();

    // 끝난 결정을 미결로 되돌리는 경로를 막는다.
    assert!(store::move_to(&pool, done.id, day::BACKLOG, 300).await.is_err());
    assert!(store::move_to(&pool, dropped.id, day::BACKLOG, 300)
        .await
        .is_err());
}

#[tokio::test]
async fn move_to_the_same_lane_is_a_no_op() {
    let pool = test_pool().await;
    let item = store::add(&pool, TODAY, "제자리", None, 100).await.unwrap();

    let same = store::move_to(&pool, item.id, TODAY, 500).await.unwrap();

    assert_eq!(same.position, item.position, "위치가 흔들리지 않는다");
    assert_eq!(same.updated_at, item.updated_at);
}

#[tokio::test]
async fn move_rejects_a_malformed_lane() {
    let pool = test_pool().await;
    let item = store::add(&pool, TODAY, "어디로", None, 100).await.unwrap();

    assert!(store::move_to(&pool, item.id, "언젠가", 500).await.is_err());
}

#[tokio::test]
async fn suggestions_exclude_what_is_parked_in_the_backlog() {
    let pool = test_pool().await;
    // 오늘에 담았던 이슈를 백로그로 민 상황.
    let taken = store::add_sourced(&pool, TODAY, "이슈 #42", None, "github", Some("42"), 100)
        .await
        .unwrap();
    store::move_to(&pool, taken.id, day::BACKLOG, 200)
        .await
        .unwrap();

    let candidates = vec![suggest::Suggestion {
        title: "이슈 #42".into(),
        source: "github".into(),
        source_ref: Some("42".into()),
        repo: None,
    }];
    let left = suggest::exclude_taken(&pool, TODAY, candidates).await.unwrap();

    // 백로그에 있다는 것은 "이미 내 목록에 있다"는 뜻이다. 제안은 아직 목록에 없는
    // 후보를 부르는 것이므로 여기서 다시 뜨면 담는 순간 같은 일이 두 곳에 생긴다.
    assert!(left.is_empty());
}
