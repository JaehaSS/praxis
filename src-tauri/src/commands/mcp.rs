//! MCP 서버 레지스트리 커맨드.
//!
//! `commands.rs`에서 갈라져 나왔다 — 모든 기능이 한 파일 끝에 줄을 붙이는
//! 구조가 병행 worktree의 머지 충돌을 만들었다.


use tauri::State;

use crate::mcp_registry;

use super::{AppState, now, pool_of};

#[tauri::command]
pub async fn mcp_list(state: State<'_, AppState>) -> Result<Vec<mcp_registry::McpServer>, String> {
    let pool = pool_of(&state)?;
    mcp_registry::list_servers(&pool)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mcp_add(
    state: State<'_, AppState>,
    name: String,
    command: String,
    args: String,
) -> Result<i64, String> {
    let pool = pool_of(&state)?;
    mcp_registry::add_server(&pool, &name, &command, &args, now())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mcp_remove(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let pool = pool_of(&state)?;
    mcp_registry::remove_server(&pool, id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn mcp_set_enabled(
    state: State<'_, AppState>,
    id: i64,
    enabled: bool,
) -> Result<(), String> {
    let pool = pool_of(&state)?;
    mcp_registry::set_enabled(&pool, id, enabled)
        .await
        .map_err(|e| e.to_string())
}

// ── Phase 1: 텔레그램 채널 CRUD ──

