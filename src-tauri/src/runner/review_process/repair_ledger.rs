use sqlx::SqlitePool;

pub(super) async fn resolve(
    pool: &SqlitePool,
    task_id: i64,
    receipt_id: i64,
    outcome: &str,
    now: i64,
) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "DELETE FROM review_process_leases \
         WHERE task_id = ? AND receipt_id = ? AND state = 'quarantined'",
    )
    .bind(task_id)
    .bind(receipt_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 1 {
        append_event(
            &mut tx,
            task_id,
            now,
            "review_process_operator_resolved",
            &format!("review process receipt {receipt_id}; outcome={outcome}"),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(result.rows_affected() == 1)
}

pub(super) async fn defer(
    pool: &SqlitePool,
    task_id: i64,
    receipt_id: i64,
    reason: &str,
    now: i64,
) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE review_process_leases SET reason = ?, updated_at = ? \
         WHERE task_id = ? AND receipt_id = ? AND state = 'quarantined'",
    )
    .bind(reason)
    .bind(now)
    .bind(task_id)
    .bind(receipt_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 1 {
        append_event(
            &mut tx,
            task_id,
            now,
            "review_process_repair_deferred",
            &format!("review process receipt {receipt_id}; reason={reason}"),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(result.rows_affected() == 1)
}

async fn append_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: i64,
    now: i64,
    kind: &str,
    detail: &str,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO runner_events (task_id, ts, kind, detail) VALUES (?, ?, ?, ?)")
        .bind(task_id)
        .bind(now)
        .bind(kind)
        .bind(detail)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
