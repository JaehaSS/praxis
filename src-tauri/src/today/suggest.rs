//! 오늘의 후보 제안 3소스 (설계 0021 §6).
//!
//! 제안은 **자동으로 목록에 들어가지 않는다** — 사용자가 "담기"를 눌러야 한다 (DR-3).
//! 여기서는 후보만 모으고, 이미 담은 것은 `exclude_taken`이 걸러 낸다.
//!
//! 지난 날의 미완료(`carry`)는 더 이상 제안이 아니다 — `today_list`가 직접 옮긴다
//! (`super::carry`). 제안으로도 띄우면 같은 항목이 목록과 후보에 동시에 보인다.
//!
//! github 소스는 `gh` CLI(네트워크)를 타므로 이 순수 모듈이 아니라 `commands.rs`에서
//! 조립한다 — 여기 있는 것은 DB·파일만 읽는 두 소스다.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::path::Path;

/// 방치로 판정하는 시간 — 반나절 미만이면 "아직 보는 중"일 수 있다.
const AWAITING_IDLE_SECS: i64 = 6 * 3600;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Suggestion {
    pub title: String,
    pub source: String,
    /// 같은 날 재제안을 막는 키. 소스별로 의미가 다르다(task id, 이슈 번호, 원장 번호…).
    pub source_ref: Option<String>,
    pub repo: Option<String>,
}

/// 6시간 넘게 검토 대기로 방치된 Task. 컬럼명은 `state`다 (`db/mod.rs:105`).
pub async fn awaiting(pool: &SqlitePool, now: i64) -> Result<Vec<Suggestion>, String> {
    let rows = sqlx::query(
        "SELECT id, instruction, repo FROM tasks \
         WHERE state = 'AwaitingReview' AND updated_at <= ? ORDER BY updated_at",
    )
    .bind(now - AWAITING_IDLE_SECS)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows
        .iter()
        .map(|r| {
            let instruction: String = r.get("instruction");
            Suggestion {
                title: format!("검토: {}", first_line(&instruction)),
                source: "awaiting".into(),
                source_ref: Some(r.get::<i64, _>("id").to_string()),
                repo: r.get("repo"),
            }
        })
        .collect())
}

/// `<repo>/docs/memory.md`의 미해결 항목. 파일이 없으면 조용히 빈 목록 (설계 0021 §12).
pub fn memory(repo: &str) -> Vec<Suggestion> {
    let path = Path::new(repo).join("docs").join("memory.md");
    match std::fs::read_to_string(path) {
        Ok(text) => parse_memory_ledger(&text),
        Err(_) => Vec::new(),
    }
}

/// 원장 파서 — `### #<번호> · <날짜> · <제목>` 헤더와 그 아래 `- **status**: <값>`만 본다.
/// `partial`/`pending`만 제안 대상이다. 형식이 바뀌면 빈 목록을 돌려주므로 호출자는
/// "제안 없음"과 "파싱 실패"를 구분하지 못한다 — 그래서 골든 테스트로 회귀를 잡는다.
pub fn parse_memory_ledger(text: &str) -> Vec<Suggestion> {
    let mut out = Vec::new();
    let mut current: Option<(String, String)> = None; // (번호, 제목)
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("### #") {
            let mut parts = rest.split('·');
            let number = parts.next().map(str::trim).unwrap_or_default().to_string();
            let _date = parts.next();
            let title = parts.next().map(str::trim).unwrap_or_default().to_string();
            current = (!number.is_empty() && !title.is_empty()).then_some((number, title));
        } else if let Some(rest) = trimmed.strip_prefix("- **status**:") {
            let status = rest.trim();
            if let Some((number, title)) = current.take() {
                if status == "partial" || status == "pending" {
                    out.push(Suggestion {
                        title: format!("원장 #{number} 마저: {title}"),
                        source: "memory".into(),
                        source_ref: Some(number),
                        repo: None,
                    });
                }
            }
        }
    }
    out
}

/// 이미 그 날 담은 (source, source_ref) 조합을 제외한다.
pub async fn exclude_taken(
    pool: &SqlitePool,
    day: &str,
    candidates: Vec<Suggestion>,
) -> Result<Vec<Suggestion>, String> {
    // 백로그도 함께 본다. 거기 있다는 것은 "이미 내 목록에 있다"는 뜻이고, 제안의 정의는
    // *아직 목록에 없는* 후보다. 빼지 않으면 오늘에서 백로그로 민 이슈가 곧바로 후보로
    // 되돌아오고, 담는 순간 같은 일이 두 곳에 존재한다 (플랜 0054 Sanity Check).
    let rows = sqlx::query(
        "SELECT source, source_ref FROM day_items \
         WHERE (day = ? OR day = 'backlog') AND source_ref IS NOT NULL",
    )
    .bind(day)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    let taken: std::collections::HashSet<(String, String)> = rows
        .iter()
        .map(|r| {
            (
                r.get::<String, _>("source"),
                r.get::<String, _>("source_ref"),
            )
        })
        .collect();
    Ok(candidates
        .into_iter()
        .filter(|s| match &s.source_ref {
            Some(r) => !taken.contains(&(s.source.clone(), r.clone())),
            None => true,
        })
        .collect())
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    if line.chars().count() > 60 {
        format!("{}…", line.chars().take(60).collect::<String>())
    } else {
        line.to_string()
    }
}
