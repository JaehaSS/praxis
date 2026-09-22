//! 금일 할 일(Day Plan) — 사람이 쓰는 계획 레이어. `Task`(에이전트 실행)와 1:1로 연결된다.
//!
//! Tauri 비의존(순수) 모듈이다 — `cargo test`로 독립 검증한다.
//! Tauri 경계는 `commands.rs`에만 둔다.
//!
//! **Runner·모바일에 노출하지 않는다** — 데스크톱 개인 계획 레이어다 (설계 0021 §3).
//!
//! 설계 정본: `docs/designs/0021.2026-08-03-today-plan-design.md`
//! 구현 플랜: `docs/plans/0026.2026-08-03-today-day-plan.md`

pub mod carry;
pub mod close;
pub mod day;
pub mod schema;
pub mod store;
pub mod suggest;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

/// 항목 상태. `dropped`("안 하기로 했다")와 `open`("못 했다")은 마감 집계에서 갈린다 —
/// 접은 계획과 밀린 계획은 다른 신호다 (설계 0021 §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DayStatus {
    Open,
    Done,
    Dropped,
}

impl DayStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            DayStatus::Open => "open",
            DayStatus::Done => "done",
            DayStatus::Dropped => "dropped",
        }
    }

    pub fn parse(s: &str) -> Result<Self, String> {
        match s {
            "open" => Ok(DayStatus::Open),
            "done" => Ok(DayStatus::Done),
            "dropped" => Ok(DayStatus::Dropped),
            other => Err(format!("알 수 없는 상태: {other}")),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DayItem {
    pub id: i64,
    pub day: String,
    pub title: String,
    pub note: Option<String>,
    pub status: String,
    pub position: i64,
    pub repo: Option<String>,
    pub task_id: Option<i64>,
    pub source: String,
    pub source_ref: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub done_at: Option<i64>,
    /// 이월돼 온 항목이면 **직전에** 있던 날. 최초 계획일이 아니다 — 매 이월마다 덮어쓴다.
    /// 이월은 행을 옮기는 파괴적 연산이라, 이게 없으면 "원래 언제 것인지"가 소실된다.
    pub carried_from: Option<String>,
}

/// 오늘 할 일 스키마를 생성한다. 앱 기동마다 호출되므로 멱등해야 한다.
pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::raw_sql(schema::MIGRATION).execute(pool).await?;
    // 구버전 DB 보강 — 이미 있으면 에러를 무시한다 (`db/mod.rs:138`과 같은 멱등 패턴).
    let _ = sqlx::query("ALTER TABLE day_items ADD COLUMN carried_from TEXT")
        .execute(pool)
        .await;
    Ok(())
}
