use axum::extract::{Path, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use super::{scope, ApiError, RunnerHttpState};

#[derive(Deserialize)]
struct ConfirmationRequest {
    expires_at: Option<i64>,
}

pub(super) fn routes() -> Router<RunnerHttpState> {
    Router::new()
        .route("/v1/memories/:id/confirmations", post(confirm))
        .route("/v1/memories/:id/evidence/code-locations", post(add_code))
        .route(
            "/v1/memories/:id/evidence/documents/local",
            post(add_local_document),
        )
        .route(
            "/v1/memories/:id/evidence/documents/external",
            post(add_external_document),
        )
        .route("/v1/memories/:id/evidence", get(list))
        .route("/v1/memories/:id/revalidate", post(revalidate))
}

async fn confirm(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(request): Json<ConfirmationRequest>,
) -> Result<Json<i64>, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::memory::add_user_confirmation(&state.pool, id, now(), request.expires_at)
        .await
        .map(Json)
        .map_err(invalid)
}

async fn add_code(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(input): Json<crate::evidence::CodeLocationInput>,
) -> Result<Json<i64>, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::evidence::add_code_location(&state.pool, id, input, now())
        .await
        .map(Json)
        .map_err(invalid)
}

async fn add_local_document(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(input): Json<crate::evidence::LocalDocumentInput>,
) -> Result<Json<i64>, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::evidence::add_local_document(&state.pool, id, input, now())
        .await
        .map(Json)
        .map_err(invalid)
}

async fn add_external_document(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(input): Json<crate::evidence::ExternalDocumentInput>,
) -> Result<Json<i64>, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::evidence::add_external_document(&state.pool, id, input, now())
        .await
        .map(Json)
        .map_err(invalid)
}

async fn list(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<Json<Vec<crate::evidence::EvidenceRecord>>, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::evidence::list_evidence(&state.pool, id)
        .await
        .map(Json)
        .map_err(internal)
}

async fn revalidate(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<Json<crate::evidence::RevalidationReport>, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::evidence::revalidate_memory(&state.pool, id, now())
        .await
        .map(Json)
        .map_err(invalid)
}

fn invalid(error: impl std::fmt::Display) -> ApiError {
    (axum::http::StatusCode::BAD_REQUEST, error.to_string())
}

fn internal(error: impl std::fmt::Display) -> ApiError {
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        error.to_string(),
    )
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
