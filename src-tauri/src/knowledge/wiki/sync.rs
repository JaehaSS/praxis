use sqlx::{Row, SqlitePool};

use super::config::{entries, verified_root};
use super::files::scan;
use super::SOURCE_ID;

#[derive(Debug, Clone, serde::Serialize)]
pub struct WikiSyncResult {
    pub indexed: usize,
    pub skipped: usize,
    pub deleted: usize,
    pub edges: usize,
    pub complete: bool,
    pub warnings: Vec<String>,
}

pub async fn sync(pool: &SqlitePool, now: i64) -> anyhow::Result<WikiSyncResult> {
    ensure_sync_supported()?;
    crate::knowledge::vault::ownership::claim_legacy_owned_nodes(pool, now).await?;
    let _admission = crate::knowledge::vault::shared_admission(pool).await?;

    let mut result = WikiSyncResult {
        indexed: 0,
        skipped: 0,
        deleted: 0,
        edges: 0,
        complete: true,
        warnings: Vec::new(),
    };
    for space in entries(pool).await? {
        let root = match verified_root(&space) {
            Ok(root) => root,
            Err(error) => {
                result.complete = false;
                result
                    .warnings
                    .push(format!("cannot access {}: {error}", space.root));
                continue;
            }
        };
        let scan = scan(&space, &root);
        for document in &scan.documents {
            let owned = match document
                .url
                .as_deref()
                .and_then(|url| url.strip_prefix("file://"))
            {
                Some(path) => {
                    crate::knowledge::vault::ownership::is_owned_path(
                        pool,
                        std::path::Path::new(path),
                    )
                    .await?
                }
                None => false,
            };
            if owned {
                continue;
            }
            match crate::knowledge::graph::upsert_document(pool, document, now).await? {
                crate::knowledge::graph::UpsertOutcome::Indexed => result.indexed += 1,
                crate::knowledge::graph::UpsertOutcome::Skipped => result.skipped += 1,
            }
            sqlx::query(
                "UPDATE knowledge_nodes SET space_id = ? WHERE source = ? AND external_id = ?",
            )
            .bind(&space.id)
            .bind(SOURCE_ID)
            .bind(&document.external_id)
            .execute(pool)
            .await?;
        }
        if scan.complete {
            result.deleted += delete_missing(pool, &space.id, &scan.documents).await?;
        } else {
            result.complete = false;
        }
        result.warnings.extend(scan.warnings);
        result.edges += rebuild_space_links(pool, &space.id, &scan.documents).await?;
    }
    Ok(result)
}

#[cfg(windows)]
fn ensure_sync_supported() -> anyhow::Result<()> {
    anyhow::bail!("Wiki synchronization is not supported on Windows")
}

#[cfg(not(windows))]
fn ensure_sync_supported() -> anyhow::Result<()> {
    Ok(())
}

async fn rebuild_space_links(
    pool: &SqlitePool,
    space_id: &str,
    docs: &[crate::knowledge::graph::Document],
) -> anyhow::Result<usize> {
    if docs.is_empty() {
        return Ok(0);
    }
    let nodes = space_nodes(pool, space_id).await?;
    let index = crate::knowledge::link::TargetIndex::build(&nodes);
    let ids: std::collections::HashMap<_, _> = nodes
        .iter()
        .map(|(id, path)| (path.as_str(), *id))
        .collect();
    let mut tx = pool.begin().await?;
    let written = write_links(&mut tx, docs, space_id, &ids, &index).await?;
    tx.commit().await?;
    Ok(written)
}

async fn space_nodes(pool: &SqlitePool, space_id: &str) -> anyhow::Result<Vec<(i64, String)>> {
    let rows = sqlx::query(
        "SELECT id, external_id FROM knowledge_nodes WHERE source = ? AND space_id = ?",
    )
    .bind(SOURCE_ID)
    .bind(space_id)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            Some((
                row.try_get("id").ok()?,
                super::files::relative_path(
                    space_id,
                    &row.try_get::<String, _>("external_id").ok()?,
                )
                .ok()?,
            ))
        })
        .collect())
}

async fn write_links(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    docs: &[crate::knowledge::graph::Document],
    space_id: &str,
    ids: &std::collections::HashMap<&str, i64>,
    index: &crate::knowledge::link::TargetIndex,
) -> anyhow::Result<usize> {
    let mut written = 0;
    for doc in docs {
        let path = super::files::relative_path(space_id, &doc.external_id)?;
        let Some(src_id) = ids.get(path.as_str()).copied() else {
            continue;
        };
        sqlx::query("DELETE FROM knowledge_edges WHERE src_id = ? AND rel = 'links_to'")
            .bind(src_id)
            .execute(&mut **tx)
            .await?;
        for target in crate::knowledge::link::parse_wikilinks(&doc.body) {
            let Some(dst_id) = index.resolve(&target) else {
                continue;
            };
            if dst_id == src_id {
                continue;
            }
            written += sqlx::query("INSERT OR IGNORE INTO knowledge_edges (src_id, dst_id, rel) VALUES (?, ?, 'links_to')")
                .bind(src_id).bind(dst_id).execute(&mut **tx).await?.rows_affected() as usize;
        }
    }
    Ok(written)
}

async fn delete_missing(
    pool: &SqlitePool,
    space_id: &str,
    documents: &[crate::knowledge::graph::Document],
) -> anyhow::Result<usize> {
    let rows =
        sqlx::query("SELECT external_id FROM knowledge_nodes WHERE source = ? AND space_id = ?")
            .bind(SOURCE_ID)
            .bind(space_id)
            .fetch_all(pool)
            .await?;
    let present: std::collections::HashSet<&str> = documents
        .iter()
        .map(|document| document.external_id.as_str())
        .collect();
    let mut deleted = 0;
    for row in rows {
        let external_id: String = row.try_get("external_id")?;
        if present.contains(external_id.as_str()) {
            continue;
        }
        crate::knowledge::graph::delete_document(pool, SOURCE_ID, &external_id).await?;
        deleted += 1;
    }
    Ok(deleted)
}
