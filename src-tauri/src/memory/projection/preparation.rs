use std::path::Path;

use sqlx::SqlitePool;

use super::super::receipt::{JournalRow, MemoryReceipt};
use super::verification::verify_journal;

/// v2 = 항상 적용/관련 메모리 두 섹션 렌더. 지정 규칙이 없는 작업의 출력은 v1과 동일하다.
// 3: M-id 인용 마커 + 지시문 (설계 0048) — 출력 바이트가 바뀌므로 승격.
const RENDERER_VERSION: i64 = 3;

pub(super) struct ProjectionPlan {
    pub(super) journal: JournalRow,
    pub(super) receipts: Vec<MemoryReceipt>,
    pub(super) preimages: Vec<crate::projector::TargetPreimage>,
    pub(super) postimages: Vec<Option<String>>,
    /// 계획을 세울 때 본 이웃 해시 — 적용 직후 readback이 같은 기준으로 소유자를 판정한다.
    pub(super) live_elsewhere: std::collections::HashSet<String>,
}

pub(super) async fn existing_projection(
    pool: &SqlitePool,
    task_id: i64,
    worktree_path: &Path,
    targets: &[&str],
    now: i64,
) -> anyhow::Result<Option<usize>> {
    let Some(journal) = super::super::receipt::load_journal(pool, task_id).await? else {
        return Ok(None);
    };
    if journal.state != "applied" {
        anyhow::bail!(
            "task has unresolved memory projection state: {}",
            journal.state
        );
    }
    let recorded_targets: Vec<String> = serde_json::from_str(&journal.target_paths_json)?;
    let targets_match = recorded_targets
        .iter()
        .map(String::as_str)
        .eq(targets.iter().copied());
    if Path::new(&journal.worktree_path) != worktree_path || !targets_match {
        anyhow::bail!("projection retry does not match the immutable task receipt");
    }
    verify_journal(pool, &journal, now)
        .await
        .map(|verified| Some(verified.count))
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn prepare_projection(
    pool: &SqlitePool,
    repo: &str,
    instruction: &str,
    query_embedding: Option<&[f32]>,
    task_id: i64,
    now: i64,
    worktree_path: &Path,
    limit: i64,
    targets: &[&str],
) -> anyhow::Result<ProjectionPlan> {
    crate::evidence::revalidate_scope(pool, repo, now).await?;
    // 지정 규칙을 먼저 고정하고 관련 메모리를 뒤에 붙인다 — preview와 같은 selector.
    let selection = super::super::application_policy::select_for_projection(
        pool,
        repo,
        instruction,
        query_embedding,
        limit,
        now,
    )
    .await?;
    let memories = selection.ordered();
    let receipts = super::super::receipt::snapshot_memories(pool, &memories, now).await?;
    let block = super::super::projection_plan::render_block(&receipts)?;
    let target_hash =
        super::super::projection_verify::sha256(block.as_deref().unwrap_or_default().as_bytes());
    let preimages = crate::projector::capture_targets(worktree_path, targets)?;
    let live_elsewhere =
        super::super::receipt::live_projection_hashes(pool, worktree_path, task_id).await?;
    let postimages = super::super::projection_plan::planned_updates(
        &preimages,
        block.as_deref(),
        &live_elsewhere,
    )?;
    let paths = targets
        .iter()
        .map(|target| (*target).to_string())
        .collect::<Vec<_>>();
    let prepared = super::super::receipt::prepare_journal(
        pool,
        task_id,
        worktree_path.to_string_lossy().as_ref(),
        &paths,
        &target_hash,
        RENDERER_VERSION,
        &receipts,
        &preimages,
        now,
    )
    .await?;
    if !prepared.is_new {
        anyhow::bail!("task has a concurrently prepared memory projection");
    }
    Ok(ProjectionPlan {
        journal: prepared.row,
        receipts,
        preimages,
        postimages,
        live_elsewhere,
    })
}
