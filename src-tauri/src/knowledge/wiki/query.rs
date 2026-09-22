use sqlx::{Row, SqlitePool};

use super::config::{entries, spaces, verified_root, WikiSpace};
use super::files::{read, relative_path};
use super::SOURCE_ID;

const DOCUMENT_LIMIT: i64 = 10_000;

#[derive(Debug, Clone, serde::Serialize)]
pub struct WikiDocument {
    pub node_id: i64,
    pub space_id: String,
    pub relative_path: String,
    pub title: String,
    pub snippet: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WikiDocumentsResult {
    pub documents: Vec<WikiDocument>,
    pub truncated: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct WikiReadDocument {
    pub node_id: i64,
    pub space_id: String,
    pub relative_path: String,
    pub title: String,
    pub path: String,
    pub body: String,
}

pub async fn documents(
    pool: &SqlitePool,
    space_id: Option<&str>,
    query: &str,
) -> anyhow::Result<WikiDocumentsResult> {
    let spaces = scoped_spaces(spaces(pool).await?, space_id)?;
    if spaces.is_empty() {
        return Ok(WikiDocumentsResult {
            documents: Vec::new(),
            truncated: false,
        });
    }
    let scope = placeholders(spaces.len());
    let rows = if query.trim().is_empty() {
        let sql = format!(
            "SELECT id, space_id, external_id, title, '' AS content FROM knowledge_nodes \
             WHERE source = ? AND space_id IN ({scope}) ORDER BY id LIMIT ?"
        );
        bind_spaces(sqlx::query(&sql).bind(SOURCE_ID), &spaces)
            .bind(DOCUMENT_LIMIT + 1)
            .fetch_all(pool)
            .await?
    } else {
        search_rows(pool, &spaces, query, &scope).await?
    };
    let truncated = rows.len() as i64 > DOCUMENT_LIMIT;
    let documents = rows
        .into_iter()
        .take(DOCUMENT_LIMIT as usize)
        .map(row_to_document)
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(WikiDocumentsResult {
        documents,
        truncated,
    })
}

pub async fn read_document(pool: &SqlitePool, node_id: i64) -> anyhow::Result<WikiReadDocument> {
    let _admission = crate::knowledge::vault::shared_admission(pool).await?;
    if crate::knowledge::vault::ownership::owns_legacy_node(pool, node_id).await? {
        anyhow::bail!("Wiki document is owned by a personal knowledge vault")
    }
    let row = sqlx::query(
        "SELECT space_id, external_id, title FROM knowledge_nodes WHERE id = ? AND source = ?",
    )
    .bind(node_id)
    .bind(SOURCE_ID)
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| anyhow::anyhow!("Wiki document is not indexed or is no longer registered"))?;
    let space_id: String = row.try_get("space_id")?;
    let external_id: String = row.try_get("external_id")?;
    let title: String = row.try_get("title")?;
    let space = entries(pool)
        .await?
        .into_iter()
        .find(|space| space.id == space_id)
        .ok_or_else(|| anyhow::anyhow!("Wiki space is no longer registered"))?;
    let relative_path = relative_path(&space_id, &external_id)?;
    let root = verified_root(&space)?;
    let (path, body) = read(&root, &relative_path, &space.exclude)?;
    Ok(WikiReadDocument {
        node_id,
        space_id,
        relative_path,
        title,
        path: path.to_string_lossy().into_owned(),
        body,
    })
}

async fn search_rows(
    pool: &SqlitePool,
    spaces: &[WikiSpace],
    query: &str,
    scope: &str,
) -> anyhow::Result<Vec<sqlx::sqlite::SqliteRow>> {
    let match_query = crate::knowledge::search::fts_query(query);
    if match_query.is_empty() {
        let sql = format!(
            "SELECT n.id, n.space_id, n.external_id, n.title, COALESCE(c.content, '') AS content FROM knowledge_nodes n \
             LEFT JOIN knowledge_chunks c ON c.node_id = n.id AND c.ord = 0 \
             WHERE n.source = ? AND n.space_id IN ({scope}) \
             AND (n.title LIKE ? OR EXISTS (SELECT 1 FROM knowledge_chunks c2 \
             WHERE c2.node_id = n.id AND c2.content LIKE ?)) ORDER BY n.id LIMIT ?"
        );
        return bind_spaces(sqlx::query(&sql).bind(SOURCE_ID), spaces)
            .bind(format!("%{query}%"))
            .bind(format!("%{query}%"))
            .bind(DOCUMENT_LIMIT + 1)
            .fetch_all(pool)
            .await
            .map_err(Into::into);
    }
    let sql = format!(
        "SELECT n.id, n.space_id, n.external_id, n.title, COALESCE(c.content, '') AS content FROM knowledge_nodes n \
         LEFT JOIN knowledge_chunks c ON c.node_id = n.id AND c.ord = 0 \
         WHERE n.source = ? AND n.space_id IN ({scope}) AND n.id IN ( \
           SELECT c2.node_id FROM knowledge_fts f JOIN knowledge_chunks c2 ON c2.id = f.rowid \
           WHERE knowledge_fts MATCH ? \
         ) ORDER BY n.id LIMIT ?"
    );
    bind_spaces(sqlx::query(&sql).bind(SOURCE_ID), spaces)
        .bind(match_query)
        .bind(DOCUMENT_LIMIT + 1)
        .fetch_all(pool)
        .await
        .map_err(Into::into)
}

fn scoped_spaces(spaces: Vec<WikiSpace>, space_id: Option<&str>) -> anyhow::Result<Vec<WikiSpace>> {
    let Some(space_id) = space_id else {
        return Ok(spaces);
    };
    let space = spaces
        .into_iter()
        .find(|space| space.id == space_id)
        .ok_or_else(|| anyhow::anyhow!("Wiki space is not registered"))?;
    Ok(vec![space])
}

fn bind_spaces<'q>(
    mut query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    spaces: &'q [WikiSpace],
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    for space in spaces {
        query = query.bind(&space.id);
    }
    query
}

fn placeholders(count: usize) -> String {
    vec!["?"; count].join(",")
}

fn row_to_document(row: sqlx::sqlite::SqliteRow) -> anyhow::Result<WikiDocument> {
    let space_id: String = row.try_get("space_id")?;
    let external_id: String = row.try_get("external_id")?;
    Ok(WikiDocument {
        node_id: row.try_get("id")?,
        relative_path: relative_path(&space_id, &external_id)?,
        space_id,
        title: row.try_get("title")?,
        snippet: snippet(&row.try_get::<String, _>("content")?),
    })
}

fn snippet(content: &str) -> String {
    content.replace('\n', " ").chars().take(200).collect()
}
