use std::path::Path;

use sqlx::SqlitePool;

use super::config::{
    canonical_root, display_name, id_for, load, load_raw, reject_overlapping, save_with,
    WikiSpaceEntry,
};
use crate::knowledge::config::{save_obsidian_with, ObsidianConfig, VaultEntry};

pub async fn legacy_view(pool: &SqlitePool) -> anyhow::Result<ObsidianConfig> {
    let config = load(pool).await?;
    Ok(ObsidianConfig {
        vaults: config
            .spaces
            .into_iter()
            .map(|space| VaultEntry {
                root: space.root,
                exclude: space.exclude,
                embed_exclude: space.embed_exclude,
            })
            .chain(config.pending_legacy)
            .collect(),
    })
}

pub async fn replace_legacy(pool: &SqlitePool, legacy: &ObsidianConfig) -> anyhow::Result<()> {
    let mut config = load_raw(pool).await?;
    let spaces = proposed_spaces(&config, legacy)?;
    let additions = additions(&config.spaces, &spaces);
    let mut tx = pool.begin().await?;
    save_obsidian_with(&mut tx, legacy).await?;
    migrate_additions(&mut tx, legacy, &additions).await?;
    config.spaces = spaces;
    config.pending_legacy.clear();
    save_with(&mut tx, &config).await?;
    tx.commit().await?;
    Ok(())
}

fn proposed_spaces(
    config: &super::config::WikiConfig,
    legacy: &ObsidianConfig,
) -> anyhow::Result<Vec<WikiSpaceEntry>> {
    let mut spaces = Vec::new();
    for entry in &legacy.vaults {
        let root = canonical_root(&entry.root)
            .map_err(|_| anyhow::anyhow!("Wiki folder is unavailable: {}", entry.root))?;
        if spaces
            .iter()
            .any(|space: &WikiSpaceEntry| canonical_root(&space.root).is_ok_and(|old| old == root))
        {
            anyhow::bail!("Wiki folder is listed more than once")
        }
        reject_overlapping(&spaces, &root)?;
        let existing = config.spaces.iter().find(|space| {
            canonical_root(&space.root).unwrap_or_else(|_| space.root.clone().into()) == root
        });
        let id = existing
            .map(|space| space.id.clone())
            .unwrap_or_else(|| id_for(&root));
        let name = existing
            .map(|space| space.name.clone())
            .unwrap_or_else(|| display_name(&root));
        spaces.push(WikiSpaceEntry {
            id,
            name,
            root: root.to_string_lossy().into_owned(),
            exclude: entry.exclude.clone(),
            embed_exclude: entry.embed_exclude.clone(),
        });
    }
    Ok(spaces)
}

fn additions<'a>(
    old: &'a [WikiSpaceEntry],
    spaces: &'a [WikiSpaceEntry],
) -> Vec<&'a WikiSpaceEntry> {
    spaces
        .iter()
        .filter(|space| !old.iter().any(|old| old.id == space.id))
        .collect()
}

async fn migrate_additions(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    legacy: &ObsidianConfig,
    additions: &[&WikiSpaceEntry],
) -> anyhow::Result<()> {
    for space in additions {
        let entry = legacy
            .vaults
            .iter()
            .find(|entry| {
                canonical_root(&entry.root).is_ok_and(|root| root == Path::new(&space.root))
            })
            .ok_or_else(|| anyhow::anyhow!("Wiki folder configuration changed during migration"))?;
        super::migration::migrate_nodes(tx, &entry.root, space).await?;
    }
    Ok(())
}
