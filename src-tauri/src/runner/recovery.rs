use sqlx::SqlitePool;

use crate::db::{self, state};

pub(super) async fn unowned_running(
    pool: &SqlitePool,
    worktree_locks: &super::worktree_lock::WorktreeLocks,
    now: i64,
) -> anyhow::Result<u64> {
    let mut recovered = 0;
    for task in db::list_running_tasks(pool).await? {
        if super::review_process::task_is_fenced(pool, task.id).await? {
            continue;
        }
        let _guard = worktree_locks
            .acquire(std::path::Path::new(&task.worktree_path))
            .await;
        let process = terminate_persisted_process(pool, &task).await?;
        let retirement = crate::memory::retire_task_projection_if_present(pool, task.id, now).await;
        let (kind, detail) = recovery_event(process, retirement);
        db::transition_state_with_runner_event(
            pool,
            task.id,
            state::FAILED,
            now,
            kind,
            Some(&detail),
        )
        .await?;
        recovered += 1;
    }
    Ok(recovered)
}

enum ProcessRecovery {
    Cleared,
    Quarantined(String),
}

async fn terminate_persisted_process(
    pool: &SqlitePool,
    task: &db::Task,
) -> anyhow::Result<ProcessRecovery> {
    let Some(pgid) = task.convo_pgid.filter(|value| *value > 0) else {
        return Ok(ProcessRecovery::Cleared);
    };
    let Some(receipt) = db::task_process_receipt(pool, task.id).await? else {
        return Ok(ProcessRecovery::Quarantined(
            "기존 실행 lease에 불변 process receipt가 없습니다".to_string(),
        ));
    };
    if receipt.pgid != pgid || receipt.process_kind != task.mode {
        return Ok(ProcessRecovery::Quarantined(
            "process receipt와 실행 lease가 일치하지 않습니다".to_string(),
        ));
    }
    let outcome = super::process_identity::terminate_if_matches(pgid, &receipt.identity_hash).await;
    match outcome {
        Ok(super::process_identity::ProcessTerminationOutcome::IdentityMismatch) => {
            return Ok(ProcessRecovery::Quarantined(
                "process birth identity does not match the durable receipt".to_string(),
            ));
        }
        Err(error) => return Ok(ProcessRecovery::Quarantined(error.to_string())),
        Ok(
            super::process_identity::ProcessTerminationOutcome::Absent
            | super::process_identity::ProcessTerminationOutcome::Terminated,
        ) => {}
    }
    db::set_convo_pgid(pool, task.id, None).await?;
    Ok(ProcessRecovery::Cleared)
}

fn recovery_event(
    process: ProcessRecovery,
    retirement: anyhow::Result<()>,
) -> (&'static str, String) {
    let process_error = match process {
        ProcessRecovery::Cleared => None,
        ProcessRecovery::Quarantined(error) => Some(error),
    };
    let projection_error = retirement.err().map(|error| error.to_string());
    match (process_error, projection_error) {
        (Some(process), Some(projection)) => (
            "recovery_quarantined",
            format!("process 격리: {process}; projection 격리: {projection}"),
        ),
        (Some(process), None) => ("process_quarantined", format!("process 격리: {process}")),
        (None, Some(projection)) => (
            "memory_projection_quarantined",
            format!("자동 projection 회수가 불가능해 격리했습니다: {projection}"),
        ),
        (None, None) => (
            "recovery",
            "Runner 재시작 후 프로세스 소유권을 확인할 수 없습니다".to_string(),
        ),
    }
}

pub(super) async fn invalid_queued(pool: &SqlitePool, now: i64) -> anyhow::Result<u64> {
    let tasks = db::list_tasks(pool).await?;
    let mut recovered = 0;
    for task in tasks.iter().filter(|task| task.state == state::QUEUED) {
        if super::review_process::task_is_fenced(pool, task.id).await? {
            continue;
        }
        let Err(error) = crate::memory::verify_task_projection(pool, task.id, now).await else {
            continue;
        };
        db::transition_state_with_runner_event(
            pool,
            task.id,
            state::FAILED,
            now,
            "memory_projection_invalid",
            Some(&error.to_string()),
        )
        .await?;
        recovered += 1;
    }
    Ok(recovered)
}

pub(super) async fn orphan_created(
    config: &super::config::RunnerConfig,
    pool: &SqlitePool,
    worktree_locks: &super::worktree_lock::WorktreeLocks,
    now: i64,
) -> anyhow::Result<u64> {
    let tasks = db::list_tasks(pool).await?;
    let mut recovered = 0;
    for task in tasks.iter().filter(|task| task.state == state::CREATED) {
        if super::review_process::task_is_fenced(pool, task.id).await? {
            continue;
        }
        recover_created(config, pool, worktree_locks, task, now).await?;
        recovered += 1;
    }
    Ok(recovered)
}

async fn recover_created(
    config: &super::config::RunnerConfig,
    pool: &SqlitePool,
    worktree_locks: &super::worktree_lock::WorktreeLocks,
    task: &db::Task,
    now: i64,
) -> anyhow::Result<()> {
    let _guard = worktree_locks
        .acquire(std::path::Path::new(&task.worktree_path))
        .await;
    let repo = super::auth::authorize_repository_path(
        &config.repository_roots,
        std::path::Path::new(&task.repo),
    )
    .map_err(anyhow::Error::msg)?;
    let path = authorized_stored_worktree(config, &repo, &task.worktree_path)?;
    crate::memory::retire_task_projection_if_present(pool, task.id, now).await?;
    crate::worktree::Worktree {
        repo,
        path,
        branch: task.branch.clone(),
        base: task.base.clone(),
        base_revision: task.base_revision.clone(),
    }
    .discard()?;
    db::transition_state_with_runner_event(
        pool,
        task.id,
        state::FAILED,
        now,
        "memory_projection_orphaned",
        Some("Runner restarted before a Created task entered approval or queue"),
    )
    .await?;
    Ok(())
}

fn authorized_stored_worktree(
    config: &super::config::RunnerConfig,
    repo: &std::path::Path,
    stored: &str,
) -> anyhow::Result<std::path::PathBuf> {
    let stored = std::path::PathBuf::from(stored);
    if stored.exists() {
        return super::auth::authorize_repository_path(&config.repository_roots, &stored)
            .map_err(anyhow::Error::msg);
    }
    if !stored.starts_with(repo.join(".praxis").join("worktrees")) {
        anyhow::bail!("stored worktree path escaped the authorized repository");
    }
    Ok(stored)
}
