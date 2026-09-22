use super::test_pool;
use crate::today::{store, suggest};

async fn insert_task(pool: &sqlx::SqlitePool, id: i64, state: &str, updated_at: i64) {
    sqlx::query(
        "INSERT INTO tasks (id, repo, branch, base, worktree_path, instruction, state, created_at, updated_at) \
         VALUES (?, '/r', 'b', 'main', '/w', ?, ?, 0, ?)",
    )
    .bind(id)
    .bind(format!("작업 {id}"))
    .bind(state)
    .bind(updated_at)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn awaiting_picks_tasks_idle_beyond_the_threshold() {
    let pool = test_pool().await;
    let now = 100_000_i64;
    // updated_at이 7시간 전 → 대상
    insert_task(&pool, 1, "AwaitingReview", now - 7 * 3600).await;
    // 1시간 전 → 아직 아님
    insert_task(&pool, 2, "AwaitingReview", now - 3600).await;
    // 오래됐지만 Running → 대상 아님
    insert_task(&pool, 3, "Running", now - 7 * 3600).await;

    let out = suggest::awaiting(&pool, now).await.unwrap();

    assert_eq!(out.len(), 1);
    assert_eq!(out[0].source_ref.as_deref(), Some("1"));
}

#[tokio::test]
async fn suggestions_already_taken_today_are_excluded() {
    let pool = test_pool().await;
    let raw = vec![suggest::Suggestion {
        title: "이슈 #9".into(),
        source: "github".into(),
        source_ref: Some("9".into()),
        repo: None,
    }];
    // 담기: source_ref까지 그대로 넣는다
    store::add_sourced(
        &pool,
        "2026-08-03",
        &raw[0].title,
        None,
        "github",
        raw[0].source_ref.as_deref(),
        300,
    )
    .await
    .unwrap();

    let after = suggest::exclude_taken(&pool, "2026-08-03", raw).await.unwrap();

    assert!(after.is_empty(), "이미 담은 제안은 다시 뜨지 않는다");
}

#[tokio::test]
async fn taken_suggestion_from_a_different_source_is_not_confused() {
    let pool = test_pool().await;
    // memory #1을 담았다고 해서 github #1까지 가려지면 안 된다 (source가 다르다).
    store::add_sourced(
        &pool,
        "2026-08-03",
        "담은 것",
        None,
        "memory",
        Some("1"),
        300,
    )
    .await
    .unwrap();
    let candidates = vec![suggest::Suggestion {
        title: "이슈 #1".into(),
        source: "github".into(),
        source_ref: Some("1".into()),
        repo: None,
    }];

    let after = suggest::exclude_taken(&pool, "2026-08-03", candidates)
        .await
        .unwrap();

    assert_eq!(after.len(), 1);
}

/// `today_take`는 중복 담기를 에러가 아니라 `Ok(None)`으로 삼키는데, 그 판정이
/// 에러 문자열의 "UNIQUE" 포함 여부에 걸려 있다. sqlx가 메시지 형식을 바꾸면 사용자에게
/// 날 에러가 노출되므로, 그 가정을 여기서 못박는다.
#[tokio::test]
async fn duplicate_sourced_add_reports_a_unique_violation() {
    let pool = test_pool().await;
    store::add_sourced(&pool, "2026-08-03", "이슈 7", None, "github", Some("7"), 100)
        .await
        .unwrap();

    let again = store::add_sourced(&pool, "2026-08-03", "이슈 7", None, "github", Some("7"), 200)
        .await;

    let message = again.expect_err("같은 날 같은 출처는 두 번 담기지 않는다");
    assert!(
        message.contains("UNIQUE"),
        "커맨드가 이 문자열로 '이미 담음'을 판정한다. 실제 메시지: {message}"
    );
}

#[test]
fn memory_parser_reads_status_field_under_each_entry() {
    let ledger = "\
### #128 · 2026-08-03 · 오늘 할 일 (Medium)

- **subject**: today/plan
- **status**: partial
- **무엇**: 스키마까지만

### #127 · 2026-08-02 · 다른 작업 (Small)

- **subject**: ui/shell
- **status**: done
- **무엇**: 끝남
";
    let out = suggest::parse_memory_ledger(ledger);
    assert_eq!(out.len(), 1, "done은 제안 대상이 아니다");
    assert_eq!(out[0].source_ref.as_deref(), Some("128"));
    assert!(out[0].title.contains("오늘 할 일"));
}

#[test]
fn memory_parser_returns_empty_on_unexpected_shape() {
    assert!(suggest::parse_memory_ledger("헤더도 없는 아무 텍스트").is_empty());
}

/// 실제 원장 파일의 헤더 형식을 골든으로 박아 둔다 — 형식이 바뀌면 파서가 조용히
/// 0건을 반환하므로(에러가 아니다) 이 테스트가 유일한 회귀 감지 장치다.
#[test]
fn memory_parser_matches_the_real_ledger_header_shape() {
    let real = "### #113 · 2026-08-03 · 금일 할 일 계획 레이어 (Large)\n\n- **subject**: today/plan\n- **status**: pending\n";
    let out = suggest::parse_memory_ledger(real);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].source_ref.as_deref(), Some("113"));
}

#[test]
fn memory_source_returns_empty_when_the_ledger_file_is_absent() {
    assert!(suggest::memory("/그런/경로는/없다").is_empty());
}
