use sqlx::SqlitePool;

const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS partial_checkpoints (
  task_id    INTEGER PRIMARY KEY,
  commit_sha TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
"#;

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(MIGRATION).execute(pool).await?;
    Ok(())
}

pub async fn save_checkpoint(
    pool: &SqlitePool,
    task_id: i64,
    commit_sha: &str,
    created_at: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO partial_checkpoints (task_id, commit_sha, created_at) VALUES (?, ?, ?) \
         ON CONFLICT(task_id) DO UPDATE SET commit_sha = excluded.commit_sha, \
         created_at = excluded.created_at",
    )
    .bind(task_id)
    .bind(commit_sha)
    .bind(created_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_checkpoint(pool: &SqlitePool, task_id: i64) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT commit_sha FROM partial_checkpoints WHERE task_id = ?")
            .bind(task_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(sha,)| sha))
}

pub async fn clear_checkpoint(pool: &SqlitePool, task_id: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM partial_checkpoints WHERE task_id = ?")
        .bind(task_id)
        .execute(pool)
        .await?;
    Ok(())
}
