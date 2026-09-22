#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory};

async fn setup(label: &str) -> (sqlx::SqlitePool, std::path::PathBuf) {
    let root = temp_root::dir().join(format!(
        "praxis-memory-version-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let database = root.join("memory.sqlite");
    let pool = db::init_pool(database.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    (pool, root)
}

async fn candidate(pool: &sqlx::SqlitePool, content: &str) -> i64 {
    memory::create_candidate(
        pool,
        memory::tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::CLAIM,
        content,
        Some("test"),
        100,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn restore_appends_a_candidate_version_without_reusing_evidence() {
    let (pool, root) = setup("append").await;
    let id = candidate(&pool, "version one").await;
    memory::add_user_confirmation(&pool, id, 110, None)
        .await
        .unwrap();
    memory::submit_for_review(&pool, id, 111).await.unwrap();
    memory::approve(&pool, id, "human", 112).await.unwrap();
    memory::update_knowledge(
        &pool,
        id,
        "version two",
        memory::knowledge_type::DECISION,
        200,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(&pool, id, 210, None)
        .await
        .unwrap();
    memory::submit_for_review(&pool, id, 211).await.unwrap();
    memory::approve(&pool, id, "human", 212).await.unwrap();

    let version = memory::management::restore_version(
        &pool,
        id,
        1,
        2,
        memory::knowledge_status::VERIFIED,
        300,
    )
    .await
    .unwrap();

    assert_eq!(version, 3);
    let current: (String, String, String, i64, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT content, knowledge_type, status, current_version, verified_at, archived_at
         FROM memories WHERE id = ?",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        current,
        (
            "version one".into(),
            memory::knowledge_type::CLAIM.into(),
            memory::knowledge_status::CANDIDATE.into(),
            3,
            None,
            None,
        )
    );
    let versions = memory::management::versions(&pool, id).await.unwrap();
    assert_eq!(
        versions
            .iter()
            .map(|row| (row.version, row.evidence_count, row.editor_kind.as_str()))
            .collect::<Vec<_>>(),
        vec![
            (3, 0, "human_restore"),
            (2, 1, "human_edit"),
            (1, 1, "candidate_intake")
        ]
    );
    let payload: String = sqlx::query_scalar(
        "SELECT payload_json FROM memory_events
         WHERE memory_id = ? AND action = 'version_restored'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&payload).unwrap(),
        serde_json::json!({ "previous_version": 2, "source_version": 1 })
    );
    let receipt_versions: Vec<i64> = sqlx::query_scalar(
        "SELECT version FROM memory_approval_receipts
         WHERE memory_id = ? ORDER BY version",
    )
    .bind(id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(receipt_versions, vec![1, 2]);

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn archived_current_version_restores_as_a_new_candidate() {
    let (pool, root) = setup("archive").await;
    let id = candidate(&pool, "archived content").await;
    memory::archive(&pool, id, 200).await.unwrap();

    let version = memory::management::restore_version(
        &pool,
        id,
        1,
        1,
        memory::knowledge_status::ARCHIVED,
        300,
    )
    .await
    .unwrap();

    assert_eq!(version, 2);
    let current: (String, i64, Option<i64>) =
        sqlx::query_as("SELECT status, current_version, archived_at FROM memories WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        current,
        (memory::knowledge_status::CANDIDATE.into(), 2, None)
    );

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
