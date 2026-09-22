use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use super::catalog::identifier;
use super::retrieval::ReferencePreview;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum DeliveryState {
    Pending,
    Delivered,
    NotDelivered,
}

impl DeliveryState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Delivered => "delivered",
            Self::NotDelivered => "not_delivered",
        }
    }
}

pub async fn record_pending(
    pool: &SqlitePool,
    task_id: i64,
    attempt_id: &str,
    preview: &ReferencePreview,
    now: i64,
) -> anyhow::Result<()> {
    for reference in &preview.references {
        sqlx::query(
            "INSERT INTO vault_usages \
             (id, task_id, attempt_id, revision_id, revision_hash, snippet, snippet_hash, delivery_state, citation_state, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', 'unknown', ?, ?)",
        )
        .bind(identifier("usage")?)
        .bind(task_id)
        .bind(attempt_id)
        .bind(&reference.revision_id)
        .bind(&reference.revision_hash)
        .bind(&reference.snippet)
        .bind(hash(&reference.snippet))
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn mark_delivery(
    pool: &SqlitePool,
    task_id: i64,
    attempt_id: &str,
    state: DeliveryState,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE vault_usages SET delivery_state = ?, updated_at = ? \
         WHERE task_id = ? AND attempt_id = ? AND delivery_state = 'pending'",
    )
    .bind(state.as_str())
    .bind(now)
    .bind(task_id)
    .bind(attempt_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn write_with_receipt<F>(
    pool: &SqlitePool,
    task_id: i64,
    attempt_id: Option<&str>,
    payload: &[u8],
    now: i64,
    write: F,
) -> Result<(), String>
where
    F: FnOnce(&[u8]) -> Result<(), String>,
{
    let result = write(payload);
    if let Some(attempt_id) = attempt_id {
        let state = if result.is_ok() {
            DeliveryState::Delivered
        } else {
            DeliveryState::NotDelivered
        };
        mark_delivery(pool, task_id, attempt_id, state, now)
            .await
            .map_err(|error| error.to_string())?;
    }
    result
}

pub async fn mark_cited(
    pool: &SqlitePool,
    task_id: i64,
    attempt_id: &str,
    revision_ids: &[String],
    now: i64,
) -> anyhow::Result<()> {
    for revision_id in revision_ids {
        sqlx::query(
            "UPDATE vault_usages SET citation_state = 'cited', updated_at = ? \
             WHERE task_id = ? AND attempt_id = ? AND revision_id = ?",
        )
        .bind(now)
        .bind(task_id)
        .bind(attempt_id)
        .bind(revision_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

fn hash(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
