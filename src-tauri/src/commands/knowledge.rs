//! 지식 항목 버전·증거·검토 커맨드.
//!
//! `commands.rs`에서 갈라져 나왔다 — 모든 기능이 한 파일 끝에 줄을 붙이는
//! 구조가 병행 worktree의 머지 충돌을 만들었다.


use tauri::State;

use crate::memory;

use super::{AppState, now, pool_of};

#[tauri::command]
pub async fn knowledge_versions(
    state: State<'_, AppState>,
    id: i64,
) -> Result<Vec<memory::versioning::MemoryVersion>, String> {
    let pool = pool_of(&state)?;
    memory::management::versions(&pool, id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn knowledge_restore_version(
    state: State<'_, AppState>,
    id: i64,
    source_version: i64,
    expected_current_version: i64,
    expected_status: String,
) -> Result<i64, String> {
    let pool = pool_of(&state)?;
    memory::management::restore_version(
        &pool,
        id,
        source_version,
        expected_current_version,
        &expected_status,
        now(),
    )
    .await
    .map_err(|error| error.to_string())
}

/// 현재 후보에 backend-minted 사람 확인 receipt를 연결한다.
#[tauri::command]
pub async fn knowledge_confirm(
    state: State<'_, AppState>,
    id: i64,
    expires_at: Option<i64>,
) -> Result<i64, String> {
    let pool = pool_of(&state)?;
    memory::add_user_confirmation(&pool, id, now(), expires_at)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn knowledge_add_code_location(
    state: State<'_, AppState>,
    id: i64,
    input: crate::evidence::CodeLocationInput,
) -> Result<i64, String> {
    let pool = pool_of(&state)?;
    crate::evidence::add_code_location(&pool, id, input, now())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn knowledge_add_local_document(
    state: State<'_, AppState>,
    id: i64,
    input: crate::evidence::LocalDocumentInput,
) -> Result<i64, String> {
    let pool = pool_of(&state)?;
    crate::evidence::add_local_document(&pool, id, input, now())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn knowledge_add_external_document(
    state: State<'_, AppState>,
    id: i64,
    input: crate::evidence::ExternalDocumentInput,
) -> Result<i64, String> {
    let pool = pool_of(&state)?;
    crate::evidence::add_external_document(&pool, id, input, now())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn knowledge_evidence(
    state: State<'_, AppState>,
    id: i64,
) -> Result<Vec<crate::evidence::EvidenceRecord>, String> {
    let pool = pool_of(&state)?;
    crate::evidence::list_evidence(&pool, id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn knowledge_revalidate(
    state: State<'_, AppState>,
    id: i64,
) -> Result<crate::evidence::RevalidationReport, String> {
    let pool = pool_of(&state)?;
    crate::evidence::revalidate_memory(&pool, id, now())
        .await
        .map_err(|error| error.to_string())
}

/// candidate/stale 항목을 사람 검토 큐로 보낸다.
#[tauri::command]
pub async fn knowledge_submit_review(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let pool = pool_of(&state)?;
    memory::submit_for_review(&pool, id, now())
        .await
        .map_err(|e| e.to_string())
}

/// 현재 version의 valid evidence가 있는 검토 대기 항목만 사람이 승인한다.
#[tauri::command]
pub async fn knowledge_approve(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let pool = pool_of(&state)?;
    memory::approve(&pool, id, "human", now())
        .await
        .map_err(|e| e.to_string())
}

/// 사람 확인부터 immutable approval receipt까지 현재 version에 원자적으로 연결한다.
#[tauri::command]
pub async fn knowledge_confirm_and_approve(
    state: State<'_, AppState>,
    id: i64,
    expected_version: i64,
) -> Result<memory::ConfirmedApproval, String> {
    let pool = pool_of(&state)?;
    memory::confirm_and_approve(&pool, id, expected_version, now())
        .await
        .map_err(|error| error.to_string())
}

