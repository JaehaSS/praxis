use sqlx::{Sqlite, SqlitePool, Transaction};

use super::LoadedEvidence;
use crate::evidence::model::{EvidenceRecord, Observation};
use crate::memory::confirm_approval::{
    ConfirmApprovalFailure, ConfirmApprovalResult, ConfirmedApproval,
};
use crate::memory::{evidence_status, knowledge_status};

pub(super) enum CommitOutcome {
    Approved(ConfirmedApproval),
    Retry,
}

pub(super) async fn commit(
    pool: &SqlitePool,
    loaded: &LoadedEvidence,
    observations: &[Observation],
    now: i64,
) -> ConfirmApprovalResult<CommitOutcome> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    if !snapshot_matches(&mut tx, loaded).await? {
        return Ok(CommitOutcome::Retry);
    }
    if !observations_are_current(&mut tx, loaded, observations).await? {
        return Ok(CommitOutcome::Retry);
    }
    ensure_valid(observations)?;
    let confirmation = crate::memory::user_confirmation::insert(
        &mut tx,
        loaded.memory_id,
        loaded.version,
        now,
        None,
    )
    .await?;
    let check_ids = persist_checks(&mut tx, observations, confirmation, now).await?;
    enter_review(&mut tx, loaded, now).await?;
    let receipt_id = super::approval::finish(
        &mut tx,
        loaded.memory_id,
        loaded.version,
        knowledge_status::PENDING_REVIEW,
        &check_ids,
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(CommitOutcome::Approved(ConfirmedApproval {
        version: loaded.version,
        receipt_id,
        already_approved: false,
    }))
}

fn ensure_valid(observations: &[Observation]) -> ConfirmApprovalResult<()> {
    if observations
        .iter()
        .any(|item| item.status != evidence_status::VALID)
    {
        return Err(ConfirmApprovalFailure::Invalid(
            "현재 version의 모든 근거가 유효해야 합니다",
        ));
    }
    Ok(())
}

async fn snapshot_matches(
    tx: &mut Transaction<'_, Sqlite>,
    loaded: &LoadedEvidence,
) -> Result<bool, sqlx::Error> {
    let current: Option<(i64, String, Option<String>)> =
        sqlx::query_as("SELECT current_version, status, scope_key FROM memories WHERE id = ?")
            .bind(loaded.memory_id)
            .fetch_optional(&mut **tx)
            .await?;
    Ok(current
        == Some((
            loaded.version,
            loaded.memory_status.clone(),
            loaded.scope_key.clone(),
        )))
}

async fn observations_are_current(
    tx: &mut Transaction<'_, Sqlite>,
    loaded: &LoadedEvidence,
    observations: &[Observation],
) -> ConfirmApprovalResult<bool> {
    if !super::persist::generation_matches(tx, loaded).await? {
        return Ok(false);
    }
    for item in observations {
        if !super::persist::identity_matches(tx, item).await? {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn persist_checks(
    tx: &mut Transaction<'_, Sqlite>,
    observations: &[Observation],
    confirmation: EvidenceRecord,
    now: i64,
) -> ConfirmApprovalResult<Vec<i64>> {
    let mut check_ids = Vec::with_capacity(observations.len() + 1);
    for item in observations {
        check_ids.push(super::persist::persist_check(tx, item, now).await?);
    }
    let confirmation = Observation {
        evidence: confirmation,
        status: evidence_status::VALID.to_string(),
        observed_hash: None,
    };
    check_ids.push(super::persist::persist_check(tx, &confirmation, now).await?);
    Ok(check_ids)
}

async fn enter_review(
    tx: &mut Transaction<'_, Sqlite>,
    loaded: &LoadedEvidence,
    now: i64,
) -> ConfirmApprovalResult<()> {
    if loaded.memory_status == knowledge_status::PENDING_REVIEW {
        return Ok(());
    }
    let changed = sqlx::query(
        "UPDATE memories SET status = ? WHERE id = ? AND current_version = ? AND status = ?",
    )
    .bind(knowledge_status::PENDING_REVIEW)
    .bind(loaded.memory_id)
    .bind(loaded.version)
    .bind(&loaded.memory_status)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(ConfirmApprovalFailure::Conflict);
    }
    insert_review_event(tx, loaded, now).await
}

async fn insert_review_event(
    tx: &mut Transaction<'_, Sqlite>,
    loaded: &LoadedEvidence,
    now: i64,
) -> ConfirmApprovalResult<()> {
    sqlx::query(
        "INSERT INTO memory_events \
         (memory_id, version, action, actor_kind, created_at) \
         VALUES (?, ?, 'review_submitted', 'human', ?)",
    )
    .bind(loaded.memory_id)
    .bind(loaded.version)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
