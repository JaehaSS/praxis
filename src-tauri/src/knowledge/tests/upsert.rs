//! 변경 감지·교체·삭제 검증.

use super::test_pool;
use crate::knowledge::graph::{
    delete_document, deleted_ids, upsert_document, Document, UpsertOutcome,
};

fn doc(external_id: &str, body: &str) -> Document {
    Document::embedded("obsidian", external_id, external_id, body)
}

#[tokio::test]
async fn unchanged_content_skips_reindexing() {
    let pool = test_pool().await;
    let d = doc("note.md", "본문 그대로");
    assert_eq!(
        upsert_document(&pool, &d, 10).await.unwrap(),
        UpsertOutcome::Indexed
    );
    // 해시가 같으므로 재청킹·재임베딩을 하지 않는다.
    assert_eq!(
        upsert_document(&pool, &d, 20).await.unwrap(),
        UpsertOutcome::Skipped
    );

    // 스킵해도 동기화 시각은 갱신돼야 한다 — 안 그러면 "언제 확인했는지"를 잃는다.
    let (synced,): (i64,) = sqlx::query_as("SELECT synced_at FROM knowledge_nodes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(synced, 20);
}

#[tokio::test]
async fn trailing_whitespace_does_not_count_as_a_change() {
    let pool = test_pool().await;
    upsert_document(&pool, &doc("n.md", "본문"), 1).await.unwrap();
    assert_eq!(
        upsert_document(&pool, &doc("n.md", "본문\n\n"), 2)
            .await
            .unwrap(),
        UpsertOutcome::Skipped
    );
}

#[tokio::test]
async fn changed_content_replaces_chunks_without_duplicating_the_node() {
    let pool = test_pool().await;
    upsert_document(&pool, &doc("note.md", "옛 본문"), 1)
        .await
        .unwrap();
    upsert_document(&pool, &doc("note.md", "새 본문"), 2)
        .await
        .unwrap();

    let (nodes,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge_nodes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(nodes, 1, "같은 external_id가 노드를 늘렸다");

    let contents: Vec<(String,)> = sqlx::query_as("SELECT content FROM knowledge_chunks")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert!(
        contents.iter().all(|c| !c.0.contains("옛 본문")),
        "이전 청크가 남았다"
    );

    // 교체 경로의 FTS도 본다. 외부 콘텐츠 인덱스는 CASCADE가 닿지 않고 DELETE 트리거가
    // 받아야만 빠진다 — 노드·청크만 검사하면 이 구멍을 놓친다.
    let (ghosts,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM knowledge_fts WHERE knowledge_fts MATCH '옛 본문'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(ghosts, 0, "교체된 청크가 FTS에 유령으로 남았다");
}

#[tokio::test]
async fn deleting_a_document_clears_chunks_and_index() {
    let pool = test_pool().await;
    upsert_document(&pool, &doc("note.md", "삭제될 고유단어 크세논"), 1)
        .await
        .unwrap();
    delete_document(&pool, "obsidian", "note.md").await.unwrap();

    let (chunks,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge_chunks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(chunks, 0);
    let (hits,): (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM knowledge_fts WHERE knowledge_fts MATCH '크세논'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(hits, 0, "삭제한 문서가 아직 검색된다");
}

#[tokio::test]
async fn multi_chunk_document_keeps_dense_ordinals_after_shrinking() {
    // 문서가 짧아지면 청크 수가 준다. 부분 갱신이면 뒤쪽 ord에 구멍이 남아
    // UNIQUE(node_id, ord) 충돌이나 유령 청크가 생긴다.
    let pool = test_pool().await;
    let long = format!("# 제목\n{}", "문장입니다. ".repeat(1000));
    upsert_document(&pool, &doc("n.md", &long), 1).await.unwrap();
    let (before,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge_chunks")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(before > 1, "픽스처가 여러 청크로 나뉘지 않았다");

    upsert_document(&pool, &doc("n.md", "# 제목\n짧아짐"), 2)
        .await
        .unwrap();
    let ords: Vec<(i64,)> = sqlx::query_as("SELECT ord FROM knowledge_chunks ORDER BY ord")
        .fetch_all(&pool)
        .await
        .unwrap();
    assert_eq!(ords, vec![(0,)]);
}

#[test]
fn deleted_ids_reports_only_the_missing_ones() {
    let stored = vec!["gone.md".to_string(), "still.md".to_string()];
    let present = vec!["still.md".to_string()];
    assert_eq!(deleted_ids(&stored, &present), vec!["gone.md"]);
}

#[test]
fn deleted_ids_is_empty_when_nothing_vanished() {
    let ids = vec!["a.md".to_string()];
    assert!(deleted_ids(&ids, &ids).is_empty());
}
