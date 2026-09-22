use sqlx::SqlitePool;

use super::{
    validate_registration, ReviewOperation, ReviewPhase, ReviewProcessLease, ReviewProcessReceipt,
    ReviewProcessState,
};

type LeaseRow = (i64, String, i64, String, Option<String>, i64);

pub async fn register(
    pool: &SqlitePool,
    task_id: i64,
    operation: ReviewOperation,
    phase: ReviewPhase,
    pgid: i64,
    identity_hash: &str,
    now: i64,
) -> anyhow::Result<ReviewProcessReceipt> {
    validate_registration(operation, phase, pgid, identity_hash)?;
    let mut tx = pool.begin().await?;
    assert_registration_allowed(&mut tx, task_id).await?;
    let receipt = sqlx::query_as::<_, ReviewProcessReceipt>(
        "INSERT INTO review_process_receipts \
         (task_id, operation, phase, pgid, identity_hash, created_at) \
         VALUES (?, ?, ?, ?, ?, ?) RETURNING *",
    )
    .bind(task_id)
    .bind(operation.as_str())
    .bind(phase.as_str())
    .bind(pgid)
    .bind(identity_hash)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;
    insert_lease(&mut tx, task_id, operation, receipt.id, now).await?;
    append_event(&mut tx, task_id, now, "review_process_started", receipt.id).await?;
    tx.commit().await?;
    Ok(receipt)
}

async fn assert_registration_allowed(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: i64,
) -> anyhow::Result<()> {
    let task_state: Option<String> = sqlx::query_scalar("SELECT state FROM tasks WHERE id = ?")
        .bind(task_id)
        .fetch_optional(&mut **tx)
        .await?;
    let Some(task_state) = task_state else {
        anyhow::bail!("review task does not exist");
    };
    if matches!(
        task_state.as_str(),
        crate::db::state::STARTING
            | crate::db::state::RUNNING
            | crate::db::state::FINALIZING
            | crate::db::state::DONE
            | crate::db::state::FAILED
            | crate::db::state::DISCARDED
    ) {
        anyhow::bail!("review task is executing, finalizing, or terminal");
    }
    let quarantined: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM review_process_leases \
         WHERE task_id = ? AND state = 'quarantined'",
    )
    .bind(task_id)
    .fetch_one(&mut **tx)
    .await?;
    if quarantined > 0 {
        anyhow::bail!("durable review process lease is quarantined");
    }
    Ok(())
}

async fn insert_lease(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: i64,
    operation: ReviewOperation,
    receipt_id: i64,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO review_process_leases \
         (task_id, operation, receipt_id, state, detail, updated_at) \
         VALUES (?, ?, ?, 'active', NULL, ?)",
    )
    .bind(task_id)
    .bind(operation.as_str())
    .bind(receipt_id)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn register_observed(
    pool: &SqlitePool,
    task_id: i64,
    operation: ReviewOperation,
    phase: ReviewPhase,
    pid: u32,
    now: i64,
) -> anyhow::Result<ReviewProcessReceipt> {
    let identity = super::super::process_identity::observe_group_leader(pid)?
        .ok_or_else(|| anyhow::anyhow!("review process exited before registration"))?;
    register(pool, task_id, operation, phase, pid as i64, &identity, now).await
}

pub async fn lease(
    pool: &SqlitePool,
    task_id: i64,
    operation: ReviewOperation,
) -> anyhow::Result<Option<ReviewProcessLease>> {
    let row: Option<LeaseRow> = sqlx::query_as(
        "SELECT task_id, operation, receipt_id, state, detail, updated_at \
         FROM review_process_leases WHERE task_id = ? AND operation = ?",
    )
    .bind(task_id)
    .bind(operation.as_str())
    .fetch_optional(pool)
    .await?;
    row.map(map_lease).transpose()
}

pub(crate) async fn resolve(
    pool: &SqlitePool,
    task_id: i64,
    operation: ReviewOperation,
    receipt_id: i64,
    now: i64,
    event_kind: &str,
) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "DELETE FROM review_process_leases \
         WHERE task_id = ? AND operation = ? AND receipt_id = ? AND state = 'active'",
    )
    .bind(task_id)
    .bind(operation.as_str())
    .bind(receipt_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 1 {
        append_event(&mut tx, task_id, now, event_kind, receipt_id).await?;
    }
    tx.commit().await?;
    Ok(result.rows_affected() == 1)
}

pub async fn quarantine(
    pool: &SqlitePool,
    task_id: i64,
    operation: ReviewOperation,
    receipt_id: i64,
    detail: &str,
    now: i64,
) -> anyhow::Result<bool> {
    let mut tx = pool.begin().await?;
    let result = sqlx::query(
        "UPDATE review_process_leases \
         SET state = 'quarantined', detail = ?, reason = NULL, updated_at = ? \
         WHERE task_id = ? AND operation = ? AND receipt_id = ?",
    )
    .bind(detail)
    .bind(now)
    .bind(task_id)
    .bind(operation.as_str())
    .bind(receipt_id)
    .execute(&mut *tx)
    .await?;
    if result.rows_affected() == 1 {
        append_event(
            &mut tx,
            task_id,
            now,
            "review_process_quarantined",
            receipt_id,
        )
        .await?;
    }
    tx.commit().await?;
    Ok(result.rows_affected() == 1)
}

fn map_lease(row: LeaseRow) -> anyhow::Result<ReviewProcessLease> {
    let state = match row.3.as_str() {
        "active" => ReviewProcessState::Active,
        "quarantined" => ReviewProcessState::Quarantined,
        _ => anyhow::bail!("invalid review process lease state"),
    };
    Ok(ReviewProcessLease {
        task_id: row.0,
        operation: row.1,
        receipt_id: row.2,
        state,
        detail: row.4,
        updated_at: row.5,
    })
}

async fn append_event(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    task_id: i64,
    now: i64,
    kind: &str,
    receipt_id: i64,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO runner_events (task_id, ts, kind, detail) VALUES (?, ?, ?, ?)")
        .bind(task_id)
        .bind(now)
        .bind(kind)
        .bind(format!("review process receipt {receipt_id}"))
        .execute(&mut **tx)
        .await?;
    Ok(())
}
