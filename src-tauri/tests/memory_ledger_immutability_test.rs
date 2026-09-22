#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory};

async fn setup(label: &str) -> (sqlx::SqlitePool, std::path::PathBuf, i64) {
    let root = temp_root::dir().join(format!(
        "praxis-memory-ledger-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let database = root.join("memory.sqlite");
    let pool = db::init_pool(database.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let id = memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::CLAIM,
        "original",
        Some("test"),
        100,
    )
    .await
    .unwrap();
    (pool, root, id)
}

async fn assert_guards(pool: &sqlx::SqlitePool, id: i64) {
    for statement in [
        "UPDATE memory_versions SET content = 'tampered' WHERE memory_id = ?",
        "DELETE FROM memory_versions WHERE memory_id = ?",
        "UPDATE memory_events SET action = 'tampered' WHERE memory_id = ?",
        "DELETE FROM memory_events WHERE memory_id = ?",
    ] {
        assert!(sqlx::query(statement).bind(id).execute(pool).await.is_err());
    }
}

#[tokio::test]
async fn versions_and_events_reject_update_and_delete() {
    let (pool, root, id) = setup("guards").await;

    assert_guards(&pool, id).await;
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT content FROM memory_versions WHERE memory_id = ? AND version = 1",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap(),
        "original"
    );

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn repeat_migration_restores_missing_guards_without_rewriting_history() {
    let (pool, root, id) = setup("repeat").await;
    for trigger in [
        "memory_versions_immutable",
        "memory_versions_no_delete",
        "memory_events_immutable",
        "memory_events_no_delete",
    ] {
        sqlx::query(&format!("DROP TRIGGER {trigger}"))
            .execute(&pool)
            .await
            .unwrap();
    }

    memory::migrate(&pool).await.unwrap();

    assert_guards(&pool, id).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM memory_events WHERE memory_id = ?",)
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
