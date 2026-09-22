use sqlx::SqlitePool;

use super::LoadedEvidence;
use crate::evidence::model::EvidenceRecord;
use crate::memory::confirm_approval::{
    ConfirmApprovalFailure, ConfirmApprovalResult, ConfirmedApproval,
};
use crate::memory::knowledge_status;

pub(super) async fn run(
    pool: &SqlitePool,
    memory_id: i64,
    expected_version: i64,
    now: i64,
) -> ConfirmApprovalResult<ConfirmedApproval> {
    for _ in 0..super::MAX_RETRIES {
        let loaded = load(pool, memory_id).await?;
        ensure_version(&loaded, expected_version)?;
        if loaded.memory_status == knowledge_status::VERIFIED {
            return existing_approval(pool, &loaded).await;
        }
        ensure_approvable(&loaded.memory_status)?;
        let observations = super::observe::all(&loaded, now);
        match super::confirm_transaction::commit(pool, &loaded, &observations, now).await? {
            super::confirm_transaction::CommitOutcome::Approved(result) => return Ok(result),
            super::confirm_transaction::CommitOutcome::Retry => {}
        }
    }
    Err(ConfirmApprovalFailure::Conflict)
}

async fn load(pool: &SqlitePool, memory_id: i64) -> ConfirmApprovalResult<LoadedEvidence> {
    let current: Option<(i64, String, Option<String>)> =
        sqlx::query_as("SELECT current_version, status, scope_key FROM memories WHERE id = ?")
            .bind(memory_id)
            .fetch_optional(pool)
            .await?;
    let Some((version, memory_status, scope_key)) = current else {
        return Err(ConfirmApprovalFailure::NotFound(
            "메모리를 찾을 수 없습니다",
        ));
    };
    let evidence: Vec<EvidenceRecord> = sqlx::query_as(
        "SELECT id, memory_id, version, kind, locator_json, snapshot_hash, status, \
                observed_at, checked_at, expires_at FROM memory_evidence \
         WHERE memory_id = ? AND version = ? ORDER BY id",
    )
    .bind(memory_id)
    .bind(version)
    .fetch_all(pool)
    .await?;
    Ok(LoadedEvidence {
        memory_id,
        version,
        memory_status,
        scope_key,
        evidence,
    })
}

fn ensure_version(loaded: &LoadedEvidence, expected_version: i64) -> ConfirmApprovalResult<()> {
    if loaded.version != expected_version {
        return Err(ConfirmApprovalFailure::Conflict);
    }
    Ok(())
}

fn ensure_approvable(status: &str) -> ConfirmApprovalResult<()> {
    let approvable = status == knowledge_status::PENDING_REVIEW
        || crate::memory::can_transition(status, knowledge_status::PENDING_REVIEW);
    if !approvable {
        return Err(ConfirmApprovalFailure::Conflict);
    }
    Ok(())
}

async fn existing_approval(
    pool: &SqlitePool,
    loaded: &LoadedEvidence,
) -> ConfirmApprovalResult<ConfirmedApproval> {
    let receipt_id: Option<i64> = sqlx::query_scalar(
        "SELECT id FROM memory_approval_receipts \
         WHERE memory_id = ? AND version = ? ORDER BY id DESC LIMIT 1",
    )
    .bind(loaded.memory_id)
    .bind(loaded.version)
    .fetch_optional(pool)
    .await?;
    let Some(receipt_id) = receipt_id else {
        return Err(ConfirmApprovalFailure::Conflict);
    };
    Ok(ConfirmedApproval {
        version: loaded.version,
        receipt_id,
        already_approved: true,
    })
}
