use std::path::Path;

use sqlx::{Row, SqlitePool};

use super::config::{
    canonical_root, display_name, id_for, reject_overlapping, save_with, WikiConfig, WikiSpaceEntry,
};
use super::SOURCE_ID;

pub(super) async fn migrate_available_legacy(
    pool: &SqlitePool,
    mut config: WikiConfig,
) -> anyhow::Result<WikiConfig> {
    let mut available = Vec::new();
    let mut pending = Vec::new();
    for legacy in std::mem::take(&mut config.pending_legacy) {
        match canonical_root(&legacy.root) {
            Ok(root) => available.push((legacy, root)),
            Err(_) => pending.push(legacy),
        }
    }
    if available.is_empty() {
        config.pending_legacy = pending;
        return Ok(config);
    }
    let mut proposed = config.spaces.clone();
    let mut additions = Vec::new();
    for (legacy, root) in &available {
        reject_overlapping(&proposed, root)?;
        let space = WikiSpaceEntry {
            id: id_for(root),
            name: display_name(root),
            root: root.to_string_lossy().into_owned(),
            exclude: legacy.exclude.clone(),
            embed_exclude: legacy.embed_exclude.clone(),
        };
        proposed.push(space.clone());
        additions.push((legacy, space));
    }
    let mut tx = pool.begin().await?;
    for (legacy, space) in &additions {
        migrate_nodes(&mut tx, &legacy.root, space).await?;
    }
    config.spaces = proposed;
    config.pending_legacy = pending;
    save_with(&mut tx, &config).await?;
    tx.commit().await?;
    Ok(config)
}

pub(super) async fn migrate_nodes(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    legacy_root: &str,
    space: &WikiSpaceEntry,
) -> anyhow::Result<()> {
    let label = Path::new(legacy_root)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "vault".into());
    let prefix = format!("{label}/");
    let rows = sqlx::query("SELECT id, external_id FROM knowledge_nodes WHERE source = 'obsidian'")
        .fetch_all(&mut **tx)
        .await?;
    for row in rows {
        let id: i64 = row.try_get("id")?;
        let external_id: String = row.try_get("external_id")?;
        let Some(relative) = external_id.strip_prefix(&prefix) else {
            continue;
        };
        sqlx::query(
            "UPDATE knowledge_nodes SET source = ?, space_id = ?, external_id = ? WHERE id = ?",
        )
        .bind(SOURCE_ID)
        .bind(&space.id)
        .bind(format!("{}/{}", space.id, relative))
        .bind(id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
