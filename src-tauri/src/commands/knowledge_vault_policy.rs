use tauri::State;

use super::knowledge_vault::{binding, pool_of, text};
use super::{now, AppState};

#[tauri::command]
pub async fn knowledge_vault_draft_policy_set(
    state: State<'_, AppState>,
    repo_root: String,
    query: String,
    client_ref: String,
    input_mode: String,
    revision_ids: Vec<String>,
) -> Result<(), String> {
    let pool = pool_of(&state)?;
    let binding = binding(&pool, &repo_root).await.map_err(text)?;
    crate::knowledge::vault::provenance::save_draft_policy(
        &pool,
        &binding,
        &client_ref,
        &query,
        &input_mode,
        &revision_ids,
        now(),
    )
    .await
    .map_err(text)
}
