use sha2::{Digest, Sha256};
use sqlx::SqlitePool;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{status, ReviewAnnotation};

const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS review_annotations (
  id         TEXT PRIMARY KEY,
  task_id    INTEGER NOT NULL,
  hunk_id    TEXT NOT NULL,
  path       TEXT NOT NULL,
  line       INTEGER NOT NULL,
  side       TEXT NOT NULL,
  body_md    TEXT NOT NULL,
  status     TEXT NOT NULL DEFAULT 'draft' CHECK(status IN ('draft','sent','resolved')),
  created_at INTEGER NOT NULL
);
"#;

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(MIGRATION).execute(pool).await?;
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_review_annotations_task ON review_annotations(task_id)",
    )
    .execute(pool)
    .await?;
    Ok(())
}

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(super) fn generate_id(task_id: i64, path: &str, line: i64, created_at: i64) -> String {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let source = format!("{task_id}|{path}|{line}|{created_at}|{sequence}");
    let digest = Sha256::digest(source.as_bytes());
    let hex: String = digest
        .iter()
        .take(12)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("ann-{hex}")
}

#[allow(clippy::too_many_arguments)]
pub async fn create_draft(
    pool: &SqlitePool,
    task_id: i64,
    hunk_id: &str,
    path: &str,
    line: i64,
    side: &str,
    body_md: &str,
    created_at: i64,
) -> anyhow::Result<ReviewAnnotation> {
    let id = generate_id(task_id, path, line, created_at);
    sqlx::query(
        "INSERT INTO review_annotations (id, task_id, hunk_id, path, line, side, body_md, status, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, 'draft', ?)",
    )
    .bind(&id)
    .bind(task_id)
    .bind(hunk_id)
    .bind(path)
    .bind(line)
    .bind(side)
    .bind(body_md)
    .bind(created_at)
    .execute(pool)
    .await?;
    Ok(ReviewAnnotation {
        id,
        task_id,
        hunk_id: hunk_id.to_string(),
        path: path.to_string(),
        line,
        side: side.to_string(),
        body_md: body_md.to_string(),
        status: status::DRAFT.to_string(),
        created_at,
    })
}

pub async fn update_draft_body(pool: &SqlitePool, id: &str, body_md: &str) -> anyhow::Result<bool> {
    let result =
        sqlx::query("UPDATE review_annotations SET body_md = ? WHERE id = ? AND status = 'draft'")
            .bind(body_md)
            .bind(id)
            .execute(pool)
            .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn list_by_task(
    pool: &SqlitePool,
    task_id: i64,
) -> anyhow::Result<Vec<ReviewAnnotation>> {
    Ok(sqlx::query_as::<_, ReviewAnnotation>(
        "SELECT id, task_id, hunk_id, path, line, side, body_md, status, created_at \
         FROM review_annotations WHERE task_id = ? ORDER BY created_at ASC",
    )
    .bind(task_id)
    .fetch_all(pool)
    .await?)
}

pub async fn list_by_ids(
    pool: &SqlitePool,
    task_id: i64,
    ids: &[String],
) -> anyhow::Result<Vec<ReviewAnnotation>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, task_id, hunk_id, path, line, side, body_md, status, created_at \
         FROM review_annotations WHERE task_id = ? AND id IN ({placeholders}) ORDER BY created_at ASC",
    );
    let mut query = sqlx::query_as::<_, ReviewAnnotation>(&sql).bind(task_id);
    for id in ids {
        query = query.bind(id);
    }
    Ok(query.fetch_all(pool).await?)
}

pub async fn mark_sent(pool: &SqlitePool, task_id: i64, ids: &[String]) -> anyhow::Result<u64> {
    set_status_from(pool, task_id, ids, status::DRAFT, status::SENT).await
}

pub async fn mark_draft(pool: &SqlitePool, task_id: i64, ids: &[String]) -> anyhow::Result<u64> {
    set_status_from(pool, task_id, ids, status::SENT, status::DRAFT).await
}

async fn set_status_from(
    pool: &SqlitePool,
    task_id: i64,
    ids: &[String],
    from: &str,
    to: &str,
) -> anyhow::Result<u64> {
    if ids.is_empty() {
        return Ok(0);
    }
    let mut transaction = pool.begin().await?;
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "UPDATE review_annotations SET status = ? \
         WHERE task_id = ? AND status = ? AND id IN ({placeholders})",
    );
    let mut query = sqlx::query(&sql).bind(to).bind(task_id).bind(from);
    for id in ids {
        query = query.bind(id);
    }
    let result = query.execute(&mut *transaction).await?;
    transaction.commit().await?;
    Ok(result.rows_affected())
}
