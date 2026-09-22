use sqlx::SqlitePool;

use super::model::{EvidenceRecord, RevalidationReport};

mod approval;
mod confirm;
mod confirm_transaction;
mod observe;
mod persist;

const MAX_RETRIES: usize = 3;

pub(super) struct LoadedEvidence {
    pub memory_id: i64,
    pub version: i64,
    pub memory_status: String,
    pub scope_key: Option<String>,
    pub evidence: Vec<EvidenceRecord>,
}

pub async fn revalidate_memory(
    pool: &SqlitePool,
    memory_id: i64,
    now: i64,
) -> anyhow::Result<RevalidationReport> {
    for _ in 0..MAX_RETRIES {
        let loaded = load_current(pool, memory_id).await?;
        let observations = observe::all(&loaded, now);
        if let Some(check_ids) =
            persist::commit(pool, memory_id, &loaded, &observations, now).await?
        {
            return Ok(persist::report(
                memory_id,
                loaded.version,
                &observations,
                check_ids,
            ));
        }
    }
    anyhow::bail!("memory evidence changed concurrently during revalidation")
}

pub(crate) async fn approve_memory(
    pool: &SqlitePool,
    memory_id: i64,
    now: i64,
) -> anyhow::Result<()> {
    for _ in 0..MAX_RETRIES {
        let loaded = load_current(pool, memory_id).await?;
        if loaded.memory_status != crate::memory::knowledge_status::PENDING_REVIEW {
            anyhow::bail!("검토 대기 항목만 승인할 수 있습니다");
        }
        let observations = observe::all(&loaded, now);
        match approval::commit(pool, &loaded, &observations, now).await? {
            approval::ApprovalCommit::Approved => return Ok(()),
            approval::ApprovalCommit::Rejected => {
                anyhow::bail!("현재 버전에는 유효한 증거가 필요합니다")
            }
            approval::ApprovalCommit::Retry => {}
        }
    }
    anyhow::bail!("memory evidence changed concurrently during approval")
}

pub(crate) async fn confirm_and_approve_memory(
    pool: &SqlitePool,
    memory_id: i64,
    expected_version: i64,
    now: i64,
) -> crate::memory::confirm_approval::ConfirmApprovalResult<
    crate::memory::confirm_approval::ConfirmedApproval,
> {
    confirm::run(pool, memory_id, expected_version, now).await
}

pub async fn revalidate_memories(
    pool: &SqlitePool,
    memory_ids: &[i64],
    now: i64,
) -> anyhow::Result<Vec<RevalidationReport>> {
    let mut reports = Vec::with_capacity(memory_ids.len());
    for memory_id in memory_ids {
        reports.push(revalidate_memory(pool, *memory_id, now).await?);
    }
    Ok(reports)
}

pub async fn revalidate_scope(
    pool: &SqlitePool,
    repository: &str,
    now: i64,
) -> anyhow::Result<Vec<RevalidationReport>> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT id FROM memories WHERE status = ? \
         AND ((tier = 'project' AND scope_key = ?) OR tier = 'global')",
    )
    .bind(crate::memory::knowledge_status::VERIFIED)
    .bind(repository)
    .fetch_all(pool)
    .await?;
    revalidate_memories(pool, &ids, now).await
}

async fn load_current(pool: &SqlitePool, memory_id: i64) -> anyhow::Result<LoadedEvidence> {
    let row: Option<(i64, String, Option<String>)> =
        sqlx::query_as("SELECT current_version, status, scope_key FROM memories WHERE id = ?")
            .bind(memory_id)
            .fetch_optional(pool)
            .await?;
    let Some((version, memory_status, scope_key)) = row else {
        anyhow::bail!("memory not found");
    };
    let evidence = sqlx::query_as(
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
