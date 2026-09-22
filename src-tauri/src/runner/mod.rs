pub mod actions;
pub mod auth;
pub mod capacity;
pub mod config;
pub mod events;
pub mod file_mutation;
pub mod finalization;
pub mod http;
pub mod instance_lock;
mod memory_gate;
pub mod memory_http;
pub mod mobile_http;
pub mod process;
pub(crate) mod process_identity;
pub mod push;
pub mod queue;
mod recovery;
pub mod review_http;
pub mod review_process;
pub mod review_process_http;
pub mod schedule;
pub mod session;
mod task_creation;
#[cfg(test)]
mod task_creation_tests;
pub mod worktree_lock;
pub mod workflow;

use serde::Deserialize;
use sqlx::SqlitePool;

use crate::db::{self, state};
use crate::goal_contract::GoalContract;

pub use finalization::finalize_task;
pub use memory_gate::{approve_pending_task, cancel_pending_task};
pub use task_creation::{create_queued_task, CreateTaskError};

pub const RETENTION_DAYS: i64 = 60;

pub struct RunnerRuntime {
    config: config::RunnerConfig,
    pool: SqlitePool,
    recovered_tasks: u64,
    worktree_locks: worktree_lock::WorktreeLocks,
    _instance_lock: instance_lock::RunnerInstanceLock,
}

/// Runner HTTP가 받는 비대화형 작업 생성 요청. Runner는 항상 격리 worktree와 durable queue를 쓴다.
#[derive(Debug, Deserialize)]
pub struct QueuedTaskRequest {
    pub repository: String,
    pub instruction: String,
    pub agent: String,
    #[serde(default = "default_agent_role")]
    pub role: String,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub reasoning_effort: String,
    #[serde(default = "default_task_mode")]
    pub mode: String,
    #[serde(default)]
    pub goal_contract: Option<GoalContract>,
    /// 세션홈에서 고른 벤더 세션을 이 작업이 이어받는다(설계 2026-09-17). `mode`가
    /// `conversation`이어야 하고, agy는 세션 id 대신 "직전 대화" 센티널을 쓰므로 거절된다.
    #[serde(default)]
    pub resume_session: Option<String>,
}

fn default_task_mode() -> String {
    "terminal".to_string()
}

fn default_agent_role() -> String {
    crate::agent::DEFAULT_ROLE.to_string()
}

impl RunnerRuntime {
    pub fn config(&self) -> &config::RunnerConfig {
        &self.config
    }

    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    pub fn recovered_tasks(&self) -> u64 {
        self.recovered_tasks
    }

    pub fn worktree_locks(&self) -> worktree_lock::WorktreeLocks {
        self.worktree_locks.clone()
    }
}

/// Unix epoch 초. 신규 Runner 코드의 공통 시각 소스.
pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

/// Runner 전용 DB를 열고, 현재 소유를 입증할 수 없는 실행 작업을 복구 실패로 기록한다.
pub async fn initialize(
    config: config::RunnerConfig,
    db_path: &str,
    now: i64,
) -> anyhow::Result<RunnerRuntime> {
    let instance_lock = instance_lock::RunnerInstanceLock::acquire(std::path::Path::new(db_path))?;
    let pool = db::init_pool(db_path).await?;
    crate::side_question::migrate(&pool).await?;
    let side_interrupted = crate::side_question::recover(&pool).await?;
    crate::memory::migrate(&pool).await?;
    finalization::migrate(&pool).await?;
    crate::annotations::migrate(&pool).await?;
    crate::partial::migrate(&pool).await?;
    crate::github::migrate(&pool).await?;
    review_process::migrate(&pool).await?;
    crate::schedule::migrate(&pool).await?;
    let worktree_locks = worktree_lock::WorktreeLocks::default();
    let review = review_process::reconcile(&pool, now).await?;
    let prepared = crate::memory::reconcile_prepared_projections(&pool, now).await?;
    let created = recovery::orphan_created(&config, &pool, &worktree_locks, now).await?;
    let finalized = finalization::reconcile(&config, &pool, &worktree_locks, now).await?;
    session::migrate(&pool).await?;
    push::migrate(&pool).await?;
    // 만료된 페어링 코드는 재기동 때 정리한다 — 남겨봐야 쓸 수 없다.
    let _ = session::purge_expired_pairings(&pool, now).await;
    let invalid = recovery::invalid_queued(&pool, now).await?;
    let running = recovery::unowned_running(&pool, &worktree_locks, now).await?;
    Ok(RunnerRuntime {
        config,
        pool,
        recovered_tasks: review
            + prepared
            + created
            + finalized
            + invalid
            + running
            + side_interrupted,
        worktree_locks,
        _instance_lock: instance_lock,
    })
}

/// task summary·schedule은 보존하고, replay용 output/event만 60일 뒤 정리한다.
pub async fn prune_history(pool: &SqlitePool, now: i64) -> anyhow::Result<u64> {
    db::prune_runner_history(pool, now - RETENTION_DAYS * 24 * 60 * 60).await
}

/// 종료된 Runner task의 이력과 replay 레코드를 영구 삭제한다.
pub async fn delete_finished_task(pool: &SqlitePool, task_id: i64) -> Result<(), String> {
    let task = db::get_task(pool, task_id)
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "작업을 찾을 수 없습니다".to_string())?;
    if !matches!(
        task.state.as_str(),
        state::DONE | state::FAILED | state::DISCARDED
    ) {
        return Err("종료된 작업만 이력에서 삭제할 수 있습니다".to_string());
    }
    db::delete_task(pool, task_id)
        .await
        .map_err(|error| error.to_string())
}
