//! 스키마 — 멱등성과 출처 CASCADE.

use super::{raw_pool, test_pool};

/// `migrate`는 앱 기동마다 호출된다 — 두 번째 호출이 깨지면 기존 설치가 전부 못 뜬다.
#[tokio::test]
async fn migrate_is_idempotent() {
    let pool = raw_pool().await;
    crate::quiz::migrate(&pool).await.unwrap();
    crate::quiz::migrate(&pool).await.unwrap();
}

/// 출처 청크가 사라진 도메인 문제는 검증할 수 없다 — 함께 지운다(설계 0044 비즈니스 규칙 4).
#[tokio::test]
async fn deleting_a_chunk_deletes_its_domain_items() {
    let pool = test_pool().await;
    seed_chunk(&pool).await;
    sqlx::query(
        "INSERT INTO quiz_items (kind, question, answer, chunk_id, status, created_at) \
         VALUES ('domain', '질문', '답', (SELECT id FROM knowledge_chunks), 'pending', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("DELETE FROM knowledge_chunks")
        .execute(&pool)
        .await
        .unwrap();

    let (items,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM quiz_items")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        items, 0,
        "CASCADE가 걸리지 않았다 — 출처 없는 도메인 문제가 남는다"
    );
}

/// CASCADE가 과하게 걸리면 안 된다 — 출처가 없는 종류(vocab 등)는 청크 삭제와 무관하다.
#[tokio::test]
async fn chunkless_items_survive_chunk_deletion() {
    let pool = test_pool().await;
    seed_chunk(&pool).await;
    sqlx::query(
        "INSERT INTO quiz_items (kind, question, answer, status, created_at) \
         VALUES ('vocab', 'epistemic?', '인식론적', 'approved', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("DELETE FROM knowledge_chunks")
        .execute(&pool)
        .await
        .unwrap();

    let (items,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM quiz_items")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(items, 1, "출처 없는 문제까지 지워졌다");
}

/// 문제가 사라지면 그 문제의 풀이 기록도 남을 이유가 없다.
#[tokio::test]
async fn deleting_an_item_deletes_its_attempts() {
    let pool = test_pool().await;
    sqlx::query(
        "INSERT INTO quiz_items (kind, question, answer, status, created_at) \
         VALUES ('trivia', '질문', '답', 'approved', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO quiz_attempts (item_id, state, opened_at) \
         VALUES ((SELECT id FROM quiz_items), 'open', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    sqlx::query("DELETE FROM quiz_items")
        .execute(&pool)
        .await
        .unwrap();

    let (attempts,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM quiz_attempts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(attempts, 0);
}

/// 청크 하나를 세운다 (`knowledge/tests/schema.rs:175`와 같은 형태).
async fn seed_chunk(pool: &sqlx::SqlitePool) {
    sqlx::query(
        "INSERT INTO knowledge_nodes (source, external_id, kind, title, updated_at, synced_at) \
         VALUES ('obsidian', 'n.md', 'document', '제목', 1, 1)",
    )
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO knowledge_chunks (node_id, ord, content) \
         VALUES ((SELECT id FROM knowledge_nodes), 0, '본문')",
    )
    .execute(pool)
    .await
    .unwrap();
}
