use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static DATABASE_COUNTER: AtomicU32 = AtomicU32::new(0);

async fn test_pool() -> SqlitePool {
    let sequence = DATABASE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = crate::testtmp::dir().join(format!(
        "praxis-patterns-{}-{sequence}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .unwrap();
    sqlx::raw_sql(
        "CREATE TABLE tasks (id INTEGER PRIMARY KEY AUTOINCREMENT, state TEXT NOT NULL, \
           role TEXT NOT NULL DEFAULT 'implementer', created_at INTEGER NOT NULL, \
           updated_at INTEGER NOT NULL);
         CREATE TABLE task_events (id INTEGER PRIMARY KEY AUTOINCREMENT, \
           task_id INTEGER NOT NULL, ts INTEGER NOT NULL, kind TEXT NOT NULL, detail TEXT);",
    )
    .execute(&pool)
    .await
    .unwrap();
    pool
}

async fn add(pool: &SqlitePool, state: &str, role: &str, created: i64, updated: i64) -> i64 {
    sqlx::query("INSERT INTO tasks (state, role, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(state)
        .bind(role)
        .bind(created)
        .bind(updated)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_rowid()
}

async fn event(pool: &SqlitePool, task_id: i64, kind: &str) {
    sqlx::query("INSERT INTO task_events (task_id, ts, kind) VALUES (?, 0, ?)")
        .bind(task_id)
        .bind(kind)
        .execute(pool)
        .await
        .unwrap();
}

const NOW: i64 = 1_787_500_000;

#[test]
fn cutoff_matches_the_outcomes_convention() {
    assert_eq!(cutoff("7d", NOW), NOW - 7 * 86_400);
    assert_eq!(cutoff("30d", NOW), NOW - 30 * 86_400);
    assert_eq!(cutoff("all", NOW), 0);
    assert_eq!(cutoff("unknown", NOW), 0);
}

#[test]
fn percentile_of_empty_is_none_not_zero() {
    assert_eq!(percentile(&[], 50), None);
    assert_eq!(percentile(&[10], 50), Some(10));
    assert_eq!(percentile(&[10, 20, 30, 40, 50], 50), Some(30));
    assert_eq!(percentile(&[10, 20, 30, 40, 50], 90), Some(50));
}

/// 퍼널의 `started`·`reviewed`는 현재 상태가 아니라 이력으로 센다 — 이미 Done인 작업도
/// 실행을 거쳐 왔다.
#[tokio::test]
async fn funnel_counts_history_not_current_state() {
    let pool = test_pool().await;
    let done = add(&pool, "Done", "implementer", NOW - 100, NOW).await;
    event(&pool, done, "running").await;
    event(&pool, done, "approved").await;

    let discarded = add(&pool, "Discarded", "implementer", NOW - 100, NOW).await;
    event(&pool, discarded, "running").await;
    event(&pool, discarded, "discarded").await;

    // 큐에서 멈춘 작업 — 실행 이벤트가 없다.
    add(&pool, "Queued", "implementer", NOW - 100, NOW).await;

    let p = compute_patterns(&pool, "all", 0, NOW).await.unwrap();
    assert_eq!(p.funnel.total, 3);
    assert_eq!(p.funnel.started, 2);
    assert_eq!(p.funnel.reviewed, 2);
    assert_eq!(p.funnel.done, 1);
    assert_eq!(p.funnel.discarded, 1);
}

/// 범위 칩을 좁혀도 추세는 통째로 남는다 — 잘라내면 추세가 아니다(§6.2).
#[tokio::test]
async fn discard_trend_ignores_the_range_chip() {
    let pool = test_pool().await;
    let long_ago = NOW - 200 * 86_400;
    add(&pool, "Discarded", "implementer", long_ago, long_ago).await;
    add(&pool, "Done", "implementer", NOW - 60, NOW).await;

    let narrow = compute_patterns(&pool, "7d", 0, NOW).await.unwrap();
    // 퍼널은 좁아지지만
    assert_eq!(narrow.funnel.total, 1);
    // 추세는 두 달 모두 남는다.
    assert_eq!(narrow.discard_trend.len(), 2);
}

/// 후속 입력은 발생 여부다. 이벤트가 여러 개여도 한 건.
#[tokio::test]
async fn followup_is_boolean() {
    let pool = test_pool().await;
    let id = add(&pool, "Done", "implementer", NOW - 60, NOW).await;
    add(&pool, "Done", "implementer", NOW - 60, NOW).await;
    event(&pool, id, "user_followup_input_observed").await;
    event(&pool, id, "user_followup_input_observed").await;

    let p = compute_patterns(&pool, "all", 0, NOW).await.unwrap();
    assert_eq!(p.followup.total, 2);
    assert_eq!(p.followup.with_followup, 1);
}

#[tokio::test]
async fn role_outcomes_split_done_and_discarded() {
    let pool = test_pool().await;
    add(&pool, "Done", "implementer", NOW - 60, NOW).await;
    add(&pool, "Discarded", "implementer", NOW - 60, NOW).await;
    add(&pool, "Done", "researcher", NOW - 60, NOW).await;

    let p = compute_patterns(&pool, "all", 0, NOW).await.unwrap();
    let implementer = p
        .role_outcomes
        .iter()
        .find(|r| r.role == "implementer")
        .unwrap();
    assert_eq!(implementer.count, 2);
    assert_eq!(implementer.done, 1);
    assert_eq!(implementer.discarded, 1);
}

/// 진행 중인 작업의 소요를 섞으면 중앙값이 계속 흔들린다.
#[tokio::test]
async fn durations_only_count_finished_tasks() {
    let pool = test_pool().await;
    add(&pool, "Done", "implementer", NOW - 300, NOW - 200).await; // 100초
    add(&pool, "Done", "implementer", NOW - 300, NOW - 100).await; // 200초
    add(&pool, "Running", "implementer", NOW - 5000, NOW).await; // 세지 않는다

    let p = compute_patterns(&pool, "all", 0, NOW).await.unwrap();
    assert_eq!(p.duration_p50, Some(100));
    assert_eq!(p.duration_p90, Some(200));
}

#[tokio::test]
async fn empty_database_yields_no_percentiles() {
    let pool = test_pool().await;
    let p = compute_patterns(&pool, "all", 0, NOW).await.unwrap();
    assert_eq!(p.funnel.total, 0);
    assert_eq!(p.duration_p50, None);
    assert!(p.discard_trend.is_empty());
}
