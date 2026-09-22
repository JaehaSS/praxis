//! Atomic finalization of projection receipts and legacy usage observations.

use sqlx::{Row, SqlitePool};

use super::knowledge_status;
use super::receipt::{EvidenceReceipt, JournalRow, MemoryReceipt};

async fn verify_memory_receipt(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    receipt: &MemoryReceipt,
    now: i64,
) -> anyhow::Result<()> {
    let row = sqlx::query(
        "SELECT m.status, m.current_version, m.application_policy, v.content, v.knowledge_type \
         FROM memories m JOIN memory_versions v \
           ON v.memory_id = m.id AND v.version = m.current_version \
         WHERE m.id = ?",
    )
    .bind(receipt.memory_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| anyhow::anyhow!("memory disappeared during projection"))?;
    let status: String = row.try_get("status")?;
    let version: i64 = row.try_get("current_version")?;
    let content: String = row.try_get("content")?;
    let knowledge_type: String = row.try_get("knowledge_type")?;
    // 정책도 비교한다 — 지정이 바뀌면 이 작업이 무엇 위에서 돌았는지가 달라진다.
    let application_policy: String = row.try_get("application_policy")?;
    if status != knowledge_status::VERIFIED
        || version != receipt.version
        || content != receipt.content
        || knowledge_type != receipt.knowledge_type
        || application_policy != receipt.application_policy
    {
        anyhow::bail!("memory changed during projection");
    }
    verify_evidence_receipt(tx, receipt, now).await
}

async fn verify_evidence_receipt(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    receipt: &MemoryReceipt,
    now: i64,
) -> anyhow::Result<()> {
    let rows = sqlx::query(
        "SELECT id, kind, locator_json, status, snapshot_hash, observed_at, expires_at \
         FROM memory_evidence WHERE memory_id = ? AND version = ? ORDER BY id",
    )
    .bind(receipt.memory_id)
    .bind(receipt.version)
    .fetch_all(&mut **tx)
    .await?;
    let current = rows
        .iter()
        .map(|row| {
            Ok(EvidenceReceipt {
                id: row.try_get("id")?,
                kind: row.try_get("kind")?,
                locator_json: row.try_get("locator_json")?,
                status: row.try_get("status")?,
                snapshot_hash: row.try_get("snapshot_hash")?,
                observed_at: row.try_get("observed_at")?,
                expires_at: row.try_get("expires_at")?,
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    if current != receipt.evidence
        || current.is_empty()
        || !current.iter().all(|item| item.trusted_at(now))
    {
        anyhow::bail!("memory evidence changed during projection");
    }
    Ok(())
}

async fn insert_receipt(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    journal: &JournalRow,
    receipt: &MemoryReceipt,
    source_check_ids_json: &str,
    now: i64,
) -> anyhow::Result<()> {
    let evidence_json = serde_json::to_string(&receipt.evidence)?;
    let injection_id = sqlx::query(
        "INSERT INTO memory_injections \
         (memory_id, version, task_id, target_hash, injected_at, projection_id, \
          evidence_snapshot_json, source_check_ids_json, target_paths_json, renderer_version) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(receipt.memory_id)
    .bind(receipt.version)
    .bind(journal.task_id)
    .bind(&journal.target_hash)
    .bind(now)
    .bind(journal.id)
    .bind(evidence_json)
    .bind(source_check_ids_json)
    .bind(&journal.target_paths_json)
    .bind(journal.renderer_version)
    .execute(&mut **tx)
    .await?
    .last_insert_rowid();
    sqlx::query(
        "INSERT INTO memory_usages (memory_id, task_id, injected_at, injection_id) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(receipt.memory_id)
    .bind(journal.task_id)
    .bind(now)
    .bind(injection_id)
    .execute(&mut **tx)
    .await?;
    sqlx::query("UPDATE memories SET usage_count = usage_count + 1, last_used = ? WHERE id = ?")
        .bind(now)
        .bind(receipt.memory_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(super) async fn finalize_projection(
    pool: &SqlitePool,
    journal: &JournalRow,
    receipts: &[MemoryReceipt],
    reports: &[crate::evidence::RevalidationReport],
    now: i64,
) -> anyhow::Result<()> {
    if reports.len() != receipts.len() {
        anyhow::bail!("projection checks do not cover every memory exactly once");
    }
    let source_check_ids = reports
        .iter()
        .flat_map(|report| report.check_ids.iter().copied())
        .collect::<Vec<_>>();
    let source_check_ids_json = serde_json::to_string(&source_check_ids)?;
    let mut tx = pool.begin().await?;
    let task_state: Option<(String,)> = sqlx::query_as("SELECT state FROM tasks WHERE id = ?")
        .bind(journal.task_id)
        .fetch_optional(&mut *tx)
        .await?;
    if task_state.as_ref().map(|row| row.0.as_str()) != Some(crate::db::state::CREATED) {
        anyhow::bail!("task left Created before memory projection finalized");
    }
    for receipt in receipts {
        verify_memory_receipt(&mut tx, receipt, now).await?;
        let report = reports
            .iter()
            .filter(|report| report.memory_id == receipt.memory_id)
            .collect::<Vec<_>>();
        if report.len() != 1 || report[0].version != receipt.version {
            anyhow::bail!("projection check identity does not match its memory receipt");
        }
        let check_ids_json = serde_json::to_string(&report[0].check_ids)?;
        insert_receipt(&mut tx, journal, receipt, &check_ids_json, now).await?;
    }
    if !receipts.is_empty() {
        crate::followup_observation::insert_observation_start(&mut tx, journal.task_id, now)
            .await?;
    }
    let changed = sqlx::query(
        "UPDATE memory_projection_journal \
         SET state = 'applied', source_check_ids_json = ?, updated_at = ? \
         WHERE id = ? AND state = 'prepared'",
    )
    .bind(&source_check_ids_json)
    .bind(now)
    .bind(journal.id)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if changed != 1 {
        anyhow::bail!("projection journal was finalized concurrently");
    }
    tx.commit().await?;
    Ok(())
}

pub(super) async fn verify_applied_projection(
    pool: &SqlitePool,
    journal: &JournalRow,
    now: i64,
) -> anyhow::Result<()> {
    let receipts: Vec<MemoryReceipt> = serde_json::from_str(&journal.ordered_memories_json)?;
    let mut tx = pool.begin().await?;
    for receipt in &receipts {
        verify_memory_receipt(&mut tx, receipt, now).await?;
    }
    tx.rollback().await?;
    Ok(())
}
