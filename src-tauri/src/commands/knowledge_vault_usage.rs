use serde::Serialize;
use sqlx::Row;
use tauri::State;

use super::knowledge_vault::{pool_of, text};
use super::AppState;

#[derive(Serialize)]
pub struct VaultUsageDto {
    pub attempt_id: String,
    pub revision_id: String,
    pub revision_hash: String,
    pub snippet: String,
    pub snippet_hash: String,
    pub delivery_state: String,
    pub citation_state: String,
}

#[tauri::command]
pub async fn knowledge_vault_usage(
    state: State<'_, AppState>,
    task_id: i64,
) -> Result<Vec<VaultUsageDto>, String> {
    let rows = sqlx::query("SELECT attempt_id, revision_id, revision_hash, snippet, snippet_hash, delivery_state, citation_state FROM vault_usages WHERE task_id = ? ORDER BY created_at, id")
        .bind(task_id).fetch_all(&pool_of(&state)?).await.map_err(text)?;
    rows.into_iter()
        .map(|row| {
            Ok(VaultUsageDto {
                attempt_id: row.try_get("attempt_id")?,
                revision_id: row.try_get("revision_id")?,
                revision_hash: row.try_get("revision_hash")?,
                snippet: row.try_get("snippet")?,
                snippet_hash: row.try_get("snippet_hash")?,
                delivery_state: row.try_get("delivery_state")?,
                citation_state: row.try_get("citation_state")?,
            })
        })
        .collect::<anyhow::Result<_>>()
        .map_err(text)
}
