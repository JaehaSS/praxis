//! 메모리 인용 관측 원장 — 스키마 불변성·멱등 관측, 판정 플로우 (설계 0048 · 플랜 0047).
//!
//! 원장은 관측된 긍정 신호만 담고(append-only), 같은 (task, memory, version, method) 재관측은
//! 행을 늘리지 않는다 — convo 모드는 한 작업에 턴이 여러 번 끝나기 때문이다.

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::{db, memory};

static COUNTER: AtomicU32 = AtomicU32::new(0);

async fn pool() -> sqlx::SqlitePool {
    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = temp_root::dir().join(format!(
        "praxis-citation-{}-{suffix}.sqlite",
        std::process::id()
    ));
    let pool = db::init_pool(path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    pool
}

async fn insert_citation(pool: &sqlx::SqlitePool, method: &str, verdict: &str) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO memory_citations \
         (task_id, memory_id, version, application_policy, method, verdict, created_at) \
         VALUES (1, 10, 1, 'relevance', ?, ?, 0)",
    )
    .bind(method)
    .bind(verdict)
    .execute(pool)
    .await?;
    Ok(())
}

#[tokio::test]
async fn citations_are_append_only() {
    let pool = pool().await;
    insert_citation(&pool, "marker", "cited").await.unwrap();
    assert!(
        sqlx::query("UPDATE memory_citations SET verdict = 'uncertain'")
            .execute(&pool)
            .await
            .is_err(),
        "update must be rejected by immutability trigger"
    );
    assert!(
        sqlx::query("DELETE FROM memory_citations")
            .execute(&pool)
            .await
            .is_err(),
        "delete must be rejected by immutability trigger"
    );
}

#[tokio::test]
async fn duplicate_observation_is_ignored() {
    let pool = pool().await;
    insert_citation(&pool, "marker", "cited").await.unwrap();
    insert_citation(&pool, "marker", "cited").await.unwrap();
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memory_citations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 1, "same (task, memory, version, method) must collapse to one row");
}

const RECEIPTS_JSON: &str = r#"[
  {"memory_id":10,"version":1,"content":"머지는 항상 로컬에서 한다","knowledge_type":"decision","application_policy":"must_apply","evidence":[]},
  {"memory_id":11,"version":2,"content":"projection은 자기 바이트만 책임진다","knowledge_type":"claim","application_policy":"relevance","evidence":[]}
]"#;

async fn insert_applied_journal(pool: &sqlx::SqlitePool, task_id: i64) {
    sqlx::query(
        "INSERT INTO memory_projection_journal \
         (task_id, state, worktree_path, target_paths_json, target_hash, renderer_version, \
          ordered_memories_json, source_check_ids_json, preimages_json, created_at, updated_at) \
         VALUES (?, 'applied', '/tmp/praxis-citation', '[]', 'hash', 3, ?, '[]', NULL, 0, 0)",
    )
    .bind(task_id)
    .bind(RECEIPTS_JSON)
    .execute(pool)
    .await
    .unwrap();
}

async fn citation_rows(pool: &sqlx::SqlitePool) -> Vec<(i64, i64, String, String, String)> {
    sqlx::query_as(
        "SELECT task_id, memory_id, application_policy, method, verdict \
         FROM memory_citations ORDER BY memory_id, method",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

#[tokio::test]
async fn observe_records_marker_citations_idempotently() {
    let pool = pool().await;
    insert_applied_journal(&pool, 7).await;
    let transcript = "M-10 규칙에 따라 로컬 머지로 진행했다. M-999는 receipt에 없다.";

    for _ in 0..2 {
        // convo 멀티턴 — 재관측이 행을 늘리면 안 된다.
        let ride = memory::citation::observe(&pool, 7, Some("sess-1"), transcript, 100)
            .await
            .unwrap();
        let ride = ride.expect("주입이 있으므로 합승 재료가 나와야 한다");
        assert!(ride.fragment.contains("PRAXIS_CITATIONS"));
        assert!(ride.fragment.contains("- 10:"));
    }

    let rows = citation_rows(&pool).await;
    assert_eq!(rows.len(), 1, "M-10 marker 1행만 (M-11 미인용, M-999 무시)");
    let (task_id, memory_id, policy, method, verdict) = &rows[0];
    assert_eq!((*task_id, *memory_id), (7, 10));
    assert_eq!((policy.as_str(), method.as_str(), verdict.as_str()), ("must_apply", "marker", "cited"));
}

#[tokio::test]
async fn record_llm_whitelists_receipt_ids() {
    let pool = pool().await;
    insert_applied_journal(&pool, 8).await;
    let ride = memory::citation::observe(&pool, 8, None, "마커 없는 세션", 0)
        .await
        .unwrap()
        .unwrap();

    let stdout = "[]\nPRAXIS_CITATIONS: {\"cited\":[11,999],\"uncertain\":[10]}\n";
    let (_, section) = memory::citation::split_section(stdout);
    let n = memory::citation::record_llm(&pool, 8, None, &ride, &section.unwrap(), 5)
        .await
        .unwrap();
    assert_eq!(n, 2, "receipt에 없는 999는 버린다 (환각·주입 방어)");

    let rows = citation_rows(&pool).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(
        (rows[0].1, rows[0].3.as_str(), rows[0].4.as_str()),
        (10, "llm", "uncertain")
    );
    assert_eq!(
        (rows[1].1, rows[1].3.as_str(), rows[1].4.as_str()),
        (11, "llm", "cited")
    );
}

#[tokio::test]
async fn observe_without_injection_is_noop() {
    let pool = pool().await;
    let ride = memory::citation::observe(&pool, 42, None, "M-10 이 있어도 주입이 없었다", 0)
        .await
        .unwrap();
    assert!(ride.is_none());
    assert!(citation_rows(&pool).await.is_empty());
}

#[tokio::test]
async fn citation_summary_splits_by_policy() {
    let pool = pool().await;
    insert_applied_journal(&pool, 9).await;
    memory::citation::observe(&pool, 9, None, "M-10 규칙대로 진행", 0)
        .await
        .unwrap();

    let counts = memory::citation::summary(&pool, 9).await.unwrap().unwrap();
    assert_eq!(
        (counts.must_apply_injected, counts.must_apply_cited),
        (1, 1),
        "M-10은 must_apply이고 인용됐다"
    );
    assert_eq!(
        (counts.relevance_injected, counts.relevance_cited),
        (1, 0),
        "M-11은 relevance이고 미인용"
    );

    assert!(
        memory::citation::summary(&pool, 999).await.unwrap().is_none(),
        "주입 없던 작업은 집계 자체가 없다"
    );
}

#[tokio::test]
async fn marker_verdict_is_binary() {
    let pool = pool().await;
    // marker는 이진 판정이라 cited만 산출한다 — uncertain은 llm 전용 (플랜 DR-P2).
    // 일반 INSERT는 CHECK가 거부하고, OR IGNORE 경로에서도 행이 남지 않아야 한다
    // (SQLite의 OR IGNORE는 CHECK 위반을 에러 없이 무시한다).
    assert!(sqlx::query(
        "INSERT INTO memory_citations \
         (task_id, memory_id, version, application_policy, method, verdict, created_at) \
         VALUES (1, 10, 1, 'relevance', 'marker', 'uncertain', 0)",
    )
    .execute(&pool)
    .await
    .is_err());
    insert_citation(&pool, "marker", "uncertain").await.unwrap();
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memory_citations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 0, "invalid combo must never land in the ledger");
    insert_citation(&pool, "llm", "uncertain").await.unwrap();
}
