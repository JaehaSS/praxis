//! Tauri-independent memory management used by Desktop and Runner transports.

use std::collections::HashMap;

use serde::Serialize;
use sqlx::SqlitePool;

use super::{evidence_status, Memory};

const MAX_MEMORY_LEN: usize = 32_768;

#[derive(Serialize)]
pub struct MemoryListItem {
    #[serde(flatten)]
    pub memory: Memory,
    pub dormant: bool,
    pub evidence_count: i64,
    pub blocking_evidence_count: i64,
}

pub async fn list(pool: &SqlitePool, now: i64) -> anyhow::Result<Vec<MemoryListItem>> {
    let memories = super::list_all(pool).await?;
    let evidence_counts = evidence_counts_by_version(pool, now).await?;
    Ok(memories
        .into_iter()
        .map(|memory| {
            let (evidence_count, blocking_evidence_count) = evidence_counts
                .get(&(memory.id, memory.current_version))
                .copied()
                .unwrap_or_default();
            MemoryListItem {
                dormant: super::is_dormant(&memory, now),
                memory,
                evidence_count,
                blocking_evidence_count,
            }
        })
        .collect())
}

async fn evidence_counts_by_version(
    pool: &SqlitePool,
    now: i64,
) -> anyhow::Result<HashMap<(i64, i64), (i64, i64)>> {
    let rows: Vec<(i64, i64, i64, i64)> = sqlx::query_as(
        r#"SELECT memory_id, version, COUNT(*), COALESCE(SUM(
             CASE WHEN status != ? OR
               (expires_at IS NOT NULL AND expires_at <= ?) THEN 1 ELSE 0 END
           ), 0)
           FROM memory_evidence
           GROUP BY memory_id, version"#,
    )
    .bind(evidence_status::VALID)
    .bind(now)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(memory_id, version, total, blocking)| ((memory_id, version), (total, blocking)))
        .collect())
}

pub async fn create_manual(
    pool: &SqlitePool,
    repo: &str,
    kind: &str,
    content: &str,
    now: i64,
) -> anyhow::Result<i64> {
    let content = checked_content(content)?;
    let repo = repo.trim();
    let canonical_scope = if repo.is_empty() {
        None
    } else {
        Some(
            std::path::Path::new(repo)
                .canonicalize()?
                .to_string_lossy()
                .into_owned(),
        )
    };
    let (tier, scope) = if canonical_scope.is_none() {
        (super::tier::GLOBAL, None)
    } else {
        (super::tier::PROJECT, canonical_scope.as_deref())
    };
    let id = super::create_candidate(
        pool,
        tier,
        scope,
        super::normalize_knowledge_type(kind),
        content,
        Some("manual"),
        now,
    )
    .await?;
    reembed(pool, id, content).await;
    Ok(id)
}

pub async fn update_manual(
    pool: &SqlitePool,
    id: i64,
    content: &str,
    kind: &str,
    now: i64,
) -> anyhow::Result<()> {
    let content = checked_content(content)?;
    super::update_knowledge(
        pool,
        id,
        content,
        super::normalize_knowledge_type(kind),
        now,
    )
    .await?;
    reembed(pool, id, content).await;
    Ok(())
}

pub async fn versions(
    pool: &SqlitePool,
    id: i64,
) -> anyhow::Result<Vec<super::versioning::MemoryVersion>> {
    super::versioning::list(pool, id).await
}

pub async fn restore_version(
    pool: &SqlitePool,
    id: i64,
    source_version: i64,
    expected_current_version: i64,
    expected_status: &str,
    now: i64,
) -> super::restore_error::RestoreResult<i64> {
    let restored = super::version_restore::restore(
        pool,
        id,
        source_version,
        expected_current_version,
        expected_status,
        now,
    )
    .await?;
    reembed_version(pool, id, restored.version, &restored.content).await;
    Ok(restored.version)
}

pub async fn preview(
    pool: &SqlitePool,
    repo: &str,
    instruction: &str,
    now: i64,
) -> anyhow::Result<Vec<Memory>> {
    crate::evidence::revalidate_scope(pool, repo.trim(), now).await?;
    let embedding = crate::embed::embed(instruction).ok();
    // 실제 projection과 **같은 selector**를 쓴다 — 갈라지면 UI가 보여준 것과
    // 에이전트가 받는 것이 달라진다.
    let selection = super::application_policy::select_for_projection(
        pool,
        repo.trim(),
        instruction,
        embedding.as_deref(),
        super::INJECTION_LIMIT,
        now,
    )
    .await?;
    Ok(selection.ordered())
}

fn checked_content(content: &str) -> anyhow::Result<&str> {
    let content = content.trim();
    if content.len() > MAX_MEMORY_LEN {
        anyhow::bail!("메모리 내용이 너무 깁니다 (최대 {MAX_MEMORY_LEN}자)");
    }
    Ok(content)
}

async fn reembed(pool: &SqlitePool, id: i64, content: &str) {
    match crate::embed::embed(content) {
        Ok(embedding) => {
            let _ = super::set_embedding(pool, id, &embedding).await;
        }
        Err(_) => {
            let _ = super::clear_embedding(pool, id).await;
        }
    }
}

async fn reembed_version(pool: &SqlitePool, id: i64, version: i64, content: &str) {
    match crate::embed::embed(content) {
        Ok(embedding) => {
            let _ = super::set_embedding_for_version(pool, id, version, &embedding).await;
        }
        Err(_) => {
            let _ = super::clear_embedding_for_version(pool, id, version).await;
        }
    }
}
