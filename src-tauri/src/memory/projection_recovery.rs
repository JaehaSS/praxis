//! Crash reconciliation for prepared filesystem projections.

use std::path::Path;

use sqlx::SqlitePool;

use super::receipt::{JournalRow, MemoryReceipt};

pub(super) async fn rollback(
    pool: &SqlitePool,
    journal: &JournalRow,
    preimages: &[crate::projector::TargetPreimage],
    postimages: &[Option<String>],
    reason: &str,
    now: i64,
) -> anyhow::Result<()> {
    let restore = crate::projector::restore_matching(
        Path::new(&journal.worktree_path),
        preimages,
        postimages,
    );
    super::projection_state::mark_projection_failure(
        pool,
        journal.id,
        restore.is_err(),
        reason,
        now,
    )
    .await?;
    restore.map_err(Into::into)
}

async fn reconcile_one(pool: &SqlitePool, journal: &JournalRow, now: i64) -> anyhow::Result<()> {
    // 크래시 당시 계획했던 postimage를 그대로 다시 만들어야 "내가 쓴 결과"를 알아보고 되돌린다.
    // 이웃 해시도 그때와 같은 기준으로 묻는다 — 이 저널은 아직 prepared라 자기 자신은 세지 않는다.
    let live_elsewhere = super::receipt::live_projection_hashes(
        pool,
        std::path::Path::new(&journal.worktree_path),
        journal.task_id,
    )
    .await
    .unwrap_or_default();
    let recovery = (|| {
        let preimages = serde_json::from_str::<Vec<crate::projector::TargetPreimage>>(
            journal.preimages_json.as_deref().unwrap_or("[]"),
        )?;
        let receipts = serde_json::from_str::<Vec<MemoryReceipt>>(&journal.ordered_memories_json)?;
        let block = super::projection_plan::render_block(&receipts)?;
        let postimages =
            super::projection_plan::planned_updates(&preimages, block.as_deref(), &live_elsewhere)?;
        Ok::<_, anyhow::Error>((preimages, postimages))
    })();
    let reason = "reconciled unresolved prepared projection after restart";
    match recovery {
        Ok((preimages, postimages)) => {
            rollback(pool, journal, &preimages, &postimages, reason, now).await
        }
        Err(error) => {
            super::projection_state::mark_projection_failure(
                pool,
                journal.id,
                true,
                &error.to_string(),
                now,
            )
            .await?;
            Err(error)
        }
    }
}

pub async fn reconcile_prepared_projections(pool: &SqlitePool, now: i64) -> anyhow::Result<u64> {
    let journals = sqlx::query_as::<_, JournalRow>(
        "SELECT id, task_id, state, worktree_path, target_paths_json, target_hash, \
                renderer_version, ordered_memories_json, preimages_json \
         FROM memory_projection_journal WHERE state = 'prepared' ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    let mut first_error = None;
    let mut recovered = 0;
    for journal in &journals {
        if crate::runner::review_process::task_is_fenced(pool, journal.task_id).await? {
            continue;
        }
        if let Err(error) = reconcile_one(pool, journal, now).await {
            first_error.get_or_insert(error);
        }
        let _ = crate::db::update_state(pool, journal.task_id, crate::db::state::FAILED, now).await;
        recovered += 1;
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(recovered)
}
