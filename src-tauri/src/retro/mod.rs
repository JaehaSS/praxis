//! 주간 회고 (설계 0054) — 관찰을 사람이 읽는 문장으로 바꾼다.
//!
//! **수치와 서술이 갈라져 있다.** 수치(`RetroFacts`)는 여기서 SQL로 확정하고, 서술은 헤드리스
//! 에이전트가 쓴다. LLM에게 숫자를 맡기지 않는 이유는 단순하다 — 회고에서 틀릴 수 있는 것은
//! 거의 전부 숫자이고, 한 번 틀린 회고는 다시 읽히지 않는다(DR-4).
//!
//! 생성 경로는 설계 0044(퀴즈)를 그대로 따른다. 스케줄 틱 → 승인 대기 작업 → 에이전트가
//! JSON 파일 출력 → `inbox`가 검증 후 적재. 새로 발명한 것이 없다(DR-5).

pub mod generate;
pub mod inbox;
pub mod schema;

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, SqlitePool};

#[cfg(test)]
mod tests;

/// 한 주의 길이(초).
pub const WEEK_SECS: i64 = 7 * 86_400;

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::raw_sql(schema::MIGRATION).execute(pool).await?;
    Ok(())
}

/// `ts`가 속한 주의 월요일 00:00 (UTC epoch).
///
/// `tz_offset_secs`를 **스케줄에 저장된 값으로** 받는 이유는 §6.3에 있다. 시스템 로컬
/// 시간대를 그때그때 읽으면 여행 중에 주 경계가 밀려 같은 주가 둘로 갈린다.
///
/// 유닉스 epoch(1970-01-01)은 목요일이다. 그래서 월요일까지의 거리는 `+3` 보정으로 나온다 —
/// day 4(1970-01-05)가 월요일이고 `(4 + 3) % 7 == 0`이다.
pub fn week_start_of(ts: i64, tz_offset_secs: i64) -> i64 {
    let local = ts + tz_offset_secs;
    let day = local.div_euclid(86_400);
    let from_monday = (day + 3).rem_euclid(7);
    (day - from_monday) * 86_400 - tz_offset_secs
}

/// 역할 하나의 결과율.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoleRate {
    pub role: String,
    pub count: i64,
    pub done_pct: f64,
}

/// 프롬프트에 주입할 **확정 수치**. 그대로 DB에도 저장한다(DR-7).
///
/// 에이전트는 이 값을 인용만 하고 새로 계산하지 않는다. `generate`가 그렇게 지시한다.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RetroFacts {
    pub week_start: i64,
    pub tasks_total: i64,
    pub tasks_done: i64,
    pub tasks_discarded: i64,
    pub discard_rate_pct: f64,
    /// 직전 주의 폐기율. 비교 대상이 없으면 None.
    pub discard_rate_prev_pct: Option<f64>,
    /// 후속 입력이 있었던 작업 비율. **횟수가 아니라 발생 여부**다 —
    /// `task_events`의 UNIQUE 인덱스 때문에 task당 한 행뿐이다(`db/mod.rs:297`).
    pub followup_pct: f64,
    pub proposals_pending: i64,
    pub proposals_applied: i64,
    /// 건수가 가장 많은 역할과 그 완료율. 표본이 없으면 None.
    pub top_role: Option<RoleRate>,
}

/// 저장된 다이제스트 한 건.
#[derive(Debug, Clone, Serialize)]
pub struct RetroDigest {
    pub week_start: i64,
    pub body: String,
    /// 파싱된 facts. 저장은 JSON 문자열이지만 화면에는 구조체로 준다.
    pub facts: RetroFacts,
    pub agent: Option<String>,
    pub model: Option<String>,
    pub generated_at: i64,
}

/// 주 네비게이션용 참조.
#[derive(Debug, Clone, Serialize, FromRow)]
pub struct RetroWeekRef {
    pub week_start: i64,
    pub generated_at: i64,
}

#[derive(FromRow)]
struct DigestRow {
    week_start: i64,
    body: String,
    facts: String,
    agent: Option<String>,
    model: Option<String>,
    generated_at: i64,
}

impl DigestRow {
    fn into_digest(self) -> RetroDigest {
        RetroDigest {
            week_start: self.week_start,
            body: self.body,
            // facts가 깨졌어도 서술은 보여준다 — 기본값으로 떨어뜨릴 뿐 화면을 죽이지 않는다.
            facts: serde_json::from_str(&self.facts).unwrap_or_default(),
            agent: self.agent,
            model: self.model,
            generated_at: self.generated_at,
        }
    }
}

/// 주 하나의 다이제스트. `week_start`가 None이면 가장 최근 주를 준다.
pub async fn get(
    pool: &SqlitePool,
    week_start: Option<i64>,
) -> anyhow::Result<Option<RetroDigest>> {
    let row: Option<DigestRow> = match week_start {
        Some(start) => {
            sqlx::query_as(
                "SELECT week_start, body, facts, agent, model, generated_at \
                   FROM retro_digests WHERE week_start = ?",
            )
            .bind(start)
            .fetch_optional(pool)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT week_start, body, facts, agent, model, generated_at \
                   FROM retro_digests ORDER BY week_start DESC LIMIT 1",
            )
            .fetch_optional(pool)
            .await?
        }
    };
    Ok(row.map(DigestRow::into_digest))
}

/// 생성된 주 목록(최신순).
pub async fn list(pool: &SqlitePool, limit: i64) -> anyhow::Result<Vec<RetroWeekRef>> {
    sqlx::query_as(
        "SELECT week_start, generated_at FROM retro_digests \
          ORDER BY week_start DESC LIMIT ?",
    )
    .bind(limit.clamp(1, 200))
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

#[derive(FromRow)]
struct WeekTaskRow {
    total: i64,
    done: i64,
    discarded: i64,
    with_followup: i64,
}

#[derive(FromRow)]
struct ProposalRow {
    pending: i64,
    applied: i64,
}

/// 한 주의 수치를 확정한다. 이 함수가 회고에서 **유일하게 숫자를 만드는 자리**다.
pub async fn collect_facts(pool: &SqlitePool, week_start: i64) -> anyhow::Result<RetroFacts> {
    let end = week_start + WEEK_SECS;
    let tasks = load_week_tasks(pool, week_start, end).await?;
    let prev = load_week_tasks(pool, week_start - WEEK_SECS, week_start).await?;
    let proposals = load_proposals(pool).await?;
    let top_role = load_top_role(pool, week_start, end).await?;

    Ok(RetroFacts {
        week_start,
        tasks_total: tasks.total,
        tasks_done: tasks.done,
        tasks_discarded: tasks.discarded,
        discard_rate_pct: rate(tasks.discarded, tasks.total).unwrap_or(0.0),
        discard_rate_prev_pct: rate(prev.discarded, prev.total),
        followup_pct: rate(tasks.with_followup, tasks.total).unwrap_or(0.0),
        proposals_pending: proposals.pending,
        proposals_applied: proposals.applied,
        top_role,
    })
}

/// 백분율. 분모가 0이면 None — 0%로 접으면 "없음"과 "0건"이 구분되지 않는다.
fn rate(part: i64, whole: i64) -> Option<f64> {
    if whole == 0 {
        return None;
    }
    Some((part as f64 * 1000.0 / whole as f64).round() / 10.0)
}

async fn load_week_tasks(
    pool: &SqlitePool,
    start: i64,
    end: i64,
) -> anyhow::Result<WeekTaskRow> {
    sqlx::query_as(
        "SELECT COUNT(*) AS total, \
                COALESCE(SUM(state = 'Done'), 0) AS done, \
                COALESCE(SUM(state = 'Discarded'), 0) AS discarded, \
                COALESCE(SUM(EXISTS(SELECT 1 FROM task_events e \
                   WHERE e.task_id = tasks.id \
                     AND e.kind = 'user_followup_input_observed')), 0) AS with_followup \
           FROM tasks WHERE created_at >= ? AND created_at < ?",
    )
    .bind(start)
    .bind(end)
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

/// 제안 적체는 주 구간으로 자르지 않는다 — 적체는 **누적된 상태**이지 그 주의 사건이 아니다.
async fn load_proposals(pool: &SqlitePool) -> anyhow::Result<ProposalRow> {
    sqlx::query_as(
        "SELECT COALESCE(SUM(status = 'proposed'), 0) AS pending, \
                COALESCE(SUM(status = 'applied'), 0) AS applied \
           FROM si_proposals",
    )
    .fetch_one(pool)
    .await
    .map_err(Into::into)
}

#[derive(FromRow)]
struct RoleRow {
    role: String,
    count: i64,
    done: i64,
}

async fn load_top_role(
    pool: &SqlitePool,
    start: i64,
    end: i64,
) -> anyhow::Result<Option<RoleRate>> {
    let row: Option<RoleRow> = sqlx::query_as(
        "SELECT role, COUNT(*) AS count, COALESCE(SUM(state = 'Done'), 0) AS done \
           FROM tasks WHERE created_at >= ? AND created_at < ? \
          GROUP BY role ORDER BY count DESC LIMIT 1",
    )
    .bind(start)
    .bind(end)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| RoleRate {
        role: r.role,
        count: r.count,
        done_pct: rate(r.done, r.count).unwrap_or(0.0),
    }))
}
