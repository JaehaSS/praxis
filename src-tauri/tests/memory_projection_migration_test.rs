//! Additive migration coverage for databases created before projection receipts.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory};

#[tokio::test]
async fn old_usage_and_injection_tables_upgrade_idempotently() {
    let db_path = temp_root::dir().join(format!(
        "praxis-projection-migration-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&db_path);
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    sqlx::raw_sql(
        "CREATE TABLE memory_injections (\
           id INTEGER PRIMARY KEY AUTOINCREMENT, memory_id INTEGER NOT NULL, version INTEGER NOT NULL, \
           task_id INTEGER NOT NULL, target_hash TEXT NOT NULL, injected_at INTEGER NOT NULL, outcome TEXT); \
         CREATE TABLE memory_usages (\
           id INTEGER PRIMARY KEY AUTOINCREMENT, memory_id INTEGER NOT NULL, task_id INTEGER NOT NULL, \
           injected_at INTEGER NOT NULL, outcome TEXT);",
    )
    .execute(&pool)
    .await
    .unwrap();

    memory::migrate(&pool).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    for (table, column) in [
        ("memory_injections", "projection_id"),
        ("memory_injections", "evidence_snapshot_json"),
        ("memory_injections", "target_paths_json"),
        ("memory_injections", "renderer_version"),
        ("memory_usages", "injection_id"),
    ] {
        let query = format!("SELECT 1 FROM pragma_table_info('{table}') WHERE name = ?");
        let exists: Option<(i64,)> = sqlx::query_as(&query)
            .bind(column)
            .fetch_optional(&pool)
            .await
            .unwrap();
        assert!(exists.is_some(), "missing {table}.{column}");
    }
    let journals: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'memory_projection_journal'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(journals.0, 1);
    let _ = std::fs::remove_file(db_path);
}
