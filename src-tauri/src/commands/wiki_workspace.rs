//! Main-window-only access to the connected local Markdown vault.
use super::{
    knowledge_vault::{pool_of, text},
    AppState,
};
use crate::knowledge::vault::workspace::{self, Document, Graph};
use tauri::{State, WebviewWindow};

fn main_window(label: &str) -> Result<(), String> {
    if label != "main" {
        return Err("위키 문서는 메인 창에서만 관리할 수 있습니다".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn wiki_workspace_graph(
    window: WebviewWindow,
    state: State<'_, AppState>,
    vault_id: String,
) -> Result<Graph, String> {
    main_window(window.label())?;
    workspace::graph(&pool_of(&state)?, &vault_id)
        .await
        .map_err(text)
}
#[tauri::command]
pub async fn wiki_workspace_read(
    window: WebviewWindow,
    state: State<'_, AppState>,
    vault_id: String,
    path: String,
) -> Result<Document, String> {
    main_window(window.label())?;
    workspace::read(&pool_of(&state)?, &vault_id, &path)
        .await
        .map_err(text)
}
#[tauri::command]
pub async fn wiki_workspace_save(
    window: WebviewWindow,
    state: State<'_, AppState>,
    vault_id: String,
    path: String,
    content: String,
    expected_hash: Option<String>,
) -> Result<Document, String> {
    main_window(window.label())?;
    workspace::save(
        &pool_of(&state)?,
        &vault_id,
        &path,
        &content,
        expected_hash.as_deref(),
    )
    .await
    .map_err(text)
}
#[tauri::command]
pub async fn wiki_workspace_trash(
    window: WebviewWindow,
    state: State<'_, AppState>,
    vault_id: String,
    path: String,
    expected_hash: String,
) -> Result<(), String> {
    main_window(window.label())?;
    workspace::trash(&pool_of(&state)?, &vault_id, &path, &expected_hash)
        .await
        .map_err(text)
}

#[cfg(test)]
mod tests {
    #[test]
    fn document_commands_are_main_window_only() {
        assert!(super::main_window("main").is_ok());
        for label in ["project-editor", "preview", "task-editor", ""] {
            assert!(super::main_window(label).is_err());
        }
    }
}
