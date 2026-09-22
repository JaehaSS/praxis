use sqlx::{Sqlite, Transaction};

/// Task deletion keeps an unlinkable, content-free decision tombstone.
///
/// This runs inside the caller's deletion transaction so a failed redaction
/// cannot leave a task without its provenance links (or vice versa).
pub async fn redact_task_decisions(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: i64,
    redacted_at: i64,
) -> anyhow::Result<()> {
    let incomplete: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM local_approval_finalizations \
         WHERE task_id = ? AND state != 'completed'",
    )
    .bind(task_id)
    .fetch_one(&mut **tx)
    .await?;
    if incomplete > 0 {
        anyhow::bail!("local approval finalization is incomplete");
    }

    let decision_ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM decision_records WHERE task_id = ? AND status = 'active'",
    )
    .bind(task_id)
    .fetch_all(&mut **tx)
    .await?;

    sqlx::query(
        "UPDATE decision_records SET kind = NULL, outcome = NULL, actor_kind = NULL, \
         task_id = NULL, summary = NULL, status = 'redacted', redacted_at = ? \
         WHERE task_id = ? AND status = 'active'",
    )
    .bind(redacted_at)
    .bind(task_id)
    .execute(&mut **tx)
    .await?;
    for decision_id in decision_ids {
        sqlx::query("DELETE FROM decision_artifact_links WHERE decision_id = ?")
            .bind(decision_id)
            .execute(&mut **tx)
            .await?;
    }
    sqlx::query(
        "DELETE FROM local_approval_finalizations WHERE task_id = ? AND state = 'completed'",
    )
    .bind(task_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
