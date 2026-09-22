//! Honest task projection receipt and current-scope memory readiness counts.

use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, Serialize)]
pub struct MemoryContextCounts {
    pub scope_total: i64,
    pub actionable: i64,
    pub verified: i64,
    pub eligible: i64,
}

#[derive(Debug, Serialize)]
pub struct MemoryProjectionSummary {
    pub state: String,
    pub selected_count: i64,
}

pub(super) struct ContextDiagnostics {
    pub memory_counts: MemoryContextCounts,
    pub project_count: i64,
    pub projection: Option<MemoryProjectionSummary>,
}

pub(super) async fn diagnostics(
    pool: &SqlitePool,
    task_id: i64,
    repo: &str,
) -> anyhow::Result<ContextDiagnostics> {
    let (memory_counts, project_count) = current_scope_counts(pool, repo).await?;
    let projection = projection_summary(pool, task_id).await?;
    Ok(ContextDiagnostics {
        memory_counts,
        project_count,
        projection,
    })
}

async fn current_scope_counts(
    pool: &SqlitePool,
    repo: &str,
) -> anyhow::Result<(MemoryContextCounts, i64)> {
    let sql = format!(
        "SELECT COUNT(*),
           COALESCE(SUM(CASE WHEN m.tier = ? AND m.scope_key = ? THEN 1 ELSE 0 END), 0),
           COALESCE(SUM(CASE WHEN m.status IN (?, ?, ?, ?) AND NOT
             (m.usage_count = 0 AND m.created_at < CAST(strftime('%s','now') AS INTEGER) - {} * 86400)
             THEN 1 ELSE 0 END), 0),
           COALESCE(SUM(CASE WHEN m.status = ? THEN 1 ELSE 0 END), 0),
           COALESCE(SUM(CASE WHEN {} THEN 1 ELSE 0 END), 0)
         FROM memories m
         WHERE (m.tier = ? AND m.scope_key = ?) OR m.tier = ?",
        super::DORMANT_DAYS,
        super::active_filter("m")
    );
    let row: (i64, i64, i64, i64, i64) = sqlx::query_as(&sql)
        .bind(super::tier::PROJECT)
        .bind(repo)
        .bind(super::knowledge_status::CANDIDATE)
        .bind(super::knowledge_status::PENDING_REVIEW)
        .bind(super::knowledge_status::STALE)
        .bind(super::knowledge_status::LEGACY_UNVERIFIED)
        .bind(super::knowledge_status::VERIFIED)
        .bind(super::tier::PROJECT)
        .bind(repo)
        .bind(super::tier::GLOBAL)
        .fetch_one(pool)
        .await?;
    Ok((
        MemoryContextCounts {
            scope_total: row.0,
            actionable: row.2,
            verified: row.3,
            eligible: row.4,
        },
        row.1,
    ))
}

async fn projection_summary(
    pool: &SqlitePool,
    task_id: i64,
) -> anyhow::Result<Option<MemoryProjectionSummary>> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT state, ordered_memories_json
         FROM memory_projection_journal WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await?;
    let Some((state, memories_json)) = row else {
        return Ok(None);
    };
    let selected_count =
        serde_json::from_str::<Vec<serde_json::Value>>(&memories_json)?.len() as i64;
    Ok(Some(MemoryProjectionSummary {
        state,
        selected_count,
    }))
}
