use std::path::Path;

use sqlx::SqlitePool;

use super::model::{
    CodeLocationInput, CodeLocator, EvidenceRecord, ExternalDocumentInput, ExternalDocumentLocator,
    LocalDocumentInput, LocalDocumentLocator,
};
use crate::memory::{evidence_kind, evidence_status, tier};

pub async fn add_code_location(
    pool: &SqlitePool,
    memory_id: i64,
    input: CodeLocationInput,
    now: i64,
) -> anyhow::Result<i64> {
    let (version, root) = project_scope(pool, memory_id).await?;
    let snapshot = super::source_snapshot::code(
        Path::new(&root),
        &input.relative_path,
        input.line_start,
        input.line_end,
    )?;
    let locator = CodeLocator {
        schema_version: 1,
        canonical_repository: snapshot.canonical_root.to_string_lossy().into_owned(),
        relative_path: input.relative_path,
        line_start: input.line_start,
        line_end: input.line_end,
        commit_oid: snapshot.commit_oid,
    };
    insert_evidence(
        pool,
        memory_id,
        version,
        evidence_kind::CODE_LOCATION,
        &serde_json::to_string(&locator)?,
        Some(&snapshot.hash),
        now,
        None,
    )
    .await
}

pub async fn add_local_document(
    pool: &SqlitePool,
    memory_id: i64,
    input: LocalDocumentInput,
    now: i64,
) -> anyhow::Result<i64> {
    let (version, root) = project_scope(pool, memory_id).await?;
    let hash = super::source_snapshot::document(Path::new(&root), &input.relative_path)?;
    let locator = LocalDocumentLocator {
        schema_version: 1,
        canonical_repository: Path::new(&root)
            .canonicalize()?
            .to_string_lossy()
            .into_owned(),
        relative_path: input.relative_path,
    };
    insert_evidence(
        pool,
        memory_id,
        version,
        evidence_kind::DOCUMENT,
        &serde_json::to_string(&locator)?,
        Some(&hash),
        now,
        input.expires_at,
    )
    .await
}

pub async fn add_external_document(
    pool: &SqlitePool,
    memory_id: i64,
    input: ExternalDocumentInput,
    now: i64,
) -> anyhow::Result<i64> {
    if input.expires_at <= now {
        anyhow::bail!("external document expiry must be in the future");
    }
    let version = current_version(pool, memory_id).await?;
    let url = super::source_snapshot::external_url(&input.url)?;
    let locator = ExternalDocumentLocator {
        schema_version: 1,
        url,
    };
    insert_evidence(
        pool,
        memory_id,
        version,
        evidence_kind::DOCUMENT,
        &serde_json::to_string(&locator)?,
        None,
        now,
        Some(input.expires_at),
    )
    .await
}

pub async fn list_evidence(
    pool: &SqlitePool,
    memory_id: i64,
) -> anyhow::Result<Vec<EvidenceRecord>> {
    sqlx::query_as(
        "SELECT e.id, e.memory_id, e.version, e.kind, e.locator_json, e.snapshot_hash, e.status, \
                e.observed_at, e.checked_at, e.expires_at FROM memory_evidence e \
         JOIN memories m ON m.id = e.memory_id AND m.current_version = e.version \
         WHERE e.memory_id = ? ORDER BY e.id",
    )
    .bind(memory_id)
    .fetch_all(pool)
    .await
    .map_err(Into::into)
}

async fn project_scope(pool: &SqlitePool, memory_id: i64) -> anyhow::Result<(i64, String)> {
    let row: Option<(String, Option<String>, i64)> =
        sqlx::query_as("SELECT tier, scope_key, current_version FROM memories WHERE id = ?")
            .bind(memory_id)
            .fetch_optional(pool)
            .await?;
    let Some((tier_name, Some(scope), version)) = row else {
        anyhow::bail!("repo-bound evidence requires a project memory scope");
    };
    if tier_name != tier::PROJECT || scope.trim().is_empty() {
        anyhow::bail!("repo-bound evidence requires a project memory scope");
    }
    Ok((version, scope))
}

async fn current_version(pool: &SqlitePool, memory_id: i64) -> anyhow::Result<i64> {
    sqlx::query_scalar("SELECT current_version FROM memories WHERE id = ?")
        .bind(memory_id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| anyhow::anyhow!("memory not found"))
}

#[allow(clippy::too_many_arguments)]
async fn insert_evidence(
    pool: &SqlitePool,
    memory_id: i64,
    version: i64,
    kind: &str,
    locator_json: &str,
    snapshot_hash: Option<&str>,
    now: i64,
    expires_at: Option<i64>,
) -> anyhow::Result<i64> {
    let mut tx = pool.begin().await?;
    let current: Option<i64> =
        sqlx::query_scalar("SELECT current_version FROM memories WHERE id = ?")
            .bind(memory_id)
            .fetch_optional(&mut *tx)
            .await?;
    if current != Some(version) {
        anyhow::bail!("memory version changed during evidence observation");
    }
    let id = sqlx::query(
        "INSERT INTO memory_evidence \
         (memory_id, version, kind, locator_json, snapshot_hash, status, observed_at, checked_at, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(memory_id)
    .bind(version)
    .bind(kind)
    .bind(locator_json)
    .bind(snapshot_hash)
    .bind(evidence_status::VALID)
    .bind(now)
    .bind(now)
    .bind(expires_at)
    .execute(&mut *tx)
    .await?
    .last_insert_rowid();
    sqlx::query(
        "INSERT INTO memory_events \
         (memory_id, version, action, actor_kind, payload_json, created_at) \
         VALUES (?, ?, 'evidence_added', 'system', ?, ?)",
    )
    .bind(memory_id)
    .bind(version)
    .bind(locator_json)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(id)
}
