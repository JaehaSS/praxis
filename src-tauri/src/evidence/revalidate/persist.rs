use sqlx::SqlitePool;

use super::LoadedEvidence;
use crate::evidence::model::{Observation, RevalidationReport};
use crate::memory::{evidence_status, knowledge_status};

pub(super) async fn commit(
    pool: &SqlitePool,
    memory_id: i64,
    loaded: &LoadedEvidence,
    observations: &[Observation],
    now: i64,
) -> anyhow::Result<Option<Vec<i64>>> {
    let mut tx = pool.begin().await?;
    let current: Option<(i64, String, Option<String>)> =
        sqlx::query_as("SELECT current_version, status, scope_key FROM memories WHERE id = ?")
            .bind(memory_id)
            .fetch_optional(&mut *tx)
            .await?;
    if current
        != Some((
            loaded.version,
            loaded.memory_status.clone(),
            loaded.scope_key.clone(),
        ))
    {
        return Ok(None);
    }
    if !generation_matches(&mut tx, loaded).await? {
        return Ok(None);
    }
    let mut check_ids = Vec::with_capacity(observations.len());
    for item in observations {
        if !identity_matches(&mut tx, item).await? {
            return Ok(None);
        }
        check_ids.push(persist_check(&mut tx, item, now).await?);
    }
    stale_if_invalid(&mut tx, memory_id, loaded, observations, now).await?;
    tx.commit().await?;
    Ok(Some(check_ids))
}

pub(super) async fn generation_matches(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    loaded: &LoadedEvidence,
) -> anyhow::Result<bool> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memory_evidence WHERE memory_id = ? AND version = ?",
    )
    .bind(loaded.memory_id)
    .bind(loaded.version)
    .fetch_one(&mut **tx)
    .await?;
    Ok(count == loaded.evidence.len() as i64)
}

pub(super) async fn identity_matches(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    item: &Observation,
) -> anyhow::Result<bool> {
    let found: Option<i64> = sqlx::query_scalar(
        "SELECT 1 FROM memory_evidence WHERE id = ? AND memory_id = ? AND version = ? \
         AND kind = ? AND locator_json = ? AND snapshot_hash IS ? AND observed_at = ? \
         AND expires_at IS ? AND status = ? AND checked_at IS ?",
    )
    .bind(item.evidence.id)
    .bind(item.evidence.memory_id)
    .bind(item.evidence.version)
    .bind(&item.evidence.kind)
    .bind(&item.evidence.locator_json)
    .bind(&item.evidence.snapshot_hash)
    .bind(item.evidence.observed_at)
    .bind(item.evidence.expires_at)
    .bind(&item.evidence.status)
    .bind(item.evidence.checked_at)
    .fetch_optional(&mut **tx)
    .await?;
    Ok(found.is_some())
}

pub(super) async fn persist_check(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    item: &Observation,
    now: i64,
) -> anyhow::Result<i64> {
    let check_id = sqlx::query(
        "INSERT INTO memory_evidence_checks \
         (evidence_id, memory_id, version, status, observed_hash, checked_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(item.evidence.id)
    .bind(item.evidence.memory_id)
    .bind(item.evidence.version)
    .bind(&item.status)
    .bind(&item.observed_hash)
    .bind(now)
    .execute(&mut **tx)
    .await?
    .last_insert_rowid();
    sqlx::query("UPDATE memory_evidence SET status = ?, checked_at = ? WHERE id = ?")
        .bind(&item.status)
        .bind(now)
        .bind(item.evidence.id)
        .execute(&mut **tx)
        .await?;
    Ok(check_id)
}

async fn stale_if_invalid(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    memory_id: i64,
    loaded: &LoadedEvidence,
    observations: &[Observation],
    now: i64,
) -> anyhow::Result<()> {
    if loaded.memory_status != knowledge_status::VERIFIED || !has_definite_invalid(observations) {
        return Ok(());
    }
    sqlx::query(
        "UPDATE memories SET status = ?, stale_at = ? WHERE id = ? AND current_version = ?",
    )
    .bind(knowledge_status::STALE)
    .bind(now)
    .bind(memory_id)
    .bind(loaded.version)
    .execute(&mut **tx)
    .await?;
    sqlx::query(
        "INSERT INTO memory_events \
         (memory_id, version, action, actor_kind, reason, created_at) \
         VALUES (?, ?, 'evidence_stale', 'system', 'source_revalidation', ?)",
    )
    .bind(memory_id)
    .bind(loaded.version)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn has_definite_invalid(observations: &[Observation]) -> bool {
    observations.iter().any(|item| {
        matches!(
            item.status.as_str(),
            evidence_status::CHANGED | evidence_status::MISSING | evidence_status::EXPIRED
        )
    })
}

pub(super) fn report(
    memory_id: i64,
    version: i64,
    observations: &[Observation],
    check_ids: Vec<i64>,
) -> RevalidationReport {
    RevalidationReport {
        memory_id,
        version,
        statuses: observations
            .iter()
            .map(|item| item.status.clone())
            .collect(),
        check_ids,
        stale: has_definite_invalid(observations),
    }
}

#[cfg(test)]
mod tests;
