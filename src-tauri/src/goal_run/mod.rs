//! 목표 계약에 예산과 수명주기를 붙인 실행 단위 — Goal Run (계획 0036).
//!
//! `goal_contract`는 프롬프트에 주입되고 `protected_paths`만 기계적으로 집행된다.
//! 목표가 미달성일 때 **같은 목표로 다시 시도하는 경로**가 없었고, 실행 예산 개념도 없었다.
//! 이 모듈은 그 둘을 붙인다 — 재진입 구동은 `schedule::runner`의 크론 틱이 맡는다(DR-2).
//!
//! 이 파일과 `decide`/`spend`/`schema`는 **Tauri 비의존**이다 — `SqlitePool`과 순수 함수만
//! 다루므로 `cargo test`로 돈다. 오케스트레이션(`AppHandle`·작업 생성·검증 실행)은 `tick`에
//! 모았다. `schedule`이 순수 `mod.rs`와 Tauri 의존 `runner.rs`로 갈린 것과 같은 구조다.

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::goal_contract::GoalContract;

pub mod decide;
pub mod schema;
pub mod spend;
pub mod tick;

#[cfg(test)]
mod tests;

/// Run 수명주기. `running` 외에는 모두 종료 상태다.
pub mod status {
    pub const RUNNING: &str = "running";
    /// 증거 게이트를 통과해 목표가 달성됨.
    pub const SATISFIED: &str = "satisfied";
    /// 예산 소진으로 정지.
    pub const EXHAUSTED: &str = "exhausted";
    /// 사용자가 중단했거나 거부해서 정지.
    pub const STOPPED: &str = "stopped";
}

/// 예산. **0은 "무제한"** 을 뜻한다 — 넷 다 0이면 정지 조건이 없으므로 거부한다.
///
/// 필드 선택은 Phase 2 탐색에서 원장을 실측하고 정했다:
/// `ConvoEvent::Result`가 턴 종료마다 `tokens_in`/`tokens_out`/`cost_usd`를 **태스크별로**
/// 싣고 있어(`convo/mod.rs:127-135`) 셋 다 원장에서 파생할 수 있다.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    /// Run이 만들 수 있는 시도(태스크) 개수.
    ///
    /// 벤더가 보고하는 에이전트 내부 턴 수(`Result.num_turns`)가 **아니다.** 사용자가
    /// 통제하려는 것은 "몇 번이나 다시 시킬 것인가"이고, 한 시도 안의 턴은 이미 벤더가
    /// 관리한다. 또한 이 항목만이 원장에 아무것도 안 남는 크래시에도 계상되므로,
    /// 다른 예산이 0으로 보일 때의 **백스톱**이다.
    #[serde(default)]
    pub max_attempts: i64,
    /// `Result`의 `tokens_in + tokens_out` 누적.
    #[serde(default)]
    pub max_tokens: i64,
    /// `Result`의 `cost_usd` 누적.
    ///
    /// 벤더 비대칭에 주의 — codex는 비용을 주지 않아 항상 0이다(`convo/mod.rs:126`).
    /// codex 작업에서는 이 예산이 발동하지 않는다. 추정으로 채우지 않는다(#235 원칙).
    #[serde(default)]
    pub max_cost_usd: f64,
    /// Run 생성 시각으로부터의 경과. 시도가 도는 동안에도 흐른다.
    #[serde(default)]
    pub max_wall_secs: i64,
}

impl Budget {
    pub fn validate(&self) -> Result<(), String> {
        if self.max_attempts < 0
            || self.max_tokens < 0
            || self.max_cost_usd < 0.0
            || self.max_wall_secs < 0
        {
            return Err("예산은 음수일 수 없습니다".into());
        }
        if !self.max_cost_usd.is_finite() {
            return Err("비용 예산이 유한한 값이어야 합니다".into());
        }
        if self.max_attempts == 0
            && self.max_tokens == 0
            && self.max_cost_usd == 0.0
            && self.max_wall_secs == 0
        {
            return Err("예산 넷 중 하나 이상은 0보다 커야 합니다".into());
        }
        Ok(())
    }
}

/// 지금까지 쓴 양. 전부 실측이며 추정값이 섞이지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct Spent {
    pub attempts: i64,
    pub tokens: i64,
    pub cost_usd: f64,
    pub elapsed_secs: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ExhaustReason {
    Attempts,
    Tokens,
    Cost,
    WallClock,
}

impl ExhaustReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Attempts => "재진입 횟수 소진",
            Self::Tokens => "토큰 예산 소진",
            Self::Cost => "비용 예산 소진",
            Self::WallClock => "시간 예산 소진",
        }
    }
}

/// 넷 중 **하나라도** 소진되면 정지한다 (OR). 폭주 방지가 목적이므로 가장 먼저 걸리는 것이
/// 이긴다. 상한이 0인 항목은 무제한이라 판정에서 뺀다.
pub fn exhausted(budget: &Budget, spent: &Spent) -> Option<ExhaustReason> {
    if budget.max_attempts > 0 && spent.attempts >= budget.max_attempts {
        return Some(ExhaustReason::Attempts);
    }
    if budget.max_tokens > 0 && spent.tokens >= budget.max_tokens {
        return Some(ExhaustReason::Tokens);
    }
    if budget.max_cost_usd > 0.0 && spent.cost_usd >= budget.max_cost_usd {
        return Some(ExhaustReason::Cost);
    }
    if budget.max_wall_secs > 0 && spent.elapsed_secs >= budget.max_wall_secs {
        return Some(ExhaustReason::WallClock);
    }
    None
}

/// 저장된 한 행. `goal_contract`·`budget`은 JSON 텍스트다.
#[derive(Debug, Clone, sqlx::FromRow)]
struct RunRow {
    id: i64,
    repo: String,
    agent: String,
    instruction: String,
    goal_contract: String,
    budget: String,
    status: String,
    created_at: i64,
    ended_at: Option<i64>,
    end_reason: Option<String>,
}

/// 파싱된 Run.
#[derive(Debug, Clone, Serialize)]
pub struct Run {
    pub id: i64,
    pub repo: String,
    pub agent: String,
    pub instruction: String,
    pub goal_contract: GoalContract,
    pub budget: Budget,
    pub status: String,
    pub created_at: i64,
    pub ended_at: Option<i64>,
    pub end_reason: Option<String>,
}

impl RunRow {
    /// 계약을 **저장 시점의 형태 그대로** 되살린다. 계약 스키마가 v2로 가도 진행 중인 Run은
    /// 만들어질 때의 v1으로 계속 판정되어야 하므로, 여기서 마이그레이션하지 않는다.
    fn parse(self) -> anyhow::Result<Run> {
        Ok(Run {
            goal_contract: serde_json::from_str(&self.goal_contract)?,
            budget: serde_json::from_str(&self.budget)?,
            id: self.id,
            repo: self.repo,
            agent: self.agent,
            instruction: self.instruction,
            status: self.status,
            created_at: self.created_at,
            ended_at: self.ended_at,
            end_reason: self.end_reason,
        })
    }
}

/// 새 Run 입력.
#[derive(Debug, Clone, Deserialize)]
pub struct NewRun {
    pub repo: String,
    pub agent: String,
    pub instruction: String,
    pub goal_contract: GoalContract,
    pub budget: Budget,
}

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::raw_sql(schema::MIGRATION).execute(pool).await?;
    Ok(())
}

pub async fn insert_run(pool: &SqlitePool, new: &NewRun, now: i64) -> anyhow::Result<i64> {
    new.goal_contract
        .validate()
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    new.budget.validate().map_err(|e| anyhow::anyhow!("{e}"))?;
    if new.repo.trim().is_empty() {
        anyhow::bail!("repo가 비어 있습니다");
    }
    if new.instruction.trim().is_empty() {
        anyhow::bail!("instruction이 비어 있습니다");
    }
    let row: (i64,) = sqlx::query_as(
        "INSERT INTO goal_runs \
         (repo, agent, instruction, goal_contract, budget, status, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?) RETURNING id",
    )
    .bind(new.repo.trim())
    .bind(new.agent.trim())
    .bind(new.instruction.trim())
    .bind(serde_json::to_string(&new.goal_contract)?)
    .bind(serde_json::to_string(&new.budget)?)
    .bind(status::RUNNING)
    .bind(now)
    .fetch_one(pool)
    .await?;
    Ok(row.0)
}

pub async fn get_run(pool: &SqlitePool, id: i64) -> anyhow::Result<Option<Run>> {
    let row: Option<RunRow> = sqlx::query_as("SELECT * FROM goal_runs WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    row.map(RunRow::parse).transpose()
}

/// 아직 도는 Run들. 파싱에 실패한 행은 **건너뛰되 로그를 남긴다** — 한 행이 깨졌다고
/// 나머지 Run이 멈추면 안 된다(크론 틱의 best-effort 관례와 같다).
pub async fn list_active_runs(pool: &SqlitePool) -> anyhow::Result<Vec<Run>> {
    let rows: Vec<RunRow> = sqlx::query_as("SELECT * FROM goal_runs WHERE status = ? ORDER BY id")
        .bind(status::RUNNING)
        .fetch_all(pool)
        .await?;
    Ok(parse_rows(rows))
}

pub async fn list_runs(pool: &SqlitePool, repo: Option<&str>) -> anyhow::Result<Vec<Run>> {
    let rows: Vec<RunRow> = match repo {
        Some(repo) => {
            sqlx::query_as("SELECT * FROM goal_runs WHERE repo = ? ORDER BY id DESC")
                .bind(repo)
                .fetch_all(pool)
                .await?
        }
        None => {
            sqlx::query_as("SELECT * FROM goal_runs ORDER BY id DESC")
                .fetch_all(pool)
                .await?
        }
    };
    Ok(parse_rows(rows))
}

/// 깨진 행은 **건너뛰되 로그를 남긴다** — 한 행이 깨졌다고 나머지 Run이 안 보이면 안 되고,
/// 조용히 사라지면 "Run이 없다"와 구분되지 않는다(크론 틱의 best-effort 관례와 같다).
fn parse_rows(rows: Vec<RunRow>) -> Vec<Run> {
    rows.into_iter()
        .filter_map(|row| {
            let id = row.id;
            match row.parse() {
                Ok(run) => Some(run),
                Err(e) => {
                    eprintln!("goal_run #{id} 파싱 실패 — 건너뜁니다: {e}");
                    None
                }
            }
        })
        .collect()
}

/// 시도를 기록한다. **태스크를 만들기 전에** 부른다 — 크론의 `mark_schedule_ran` 선기록과
/// 같은 순서다(`schedule/runner.rs:63-65`). 반대로 하면 태스크 생성 후 기록이 실패했을 때
/// 같은 Run이 다음 틱에 또 태스크를 만든다.
pub async fn record_attempt(
    pool: &SqlitePool,
    run_id: i64,
    task_id: i64,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO goal_run_attempts (run_id, task_id, seq, created_at) \
         VALUES (?, ?, (SELECT COALESCE(MAX(seq), 0) + 1 FROM goal_run_attempts WHERE run_id = ?), ?)",
    )
    .bind(run_id)
    .bind(task_id)
    .bind(run_id)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// 시도로 만들어진 task id 목록 (오래된 순).
pub async fn attempt_task_ids(pool: &SqlitePool, run_id: i64) -> anyhow::Result<Vec<i64>> {
    let rows: Vec<(i64,)> =
        sqlx::query_as("SELECT task_id FROM goal_run_attempts WHERE run_id = ? ORDER BY seq")
            .bind(run_id)
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|r| r.0).collect())
}

/// 가장 최근 시도. 없으면 `None` (Run 생성 직후).
#[derive(Debug, Clone, Copy, PartialEq, sqlx::FromRow)]
pub struct AttemptRow {
    pub task_id: i64,
    pub seq: i64,
    /// `None`이면 아직 판정하지 않았거나(=`gate_evaluated_at`도 `None`),
    /// 판정했으나 검증 커맨드가 없었다.
    pub gate_ready: Option<bool>,
    pub gate_evaluated_at: Option<i64>,
}

impl AttemptRow {
    pub fn gate_evaluated(&self) -> bool {
        self.gate_evaluated_at.is_some()
    }
}

pub async fn latest_attempt(pool: &SqlitePool, run_id: i64) -> anyhow::Result<Option<AttemptRow>> {
    let row: Option<AttemptRow> = sqlx::query_as(
        "SELECT task_id, seq, gate_ready, gate_evaluated_at FROM goal_run_attempts \
         WHERE run_id = ? ORDER BY seq DESC LIMIT 1",
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// 게이트 판정을 적는다. `ready`가 `None`이면 "평가했으나 검증 커맨드가 없었다"는 뜻이고,
/// 그 구분은 `gate_evaluated_at`이 담는다 — 재평가를 막으려면 둘 다 써야 한다.
pub async fn record_gate(
    pool: &SqlitePool,
    task_id: i64,
    ready: Option<bool>,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE goal_run_attempts SET gate_ready = ?, gate_evaluated_at = ? WHERE task_id = ?",
    )
    .bind(ready)
    .bind(now)
    .bind(task_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// 이 태스크가 속한 Run. 승인/거부 경로에서 "이게 Goal Run의 시도인가"를 묻는 데 쓴다.
pub async fn run_id_of_task(pool: &SqlitePool, task_id: i64) -> anyhow::Result<Option<i64>> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT run_id FROM goal_run_attempts WHERE task_id = ?")
            .bind(task_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|r| r.0))
}

/// Run을 종료 상태로 옮긴다. 이미 끝난 Run은 건드리지 않는다 — 먼저 쓴 종료 사유가 이긴다.
pub async fn end_run(
    pool: &SqlitePool,
    id: i64,
    new_status: &str,
    reason: &str,
    now: i64,
) -> anyhow::Result<bool> {
    let result = sqlx::query(
        "UPDATE goal_runs SET status = ?, ended_at = ?, end_reason = ? \
         WHERE id = ? AND status = ?",
    )
    .bind(new_status)
    .bind(now)
    .bind(reason)
    .bind(id)
    .bind(status::RUNNING)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

/// 사용자 중단.
pub async fn stop_run(pool: &SqlitePool, id: i64, now: i64) -> anyhow::Result<bool> {
    end_run(pool, id, status::STOPPED, "사용자가 중단했습니다", now).await
}
