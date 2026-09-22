use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::db;

pub(super) async fn claim(
    pool: &SqlitePool,
    task_id: i64,
    exclude_generated_mcp: bool,
    now: i64,
    require_enabled: bool,
) -> anyhow::Result<db::Task> {
    let mut tx = pool.begin().await?;
    if require_enabled {
        ensure_feature_enabled(&mut tx).await?;
    }
    lock_task(&mut tx, task_id, now).await?;
    prepare(&mut tx, task_id, exclude_generated_mcp, now).await?;
    let task = sqlx::query_as::<_, db::Task>("SELECT * FROM tasks WHERE id = ?")
        .bind(task_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(task)
}

async fn ensure_feature_enabled(tx: &mut Transaction<'_, Sqlite>) -> anyhow::Result<()> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(super::FLAG_KEY)
        .fetch_optional(&mut **tx)
        .await?;
    if value.as_deref() != Some("true") {
        anyhow::bail!("decision ledger became disabled before approval claim");
    }
    Ok(())
}

async fn lock_task(tx: &mut Transaction<'_, Sqlite>, task_id: i64, now: i64) -> anyhow::Result<()> {
    let state: Option<String> = sqlx::query_scalar("SELECT state FROM tasks WHERE id = ?")
        .bind(task_id)
        .fetch_optional(&mut **tx)
        .await?;
    match state.as_deref() {
        Some(db::state::AWAITING_REVIEW) => claim_review_task(tx, task_id, now).await,
        Some(db::state::FINALIZING) => require_existing_journal(tx, task_id).await,
        Some(_) => anyhow::bail!("검토 대기 상태의 작업만 승인할 수 있습니다"),
        None => anyhow::bail!("작업을 찾을 수 없습니다"),
    }
}

async fn claim_review_task(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: i64,
    now: i64,
) -> anyhow::Result<()> {
    let updated =
        sqlx::query("UPDATE tasks SET state = ?, updated_at = ? WHERE id = ? AND state = ?")
            .bind(db::state::FINALIZING)
            .bind(now)
            .bind(task_id)
            .bind(db::state::AWAITING_REVIEW)
            .execute(&mut **tx)
            .await?;
    if updated.rows_affected() != 1 {
        anyhow::bail!("local approval task claim was lost");
    }
    Ok(())
}

async fn require_existing_journal(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: i64,
) -> anyhow::Result<()> {
    let exists: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM local_approval_finalizations WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&mut **tx)
            .await?;
    if exists != 1 {
        anyhow::bail!("finalizing task is owned by another finalizer");
    }
    Ok(())
}

async fn prepare(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: i64,
    exclude_generated_mcp: bool,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO local_approval_finalizations \
         (task_id, state, exclude_generated_mcp, created_at, updated_at) \
         VALUES (?, 'prepared', ?, ?, ?)",
    )
    .bind(task_id)
    .bind(exclude_generated_mcp)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    let frozen: bool = sqlx::query_scalar(
        "SELECT exclude_generated_mcp FROM local_approval_finalizations WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_one(&mut **tx)
    .await?;
    if frozen != exclude_generated_mcp {
        anyhow::bail!("local approval commit policy conflicts with the durable journal");
    }
    Ok(())
}
