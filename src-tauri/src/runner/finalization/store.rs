use serde::Deserialize;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::db;

#[derive(Clone, Debug, Deserialize, sqlx::FromRow)]
pub(super) struct FinalizationRow {
    pub decision: String,
    pub state: String,
    pub commit_sha: Option<String>,
}

pub(super) async fn claim(
    pool: &SqlitePool,
    task_id: i64,
    decision: &str,
    now: i64,
) -> anyhow::Result<db::Task> {
    let mut tx = pool.begin().await?;
    lock_task_for_finalization(&mut tx, task_id, now).await?;
    prepare_journal(&mut tx, task_id, decision, now).await?;
    tx.commit().await?;
    db::get_task(pool, task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("작업을 찾을 수 없습니다"))
}

async fn lock_task_for_finalization(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: i64,
    now: i64,
) -> anyhow::Result<()> {
    let state: Option<(String,)> = sqlx::query_as("SELECT state FROM tasks WHERE id = ?")
        .bind(task_id)
        .fetch_optional(&mut **tx)
        .await?;
    match state.as_ref().map(|row| row.0.as_str()) {
        Some(db::state::AWAITING_REVIEW) => {
            sqlx::query("UPDATE tasks SET state = ?, updated_at = ? WHERE id = ? AND state = ?")
                .bind(db::state::FINALIZING)
                .bind(now)
                .bind(task_id)
                .bind(db::state::AWAITING_REVIEW)
                .execute(&mut **tx)
                .await?;
        }
        Some(db::state::FINALIZING) => {}
        Some(_) => anyhow::bail!("검토 대기 상태의 작업만 승인하거나 버릴 수 있습니다"),
        None => anyhow::bail!("작업을 찾을 수 없습니다"),
    }
    Ok(())
}

async fn prepare_journal(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: i64,
    decision: &str,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO runner_finalizations \
         (task_id, decision, state, created_at, updated_at) VALUES (?, ?, 'prepared', ?, ?)",
    )
    .bind(task_id)
    .bind(decision)
    .bind(now)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    let row: (String, String) =
        sqlx::query_as("SELECT decision, state FROM runner_finalizations WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&mut **tx)
            .await?;
    if row.0 != decision {
        let changed = sqlx::query(
            "UPDATE runner_finalizations SET decision = ?, state = 'prepared', \
             commit_sha = NULL, failure_reason = NULL, updated_at = ? \
             WHERE task_id = ? AND state NOT IN ('merged', 'cleaned', 'completed')",
        )
        .bind(decision)
        .bind(now)
        .bind(task_id)
        .execute(&mut **tx)
        .await?
        .rows_affected();
        if changed != 1 {
            anyhow::bail!("finalization decision conflicts with an irreversible journal");
        }
    } else if row.1 == "completed" {
        anyhow::bail!("finalization decision conflicts with the durable journal");
    }
    Ok(())
}

pub(super) async fn load(pool: &SqlitePool, task_id: i64) -> anyhow::Result<FinalizationRow> {
    sqlx::query_as(
        "SELECT decision, state, commit_sha \
         FROM runner_finalizations WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

pub(super) async fn stage(
    pool: &SqlitePool,
    task_id: i64,
    state: &str,
    commit_sha: Option<&str>,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE runner_finalizations SET state = ?, commit_sha = COALESCE(?, commit_sha), \
         failure_reason = NULL, updated_at = ? WHERE task_id = ? AND state != 'completed'",
    )
    .bind(state)
    .bind(commit_sha)
    .bind(now)
    .bind(task_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) async fn failure(
    pool: &SqlitePool,
    task_id: i64,
    reason: &str,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE runner_finalizations SET failure_reason = ?, updated_at = ? WHERE task_id = ?",
    )
    .bind(reason)
    .bind(now)
    .bind(task_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) async fn finalizing_ids(pool: &SqlitePool) -> anyhow::Result<Vec<i64>> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT f.task_id FROM runner_finalizations f JOIN tasks t ON t.id = f.task_id \
         WHERE f.state != 'completed' AND t.state = ? ORDER BY f.task_id",
    )
    .bind(db::state::FINALIZING)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|row| row.0).collect())
}
