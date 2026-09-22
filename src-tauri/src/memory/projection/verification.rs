use sqlx::SqlitePool;

use super::super::receipt::{JournalRow, MemoryReceipt};

pub(super) struct JournalVerification {
    pub(super) count: usize,
    pub(super) source_check_ids: Vec<i64>,
}

pub struct ProjectionStartReceipt {
    pub projection_id: i64,
    pub source_check_ids: Vec<i64>,
    pub memory_count: usize,
}

pub(super) async fn verify_journal(
    pool: &SqlitePool,
    journal: &JournalRow,
    now: i64,
) -> anyhow::Result<JournalVerification> {
    let receipts: Vec<MemoryReceipt> = serde_json::from_str(&journal.ordered_memories_json)?;
    let memory_ids = receipts
        .iter()
        .map(|receipt| receipt.memory_id)
        .collect::<Vec<_>>();
    let reports = crate::evidence::revalidate_memories(pool, &memory_ids, now).await?;
    let source_check_ids = reports
        .into_iter()
        .flat_map(|report| report.check_ids)
        .collect();
    let live_elsewhere = super::super::receipt::live_projection_hashes(
        pool,
        std::path::Path::new(&journal.worktree_path),
        journal.task_id,
    )
    .await?;
    let count = match super::super::projection_verify::verify_projection(journal, &live_elsewhere) {
        Ok(count) => count,
        Err(error) => {
            super::super::projection_state::mark_applied_degraded(
                pool,
                journal.id,
                &error.to_string(),
            )
            .await?;
            return Err(error);
        }
    };
    super::super::receipt_finalize::verify_applied_projection(pool, journal, now).await?;
    Ok(JournalVerification {
        count,
        source_check_ids,
    })
}

/// 파일형 메모리(설계 2026-09-13)는 원장을 남기지 않는다 — 정본이 파일이고 블록은 그 사본이라
/// 되돌릴 preimage도 검증할 증거 행도 없다. 그래서 **원장이 아예 없으면 통과**시킨다.
/// 원장이 있는데 상태가 어긋난 경우(옛 DB 투영을 받은 작업)는 그대로 막힌다.
pub async fn verify_task_projection(
    pool: &SqlitePool,
    task_id: i64,
    now: i64,
) -> anyhow::Result<usize> {
    let Some(journal) = load_applied_journal_if_present(pool, task_id).await? else {
        return Ok(0);
    };
    verify_journal(pool, &journal, now)
        .await
        .map(|verified| verified.count)
}

/// `None`이면 파일형 투영이다 — 시작 영수증 없이 시작해도 된다(위 주석 참고).
pub async fn verify_task_projection_for_start(
    pool: &SqlitePool,
    task_id: i64,
    now: i64,
) -> anyhow::Result<Option<ProjectionStartReceipt>> {
    let Some(journal) = load_applied_journal_if_present(pool, task_id).await? else {
        return Ok(None);
    };
    let verified = verify_journal(pool, &journal, now).await?;
    Ok(Some(ProjectionStartReceipt {
        projection_id: journal.id,
        source_check_ids: verified.source_check_ids,
        memory_count: verified.count,
    }))
}

async fn load_applied_journal_if_present(
    pool: &SqlitePool,
    task_id: i64,
) -> anyhow::Result<Option<JournalRow>> {
    let Some(journal) = super::super::receipt::load_journal(pool, task_id).await? else {
        return Ok(None);
    };
    if journal.state != "applied" {
        anyhow::bail!(
            "task has unresolved memory projection state: {}",
            journal.state
        );
    }
    Ok(Some(journal))
}

/// P1: 호출 끊음, P2에서 제거 — 옛 DB 투영 경로가 쓰던 "원장이 없으면 실패" 규약.
#[allow(dead_code)]
async fn load_applied_journal(pool: &SqlitePool, task_id: i64) -> anyhow::Result<JournalRow> {
    load_applied_journal_if_present(pool, task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("task has no memory projection receipt"))
}
