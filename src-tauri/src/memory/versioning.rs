use serde::Serialize;
use sqlx::{FromRow, SqlitePool};

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct MemoryVersion {
    pub memory_id: i64,
    pub version: i64,
    pub content: String,
    pub knowledge_type: String,
    pub scope_snapshot: Option<String>,
    pub created_at: i64,
    pub editor_kind: String,
    pub evidence_count: i64,
}

pub(super) async fn list(pool: &SqlitePool, memory_id: i64) -> anyhow::Result<Vec<MemoryVersion>> {
    let rows = sqlx::query_as(
        r#"SELECT v.memory_id, v.version, v.content, v.knowledge_type, v.scope_snapshot,
                  v.created_at, v.editor_kind,
                  (SELECT COUNT(*) FROM memory_evidence e
                   WHERE e.memory_id = v.memory_id AND e.version = v.version) AS evidence_count
           FROM memory_versions v
           WHERE v.memory_id = ?
           ORDER BY v.version DESC"#,
    )
    .bind(memory_id)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        anyhow::bail!("메모리를 찾을 수 없습니다");
    }
    Ok(rows)
}
