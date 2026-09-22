//! 하루 마감 — 집계 + `docs/memory.md` 원장 초안.
//!
//! **원장에 자동으로 쓰지 않는다** (설계 0021 DR-6). 원장은 `subject`/`status` 필수이고
//! 위반 시 `.githooks/pre-commit`이 커밋을 막는다. 미검토 자동 삽입은 잘못된 원장을 남긴다.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DayClosing {
    pub day: String,
    pub closed_at: i64,
    pub done: i64,
    pub open: i64,
    pub dropped: i64,
    pub draft: String,
}

pub async fn close_day(pool: &SqlitePool, day: &str, now: i64) -> Result<DayClosing, String> {
    let rows = sqlx::query("SELECT title, status FROM day_items WHERE day = ? ORDER BY position")
        .bind(day)
        .fetch_all(pool)
        .await
        .map_err(|e| e.to_string())?;
    let mut done = Vec::new();
    let mut open = Vec::new();
    let mut dropped = Vec::new();
    for row in &rows {
        let title: String = row.get("title");
        match row.get::<String, _>("status").as_str() {
            "done" => done.push(title),
            "dropped" => dropped.push(title),
            _ => open.push(title),
        }
    }
    // 원장 번호는 사용자가 채운다 — 여기서 추측하면 중복 번호를 만든다
    // (브랜치 병행 시 union 머지가 번호 중복을 못 막는다, CLAUDE.md "문서 머지").
    let draft = render_draft(day, 0, &done, &open, &dropped);
    let closing = DayClosing {
        day: day.to_string(),
        closed_at: now,
        done: done.len() as i64,
        open: open.len() as i64,
        dropped: dropped.len() as i64,
        draft,
    };
    sqlx::query(
        "INSERT INTO day_closings (day, closed_at, done, open, dropped, draft) \
         VALUES (?, ?, ?, ?, ?, ?) \
         ON CONFLICT(day) DO UPDATE SET closed_at = excluded.closed_at, done = excluded.done, \
           open = excluded.open, dropped = excluded.dropped, draft = excluded.draft",
    )
    .bind(&closing.day)
    .bind(closing.closed_at)
    .bind(closing.done)
    .bind(closing.open)
    .bind(closing.dropped)
    .bind(&closing.draft)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(closing)
}

/// 마감 기록 조회 — 이미 마감한 날인지 확인용.
pub async fn get_closing(pool: &SqlitePool, day: &str) -> Result<Option<DayClosing>, String> {
    let row = sqlx::query("SELECT day, closed_at, done, open, dropped, draft FROM day_closings WHERE day = ?")
        .bind(day)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.map(|r| DayClosing {
        day: r.get("day"),
        closed_at: r.get("closed_at"),
        done: r.get("done"),
        open: r.get("open"),
        dropped: r.get("dropped"),
        draft: r.get("draft"),
    }))
}

/// 원장 초안. `number`가 0이면 자리표시자(`#N`)를 남긴다 — 사용자가 최대 번호 + 1로 채운다.
pub fn render_draft(
    day: &str,
    number: i64,
    done: &[String],
    open: &[String],
    dropped: &[String],
) -> String {
    let label = if number > 0 {
        format!("#{number}")
    } else {
        "#N".to_string()
    };
    let mut out = format!("### {label} · {day} · <제목> (Small)\n\n");
    out.push_str("- **subject**: <docs/subjects.yml의 키>\n");
    out.push_str("- **status**: done\n");
    for (heading, items) in [("한 것", done), ("못 한 것", open), ("접은 것", dropped)] {
        if items.is_empty() {
            continue;
        }
        out.push_str(&format!("- **{heading}**:\n"));
        for item in items {
            out.push_str(&format!("  - {item}\n"));
        }
    }
    out
}
