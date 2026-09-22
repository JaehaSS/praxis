#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory};

async fn setup(name: &str) -> (sqlx::SqlitePool, std::path::PathBuf) {
    let root = temp_root::dir().join(format!(
        "praxis-memory-archive-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let database = root.join("memory.sqlite");
    let pool = db::init_pool(database.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    (pool, root)
}

async fn candidate(pool: &sqlx::SqlitePool) -> i64 {
    memory::create_candidate(
        pool,
        memory::tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::CONVENTION,
        "retain archive audit",
        Some("test"),
        2_000_000_000,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn archive_commits_state_and_audit_event_together() {
    let (pool, root) = setup("success").await;
    let id = candidate(&pool).await;

    memory::archive(&pool, id, 2_000_000_001).await.unwrap();

    let state: (String, Option<i64>) =
        sqlx::query_as("SELECT status, archived_at FROM memories WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let events: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memory_events WHERE memory_id = ? AND action = 'archived'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(state, ("archived".to_string(), Some(2_000_000_001)));
    assert_eq!(events, 1);

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn archive_rolls_back_state_when_audit_event_fails() {
    let (pool, root) = setup("rollback").await;
    let id = candidate(&pool).await;
    sqlx::query(
        "CREATE TRIGGER reject_archive_event BEFORE INSERT ON memory_events
         WHEN NEW.action = 'archived'
         BEGIN SELECT RAISE(ABORT, 'archive audit rejected'); END",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert!(memory::archive(&pool, id, 2_000_000_001).await.is_err());

    let state: (String, Option<i64>) =
        sqlx::query_as("SELECT status, archived_at FROM memories WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, ("candidate".to_string(), None));

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
