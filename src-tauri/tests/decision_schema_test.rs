#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::{db, decision};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_db(label: &str) -> String {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    temp_root::dir()
        .join(format!(
            "praxis-decision-{label}-{}-{sequence}.sqlite",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

async fn table_count(pool: &sqlx::SqlitePool, name: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?")
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn columns(pool: &sqlx::SqlitePool, table: &str) -> Vec<String> {
    let query = format!("SELECT name FROM pragma_table_info('{table}') ORDER BY cid");
    sqlx::query_scalar(&query).fetch_all(pool).await.unwrap()
}

#[tokio::test]
async fn migration_is_additive_idempotent_and_disabled_by_default() {
    let path = temp_db("schema");
    let pool = db::init_pool(&path).await.unwrap();
    assert!(!decision::is_enabled(&pool).await.unwrap());
    assert_schema(&pool).await;
    drop(pool);
    assert_reopened_schema(&path).await;
    let _ = std::fs::remove_file(path);
}

async fn assert_schema(pool: &sqlx::SqlitePool) {
    for table in [
        "decision_records",
        "decision_artifact_links",
        "local_approval_finalizations",
    ] {
        assert_eq!(table_count(pool, table).await, 1, "missing {table}");
    }
    assert_eq!(
        columns(pool, "decision_records").await,
        [
            "id",
            "decision_key_hash",
            "kind",
            "outcome",
            "actor_kind",
            "task_id",
            "summary",
            "status",
            "created_at",
            "redacted_at",
        ]
    );
    assert_eq!(
        columns(pool, "local_approval_finalizations").await,
        [
            "task_id",
            "state",
            "commit_sha",
            "exclude_generated_mcp",
            "failure_code",
            "created_at",
            "updated_at",
        ]
    );
}

async fn assert_reopened_schema(path: &str) {
    let reopened = db::init_pool(path).await.unwrap();
    assert_eq!(table_count(&reopened, "decision_records").await, 1);
    let records: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM decision_records")
        .fetch_one(&reopened)
        .await
        .unwrap();
    assert_eq!(records, 0, "migration must not create ledger rows");
    drop(reopened);
}

#[tokio::test]
async fn migration_replaces_a_stale_immutable_trigger() {
    let path = temp_db("trigger");
    let pool = db::init_pool(&path).await.unwrap();
    sqlx::raw_sql(
        "DROP TRIGGER decision_records_immutable;
         CREATE TRIGGER decision_records_immutable
         BEFORE UPDATE ON decision_records
         BEGIN SELECT RAISE(ABORT, 'stale trigger'); END;",
    )
    .execute(&pool)
    .await
    .unwrap();

    decision::migrate(&pool).await.unwrap();

    let trigger_sql: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'decision_records_immutable'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!trigger_sql.contains("stale trigger"));
    assert!(trigger_sql.contains("decision record is immutable"));
    drop(pool);
    let _ = std::fs::remove_file(path);
}
