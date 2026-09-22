use sqlx::SqlitePool;

const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS task_issue_refs (
  task_id   INTEGER PRIMARY KEY,
  issue_ref TEXT NOT NULL
);
"#;

/// 확인된 PR URL 캐시. `gh pr view`는 blocking + 네트워크라 알림마다 부르지 않는다.
const PR_MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS task_pr_refs (
  task_id INTEGER PRIMARY KEY,
  pr_url  TEXT NOT NULL
);
"#;

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(MIGRATION).execute(pool).await?;
    sqlx::query(PR_MIGRATION).execute(pool).await?;
    Ok(())
}

pub async fn set_issue_ref(pool: &SqlitePool, task_id: i64, issue_ref: &str) -> anyhow::Result<()> {
    sqlx::query("INSERT OR REPLACE INTO task_issue_refs (task_id, issue_ref) VALUES (?, ?)")
        .bind(task_id)
        .bind(issue_ref)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_issue_ref(pool: &SqlitePool, task_id: i64) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT issue_ref FROM task_issue_refs WHERE task_id = ?")
            .bind(task_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(issue_ref,)| issue_ref))
}

pub async fn set_pr_ref(pool: &SqlitePool, task_id: i64, pr_url: &str) -> anyhow::Result<()> {
    sqlx::query("INSERT OR REPLACE INTO task_pr_refs (task_id, pr_url) VALUES (?, ?)")
        .bind(task_id)
        .bind(pr_url)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn get_pr_ref(pool: &SqlitePool, task_id: i64) -> anyhow::Result<Option<String>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT pr_url FROM task_pr_refs WHERE task_id = ?")
            .bind(task_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(pr_url,)| pr_url))
}
