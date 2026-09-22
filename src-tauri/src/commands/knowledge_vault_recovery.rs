use serde::Serialize;
use sqlx::{Row, SqlitePool};
use tauri::State;

use crate::knowledge::vault;

use super::knowledge_vault::{pool_of, text, VaultDto};
use super::{now, AppState};

#[derive(Serialize)]
pub struct RecoveryDto {
    pub recovered: usize,
    pub conflicts: usize,
    pub reindex_needed: usize,
}

#[derive(Serialize)]
pub struct OperationConflictDto {
    pub id: String,
    pub vault_id: String,
    pub document_id: String,
    pub revision_id: String,
    pub relative_path: String,
}

#[derive(Serialize)]
pub struct BindingHistoryDto {
    pub id: String,
    pub epoch: String,
    pub canonical_repo_root: String,
    pub active: bool,
}

#[tauri::command]
pub async fn knowledge_vault_recover_operations(
    state: State<'_, AppState>,
) -> Result<RecoveryDto, String> {
    let report = vault::recover_operations(&pool_of(&state)?, now())
        .await
        .map_err(text)?;
    Ok(RecoveryDto {
        recovered: report.recovered,
        conflicts: report.conflicts,
        reindex_needed: report.reindex_needed,
    })
}

#[tauri::command]
pub async fn knowledge_vault_rebind(
    state: State<'_, AppState>,
    vault_id: String,
    new_root: String,
    confirmed_revisions: Vec<String>,
) -> Result<VaultDto, String> {
    let vault = vault::rebind_vault(
        &pool_of(&state)?,
        &vault_id,
        std::path::Path::new(&new_root),
        &confirmed_revisions,
        now(),
    )
    .await
    .map_err(text)?;
    Ok(VaultDto {
        id: vault.id,
        vault_root: vault.canonical_root,
        enabled: vault.enabled,
    })
}

pub(super) async fn operation_conflicts(
    pool: &SqlitePool,
) -> anyhow::Result<Vec<OperationConflictDto>> {
    let rows = sqlx::query(
        "SELECT id, vault_id, document_id, revision_id, relative_path \
         FROM vault_operations WHERE state = 'conflict' ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(OperationConflictDto {
                id: row.try_get("id")?,
                vault_id: row.try_get("vault_id")?,
                document_id: row.try_get("document_id")?,
                revision_id: row.try_get("revision_id")?,
                relative_path: row.try_get("relative_path")?,
            })
        })
        .collect()
}

pub(super) async fn binding_history(pool: &SqlitePool) -> anyhow::Result<Vec<BindingHistoryDto>> {
    let rows = sqlx::query(
        "SELECT id, epoch, canonical_root, active FROM vault_project_bindings ORDER BY registered_at DESC",
    )
    .fetch_all(pool)
    .await?;
    rows.iter()
        .map(|row| {
            Ok(BindingHistoryDto {
                id: row.try_get("id")?,
                epoch: row.try_get("epoch")?,
                canonical_repo_root: row.try_get("canonical_root")?,
                active: row.try_get::<i64, _>("active")? != 0,
            })
        })
        .collect()
}
