//! 작업 패턴 집계 (설계 0054 §6.2) — "작업이 어떻게 굴러갔는가".
//!
//! 사용량 인사이트가 `~/.claude` 트랜스크립트를 읽는 것과 달리 여기는 **작업 DB만** 본다.
//! LLM이 끼지 않는다.
//!
//! 설계 단계에서 지표 둘을 뺐다. `goal_run_attempts`(되돌아감 횟수)는 실측 0건이고
//! `tasks.blocked_reason`은 전량 NULL이라, 화면에 올리면 빈 레인이 된다. 스키마에 컬럼이
//! 있다는 것과 값이 쌓인다는 것은 다르다(DR-3).

use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

/// 상태별 현재 분포.
#[derive(Debug, Clone, Serialize, FromRow, PartialEq)]
pub struct StateCount {
    pub state: String,
    pub count: i64,
}

/// 도달 퍼널. `started`·`reviewed`는 현재 상태가 아니라 **이력**(`task_events`)으로 센다 —
/// 이미 Done인 작업도 실행을 거쳐 왔기 때문이다.
#[derive(Debug, Clone, Default, Serialize, FromRow, PartialEq)]
pub struct Funnel {
    pub total: i64,
    pub started: i64,
    pub reviewed: i64,
    pub done: i64,
    pub discarded: i64,
    pub failed: i64,
}

/// 한 달의 폐기율.
#[derive(Debug, Clone, Serialize, FromRow, PartialEq)]
pub struct MonthRate {
    /// YYYY-MM (로컬)
    pub month: String,
    pub total: i64,
    pub discarded: i64,
}

/// 후속 입력 발생 여부. **횟수가 아니다** — `task_events`의 UNIQUE 인덱스 때문에
/// task당 한 행뿐이라 셀 수가 없다(`db/mod.rs:297`).
#[derive(Debug, Clone, Default, Serialize, FromRow, PartialEq)]
pub struct FollowupSplit {
    pub total: i64,
    pub with_followup: i64,
}

/// 역할 하나의 결말 분포. `model`을 축에 넣지 않는 이유는 실측에서 대부분 NULL이었기
/// 때문이다 — 조합표를 만들면 표본이 한 자릿수로 쪼개진다(DR-3).
#[derive(Debug, Clone, Serialize, FromRow, PartialEq)]
pub struct RoleOutcome {
    pub role: String,
    pub count: i64,
    pub done: i64,
    pub discarded: i64,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskPatterns {
    pub funnel: Funnel,
    pub states: Vec<StateCount>,
    /// **항상 전체 월별이다.** 범위 칩을 따르지 않는다 — 추세는 잘라내면 추세가 아니다(§6.2).
    pub discard_trend: Vec<MonthRate>,
    pub followup: FollowupSplit,
    pub role_outcomes: Vec<RoleOutcome>,
    /// 착수→종료 소요(초). 표본이 없으면 None.
    pub duration_p50: Option<i64>,
    pub duration_p90: Option<i64>,
}

pub async fn compute_patterns(
    pool: &SqlitePool,
    range: &str,
    tz_offset_secs: i64,
    now: i64,
) -> anyhow::Result<TaskPatterns> {
    let cutoff = cutoff(range, now);
    let durations = load_durations(pool, cutoff).await?;
    Ok(TaskPatterns {
        funnel: load_funnel(pool, cutoff).await?,
        states: load_states(pool, cutoff).await?,
        discard_trend: load_discard_trend(pool, tz_offset_secs).await?,
        followup: load_followup(pool, cutoff).await?,
        role_outcomes: load_role_outcomes(pool, cutoff).await?,
        duration_p50: percentile(&durations, 50),
        duration_p90: percentile(&durations, 90),
    })
}

/// `outcomes.rs`와 같은 규칙. 다만 기준 컬럼은 `created_at`이다 — 퍼널이 답하는 질문은
/// "이 기간에 **시작한** 작업이 어디까지 갔는가"이기 때문이다.
fn cutoff(range: &str, now: i64) -> i64 {
    match range {
        "7d" => now.saturating_sub(7 * 86_400),
        "30d" => now.saturating_sub(30 * 86_400),
        _ => 0,
    }
}

/// 정렬된 표본에서 백분위. 비었으면 None — 0으로 접으면 "없음"과 "0초"가 섞인다.
///
/// nearest-rank를 쓴다 — "표본의 p%가 이 값 이하"라는 뜻이 되어 소요 시간에서 읽기가
/// 자연스럽다. 보간법을 쓰면 실제로는 존재하지 않는 소요가 화면에 뜬다.
fn percentile(sorted: &[i64], p: usize) -> Option<i64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (p * sorted.len()).div_ceil(100).max(1);
    sorted.get(rank - 1).copied()
}

async fn load_funnel(pool: &SqlitePool, cutoff: i64) -> anyhow::Result<Funnel> {
    sqlx::query_as(
        "SELECT COUNT(*) AS total, \
                COALESCE(SUM(EXISTS(SELECT 1 FROM task_events e \
                   WHERE e.task_id = tasks.id AND e.kind = 'running')), 0) AS started, \
                COALESCE(SUM(state IN ('AwaitingReview', 'Finalizing', 'Done', 'Discarded') \
                   OR EXISTS(SELECT 1 FROM task_events e WHERE e.task_id = tasks.id \
                     AND e.kind IN ('approved', 'discarded'))), 0) AS reviewed, \
                COALESCE(SUM(state = 'Done'), 0) AS done, \
                COALESCE(SUM(state = 'Discarded'), 0) AS discarded, \
                COALESCE(SUM(state = 'Failed'), 0) AS failed \
           FROM tasks WHERE created_at >= ?",
    )
    .bind(cutoff)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

async fn load_states(pool: &SqlitePool, cutoff: i64) -> anyhow::Result<Vec<StateCount>> {
    sqlx::query_as(
        "SELECT state, COUNT(*) AS count FROM tasks WHERE created_at >= ? \
          GROUP BY state ORDER BY count DESC",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

/// 월 버킷은 로컬 기준이다. UTC로 자르면 월 경계 근처 작업이 옆 달로 새고, 그러면
/// 폐기율 추세가 실제와 어긋난다(§5.1).
async fn load_discard_trend(
    pool: &SqlitePool,
    tz_offset_secs: i64,
) -> anyhow::Result<Vec<MonthRate>> {
    sqlx::query_as(
        "SELECT strftime('%Y-%m', created_at + ?, 'unixepoch') AS month, \
                COUNT(*) AS total, \
                COALESCE(SUM(state = 'Discarded'), 0) AS discarded \
           FROM tasks GROUP BY month ORDER BY month",
    )
    .bind(tz_offset_secs)
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

async fn load_followup(pool: &SqlitePool, cutoff: i64) -> anyhow::Result<FollowupSplit> {
    sqlx::query_as(
        "SELECT COUNT(*) AS total, \
                COALESCE(SUM(EXISTS(SELECT 1 FROM task_events e \
                   WHERE e.task_id = tasks.id \
                     AND e.kind = 'user_followup_input_observed')), 0) AS with_followup \
           FROM tasks WHERE created_at >= ?",
    )
    .bind(cutoff)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

async fn load_role_outcomes(pool: &SqlitePool, cutoff: i64) -> anyhow::Result<Vec<RoleOutcome>> {
    sqlx::query_as(
        "SELECT role, COUNT(*) AS count, \
                COALESCE(SUM(state = 'Done'), 0) AS done, \
                COALESCE(SUM(state = 'Discarded'), 0) AS discarded \
           FROM tasks WHERE created_at >= ? \
          GROUP BY role ORDER BY count DESC",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

/// 종료된 작업의 소요만 센다. 진행 중인 작업의 `updated_at`은 "지금까지"일 뿐이라
/// 섞으면 중앙값이 계속 흔들린다.
async fn load_durations(pool: &SqlitePool, cutoff: i64) -> anyhow::Result<Vec<i64>> {
    let rows: Vec<(i64,)> = sqlx::query_as(
        "SELECT updated_at - created_at AS secs FROM tasks \
          WHERE created_at >= ? AND state IN ('Done', 'Discarded', 'Failed') \
            AND updated_at >= created_at \
          ORDER BY secs",
    )
    .bind(cutoff)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(s,)| s).collect())
}

#[cfg(test)]
mod tests;
