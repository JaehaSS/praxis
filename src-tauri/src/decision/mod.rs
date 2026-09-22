//! 로컬 승인 결과와 관측 가능한 근거를 잇는 privacy-preserving ledger.

use sqlx::SqlitePool;

mod approval_claim;
pub mod approval_completion;
pub mod approval_journal;
mod approval_policy;
mod approval_stages;
pub mod local_approval;
pub mod provenance;
mod provenance_links;
pub mod record;
pub mod redaction;
mod schema;
pub mod why_trace;

pub const FLAG_KEY: &str = "decision_ledger_enabled";

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    schema::migrate(pool).await
}

pub async fn is_enabled(pool: &SqlitePool) -> anyhow::Result<bool> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(FLAG_KEY)
        .fetch_optional(pool)
        .await?;
    Ok(value.as_deref() == Some("true"))
}
