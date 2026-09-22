use super::directory;
use crate::knowledge::config::ObsidianConfig;
use crate::knowledge::wiki::{
    active_node_ids, connect, documents, read_document, replace_legacy, sync,
};

#[tokio::test]
async fn body_search_is_scoped_and_excludes_gmail_nodes() {
    let pool = super::super::super::tests::test_pool().await;
    let first = directory("search-a");
    let second = directory("search-b");
    std::fs::write(first.join("a.md"), "고유본문검색어").unwrap();
    std::fs::write(second.join("b.md"), "고유본문검색어").unwrap();
    let a = connect(&pool, first.to_str().unwrap()).await.unwrap();
    connect(&pool, second.to_str().unwrap()).await.unwrap();
    sqlx::query("INSERT INTO knowledge_nodes (source, external_id, kind, title, updated_at, synced_at) VALUES ('gmail', 'mail-1', 'email', '고유본문검색어', 1, 1)")
        .execute(&pool).await.unwrap();
    sync(&pool, 1).await.unwrap();
    let result = documents(&pool, Some(&a.id), "고유본문검색어")
        .await
        .unwrap();
    assert_eq!(result.documents.len(), 1);
    assert_eq!(result.documents[0].space_id, a.id);
}

#[tokio::test]
async fn short_body_and_title_queries_search_every_chunk() {
    let pool = super::super::super::tests::test_pool().await;
    let root = directory("short-query");
    let space = connect(&pool, root.to_str().unwrap()).await.unwrap();
    sqlx::query("INSERT INTO knowledge_nodes (source, space_id, external_id, kind, title, updated_at, synced_at) VALUES ('wiki', ?, ?, 'document', 'titleonlyunique', 1, 1)")
        .bind(&space.id).bind(format!("{}/note.md", space.id)).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO knowledge_chunks (node_id, ord, doc_title, content) VALUES (1, 0, 'titleonlyunique', 'first chunk')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO knowledge_chunks (node_id, ord, doc_title, content) VALUES (1, 1, 'titleonlyunique', 'xy')")
        .execute(&pool).await.unwrap();
    assert_eq!(
        documents(&pool, Some(&space.id), "xy")
            .await
            .unwrap()
            .documents
            .len(),
        1
    );
    assert_eq!(
        documents(&pool, Some(&space.id), "titleonlyunique")
            .await
            .unwrap()
            .documents
            .len(),
        1
    );
}

#[tokio::test]
async fn reads_reject_traversal_symlinks_exclusions_and_large_files() {
    let root = directory("read-boundary");
    std::fs::write(root.join("note.md"), "body").unwrap();
    assert!(super::super::files::read(&root, "../outside.md", &[]).is_err());
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git").join("secret.md"), "secret").unwrap();
    assert!(super::super::files::read(&root, ".git/secret.md", &[]).is_err());
    std::fs::write(root.join("large.md"), vec![b'x'; 2 * 1024 * 1024 + 1]).unwrap();
    assert!(super::super::files::read(&root, "large.md", &[]).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("/etc/hosts", root.join("escape.md")).unwrap();
        assert!(super::super::files::read(&root, "escape.md", &[]).is_err());
        std::fs::create_dir_all(root.join("real")).unwrap();
        std::fs::write(root.join("real").join("inside.md"), "body").unwrap();
        std::os::unix::fs::symlink(root.join("real"), root.join("linked")).unwrap();
        assert!(super::super::files::read(&root, "linked/inside.md", &[]).is_err());
    }
}

#[tokio::test]
async fn read_document_uses_current_registered_root() {
    let pool = super::super::super::tests::test_pool().await;
    let root = directory("read-indexed");
    std::fs::write(root.join("note.md"), "current body").unwrap();
    connect(&pool, root.to_str().unwrap()).await.unwrap();
    sync(&pool, 1).await.unwrap();
    let (node_id,): (i64,) = sqlx::query_as("SELECT id FROM knowledge_nodes WHERE source = 'wiki'")
        .fetch_one(&pool)
        .await
        .unwrap();
    let document = read_document(&pool, node_id).await.unwrap();
    assert_eq!(document.body, "current body");
    assert!(document.path.ends_with("note.md"));
}

#[tokio::test]
async fn removed_spaces_are_hidden_from_generic_knowledge_results() {
    let pool = super::super::super::tests::test_pool().await;
    let root = directory("removed-visibility");
    std::fs::write(root.join("note.md"), "body").unwrap();
    connect(&pool, root.to_str().unwrap()).await.unwrap();
    sync(&pool, 1).await.unwrap();
    let (node_id,): (i64,) = sqlx::query_as("SELECT id FROM knowledge_nodes WHERE source = 'wiki'")
        .fetch_one(&pool)
        .await
        .unwrap();
    replace_legacy(&pool, &ObsidianConfig::default())
        .await
        .unwrap();
    assert!(active_node_ids(&pool, &[node_id]).await.unwrap().is_empty());
}
