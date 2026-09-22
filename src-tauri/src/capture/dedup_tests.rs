use super::*;

use std::sync::atomic::{AtomicU32, Ordering};

static DATABASE_COUNTER: AtomicU32 = AtomicU32::new(0);

#[tokio::test]
async fn no_embedding_fallback_compares_unverified_candidates_in_the_same_scope() {
    let pool = test_pool().await;
    let repository = "/no-embedding-dedupe";
    memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some(repository),
        memory::knowledge_type::CONVENTION,
        "이 레포는  SQLx WAL 모드로 SQLite를 쓴다",
        Some("convo-task-1"),
        1,
    )
    .await
    .unwrap();

    let duplicate = is_duplicate(
        &pool,
        repository,
        "이 레포는 sqlx wal 모드로 sqlite를 쓴다",
        None,
    )
    .await
    .unwrap();

    assert!(duplicate);
}

#[tokio::test]
async fn no_embedding_fallback_does_not_cross_project_scopes() {
    let pool = test_pool().await;
    memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some("/other-project"),
        memory::knowledge_type::CONVENTION,
        "검증은 cargo test로 실행한다",
        Some("convo-task-2"),
        1,
    )
    .await
    .unwrap();

    let duplicate = is_duplicate(
        &pool,
        "/current-project",
        "검증은 cargo test로 실행한다",
        None,
    )
    .await
    .unwrap();

    assert!(!duplicate);
}

#[tokio::test]
async fn no_embedding_fallback_keeps_distinct_content_in_the_same_scope() {
    let pool = test_pool().await;
    let repository = "/distinct-content";
    memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some(repository),
        memory::knowledge_type::CONVENTION,
        "검증은 cargo test로 실행한다",
        Some("convo-task-3"),
        1,
    )
    .await
    .unwrap();

    let duplicate = is_duplicate(
        &pool,
        repository,
        "포맷은 rustfmt --edition 2021로 실행한다",
        None,
    )
    .await
    .unwrap();

    assert!(!duplicate);
}

#[tokio::test]
async fn embedding_path_uses_scope_local_cosine_similarity() {
    let pool = test_pool().await;
    let repository = "/embedding-dedupe";
    let memory_id = memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some(repository),
        memory::knowledge_type::CLAIM,
        "임베딩 비교 기준",
        Some("convo-task-4"),
        1,
    )
    .await
    .unwrap();
    memory::set_embedding(&pool, memory_id, &[1.0, 0.0])
        .await
        .unwrap();

    let matching = is_duplicate(&pool, repository, "새 후보", Some(&[1.0, 0.0]))
        .await
        .unwrap();
    let distinct = is_duplicate(&pool, repository, "새 후보", Some(&[0.0, 1.0]))
        .await
        .unwrap();

    assert!(matching);
    assert!(!distinct);
}

async fn test_pool() -> sqlx::SqlitePool {
    let sequence = DATABASE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = crate::testtmp::dir().join(format!(
        "praxis-capture-dedup-{}-{sequence}.sqlite",
        std::process::id()
    ));
    let pool = crate::db::init_pool(path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    pool
}
