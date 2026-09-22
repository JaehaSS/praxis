use super::knowledge_status;
use super::restore_error::{RestoreFailure, RestoreResult};
use sqlx::{FromRow, Sqlite, SqlitePool, Transaction};
pub(super) struct RestoredVersion {
    pub version: i64,
    pub content: String,
}
#[derive(FromRow)]
struct CurrentSnapshot {
    current_version: i64,
    status: String,
    scope_key: Option<String>,
    application_policy: String,
}
#[derive(FromRow)]
struct SourceSnapshot {
    content: String,
    knowledge_type: String,
}
pub(super) async fn restore(
    pool: &SqlitePool,
    memory_id: i64,
    source_version: i64,
    expected_current_version: i64,
    expected_status: &str,
    now: i64,
) -> RestoreResult<RestoredVersion> {
    let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
    let current = read_current(&mut tx, memory_id).await?;
    ensure_restore_owner(
        &current,
        source_version,
        expected_current_version,
        expected_status,
    )?;
    let source = read_source(&mut tx, memory_id, source_version).await?;
    let next_version = current.current_version + 1;
    append_restored_version(
        &mut tx,
        memory_id,
        source_version,
        next_version,
        &current,
        &source,
        now,
    )
    .await?;
    tx.commit().await?;
    Ok(RestoredVersion {
        version: next_version,
        content: source.content,
    })
}
async fn read_current(
    tx: &mut Transaction<'_, Sqlite>,
    memory_id: i64,
) -> RestoreResult<CurrentSnapshot> {
    sqlx::query_as(
        "SELECT current_version, status, scope_key, application_policy FROM memories WHERE id = ?",
    )
    .bind(memory_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(RestoreFailure::NotFound("메모리를 찾을 수 없습니다"))
}
async fn read_source(
    tx: &mut Transaction<'_, Sqlite>,
    memory_id: i64,
    source_version: i64,
) -> RestoreResult<SourceSnapshot> {
    sqlx::query_as(
        "SELECT content, knowledge_type FROM memory_versions
         WHERE memory_id = ? AND version = ?",
    )
    .bind(memory_id)
    .bind(source_version)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or(RestoreFailure::NotFound(
        "복구할 memory version을 찾을 수 없습니다",
    ))
}
fn ensure_restore_owner(
    current: &CurrentSnapshot,
    source_version: i64,
    expected_current_version: i64,
    expected_status: &str,
) -> RestoreResult<()> {
    if current.current_version != expected_current_version || current.status != expected_status {
        return Err(RestoreFailure::Conflict);
    }
    if source_version == current.current_version && current.status != knowledge_status::ARCHIVED {
        return Err(RestoreFailure::Invalid(
            "현재 version은 복구할 필요가 없습니다",
        ));
    }
    if source_version <= 0 || source_version > current.current_version {
        return Err(RestoreFailure::NotFound(
            "복구할 memory version을 찾을 수 없습니다",
        ));
    }
    Ok(())
}
async fn append_restored_version(
    tx: &mut Transaction<'_, Sqlite>,
    memory_id: i64,
    source_version: i64,
    next_version: i64,
    current: &CurrentSnapshot,
    source: &SourceSnapshot,
    now: i64,
) -> RestoreResult<()> {
    update_current(tx, memory_id, next_version, current, source).await?;
    insert_version(tx, memory_id, next_version, current, source, now).await?;
    // 복원은 새 version을 만든다 — 이전 version에 대한 지정을 물려받지 않는다.
    super::application_policy::reset_on_lifecycle_change(
        tx,
        memory_id,
        &current.application_policy,
        next_version,
        now,
    )
    .await?;
    insert_event(
        tx,
        memory_id,
        source_version,
        current.current_version,
        next_version,
        now,
    )
    .await
}

async fn update_current(
    tx: &mut Transaction<'_, Sqlite>,
    memory_id: i64,
    next_version: i64,
    current: &CurrentSnapshot,
    source: &SourceSnapshot,
) -> RestoreResult<()> {
    let updated = sqlx::query(
        "UPDATE memories
         SET content = ?, kind = ?, knowledge_type = ?, status = ?, current_version = ?, embedding = NULL,
             verified_at = NULL, stale_at = NULL, archived_at = NULL
         WHERE id = ? AND current_version = ? AND status = ?",
    )
    .bind(&source.content)
    .bind(&source.knowledge_type)
    .bind(&source.knowledge_type)
    .bind(knowledge_status::CANDIDATE)
    .bind(next_version)
    .bind(memory_id)
    .bind(current.current_version)
    .bind(&current.status)
    .execute(&mut **tx)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(RestoreFailure::Conflict);
    }
    Ok(())
}

async fn insert_version(
    tx: &mut Transaction<'_, Sqlite>,
    memory_id: i64,
    next_version: i64,
    current: &CurrentSnapshot,
    source: &SourceSnapshot,
    now: i64,
) -> RestoreResult<()> {
    sqlx::query(
        "INSERT INTO memory_versions
         (memory_id, version, content, knowledge_type, scope_snapshot, created_at, editor_kind)
         VALUES (?, ?, ?, ?, ?, ?, 'human_restore')",
    )
    .bind(memory_id)
    .bind(next_version)
    .bind(&source.content)
    .bind(&source.knowledge_type)
    .bind(&current.scope_key)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn insert_event(
    tx: &mut Transaction<'_, Sqlite>,
    memory_id: i64,
    source_version: i64,
    previous_version: i64,
    next_version: i64,
    now: i64,
) -> RestoreResult<()> {
    let payload = serde_json::json!({
        "source_version": source_version,
        "previous_version": previous_version,
    });
    sqlx::query(
        "INSERT INTO memory_events
         (memory_id, version, action, actor_kind, payload_json, created_at)
         VALUES (?, ?, 'version_restored', 'human', ?, ?)",
    )
    .bind(memory_id)
    .bind(next_version)
    .bind(payload.to_string())
    .bind(now)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
