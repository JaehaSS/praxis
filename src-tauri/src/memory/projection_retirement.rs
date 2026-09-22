//! Exact, idempotent removal of ephemeral projection bytes before merge.

use std::path::Path;

use sqlx::SqlitePool;

use super::receipt::MemoryReceipt;

pub async fn retire_task_projection(
    pool: &SqlitePool,
    task_id: i64,
    now: i64,
) -> anyhow::Result<()> {
    let journal = super::receipt::load_journal(pool, task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("task has no memory projection receipt"))?;
    retire_journal(pool, now, &journal).await
}

pub async fn retire_task_projection_if_present(
    pool: &SqlitePool,
    task_id: i64,
    now: i64,
) -> anyhow::Result<()> {
    let Some(journal) = super::receipt::load_journal(pool, task_id).await? else {
        return Ok(());
    };
    retire_journal(pool, now, &journal).await
}

async fn retire_journal(
    pool: &SqlitePool,
    now: i64,
    journal: &super::receipt::JournalRow,
) -> anyhow::Result<()> {
    if journal.state == "retired" {
        return Ok(());
    }
    // Retirement's postcondition is "no projection bytes remain in the targets".
    // When no target still holds this journal's own block, that already holds;
    // demanding an exact preimage here would permanently block targets that
    // legitimately evolved underneath the projection (e.g. a tracked CLAUDE.md
    // gaining real content), and lets a degraded journal heal once its residue
    // is gone.
    if matches!(journal.state.as_str(), "applied" | "degraded")
        && projection_bytes_are_gone(journal)?
    {
        return mark_retired(pool, journal.id, now).await;
    }
    if journal.state != "applied" {
        anyhow::bail!(
            "task has unresolved memory projection state: {}",
            journal.state
        );
    }
    let preimages = parse_preimages(journal)?;
    let targets = preimages
        .iter()
        .map(|preimage| preimage.relative_path.as_str())
        .collect::<Vec<_>>();
    let root = Path::new(&journal.worktree_path);
    restore_projection(pool, journal, root, &targets, &preimages).await?;
    mark_retired(pool, journal.id, now).await
}

async fn restore_projection(
    pool: &SqlitePool,
    journal: &super::receipt::JournalRow,
    root: &Path,
    targets: &[&str],
    preimages: &[crate::projector::TargetPreimage],
) -> anyhow::Result<()> {
    let current = crate::projector::capture_targets(root, targets)?;
    if current != preimages {
        let live_elsewhere =
            super::receipt::live_projection_hashes(pool, root, journal.task_id).await?;
        super::projection_verify::verify_projection(journal, &live_elsewhere)?;
        let postimages = projected_postimages(journal, preimages)?;
        if let Err(error) = crate::projector::restore_matching(root, preimages, &postimages) {
            super::projection_state::mark_applied_degraded(pool, journal.id, &error.to_string())
                .await?;
            return Err(error.into());
        }
    }
    if crate::projector::capture_targets(root, targets)? != preimages {
        super::projection_state::mark_applied_degraded(
            pool,
            journal.id,
            "projection retirement did not restore exact preimages",
        )
        .await?;
        anyhow::bail!("projection retirement did not restore exact preimages");
    }
    Ok(())
}

/// 이 투영이 남긴 바이트가 대상에 더는 없는가.
///
/// "마커가 하나라도 보이면 아직 남아 있다"로 재면 안 된다 — 같은 폴더를 공유하는 다른 작업의
/// 블록까지 내 잔재로 세어, 그 작업이 살아 있는 동안 내 회수가 막힌다. 세는 것은 **내 해시와
/// 같은 블록** 하나뿐이다.
fn projection_bytes_are_gone(journal: &super::receipt::JournalRow) -> anyhow::Result<bool> {
    // 낼 블록이 없었던 투영은 대상에 한 바이트도 쓰지 않는다 — 회수할 것이 애초에 없다.
    let receipts: Vec<MemoryReceipt> = serde_json::from_str(&journal.ordered_memories_json)?;
    if receipts.is_empty() {
        return Ok(true);
    }
    let root = Path::new(&journal.worktree_path);
    // 워크트리가 통째로 사라졌으면 남아 있을 투영 바이트도 없다 — 사후조건이 이미 성립한다.
    // 그래도 캡처를 시도하면 루트 canonicalize가 NotFound로 실패하고, 그 에러가 폐기·거부
    // 경로를 막아 워크트리 없는 작업이 영영 지워지지 않는다. 파일 단위 부재는 이미 content:
    // None으로 흡수되므로, 루트 부재만 여기서 같은 의미로 받는다.
    if !root.exists() {
        return Ok(true);
    }
    let targets: Vec<String> = serde_json::from_str(&journal.target_paths_json)?;
    let references = targets.iter().map(String::as_str).collect::<Vec<_>>();
    let current = crate::projector::capture_targets(root, &references)?;
    Ok(current.iter().all(|target| {
        let content = target.content.as_deref().unwrap_or_default();
        super::projection_verify::exact_managed_block(content).is_none_or(|block| {
            super::projection_verify::sha256(block.as_bytes()) != journal.target_hash
        })
    }))
}

async fn mark_retired(pool: &SqlitePool, journal_id: i64, now: i64) -> anyhow::Result<()> {
    let changed = sqlx::query(
        "UPDATE memory_projection_journal \
         SET state = 'retired', preimages_json = NULL, failure_reason = NULL, updated_at = ? \
         WHERE id = ? AND state IN ('applied', 'degraded')",
    )
    .bind(now)
    .bind(journal_id)
    .execute(pool)
    .await?
    .rows_affected();
    if changed != 1 {
        anyhow::bail!("projection retirement was finalized concurrently");
    }
    Ok(())
}

fn parse_preimages(
    journal: &super::receipt::JournalRow,
) -> anyhow::Result<Vec<crate::projector::TargetPreimage>> {
    let encoded = journal
        .preimages_json
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("projection receipt has no rollback material"))?;
    serde_json::from_str(encoded).map_err(Into::into)
}

/// 이 저널이 적용했던 postimage를 다시 만든다 — 회수가 낙관적 비교에 쓰는 기준이다.
///
/// 이웃 해시를 빈 집합으로 넘겨도 안전하다. 그것을 참조하는 것은 **낼 블록이 없던 투영**뿐인데,
/// 그런 저널은 남긴 바이트가 없어 `projection_bytes_are_gone`에서 이미 회수를 마치므로 여기에
/// 도달하지 않는다. 여기 오는 저널은 언제나 자기 블록을 가진 쪽이다.
fn projected_postimages(
    journal: &super::receipt::JournalRow,
    preimages: &[crate::projector::TargetPreimage],
) -> anyhow::Result<Vec<Option<String>>> {
    let receipts = serde_json::from_str::<Vec<MemoryReceipt>>(&journal.ordered_memories_json)?;
    let block = super::projection_plan::render_block(&receipts)?;
    super::projection_plan::planned_updates(preimages, block.as_deref(), &Default::default())
}
