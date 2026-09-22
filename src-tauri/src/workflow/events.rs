use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct WorkflowEvent {
    pub sequence: i64,
    pub run_id: String,
    pub kind: String,
    pub detail: String,
    pub created_at: i64,
}

pub(super) async fn append(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    kind: &str,
    detail: &str,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO workflow_events(run_id,kind,detail,created_at) VALUES(?,?,?,?)")
        .bind(run_id)
        .bind(kind)
        .bind(detail)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
