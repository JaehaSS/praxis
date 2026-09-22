use std::path::Path;

use sqlx::{Row, SqlitePool};

use super::catalog::identifier;
use super::platform;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectBinding {
    pub id: String,
    pub epoch: String,
    pub canonical_root: String,
}

pub async fn register_project(
    pool: &SqlitePool,
    root: &Path,
    now: i64,
) -> anyhow::Result<ProjectBinding> {
    let identity = platform::verified_root(root)?;
    let id = identifier("project")?;
    let epoch = identifier("epoch")?;
    sqlx::query("INSERT INTO vault_project_bindings (id, canonical_root, root_device, root_inode, epoch, registered_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&id).bind(&identity.canonical_root).bind(identity.device).bind(identity.inode).bind(&epoch).bind(now).execute(pool).await?;
    Ok(ProjectBinding {
        id,
        epoch,
        canonical_root: identity.canonical_root,
    })
}

pub async fn resolve_project(
    pool: &SqlitePool,
    root: &Path,
) -> anyhow::Result<Option<ProjectBinding>> {
    let identity = platform::verified_root(root)?;
    let row = sqlx::query("SELECT id, epoch, canonical_root FROM vault_project_bindings WHERE canonical_root = ? AND root_device = ? AND root_inode = ? AND active = 1")
        .bind(&identity.canonical_root).bind(identity.device).bind(identity.inode).fetch_optional(pool).await?;
    row.as_ref().map(binding_from_row).transpose()
}

pub async fn rebind_project(
    pool: &SqlitePool,
    binding_id: &str,
    root: &Path,
    now: i64,
) -> anyhow::Result<ProjectBinding> {
    let identity = platform::verified_root(root)?;
    let binding = ProjectBinding {
        id: identifier("project")?,
        epoch: identifier("epoch")?,
        canonical_root: identity.canonical_root,
    };
    let mut tx = pool.begin().await?;
    let deactivated =
        sqlx::query("UPDATE vault_project_bindings SET active = 0 WHERE id = ? AND active = 1")
            .bind(binding_id)
            .execute(&mut *tx)
            .await?
            .rows_affected();
    if deactivated != 1 {
        anyhow::bail!("project binding is no longer active")
    }
    sqlx::query("INSERT INTO vault_project_bindings (id, canonical_root, root_device, root_inode, epoch, registered_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&binding.id)
        .bind(&binding.canonical_root)
        .bind(identity.device)
        .bind(identity.inode)
        .bind(&binding.epoch)
        .bind(now)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(binding)
}

pub async fn add_worktree(
    pool: &SqlitePool,
    binding: &ProjectBinding,
    root: &Path,
    now: i64,
) -> anyhow::Result<()> {
    let identity = platform::verified_root(root)?;
    sqlx::query("INSERT INTO vault_project_worktrees (binding_id, canonical_root, created_at) VALUES (?, ?, ?) ON CONFLICT(canonical_root) DO NOTHING")
        .bind(&binding.id).bind(identity.canonical_root).bind(now).execute(pool).await?;
    Ok(())
}

fn binding_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<ProjectBinding> {
    Ok(ProjectBinding {
        id: row.try_get("id")?,
        epoch: row.try_get("epoch")?,
        canonical_root: row.try_get("canonical_root")?,
    })
}
