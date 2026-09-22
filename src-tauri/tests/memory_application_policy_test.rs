//! 메모리 적용 정책(`application_policy`)의 additive 도입 계약.
//!
//! 정책 컬럼은 기존 DB를 건드리지 않고 추가돼야 한다 — 모든 기존 행은 `relevance`로
//! 읽히고, 앱 재시작으로 migrate가 반복 실행돼도 값과 감사 원장이 그대로여야 한다.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory};
use sqlx::SqlitePool;

fn database_path() -> String {
    temp_root::dir()
        .join(format!(
            "praxis-memory-application-policy-{}.sqlite",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

fn remove_database_files(path: &str) {
    for target in [
        path.to_string(),
        format!("{path}-wal"),
        format!("{path}-shm"),
    ] {
        let _ = std::fs::remove_file(target);
    }
}

async fn count_memory_events(pool: &SqlitePool) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM memory_events")
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn existing_rows_default_to_relevance_and_migration_is_idempotent() {
    let path = database_path();
    remove_database_files(&path);
    let pool = db::init_pool(&path).await.unwrap();
    memory::migrate(&pool).await.unwrap();

    for content in ["첫 규칙", "둘째 규칙"] {
        memory::create_candidate(
            &pool,
            memory::tier::PROJECT,
            Some("/repo"),
            memory::knowledge_type::DECISION,
            content,
            Some("test"),
            100,
        )
        .await
        .unwrap();
    }

    let first = memory::list_all(&pool).await.unwrap();
    assert_eq!(first.len(), 2);
    assert!(
        first
            .iter()
            .all(|m| m.application_policy == memory::application_policy::policy::RELEVANCE),
        "정책을 지정한 적이 없는 행은 relevance로 읽혀야 한다"
    );
    let events_before = count_memory_events(&pool).await;

    // 앱 재시작 경로 — 같은 DB에 migrate를 다시 적용한다.
    memory::migrate(&pool).await.unwrap();

    let second = memory::list_all(&pool).await.unwrap();
    assert_eq!(
        second
            .iter()
            .map(|m| m.application_policy.as_str())
            .collect::<Vec<_>>(),
        first
            .iter()
            .map(|m| m.application_policy.as_str())
            .collect::<Vec<_>>(),
        "migration 재실행이 정책 값을 바꾸면 안 된다"
    );
    assert_eq!(
        count_memory_events(&pool).await,
        events_before,
        "migration은 감사 event를 만들면 안 된다"
    );

    remove_database_files(&path);
}

#[tokio::test]
async fn policy_constants_reject_unknown_values() {
    use memory::application_policy::policy;
    assert!(policy::is_valid(policy::RELEVANCE));
    assert!(policy::is_valid(policy::MUST_APPLY));
    assert!(!policy::is_valid("always"));
    assert!(!policy::is_valid(""));
}
