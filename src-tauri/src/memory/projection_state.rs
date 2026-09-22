//! Projection journal state transitions that preserve repair material on degradation.

use sqlx::SqlitePool;

pub(super) async fn mark_projection_failure(
    pool: &SqlitePool,
    journal_id: i64,
    degraded: bool,
    reason: &str,
    now: i64,
) -> anyhow::Result<()> {
    let state = if degraded { "degraded" } else { "rolled_back" };
    sqlx::query(
        "UPDATE memory_projection_journal \
         SET state = ?, failure_reason = ?, \
             preimages_json = CASE WHEN ? THEN preimages_json ELSE NULL END, updated_at = ? \
         WHERE id = ? AND state = 'prepared'",
    )
    .bind(state)
    .bind(reason)
    .bind(degraded)
    .bind(now)
    .bind(journal_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub(super) async fn mark_applied_degraded(
    pool: &SqlitePool,
    journal_id: i64,
    reason: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE memory_projection_journal SET state = 'degraded', failure_reason = ? \
         WHERE id = ? AND state = 'applied'",
    )
    .bind(reason)
    .bind(journal_id)
    .execute(pool)
    .await?;
    Ok(())
}
