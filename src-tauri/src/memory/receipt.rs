//! Durable projection preparation records and immutable evidence snapshots.

use std::path::Path;

use serde::{Deserialize, Serialize};
use sqlx::{FromRow, Row, SqlitePool};

use super::{evidence_status, Memory};
use crate::projector::TargetPreimage;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct EvidenceReceipt {
    pub id: i64,
    pub kind: String,
    pub locator_json: String,
    pub status: String,
    pub snapshot_hash: Option<String>,
    pub observed_at: i64,
    pub expires_at: Option<i64>,
}

impl EvidenceReceipt {
    pub(super) fn trusted_at(&self, now: i64) -> bool {
        self.status == evidence_status::VALID
            && self.expires_at.is_none_or(|expires_at| expires_at > now)
    }
}

/// 정책 도입 전에 쓰인 receipt에는 이 필드가 없다. 기본값이 없으면 기존 작업의
/// 시작 검증이 전부 역직렬화에서 깨진다.
fn default_application_policy() -> String {
    super::application_policy::policy::RELEVANCE.to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct MemoryReceipt {
    pub memory_id: i64,
    pub version: i64,
    pub content: String,
    pub knowledge_type: String,
    /// 이 작업이 선택될 당시의 적용 정책 — 과거 선택을 현재 상태로 추정하지 않는다.
    #[serde(default = "default_application_policy")]
    pub application_policy: String,
    pub evidence: Vec<EvidenceReceipt>,
}

#[derive(Clone, Debug, FromRow)]
pub(super) struct JournalRow {
    pub id: i64,
    pub task_id: i64,
    pub state: String,
    pub worktree_path: String,
    pub target_paths_json: String,
    pub target_hash: String,
    pub renderer_version: i64,
    pub ordered_memories_json: String,
    pub preimages_json: Option<String>,
}

pub(super) struct PreparedJournal {
    pub row: JournalRow,
    pub is_new: bool,
}

async fn evidence_for(
    pool: &SqlitePool,
    memory_id: i64,
    version: i64,
) -> anyhow::Result<Vec<EvidenceReceipt>> {
    let rows = sqlx::query(
        "SELECT id, kind, locator_json, status, snapshot_hash, observed_at, expires_at \
         FROM memory_evidence WHERE memory_id = ? AND version = ? ORDER BY id",
    )
    .bind(memory_id)
    .bind(version)
    .fetch_all(pool)
    .await?;
    rows.iter()
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
        .collect()
}

pub(super) async fn snapshot_memories(
    pool: &SqlitePool,
    memories: &[Memory],
    now: i64,
) -> anyhow::Result<Vec<MemoryReceipt>> {
    let mut receipts = Vec::with_capacity(memories.len());
    for memory in memories {
        super::validate_knowledge_content(&memory.content)?;
        let evidence = evidence_for(pool, memory.id, memory.current_version).await?;
        if evidence.is_empty() || !evidence.iter().all(|item| item.trusted_at(now)) {
            anyhow::bail!("memory {} lost trusted evidence", memory.id);
        }
        receipts.push(MemoryReceipt {
            memory_id: memory.id,
            version: memory.current_version,
            content: memory.content.clone(),
            knowledge_type: memory.knowledge_type.clone(),
            application_policy: memory.application_policy.clone(),
            evidence,
        });
    }
    Ok(receipts)
}

pub(super) async fn load_journal(
    pool: &SqlitePool,
    task_id: i64,
) -> anyhow::Result<Option<JournalRow>> {
    sqlx::query_as(
        "SELECT id, task_id, state, worktree_path, target_paths_json, target_hash, \
                renderer_version, ordered_memories_json, preimages_json \
         FROM memory_projection_journal WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn prepare_journal(
    pool: &SqlitePool,
    task_id: i64,
    worktree_path: &str,
    target_paths: &[String],
    target_hash: &str,
    renderer_version: i64,
    memories: &[MemoryReceipt],
    preimages: &[TargetPreimage],
    now: i64,
) -> anyhow::Result<PreparedJournal> {
    let targets_json = serde_json::to_string(target_paths)?;
    let memories_json = serde_json::to_string(memories)?;
    let preimages_json = serde_json::to_string(preimages)?;
    let result = sqlx::query(
        "INSERT OR IGNORE INTO memory_projection_journal \
         (task_id, state, worktree_path, target_paths_json, target_hash, renderer_version, \
          ordered_memories_json, preimages_json, created_at, updated_at) \
         VALUES (?, 'prepared', ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(task_id)
    .bind(worktree_path)
    .bind(&targets_json)
    .bind(target_hash)
    .bind(renderer_version)
    .bind(&memories_json)
    .bind(&preimages_json)
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    let row = load_journal(pool, task_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("projection journal was not persisted"))?;
    Ok(PreparedJournal {
        row,
        is_new: result.rows_affected() == 1,
    })
}

/// 같은 폴더에서 **지금 살아 있는 다른 투영들**의 블록 해시.
///
/// 워크트리를 쓰지 않으면 한 프로젝트 루트의 `CLAUDE.md`를 작업 여럿이 공유한다. 거기 떠 있는
/// 블록이 지난 세션의 잔재인지 옆 작업이 쓰는 중인 것인지는 파일에 적혀 있지 않다 — 적용 중인
/// 저널만 안다. 이 해시 집합이 `planned_updates`에서 그 둘을 가른다.
pub(super) async fn live_projection_hashes(
    pool: &SqlitePool,
    worktree_path: &Path,
    task_id: i64,
) -> anyhow::Result<std::collections::HashSet<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT target_hash FROM memory_projection_journal \
         WHERE state = 'applied' AND worktree_path = ? AND task_id <> ?",
    )
    .bind(worktree_path.to_string_lossy().as_ref())
    .bind(task_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(hash,)| hash).collect())
}

#[cfg(test)]
mod tests {
    use super::MemoryReceipt;
    use crate::memory::application_policy::policy;

    #[test]
    fn receipt_without_policy_field_reads_as_relevance() {
        // 정책 도입 전에 저장된 journal JSON — 여기서 깨지면 진행 중이던 모든 작업의
        // 시작 검증이 역직렬화 단계에서 실패한다.
        let json = r#"{"memory_id":1,"version":1,"content":"기존","knowledge_type":"decision","evidence":[]}"#;
        let receipt: MemoryReceipt = serde_json::from_str(json).expect("구 receipt 역직렬화");
        assert_eq!(receipt.application_policy, policy::RELEVANCE);
    }

    #[test]
    fn receipt_roundtrips_its_policy() {
        let json = r#"{"memory_id":1,"version":1,"content":"규칙","knowledge_type":"decision","application_policy":"must_apply","evidence":[]}"#;
        let receipt: MemoryReceipt = serde_json::from_str(json).expect("역직렬화");
        assert_eq!(receipt.application_policy, policy::MUST_APPLY);
        let encoded = serde_json::to_string(&receipt).expect("직렬화");
        assert!(encoded.contains(r#""application_policy":"must_apply""#));
    }
}
