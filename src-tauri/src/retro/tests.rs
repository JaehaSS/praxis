use super::*;
use std::sync::atomic::{AtomicU32, Ordering};

static DATABASE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// 임시 파일 DB인 이유는 `sqlite::memory:`가 풀의 커넥션마다 별개의 빈 DB를 보기 때문이다
/// (`quiz/tests/mod.rs:20`과 같은 관례).
async fn test_pool() -> SqlitePool {
    let sequence = DATABASE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = crate::testtmp::dir().join(format!(
        "praxis-retro-{}-{sequence}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}?mode=rwc", path.display()))
        .await
        .unwrap();

    // 회고가 읽는 표만 세운다 — 앱 전체 스키마를 끌어오면 이 테스트가 무관한 변경에 깨진다.
    sqlx::raw_sql(
        "CREATE TABLE tasks (id INTEGER PRIMARY KEY AUTOINCREMENT, state TEXT NOT NULL, \
           role TEXT NOT NULL DEFAULT 'implementer', created_at INTEGER NOT NULL, \
           updated_at INTEGER NOT NULL);
         CREATE TABLE task_events (id INTEGER PRIMARY KEY AUTOINCREMENT, \
           task_id INTEGER NOT NULL, ts INTEGER NOT NULL, kind TEXT NOT NULL, detail TEXT);
         CREATE TABLE si_proposals (id INTEGER PRIMARY KEY AUTOINCREMENT, status TEXT NOT NULL);",
    )
    .execute(&pool)
    .await
    .unwrap();
    migrate(&pool).await.unwrap();
    pool
}

async fn add_task(pool: &SqlitePool, state: &str, role: &str, created_at: i64) -> i64 {
    sqlx::query("INSERT INTO tasks (state, role, created_at, updated_at) VALUES (?, ?, ?, ?)")
        .bind(state)
        .bind(role)
        .bind(created_at)
        .bind(created_at)
        .execute(pool)
        .await
        .unwrap()
        .last_insert_rowid()
}

/// 2026-08-24(월) 00:00 KST.
const KST: i64 = 9 * 3600;
const MONDAY_KST: i64 = 1_787_497_200;

#[test]
fn week_start_lands_on_monday_midnight_local() {
    // 그 주 아무 시각이나 같은 월요일로 접힌다.
    let wednesday = MONDAY_KST + 2 * 86_400 + 13 * 3600;
    assert_eq!(week_start_of(wednesday, KST), week_start_of(MONDAY_KST, KST));
    // 월요일 00:00 자신도 그 주에 속한다 — 경계가 앞 주로 새지 않는다.
    assert_eq!(week_start_of(MONDAY_KST, KST), MONDAY_KST);
    // 1초 전은 지난주다.
    assert_eq!(
        week_start_of(MONDAY_KST - 1, KST),
        MONDAY_KST - WEEK_SECS
    );
}

/// 시간대가 다르면 주 경계도 다르다 — 이 비대칭이 `tz_offset_secs`를 인자로 받는 이유다.
#[test]
fn week_start_follows_the_given_offset() {
    let utc = week_start_of(MONDAY_KST, 0);
    assert_ne!(utc, week_start_of(MONDAY_KST, KST));
    assert_eq!(utc.rem_euclid(86_400), 0);
}

#[tokio::test]
async fn facts_count_only_the_target_week() {
    let pool = test_pool().await;
    let start = MONDAY_KST;
    add_task(&pool, "Done", "implementer", start + 3600).await;
    add_task(&pool, "Done", "implementer", start + 7200).await;
    add_task(&pool, "Discarded", "implementer", start + 10_800).await;
    // 다음 주 작업은 세지 않는다.
    add_task(&pool, "Done", "researcher", start + WEEK_SECS + 60).await;
    // 지난주 작업은 직전 비교에만 쓰인다.
    add_task(&pool, "Discarded", "implementer", start - 86_400).await;

    let facts = collect_facts(&pool, start).await.unwrap();
    assert_eq!(facts.tasks_total, 3);
    assert_eq!(facts.tasks_done, 2);
    assert_eq!(facts.tasks_discarded, 1);
    assert_eq!(facts.discard_rate_pct, 33.3);
    assert_eq!(facts.discard_rate_prev_pct, Some(100.0));
    assert_eq!(facts.top_role.unwrap().role, "implementer");
}

/// 후속 입력은 **발생 여부**다. 같은 작업에 이벤트가 둘이어도 한 건으로 센다
/// (`db/mod.rs:297`의 UNIQUE 인덱스가 실제로도 그것을 보장한다).
#[tokio::test]
async fn followup_is_boolean_not_a_count() {
    let pool = test_pool().await;
    let start = MONDAY_KST;
    let first = add_task(&pool, "Done", "implementer", start + 60).await;
    add_task(&pool, "Done", "implementer", start + 120).await;
    for _ in 0..3 {
        sqlx::query("INSERT INTO task_events (task_id, ts, kind) VALUES (?, ?, ?)")
            .bind(first)
            .bind(start)
            .bind("user_followup_input_observed")
            .execute(&pool)
            .await
            .unwrap();
    }

    let facts = collect_facts(&pool, start).await.unwrap();
    assert_eq!(facts.followup_pct, 50.0);
}

/// 빈 주의 폐기율은 0%가 아니라 "비교 대상 없음"이다.
#[tokio::test]
async fn empty_previous_week_has_no_rate() {
    let pool = test_pool().await;
    add_task(&pool, "Done", "implementer", MONDAY_KST + 60).await;
    let facts = collect_facts(&pool, MONDAY_KST).await.unwrap();
    assert_eq!(facts.discard_rate_prev_pct, None);
}

/// 제안 적체는 주 구간으로 자르지 않는다 — 누적된 상태이지 그 주의 사건이 아니다.
#[tokio::test]
async fn proposals_are_cumulative() {
    let pool = test_pool().await;
    add_task(&pool, "Done", "implementer", MONDAY_KST + 60).await;
    for status in ["proposed", "proposed", "rejected", "applied"] {
        sqlx::query("INSERT INTO si_proposals (status) VALUES (?)")
            .bind(status)
            .execute(&pool)
            .await
            .unwrap();
    }
    let facts = collect_facts(&pool, MONDAY_KST).await.unwrap();
    assert_eq!(facts.proposals_pending, 2);
    assert_eq!(facts.proposals_applied, 1);
}

#[tokio::test]
async fn same_week_is_never_written_twice() {
    let pool = test_pool().await;
    add_task(&pool, "Done", "implementer", MONDAY_KST + 60).await;
    let digest = inbox::ValidDigest {
        week_start: MONDAY_KST,
        body: "이번 주에는 작업 한 건이 승인됐고 폐기는 없었다. 흐름은 지난주와 같다.".into(),
    };
    assert!(inbox::store(&pool, &digest, 1).await.unwrap());
    // 두 번째는 거부된다 — 덮어쓰면 어느 서술이 맞는지 알 수 없다.
    assert!(!inbox::store(&pool, &digest, 2).await.unwrap());

    let stored = get(&pool, Some(MONDAY_KST)).await.unwrap().unwrap();
    assert_eq!(stored.facts.tasks_total, 1);
    assert_eq!(list(&pool, 10).await.unwrap().len(), 1);
}

/// 적재 시점에 수치를 다시 만든다 — 에이전트가 준 값을 믿지 않는다(DR-7).
#[tokio::test]
async fn facts_are_recomputed_at_store_time() {
    let pool = test_pool().await;
    add_task(&pool, "Discarded", "tester", MONDAY_KST + 60).await;
    let digest = inbox::ValidDigest {
        week_start: MONDAY_KST,
        body: "폐기가 한 건 있었고 승인은 없었다. 지난주와 견줄 기록은 남아 있지 않다.".into(),
    };
    inbox::store(&pool, &digest, 1).await.unwrap();

    let stored = get(&pool, None).await.unwrap().unwrap();
    assert_eq!(stored.facts.tasks_discarded, 1);
    assert_eq!(stored.facts.discard_rate_pct, 100.0);
}
