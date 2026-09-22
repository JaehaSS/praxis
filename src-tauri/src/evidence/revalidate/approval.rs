//! Atomic binding between an approval and the exact evidence-check generation it trusted.

use sqlx::SqlitePool;

use super::LoadedEvidence;
use crate::evidence::model::Observation;
use crate::memory::{evidence_status, knowledge_status};

pub(super) enum ApprovalCommit {
    Approved,
    Rejected,
    Retry,
}

pub(super) async fn commit(
    pool: &SqlitePool,
    loaded: &LoadedEvidence,
    observations: &[Observation],
    now: i64,
) -> anyhow::Result<ApprovalCommit> {
    let mut tx = pool.begin().await?;
    let current: Option<(i64, String, Option<String>)> =
        sqlx::query_as("SELECT current_version, status, scope_key FROM memories WHERE id = ?")
            .bind(loaded.memory_id)
            .fetch_optional(&mut *tx)
            .await?;
    let expected = Some((
        loaded.version,
        loaded.memory_status.clone(),
        loaded.scope_key.clone(),
    ));
    if current != expected || !super::persist::generation_matches(&mut tx, loaded).await? {
        return Ok(ApprovalCommit::Retry);
    }
    for item in observations {
        if !super::persist::identity_matches(&mut tx, item).await? {
            return Ok(ApprovalCommit::Retry);
        }
    }
    let mut check_ids = Vec::with_capacity(observations.len());
    for item in observations {
        check_ids.push(super::persist::persist_check(&mut tx, item, now).await?);
    }
    if !trusted(observations) {
        tx.commit().await?;
        return Ok(ApprovalCommit::Rejected);
    }
    finish(
        &mut tx,
        loaded.memory_id,
        loaded.version,
        knowledge_status::PENDING_REVIEW,
        &check_ids,
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(ApprovalCommit::Approved)
}

fn trusted(observations: &[Observation]) -> bool {
    !observations.is_empty()
        && observations
            .iter()
            .all(|item| item.status == evidence_status::VALID)
}

pub(super) async fn finish(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    memory_id: i64,
    version: i64,
    expected_status: &str,
    check_ids: &[i64],
    now: i64,
) -> anyhow::Result<i64> {
    let changed = sqlx::query(
        "UPDATE memories SET status = ?, verified_at = ?, stale_at = NULL \
         WHERE id = ? AND status = ? AND current_version = ?",
    )
    .bind(knowledge_status::VERIFIED)
    .bind(now)
    .bind(memory_id)
    .bind(expected_status)
    .bind(version)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if changed != 1 {
        anyhow::bail!("검토 상태가 동시에 변경되었습니다");
    }
    let checks_json = serde_json::to_string(check_ids)?;
    let receipt_id = sqlx::query(
        "INSERT INTO memory_approval_receipts \
         (memory_id, version, source_check_ids_json, approved_at) VALUES (?, ?, ?, ?)",
    )
    .bind(memory_id)
    .bind(version)
    .bind(&checks_json)
    .bind(now)
    .execute(&mut **tx)
    .await?
    .last_insert_rowid();
    sqlx::query(
        "INSERT INTO memory_events \
         (memory_id, version, action, actor_kind, payload_json, created_at) \
         VALUES (?, ?, 'approved', 'human', ?, ?)",
    )
    .bind(memory_id)
    .bind(version)
    .bind(checks_json)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(receipt_id)
}
