#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory};

async fn setup(label: &str) -> (sqlx::SqlitePool, std::path::PathBuf) {
    let root = temp_root::dir().join(format!(
        "praxis-memory-version-status-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let database = root.join("memory.sqlite");
    let pool = db::init_pool(database.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    (pool, root)
}

async fn memory_with_status(pool: &sqlx::SqlitePool, status: &str) -> i64 {
    let id = memory::create_candidate(
        pool,
        memory::tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::CLAIM,
        "version one",
        Some("test"),
        100,
    )
    .await
    .unwrap();
    memory::update_knowledge(pool, id, "version two", memory::knowledge_type::CLAIM, 200)
        .await
        .unwrap();
    sqlx::query("UPDATE memories SET status = ? WHERE id = ?")
        .bind(status)
        .bind(id)
        .execute(pool)
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn every_lifecycle_status_can_restore_history_as_a_candidate() {
    let (pool, root) = setup("history").await;
    let statuses = [
        memory::knowledge_status::CANDIDATE,
        memory::knowledge_status::PENDING_REVIEW,
        memory::knowledge_status::VERIFIED,
        memory::knowledge_status::STALE,
        memory::knowledge_status::REJECTED,
        memory::knowledge_status::ARCHIVED,
        memory::knowledge_status::LEGACY_UNVERIFIED,
    ];

    for status in statuses {
        let id = memory_with_status(&pool, status).await;
        let version = memory::management::restore_version(&pool, id, 1, 2, status, 300)
            .await
            .unwrap();
        let restored_status: String =
            sqlx::query_scalar("SELECT status FROM memories WHERE id = ?")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!((version, restored_status.as_str()), (3, "candidate"));
    }

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn only_archived_memories_can_restore_the_current_version() {
    let (pool, root) = setup("current").await;
    for status in [
        memory::knowledge_status::CANDIDATE,
        memory::knowledge_status::PENDING_REVIEW,
        memory::knowledge_status::VERIFIED,
        memory::knowledge_status::STALE,
        memory::knowledge_status::REJECTED,
        memory::knowledge_status::LEGACY_UNVERIFIED,
    ] {
        let id = memory_with_status(&pool, status).await;
        let error = memory::management::restore_version(&pool, id, 2, 2, status, 300)
            .await
            .unwrap_err();
        assert_eq!(
            error.kind(),
            memory::restore_error::RestoreFailureKind::Invalid
        );
    }

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
