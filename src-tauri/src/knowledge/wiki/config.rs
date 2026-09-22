use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::SqlitePool;

use super::CONFIG_ID;
use crate::knowledge::config::{ObsidianConfig, VaultEntry};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WikiSpace {
    pub id: String,
    pub name: String,
    pub root: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(super) struct WikiConfig {
    #[serde(default)]
    pub(super) spaces: Vec<WikiSpaceEntry>,
    #[serde(default)]
    pub(super) pending_legacy: Vec<VaultEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct WikiSpaceEntry {
    pub id: String,
    pub name: String,
    pub root: String,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub embed_exclude: Vec<String>,
}

impl WikiSpaceEntry {
    fn public(&self) -> WikiSpace {
        WikiSpace {
            id: self.id.clone(),
            name: self.name.clone(),
            root: self.root.clone(),
        }
    }
}

pub async fn spaces(pool: &SqlitePool) -> anyhow::Result<Vec<WikiSpace>> {
    Ok(load(pool)
        .await?
        .spaces
        .iter()
        .map(WikiSpaceEntry::public)
        .collect())
}

pub(crate) async fn entries(pool: &SqlitePool) -> anyhow::Result<Vec<WikiSpaceEntry>> {
    Ok(load(pool).await?.spaces)
}

pub(crate) async fn active_space_ids(pool: &SqlitePool) -> anyhow::Result<Vec<String>> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT config FROM knowledge_sources WHERE id = ?")
            .bind(CONFIG_ID)
            .fetch_optional(pool)
            .await?;
    Ok(row
        .and_then(|(json,)| json)
        .and_then(|json| serde_json::from_str::<WikiConfig>(&json).ok())
        .unwrap_or_default()
        .spaces
        .into_iter()
        .map(|space| space.id)
        .collect())
}

pub async fn connect(pool: &SqlitePool, root: &str) -> anyhow::Result<WikiSpace> {
    let _admission = crate::knowledge::vault::exclusive_admission(pool).await?;
    let canonical = canonical_root(root)?;
    crate::knowledge::vault::ownership::reject_overlapping_root(pool, &canonical).await?;
    let mut config = load(pool).await?;
    reject_overlapping(&config.spaces, &canonical)?;
    let space = WikiSpaceEntry {
        id: id_for(&canonical),
        name: display_name(&canonical),
        root: canonical.to_string_lossy().into_owned(),
        exclude: Vec::new(),
        embed_exclude: Vec::new(),
    };
    config.spaces.push(space.clone());
    save(pool, &config).await?;
    Ok(space.public())
}

pub fn canonical_root(root: &str) -> anyhow::Result<PathBuf> {
    let canonical = Path::new(root).canonicalize()?;
    if !canonical.is_dir() {
        anyhow::bail!("Wiki folder must be an existing readable directory")
    }
    Ok(canonical)
}

pub(crate) fn verified_root(space: &WikiSpaceEntry) -> anyhow::Result<PathBuf> {
    let canonical = canonical_root(&space.root)?;
    if canonical != Path::new(&space.root) {
        anyhow::bail!("Wiki folder no longer resolves to its registered canonical root")
    }
    Ok(canonical)
}

pub(super) fn display_name(root: &Path) -> String {
    root.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned())
}

pub(super) fn id_for(root: &Path) -> String {
    let mut hash = Sha256::new();
    hash.update(root.to_string_lossy().as_bytes());
    format!("wiki-{:x}", hash.finalize())
}

pub(super) fn reject_overlapping(spaces: &[WikiSpaceEntry], root: &Path) -> anyhow::Result<()> {
    for space in spaces {
        let existing = canonical_root(&space.root).unwrap_or_else(|_| PathBuf::from(&space.root));
        if existing == root || existing.starts_with(root) || root.starts_with(&existing) {
            anyhow::bail!(
                "Wiki folder duplicates or nests registered folder: {}",
                space.root
            )
        }
    }
    Ok(())
}

pub(super) async fn load(pool: &SqlitePool) -> anyhow::Result<WikiConfig> {
    let config = load_raw(pool).await?;
    super::migration::migrate_available_legacy(pool, config).await
}

pub(super) async fn load_raw(pool: &SqlitePool) -> anyhow::Result<WikiConfig> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT config FROM knowledge_sources WHERE id = ?")
            .bind(CONFIG_ID)
            .fetch_optional(pool)
            .await?;
    let config = match row {
        Some((Some(json),)) => serde_json::from_str(&json)?,
        _ => legacy_config(pool).await?,
    };
    Ok(config)
}

async fn legacy_config(pool: &SqlitePool) -> anyhow::Result<WikiConfig> {
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT config FROM knowledge_sources WHERE id = 'obsidian'")
            .fetch_optional(pool)
            .await?;
    let Some((Some(json),)) = row else {
        return Ok(WikiConfig::default());
    };
    Ok(WikiConfig {
        spaces: Vec::new(),
        pending_legacy: serde_json::from_str::<ObsidianConfig>(&json)?.vaults,
    })
}

async fn save(pool: &SqlitePool, config: &WikiConfig) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    save_with(&mut tx, config).await?;
    tx.commit().await?;
    Ok(())
}

pub(super) async fn save_with(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    config: &WikiConfig,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO knowledge_sources (id, status, config) VALUES (?, 'connected', ?) \
         ON CONFLICT(id) DO UPDATE SET config = excluded.config, status = 'connected'",
    )
    .bind(CONFIG_ID)
    .bind(serde_json::to_string(config)?)
    .execute(&mut **tx)
    .await?;
    Ok(())
}
