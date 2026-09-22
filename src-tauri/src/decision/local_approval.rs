use std::path::PathBuf;

use sqlx::SqlitePool;

use crate::{db, worktree::Worktree};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Route {
    Legacy,
    Ledger,
}

pub async fn route(pool: &SqlitePool, task: &db::Task) -> anyhow::Result<Route> {
    if !super::is_enabled(pool).await? {
        return Ok(Route::Legacy);
    }
    from_task(task).validate_isolated_approval()?;
    Ok(Route::Ledger)
}

pub async fn claim(
    pool: &SqlitePool,
    task_id: i64,
    exclude_generated_mcp: bool,
    now: i64,
) -> anyhow::Result<db::Task> {
    let pending = db::get_task(pool, task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("작업을 찾을 수 없습니다"))?;
    if route(pool, &pending).await? != Route::Ledger {
        anyhow::bail!("decision ledger is disabled");
    }
    super::approval_journal::claim_enabled(pool, task_id, exclude_generated_mcp, now).await
}

pub async fn resume(pool: &SqlitePool, task: &db::Task, now: i64) -> anyhow::Result<()> {
    let worktree = from_task(task);
    if let Err(error) = super::approval_stages::resume(pool, task, &worktree, now).await {
        restore_if_reversible(pool, task, now).await;
        return Err(error);
    }
    Ok(())
}

pub async fn finalize(
    pool: &SqlitePool,
    task_id: i64,
    exclude_generated_mcp: bool,
    now: i64,
) -> anyhow::Result<()> {
    let task = claim(pool, task_id, exclude_generated_mcp, now).await?;
    resume(pool, &task, now).await
}

pub async fn reconcile(pool: &SqlitePool, now: i64) -> anyhow::Result<u64> {
    if !super::is_enabled(pool).await? {
        return Ok(0);
    }
    let ids = super::approval_journal::finalizing_ids(pool).await?;
    let mut recovered = 0;
    for task_id in ids {
        let task = db::get_task(pool, task_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("finalizing task disappeared"))?;
        resume(pool, &task, now).await?;
        recovered += 1;
    }
    Ok(recovered)
}

fn from_task(task: &db::Task) -> Worktree {
    Worktree {
        repo: PathBuf::from(&task.repo),
        path: PathBuf::from(&task.worktree_path),
        branch: task.branch.clone(),
        base: task.base.clone(),
        base_revision: task.base_revision.clone(),
    }
}

async fn restore_if_reversible(pool: &SqlitePool, task: &db::Task, now: i64) {
    let Ok(journal) = super::approval_journal::load(pool, task.id).await else {
        return;
    };
    let reversible = matches!(
        journal.stage(),
        Ok(super::approval_journal::Stage::Prepared
            | super::approval_journal::Stage::ProjectionRetired)
    ) && journal.commit_sha.is_none();
    if reversible {
        let _ = db::restore_awaiting_review(pool, task.id, now).await;
    }
}
