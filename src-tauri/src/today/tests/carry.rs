use super::test_pool;
use crate::today::{carry, store, DayStatus};

const TODAY: &str = "2026-08-07";

#[tokio::test]
async fn moves_yesterdays_open_items_to_today() {
    let pool = test_pool().await;
    store::add(&pool, "2026-08-06", "어제 못 한 것", None, 100)
        .await
        .unwrap();

    let moved = carry::carry_forward(&pool, TODAY, 500).await.unwrap();

    assert_eq!(moved, 1);
    let today = store::list(&pool, TODAY).await.unwrap();
    assert_eq!(today.len(), 1);
    assert_eq!(today[0].title, "어제 못 한 것");
    // 옮긴 것이지 복제한 것이 아니다 — 어제는 비어야 한다.
    assert!(store::list(&pool, "2026-08-06").await.unwrap().is_empty());
}

#[tokio::test]
async fn leaves_done_and_dropped_behind() {
    let pool = test_pool().await;
    let done = store::add(&pool, "2026-08-06", "어제 한 것", None, 100)
        .await
        .unwrap();
    store::set_status(&pool, done.id, DayStatus::Done, 200)
        .await
        .unwrap();
    let dropped = store::add(&pool, "2026-08-06", "어제 접은 것", None, 100)
        .await
        .unwrap();
    store::set_status(&pool, dropped.id, DayStatus::Dropped, 200)
        .await
        .unwrap();

    let moved = carry::carry_forward(&pool, TODAY, 500).await.unwrap();

    assert_eq!(moved, 0, "끝냈거나 접은 계획은 따라오지 않는다");
    assert_eq!(store::list(&pool, "2026-08-06").await.unwrap().len(), 2);
}

/// 주말·휴가로 앱을 안 열면 하루씩 잇는 방식은 그 구간에서 끊긴다.
#[tokio::test]
async fn spans_a_gap_of_several_days() {
    let pool = test_pool().await;
    store::add(&pool, "2026-08-01", "엿새 전 것", None, 100)
        .await
        .unwrap();
    store::add(&pool, "2026-08-04", "사흘 전 것", None, 100)
        .await
        .unwrap();

    let moved = carry::carry_forward(&pool, TODAY, 500).await.unwrap();

    assert_eq!(moved, 2);
    let titles: Vec<String> = store::list(&pool, TODAY)
        .await
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect();
    assert_eq!(titles, vec!["엿새 전 것", "사흘 전 것"], "오래된 것부터 온다");
}

#[tokio::test]
async fn does_not_touch_future_days() {
    let pool = test_pool().await;
    store::add(&pool, "2026-08-08", "내일 할 것", None, 100)
        .await
        .unwrap();

    let moved = carry::carry_forward(&pool, TODAY, 500).await.unwrap();

    assert_eq!(moved, 0, "앞당기지 않는다 — 이월은 과거에서만 온다");
    assert_eq!(store::list(&pool, "2026-08-08").await.unwrap().len(), 1);
}

/// 이월은 행을 옮기는 파괴적 연산이라, 이게 없으면 원래 언제 것인지가 소실된다.
#[tokio::test]
async fn records_the_day_it_came_from() {
    let pool = test_pool().await;
    store::add(&pool, "2026-08-06", "밀린 것", None, 100)
        .await
        .unwrap();

    carry::carry_forward(&pool, TODAY, 500).await.unwrap();

    let item = &store::list(&pool, TODAY).await.unwrap()[0];
    assert_eq!(item.carried_from.as_deref(), Some("2026-08-06"));
}

/// 직전 날짜다 — 최초 계획일이 아니다. 이틀 연속 밀리면 어제로 갱신된다.
#[tokio::test]
async fn carried_from_tracks_the_previous_day_not_the_original() {
    let pool = test_pool().await;
    store::add(&pool, "2026-08-05", "이틀 밀린 것", None, 100)
        .await
        .unwrap();

    carry::carry_forward(&pool, "2026-08-06", 400).await.unwrap();
    carry::carry_forward(&pool, TODAY, 500).await.unwrap();

    let item = &store::list(&pool, TODAY).await.unwrap()[0];
    assert_eq!(item.carried_from.as_deref(), Some("2026-08-06"));
}

/// 복제가 아니라 이동을 택한 이유. 링크가 끊기면 착수한 일이 미착수로 보여 이중 착수를 부른다.
#[tokio::test]
async fn keeps_the_task_link() {
    let pool = test_pool().await;
    let item = store::add(&pool, "2026-08-06", "착수한 것", Some("/r"), 100)
        .await
        .unwrap();
    store::link_task(&pool, item.id, 42, 200).await.unwrap();

    carry::carry_forward(&pool, TODAY, 500).await.unwrap();

    let moved = &store::list(&pool, TODAY).await.unwrap()[0];
    assert_eq!(moved.task_id, Some(42));
    assert_eq!(moved.id, item.id, "같은 행이어야 링크가 유지된다");
}

#[tokio::test]
async fn appends_after_the_items_already_planned_for_today() {
    let pool = test_pool().await;
    store::add(&pool, TODAY, "오늘 정한 것", None, 400)
        .await
        .unwrap();
    store::add(&pool, "2026-08-06", "밀린 것", None, 100)
        .await
        .unwrap();

    carry::carry_forward(&pool, TODAY, 500).await.unwrap();

    let titles: Vec<String> = store::list(&pool, TODAY)
        .await
        .unwrap()
        .into_iter()
        .map(|i| i.title)
        .collect();
    assert_eq!(titles, vec!["오늘 정한 것", "밀린 것"]);
}

/// `today_list`는 조회마다 이 함수를 부른다 — 두 번째 호출이 아무것도 바꾸지 않아야 한다.
#[tokio::test]
async fn is_idempotent() {
    let pool = test_pool().await;
    store::add(&pool, "2026-08-06", "밀린 것", None, 100)
        .await
        .unwrap();

    carry::carry_forward(&pool, TODAY, 500).await.unwrap();
    let second = carry::carry_forward(&pool, TODAY, 600).await.unwrap();

    assert_eq!(second, 0);
    assert_eq!(store::list(&pool, TODAY).await.unwrap().len(), 1);
}

/// 같은 출처의 일이 오늘 이미 있으면 `idx_day_items_source`가 이동을 막는다.
/// 그 한 건 때문에 나머지 이월까지 죽으면 안 된다.
#[tokio::test]
async fn a_source_collision_skips_only_that_item() {
    let pool = test_pool().await;
    store::add_sourced(&pool, "2026-08-06", "이슈 7", None, "github", Some("7"), 100)
        .await
        .unwrap();
    store::add(&pool, "2026-08-06", "그냥 밀린 것", None, 100)
        .await
        .unwrap();
    // 오늘 같은 이슈를 이미 담았다.
    store::add_sourced(&pool, TODAY, "이슈 7", None, "github", Some("7"), 300)
        .await
        .unwrap();

    let moved = carry::carry_forward(&pool, TODAY, 500).await.unwrap();

    assert_eq!(moved, 1, "충돌한 한 건만 빠진다");
    let today = store::list(&pool, TODAY).await.unwrap();
    assert_eq!(today.len(), 2);
    assert!(today.iter().any(|i| i.title == "그냥 밀린 것"));
    // 충돌한 항목은 지난 날에 그대로 남는다 — 삭제하거나 접지 않는다.
    let past = store::list(&pool, "2026-08-06").await.unwrap();
    assert_eq!(past.len(), 1);
    assert_eq!(past[0].source_ref.as_deref(), Some("7"));
}

#[tokio::test]
async fn rejects_a_malformed_day() {
    let pool = test_pool().await;
    assert!(carry::carry_forward(&pool, "8월 7일", 500).await.is_err());
}
