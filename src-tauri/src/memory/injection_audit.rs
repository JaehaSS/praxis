//! Immutable and legacy-compatible task injection read model.

use serde::Serialize;
use sqlx::{Row, SqlitePool};

use super::receipt::{EvidenceReceipt, MemoryReceipt};

#[derive(Debug, Clone, Serialize)]
pub struct InjectedMemory {
    pub memory_id: i64,
    pub version: Option<i64>,
    pub kind: Option<String>,
    pub content: Option<String>,
    pub confidence: Option<f64>,
    pub evidence_count: i64,
    pub evidence_status: Option<String>,
    pub target_hash: Option<String>,
    pub target_paths: Vec<String>,
    pub renderer_version: Option<i64>,
    pub injected_at: i64,
    pub outcome: Option<String>,
    pub exists: bool,
}

fn evidence_status(receipts: &[EvidenceReceipt]) -> Option<String> {
    let mut statuses = receipts
        .iter()
        .map(|receipt| receipt.status.as_str())
        .collect::<Vec<_>>();
    statuses.sort_unstable();
    statuses.dedup();
    (!statuses.is_empty()).then(|| statuses.join(","))
}

async fn immutable_rows(pool: &SqlitePool, task_id: i64) -> anyhow::Result<Vec<InjectedMemory>> {
    let rows = sqlx::query(
        "SELECT i.memory_id, i.version, i.injected_at, i.outcome, i.target_hash, \
                i.target_paths_json, i.renderer_version, i.evidence_snapshot_json, \
                j.ordered_memories_json, m.id AS current_id \
         FROM memory_injections i \
         JOIN memory_projection_journal j ON j.id = i.projection_id \
         LEFT JOIN memories m ON m.id = i.memory_id \
         WHERE i.task_id = ? ORDER BY i.injected_at, i.memory_id",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let evidence: Vec<EvidenceReceipt> =
                serde_json::from_str(row.try_get("evidence_snapshot_json")?)?;
            let memory_id: i64 = row.try_get("memory_id")?;
            let version: i64 = row.try_get("version")?;
            let receipts: Vec<MemoryReceipt> =
                serde_json::from_str(row.try_get("ordered_memories_json")?)?;
            let receipt = receipts
                .iter()
                .find(|receipt| receipt.memory_id == memory_id && receipt.version == version)
                .ok_or_else(|| anyhow::anyhow!("immutable projection receipt is incomplete"))?;
            Ok(InjectedMemory {
                memory_id,
                version: Some(version),
                kind: Some(receipt.knowledge_type.clone()),
                content: Some(receipt.content.clone()),
                confidence: None,
                evidence_count: evidence.len() as i64,
                evidence_status: evidence_status(&evidence),
                target_hash: Some(row.try_get("target_hash")?),
                target_paths: serde_json::from_str(row.try_get("target_paths_json")?)?,
                renderer_version: Some(row.try_get("renderer_version")?),
                injected_at: row.try_get("injected_at")?,
                outcome: row.try_get("outcome")?,
                exists: row.try_get::<Option<i64>, _>("current_id")?.is_some(),
            })
        })
        .collect()
}

async fn legacy_rows(pool: &SqlitePool, task_id: i64) -> anyhow::Result<Vec<InjectedMemory>> {
    let rows = sqlx::query(
        "SELECT u.memory_id, u.injected_at, u.outcome, m.kind, m.content, m.confidence \
         FROM memory_usages u LEFT JOIN memories m ON m.id = u.memory_id \
         WHERE u.task_id = ? AND u.injection_id IS NULL ORDER BY u.injected_at, u.memory_id",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            let kind: Option<String> = row.try_get("kind")?;
            Ok(InjectedMemory {
                memory_id: row.try_get("memory_id")?,
                version: None,
                exists: kind.is_some(),
                kind,
                content: row.try_get("content")?,
                confidence: row.try_get("confidence")?,
                evidence_count: 0,
                evidence_status: None,
                target_hash: None,
                target_paths: Vec::new(),
                renderer_version: None,
                injected_at: row.try_get("injected_at")?,
                outcome: row.try_get("outcome")?,
            })
        })
        .collect()
}

pub async fn injections_for_task(
    pool: &SqlitePool,
    task_id: i64,
) -> anyhow::Result<Vec<InjectedMemory>> {
    let mut rows = immutable_rows(pool, task_id).await?;
    rows.extend(legacy_rows(pool, task_id).await?);
    rows.sort_by_key(|row| (row.injected_at, row.memory_id));
    Ok(rows)
}
