use std::path::Path;

use axum::http::StatusCode;

use super::{ApiError, RunnerHttpState};

pub(super) fn authorized_scope(
    state: &RunnerHttpState,
    repository: &str,
) -> Result<String, ApiError> {
    if repository.trim().is_empty() {
        return Ok(String::new());
    }
    crate::runner::auth::authorize_repository_path(
        &state.config.repository_roots,
        Path::new(repository),
    )
    .map(|path| path.to_string_lossy().into_owned())
    .map_err(|_| denied())
}

pub(super) async fn authorize_memory(
    state: &RunnerHttpState,
    memory_id: i64,
) -> Result<(), ApiError> {
    let row: Option<(String, Option<String>)> =
        sqlx::query_as("SELECT tier, scope_key FROM memories WHERE id = ?")
            .bind(memory_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(|error| {
                eprintln!("Runner memory scope 조회 실패(memory_id={memory_id}): {error}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "메모리 범위 조회 저장소 오류".into(),
                )
            })?;
    let Some((tier, scope)) = row else {
        return Err((StatusCode::NOT_FOUND, "메모리를 찾을 수 없습니다".into()));
    };
    if tier == crate::memory::tier::GLOBAL {
        return Ok(());
    }
    let Some(scope) = scope else {
        return Err(denied());
    };
    authorized_scope(state, &scope).map(|_| ())
}

pub(super) fn is_authorized(state: &RunnerHttpState, memory: &crate::memory::Memory) -> bool {
    if memory.tier == crate::memory::tier::GLOBAL {
        return true;
    }
    memory
        .scope_key
        .as_deref()
        .is_some_and(|scope| authorized_scope(state, scope).is_ok())
}

fn denied() -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        "허용되지 않는 repository 경로입니다".into(),
    )
}
