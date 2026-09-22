//! Authenticated Runner-owned memory and context API.

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use serde::Deserialize;

use super::http::RunnerHttpState;

mod application_policy;
mod approval;
mod context;
mod evidence;
mod scope;
mod versioning;

#[derive(Deserialize)]
struct MemoryCreateRequest {
    repository: String,
    kind: String,
    content: String,
}

#[derive(Deserialize)]
struct MemoryUpdateRequest {
    kind: String,
    content: String,
}

#[derive(Deserialize)]
struct PreviewRequest {
    repository: String,
    instruction: String,
}

pub fn routes() -> Router<RunnerHttpState> {
    Router::new()
        .route("/v1/memories", get(memory_list).post(memory_create))
        .route(
            "/v1/memories/:id",
            put(memory_update).delete(memory_archive),
        )
        .route("/v1/memories/:id/purge", post(memory_purge))
        .route("/v1/memories/:id/review", post(memory_review))
        .route("/v1/memories/:id/approve", post(memory_approve))
        .route("/v1/memories/:id/usages", get(memory_usages))
        .route("/v1/memories/preview", post(memory_preview))
        .merge(application_policy::routes())
        .merge(approval::routes())
        .merge(evidence::routes())
        .merge(versioning::routes())
        .merge(context::routes())
}

async fn memory_list(
    State(state): State<RunnerHttpState>,
) -> Result<Json<Vec<crate::memory::management::MemoryListItem>>, ApiError> {
    let rows = crate::memory::management::list(&state.pool, now())
        .await
        .map_err(internal)?;
    Ok(Json(
        rows.into_iter()
            .filter(|item| scope::is_authorized(&state, &item.memory))
            .collect(),
    ))
}

async fn memory_create(
    State(state): State<RunnerHttpState>,
    Json(request): Json<MemoryCreateRequest>,
) -> Result<Json<i64>, ApiError> {
    let repo = scope::authorized_scope(&state, &request.repository)?;
    crate::memory::management::create_manual(
        &state.pool,
        &repo,
        &request.kind,
        &request.content,
        now(),
    )
    .await
    .map(Json)
    .map_err(invalid)
}

async fn memory_update(
    State(state): State<RunnerHttpState>,
    AxumPath(id): AxumPath<i64>,
    Json(request): Json<MemoryUpdateRequest>,
) -> Result<StatusCode, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::memory::management::update_manual(
        &state.pool,
        id,
        &request.content,
        &request.kind,
        now(),
    )
    .await
    .map_err(invalid)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn memory_archive(
    State(state): State<RunnerHttpState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::memory::archive(&state.pool, id, now())
        .await
        .map_err(invalid)?;
    Ok(StatusCode::NO_CONTENT)
}

/// 보관된 메모리의 영구 삭제. 보관(`DELETE /:id`)과 달리 본문이 사라지고 되돌릴 수 없다.
async fn memory_purge(
    State(state): State<RunnerHttpState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::memory::purge(&state.pool, id, now())
        .await
        .map_err(invalid)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn memory_review(
    State(state): State<RunnerHttpState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::memory::submit_for_review(&state.pool, id, now())
        .await
        .map_err(invalid)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn memory_approve(
    State(state): State<RunnerHttpState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<StatusCode, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::memory::approve(&state.pool, id, "human", now())
        .await
        .map_err(invalid)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn memory_usages(
    State(state): State<RunnerHttpState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<Vec<crate::memory::MemoryUsageRow>>, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::memory::usages_for_memory(&state.pool, id)
        .await
        .map(Json)
        .map_err(internal)
}

async fn memory_preview(
    State(state): State<RunnerHttpState>,
    Json(request): Json<PreviewRequest>,
) -> Result<Json<Vec<crate::memory::Memory>>, ApiError> {
    let repo = scope::authorized_scope(&state, &request.repository)?;
    crate::memory::management::preview(&state.pool, &repo, &request.instruction, now())
        .await
        .map(Json)
        .map_err(internal)
}

pub(super) type ApiError = (StatusCode, String);

fn invalid(error: impl std::fmt::Display) -> ApiError {
    (StatusCode::BAD_REQUEST, error.to_string())
}

fn internal(error: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
