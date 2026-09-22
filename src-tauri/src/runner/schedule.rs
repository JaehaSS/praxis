//! Headless Runner용 크론 실행 루프. Tauri 앱 핸들 없이 durable queue에 작업만 적재한다.

use sqlx::SqlitePool;

use crate::db;
use crate::runner::worktree_lock::WorktreeLocks;
use crate::runner::{config::RunnerConfig, create_queued_task, QueuedTaskRequest};

const TICK_INTERVAL_SECS: u64 = 60;

#[derive(serde::Deserialize)]
struct TaskPayload {
    repo: String,
    instruction: String,
    #[serde(default)]
    agent: String,
    #[serde(default)]
    model: String,
    #[serde(default)]
    mode: String,
    #[serde(default)]
    goal_contract: Option<crate::goal_contract::GoalContract>,
}

#[derive(serde::Deserialize)]
struct ReminderPayload {
    text: String,
}

/// 매 스케줄을 한 틱에 한 번만 확인한다. 발화 시각은 실행 전 기록해 재시작·지연에도 중복을 막는다.
pub async fn tick_loop(config: RunnerConfig, pool: SqlitePool, worktree_locks: WorktreeLocks) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(TICK_INTERVAL_SECS));
    loop {
        interval.tick().await;
        let now = now();
        let schedules = match db::list_enabled_schedules(&pool).await {
            Ok(schedules) => schedules,
            Err(error) => {
                eprintln!("Runner schedule 목록 조회 실패: {error}");
                continue;
            }
        };
        for schedule in schedules {
            run_if_due(&config, &pool, &worktree_locks, &schedule, now).await;
        }
    }
}

async fn run_if_due(
    config: &RunnerConfig,
    pool: &SqlitePool,
    worktree_locks: &WorktreeLocks,
    schedule: &db::Schedule,
    now: i64,
) {
    let due = match schedule.run_at {
        Some(run_at) => now >= run_at && schedule.last_run_at.is_none(),
        None => crate::schedule::is_due(
            &schedule.cron,
            schedule.last_run_at.unwrap_or(schedule.created_at),
            now,
            schedule.tz_offset_secs,
        )
        .unwrap_or_else(|error| {
            eprintln!("Runner schedule #{} cron 파싱 실패: {error}", schedule.id);
            false
        }),
    };
    if !due {
        return;
    }
    if let Err(error) = db::mark_schedule_ran(pool, schedule.id, now).await {
        eprintln!("Runner schedule #{} 발화 기록 실패: {error}", schedule.id);
        return;
    }
    match schedule.kind.as_str() {
        "task" => queue_task(config, pool, worktree_locks, schedule, now).await,
        "reminder" => emit_reminder(schedule),
        kind => eprintln!(
            "Runner schedule #{} 지원하지 않는 kind: {kind}",
            schedule.id
        ),
    }
    if schedule.run_at.is_some() {
        if let Err(error) = db::set_schedule_enabled(pool, schedule.id, false).await {
            eprintln!(
                "Runner schedule #{} 자동 비활성화 실패: {error}",
                schedule.id
            );
        }
    }
}

async fn queue_task(
    config: &RunnerConfig,
    pool: &SqlitePool,
    worktree_locks: &WorktreeLocks,
    schedule: &db::Schedule,
    now: i64,
) {
    let payload: TaskPayload = match serde_json::from_str(&schedule.payload) {
        Ok(payload) => payload,
        Err(error) => {
            eprintln!(
                "Runner schedule #{} task payload 오류: {error}",
                schedule.id
            );
            return;
        }
    };
    let request = QueuedTaskRequest {
        repository: payload.repo,
        instruction: payload.instruction,
        agent: if payload.agent.trim().is_empty() {
            "claude".to_string()
        } else {
            payload.agent
        },
        role: crate::agent::DEFAULT_ROLE.to_string(),
        model: payload.model,
        reasoning_effort: String::new(),
        mode: if payload.mode.trim().is_empty() {
            "terminal".to_string()
        } else {
            payload.mode
        },
        goal_contract: payload.goal_contract,
        // 스케줄은 항상 새 대화에서 시작한다 — 이어받을 세션이 없다.
        resume_session: None,
    };
    if let Err(error) = create_queued_task(config, pool, worktree_locks, request, now).await {
        eprintln!("Runner schedule #{} task queue 실패: {error}", schedule.id);
    }
}

fn emit_reminder(schedule: &db::Schedule) {
    match serde_json::from_str::<ReminderPayload>(&schedule.payload) {
        Ok(payload) => eprintln!("Runner reminder #{}: {}", schedule.id, payload.text),
        Err(error) => eprintln!(
            "Runner schedule #{} reminder payload 오류: {error}",
            schedule.id
        ),
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
