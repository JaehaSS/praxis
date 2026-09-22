use super::directory;
use crate::knowledge::wiki::{connect, sync, WikiSpace};

#[tokio::test]
async fn wiki_sync_marks_new_documents_eligible_without_embedding_them() {
    let (pool, space) = synced_space("eligible").await;
    let row: (bool, Option<Vec<u8>>) = sqlx::query_as(
        "SELECT embed_enabled, embedding FROM knowledge_nodes n JOIN knowledge_chunks c ON c.node_id = n.id WHERE n.space_id = ?",
    )
    .bind(space.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(row.0);
    assert!(row.1.is_none());
}

#[tokio::test]
async fn wiki_fts_refresh_preserves_existing_embeddings() {
    let (pool, space) = synced_space("preserve").await;
    set_embedding(&pool).await;
    sync(&pool, 2).await.unwrap();
    let row: (Option<Vec<u8>>, Option<String>) = sqlx::query_as(
        "SELECT embedding, embed_model FROM knowledge_chunks c JOIN knowledge_nodes n ON n.id = c.node_id WHERE n.space_id = ?",
    )
    .bind(space.id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, Some(vec![1, 2, 3]));
    assert_eq!(row.1.as_deref(), Some("test-model"));
}

#[tokio::test]
async fn exclusion_clears_existing_embeddings_for_unchanged_documents() {
    let (pool, space) = synced_space("exclude").await;
    set_embedding(&pool).await;
    set_embed_exclude(&pool, &space, vec!["note.md"]).await;
    sync(&pool, 2).await.unwrap();
    assert_state(&pool, &space, false).await;

    // 재갱신이 NULL 청크를 다시 UPDATE하면 실제 FTS 트리거도 재실행된다.
    sqlx::query(
        "CREATE TRIGGER reject_redundant_vector_clear BEFORE UPDATE ON knowledge_chunks \
         WHEN OLD.embedding IS NULL AND OLD.embed_model IS NULL \
         BEGIN SELECT RAISE(ABORT, 'unexpected unchanged chunk rewrite'); END",
    )
    .execute(&pool)
    .await
    .unwrap();
    sync(&pool, 3).await.unwrap();
}

#[tokio::test]
async fn reallowing_an_unchanged_document_queues_it_without_embedding() {
    let (pool, space) = synced_space("reallow").await;
    set_embedding(&pool).await;
    set_embed_exclude(&pool, &space, vec!["note.md"]).await;
    sync(&pool, 2).await.unwrap();
    set_embed_exclude(&pool, &space, vec![]).await;
    sync(&pool, 3).await.unwrap();
    assert_state(&pool, &space, true).await;
}

async fn synced_space(name: &str) -> (sqlx::SqlitePool, WikiSpace) {
    let pool = super::super::super::tests::test_pool().await;
    let root = directory(name);
    std::fs::write(root.join("note.md"), "same body").unwrap();
    let space = connect(&pool, root.to_str().unwrap()).await.unwrap();
    sync(&pool, 1).await.unwrap();
    (pool, space)
}

async fn set_embedding(pool: &sqlx::SqlitePool) {
    sqlx::query("UPDATE knowledge_chunks SET embedding = ?, embed_model = 'test-model'")
        .bind(vec![1_u8, 2, 3])
        .execute(pool)
        .await
        .unwrap();
}

async fn set_embed_exclude(pool: &sqlx::SqlitePool, space: &WikiSpace, exclude: Vec<&str>) {
    let config = serde_json::json!({"spaces":[{
        "id": space.id, "name": space.name, "root": space.root,
        "exclude": [], "embed_exclude": exclude
    }]});
    sqlx::query("UPDATE knowledge_sources SET config = ? WHERE id = 'wiki'")
        .bind(config.to_string())
        .execute(pool)
        .await
        .unwrap();
}

async fn assert_state(pool: &sqlx::SqlitePool, space: &WikiSpace, enabled: bool) {
    let row: (bool, Option<Vec<u8>>, Option<String>) = sqlx::query_as(
        "SELECT n.embed_enabled, c.embedding, c.embed_model FROM knowledge_nodes n JOIN knowledge_chunks c ON c.node_id = n.id WHERE n.space_id = ?",
    )
    .bind(&space.id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(row.0, enabled);
    assert!(row.1.is_none());
    assert!(row.2.is_none());
}
