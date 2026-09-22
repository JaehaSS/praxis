//! 출처 청크 선택 — 소스 한정과 빈 본문 배제.

use super::test_pool;

/// MVP는 Obsidian만 본다 (설계 0044 DR-6). 개인 메일이 퀴즈로 새어 나가면 안 된다.
#[tokio::test]
async fn only_obsidian_chunks_are_picked() {
    let pool = test_pool().await;
    seed_chunk(&pool, "obsidian", "note.md", "옵시디언 본문").await;
    seed_chunk(&pool, "gmail", "msg-1", "메일 본문").await;

    let picked = crate::quiz::pick_source_chunks(&pool, 10).await.unwrap();

    assert_eq!(picked.len(), 1, "gmail 청크까지 뽑혔다");
    assert_eq!(picked[0].content, "옵시디언 본문");
}

/// 빈 청크로는 문제를 만들 수 없다 — 프롬프트만 늘리고 빈손으로 끝난다.
#[tokio::test]
async fn blank_chunks_are_skipped() {
    let pool = test_pool().await;
    seed_chunk(&pool, "obsidian", "empty.md", "   ").await;
    seed_chunk(&pool, "obsidian", "real.md", "실제 본문").await;

    let picked = crate::quiz::pick_source_chunks(&pool, 10).await.unwrap();

    assert_eq!(picked.len(), 1);
    assert_eq!(picked[0].content, "실제 본문");
}

#[tokio::test]
async fn limit_is_respected() {
    let pool = test_pool().await;
    for i in 0..5 {
        seed_chunk(&pool, "obsidian", &format!("n{i}.md"), "본문").await;
    }

    let picked = crate::quiz::pick_source_chunks(&pool, 3).await.unwrap();

    assert_eq!(picked.len(), 3);
}

#[tokio::test]
async fn no_chunks_yields_empty() {
    let pool = test_pool().await;
    assert!(crate::quiz::pick_source_chunks(&pool, 10)
        .await
        .unwrap()
        .is_empty());
}

async fn seed_chunk(pool: &sqlx::SqlitePool, source: &str, external_id: &str, content: &str) {
    sqlx::query(
        "INSERT INTO knowledge_nodes (source, external_id, kind, title, updated_at, synced_at) \
         VALUES (?, ?, 'document', '제목', 1, 1)",
    )
    .bind(source)
    .bind(external_id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO knowledge_chunks (node_id, ord, doc_title, content) \
         VALUES ((SELECT id FROM knowledge_nodes WHERE external_id = ?), 0, '제목', ?)",
    )
    .bind(external_id)
    .bind(content)
    .execute(pool)
    .await
    .unwrap();
}
