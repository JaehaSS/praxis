use std::path::Path;

use sqlx::SqlitePool;

use super::preparation::{existing_projection, prepare_projection, ProjectionPlan};

#[allow(clippy::too_many_arguments)]
pub async fn inject_into_worktree(
    pool: &SqlitePool,
    repo: &str,
    instruction: &str,
    query_embedding: Option<&[f32]>,
    task_id: i64,
    now: i64,
    worktree_path: &Path,
    limit: i64,
    targets: &[&str],
) -> anyhow::Result<usize> {
    if let Some(count) = existing_projection(pool, task_id, worktree_path, targets, now).await? {
        return Ok(count);
    }
    let plan = prepare_projection(
        pool,
        repo,
        instruction,
        query_embedding,
        task_id,
        now,
        worktree_path,
        limit,
        targets,
    )
    .await?;
    if let Err(error) =
        crate::projector::apply_updates(worktree_path, &plan.preimages, &plan.postimages)
    {
        super::super::projection_recovery::rollback(
            pool,
            &plan.journal,
            &plan.preimages,
            &plan.postimages,
            &error.to_string(),
            now,
        )
        .await?;
        return Err(error.into());
    }
    if let Err(error) =
        super::super::projection_verify::verify_projection(&plan.journal, &plan.live_elsewhere)
    {
        rollback_plan(pool, &plan, &error, now).await?;
        return Err(error);
    }
    let memory_ids = plan
        .receipts
        .iter()
        .map(|receipt| receipt.memory_id)
        .collect::<Vec<_>>();
    let reports = match crate::evidence::revalidate_memories(pool, &memory_ids, now).await {
        Ok(reports) => reports,
        Err(error) => {
            rollback_plan(pool, &plan, &error, now).await?;
            return Err(error);
        }
    };
    if let Err(error) = super::super::receipt_finalize::finalize_projection(
        pool,
        &plan.journal,
        &plan.receipts,
        &reports,
        now,
    )
    .await
    {
        rollback_plan(pool, &plan, &error, now).await?;
        return Err(error);
    }
    Ok(plan.receipts.len())
}

async fn rollback_plan(
    pool: &SqlitePool,
    plan: &ProjectionPlan,
    error: &anyhow::Error,
    now: i64,
) -> anyhow::Result<()> {
    super::super::projection_recovery::rollback(
        pool,
        &plan.journal,
        &plan.preimages,
        &plan.postimages,
        &error.to_string(),
        now,
    )
    .await
}
