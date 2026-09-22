use sqlx::{Row, SqlitePool};

use super::files::read_revision;
use super::scope::ScopeRequest;

const MAX_TEXT_BYTES: usize = 2 * 1024 * 1024;
const SEARCH_BATCH: i64 = 100;
const MAX_SEARCH_CANDIDATES: i64 = 10_000;

#[derive(Debug, Clone)]
pub struct VaultSearchHit {
    pub document_id: String,
    pub revision_id: String,
    pub title: String,
    pub snippet: String,
}

#[derive(Debug, Clone)]
pub struct VaultSearchPage {
    pub hits: Vec<VaultSearchHit>,
    pub has_more: bool,
}

pub async fn index_revision(pool: &SqlitePool, revision_id: &str) -> anyhow::Result<()> {
    let row = sqlx::query("SELECT r.rowid AS revision_rowid, d.title, r.relative_path, r.size FROM vault_revisions r JOIN vault_documents d ON d.id = r.document_id WHERE r.id = ?")
        .bind(revision_id).fetch_one(pool).await?;
    let rowid: i64 = row.try_get("revision_rowid")?;
    let title: String = row.try_get("title")?;
    let path: String = row.try_get("relative_path")?;
    let size: i64 = row.try_get("size")?;
    let content = if is_text_path(&path) && size <= MAX_TEXT_BYTES as i64 {
        text_content(&path, &read_revision(pool, revision_id).await?).unwrap_or_default()
    } else {
        String::new()
    };
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM vault_fts WHERE rowid = ?")
        .bind(rowid)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO vault_fts (rowid, revision_id, title, content) VALUES (?, ?, ?, ?)")
        .bind(rowid)
        .bind(revision_id)
        .bind(title)
        .bind(content)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn migrate_projection_layout(pool: &SqlitePool) -> anyhow::Result<bool> {
    let current: Option<i64> =
        sqlx::query_scalar("SELECT version FROM vault_index_layout WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    if current == Some(1) {
        return Ok(false);
    }
    let mut tx = pool.begin().await?;
    let had_projection: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM vault_fts")
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM vault_fts")
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO vault_index_layout (id, version, rebuild_needed) VALUES (1, 1, ?) ON CONFLICT(id) DO UPDATE SET version = excluded.version, rebuild_needed = excluded.rebuild_needed")
        .bind(i64::from(had_projection > 0))
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(had_projection > 0)
}

pub(crate) async fn mark_rebuild_complete(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query("UPDATE vault_index_layout SET rebuild_needed = 0 WHERE id = 1")
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn rebuild_needed(pool: &SqlitePool) -> anyhow::Result<bool> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT rebuild_needed FROM vault_index_layout WHERE id = 1")
            .fetch_optional(pool)
            .await?
            .unwrap_or(0)
            != 0,
    )
}

pub async fn search(
    pool: &SqlitePool,
    query: &str,
    request: &ScopeRequest,
    limit: i64,
) -> anyhow::Result<Vec<VaultSearchHit>> {
    let trimmed = query.trim();
    if trimmed.is_empty() || limit <= 0 || matches!(request.scope, super::Scope::PrivateData) {
        return Ok(Vec::new());
    }
    let target = limit.min(100);
    let matchable = crate::knowledge::search::fts_query(trimmed);
    let mut offset = 0;
    let mut hits = Vec::new();
    while hits.len() < target as usize && offset < MAX_SEARCH_CANDIDATES {
        let rows = search_rows(pool, trimmed, &matchable, SEARCH_BATCH, offset).await?;
        if rows.is_empty() {
            break;
        }
        offset += rows.len() as i64;
        for row in rows {
            let revision_id: String = row.try_get("revision_id")?;
            if !scope_for_revision(pool, &revision_id, request).await? {
                continue;
            }
            hits.push(VaultSearchHit {
                document_id: row.try_get("document_id")?,
                revision_id,
                title: row.try_get("title")?,
                snippet: snippet(&row.try_get::<String, _>("content")?),
            });
            if hits.len() == target as usize {
                break;
            }
        }
    }
    Ok(hits)
}

/// Human browsing includes private-data.  Automatic task retrieval calls
/// `search` with a non-private project/common request instead.
pub async fn search_browse(
    pool: &SqlitePool,
    query: &str,
    offset: i64,
) -> anyhow::Result<VaultSearchPage> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(VaultSearchPage {
            hits: Vec::new(),
            has_more: false,
        });
    }
    let matchable = crate::knowledge::search::fts_query(trimmed);
    let rows = search_rows(pool, trimmed, &matchable, 101, offset.max(0)).await?;
    let has_more = rows.len() > 100;
    let hits = rows
        .into_iter()
        .take(100)
        .map(|row| {
            Ok(VaultSearchHit {
                document_id: row.try_get("document_id")?,
                revision_id: row.try_get("revision_id")?,
                title: row.try_get("title")?,
                snippet: snippet(&row.try_get::<String, _>("content")?),
            })
        })
        .collect::<anyhow::Result<_>>()?;
    Ok(VaultSearchPage { hits, has_more })
}

fn text_content(path: &str, bytes: &[u8]) -> Option<String> {
    let text = is_text_path(path)
        .then(|| std::str::from_utf8(bytes).ok())
        .flatten()?;
    (text.len() <= MAX_TEXT_BYTES).then(|| text.to_owned())
}

fn is_text_path(path: &str) -> bool {
    matches!(
        path.rsplit('.').next(),
        Some("md" | "MD" | "txt" | "TXT" | "csv" | "CSV")
    )
}

async fn scope_for_revision(
    pool: &SqlitePool,
    revision_id: &str,
    request: &ScopeRequest,
) -> anyhow::Result<bool> {
    Ok(super::scope_for_sources(pool, &[revision_id.into()])
        .await
        .ok()
        .flatten()
        .is_some_and(|scope| super::scope::scope_allows(&scope, request)))
}

async fn search_rows(
    pool: &SqlitePool,
    query: &str,
    matchable: &str,
    limit: i64,
    offset: i64,
) -> anyhow::Result<Vec<sqlx::sqlite::SqliteRow>> {
    let base = "SELECT d.id AS document_id, r.id AS revision_id, d.title, f.content FROM vault_fts f JOIN vault_revisions r ON r.rowid = f.rowid JOIN vault_documents d ON d.id = r.document_id AND d.current_revision = r.id JOIN vaults v ON v.id = d.vault_id WHERE d.state = 'active' AND v.enabled = 1";
    if matchable.is_empty() {
        let pattern = format!("%{}%", escape_like(query));
        return Ok(sqlx::query(&format!("{base} AND (d.title LIKE ? ESCAPE '\\' OR f.content LIKE ? ESCAPE '\\') ORDER BY d.id LIMIT ? OFFSET ?"))
            .bind(&pattern)
            .bind(&pattern)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await?);
    }
    Ok(sqlx::query(&format!(
        "{base} AND vault_fts MATCH ? ORDER BY rank, f.rowid LIMIT ? OFFSET ?"
    ))
    .bind(matchable)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await?)
}

fn escape_like(query: &str) -> String {
    query
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn snippet(content: &str) -> String {
    content.replace('\n', " ").chars().take(200).collect()
}
