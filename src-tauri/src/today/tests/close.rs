use super::test_pool;
use crate::today::{close, store, DayStatus};

#[tokio::test]
async fn closing_counts_open_and_dropped_separately() {
    let pool = test_pool().await;
    let a = store::add(&pool, "2026-08-03", "함", None, 100)
        .await
        .unwrap();
    store::set_status(&pool, a.id, DayStatus::Done, 200)
        .await
        .unwrap();
    store::add(&pool, "2026-08-03", "못 함", None, 100)
        .await
        .unwrap();
    let c = store::add(&pool, "2026-08-03", "접음", None, 100)
        .await
        .unwrap();
    store::set_status(&pool, c.id, DayStatus::Dropped, 200)
        .await
        .unwrap();

    let closing = close::close_day(&pool, "2026-08-03", 900).await.unwrap();

    assert_eq!((closing.done, closing.open, closing.dropped), (1, 1, 1));
}

#[tokio::test]
async fn closing_twice_overwrites_rather_than_erroring() {
    let pool = test_pool().await;
    store::add(&pool, "2026-08-03", "A", None, 100)
        .await
        .unwrap();
    close::close_day(&pool, "2026-08-03", 900).await.unwrap();
    let second = close::close_day(&pool, "2026-08-03", 1000).await.unwrap();
    assert_eq!(second.closed_at, 1000, "다시 마감하면 갱신된다");

    let stored = close::get_closing(&pool, "2026-08-03").await.unwrap().unwrap();
    assert_eq!(stored.closed_at, 1000);
}

#[tokio::test]
async fn closing_an_empty_day_yields_zero_counts() {
    let pool = test_pool().await;
    let closing = close::close_day(&pool, "2026-08-03", 900).await.unwrap();
    assert_eq!((closing.done, closing.open, closing.dropped), (0, 0, 0));
}

#[test]
fn draft_has_the_required_ledger_fields() {
    let draft = close::render_draft(
        "2026-08-03",
        128,
        &["함".into()],
        &["못 함".into()],
        &["접음".into()],
    );
    // pre-commit이 요구하는 필드가 초안에 있어야 한다 (CLAUDE.md "작업을 마치면").
    assert!(draft.contains("### #128 · 2026-08-03 ·"));
    assert!(draft.contains("- **subject**:"));
    assert!(draft.contains("- **status**:"));
    assert!(draft.contains("함"));
    assert!(draft.contains("못 함"));
}

#[test]
fn draft_omits_empty_groups() {
    let draft = close::render_draft("2026-08-03", 1, &["함".into()], &[], &[]);
    assert!(!draft.contains("못 한 것"));
    assert!(!draft.contains("접은 것"));
}

#[test]
fn draft_without_number_leaves_a_placeholder() {
    let draft = close::render_draft("2026-08-03", 0, &[], &[], &[]);
    // 번호를 추측하면 브랜치 병행 시 중복 번호를 만든다 — 사용자가 채우도록 남긴다.
    assert!(draft.contains("### #N · 2026-08-03"));
}
