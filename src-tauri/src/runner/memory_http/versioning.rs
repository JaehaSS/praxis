use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use super::{internal, now, scope, ApiError, RunnerHttpState};

#[derive(Deserialize)]
struct RestoreVersionRequest {
    expected_current_version: i64,
    expected_status: String,
}

pub(super) fn routes() -> Router<RunnerHttpState> {
    Router::new()
        .route("/v1/memories/:id/versions", get(list))
        .route("/v1/memories/:id/versions/:version/restore", post(restore))
}

async fn list(
    State(state): State<RunnerHttpState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Vec<crate::memory::versioning::MemoryVersion>>, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::memory::management::versions(&state.pool, id)
        .await
        .map(Json)
        .map_err(internal)
}

async fn restore(
    State(state): State<RunnerHttpState>,
    AxumPath((id, source_version)): AxumPath<(i64, i64)>,
    Json(request): Json<RestoreVersionRequest>,
) -> Result<Json<i64>, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::memory::management::restore_version(
        &state.pool,
        id,
        source_version,
        request.expected_current_version,
        &request.expected_status,
        now(),
    )
    .await
    .map(Json)
    .map_err(restore_api_error)
}

fn restore_api_error(error: crate::memory::restore_error::RestoreFailure) -> ApiError {
    use crate::memory::restore_error::RestoreFailureKind;

    match error.kind() {
        RestoreFailureKind::NotFound => (StatusCode::NOT_FOUND, error.to_string()),
        RestoreFailureKind::Invalid => (StatusCode::BAD_REQUEST, error.to_string()),
        RestoreFailureKind::Conflict => (StatusCode::CONFLICT, error.to_string()),
        RestoreFailureKind::Storage => internal("메모리 버전 복원 저장소 오류"),
    }
}
