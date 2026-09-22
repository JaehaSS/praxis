use std::path::PathBuf;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;

use crate::db;
use crate::runner::http::RunnerHttpState;

type ApiError = (StatusCode, String);

#[derive(Deserialize)]
struct VerifyRequest {
    preview_token: String,
}

pub fn routes() -> Router<RunnerHttpState> {
    Router::new()
        .route("/v1/tasks/:id/verify/spec", get(verify_spec))
        .route("/v1/tasks/:id/verify", post(task_verify))
        .route("/v1/tasks/:id/evidence", get(evidence))
}

async fn verify_spec(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<Json<crate::review_ops::verify::VerifyPreview>, ApiError> {
    let (_, root) = authorized_task(&state, id).await?;
    crate::review_ops::verify::preview(&state.pool, id, &root)
        .await
        .map(Json)
        .map_err(service_error)
}

async fn task_verify(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
    Json(request): Json<VerifyRequest>,
) -> Result<Json<crate::verify::VerifyReport>, ApiError> {
    let (_, root) = authorized_task(&state, id).await?;
    crate::runner::review_process::assert_task_not_quarantined(&state.pool, id)
        .await
        .map_err(service_error)?;
    let build = crate::runner::review_process::registrar(
        state.pool.clone(),
        id,
        crate::runner::review_process::ReviewOperation::Verify,
        crate::runner::review_process::ReviewPhase::VerifyBuild,
    )
    .map_err(service_error)?;
    let test = crate::runner::review_process::registrar(
        state.pool.clone(),
        id,
        crate::runner::review_process::ReviewOperation::Verify,
        crate::runner::review_process::ReviewPhase::VerifyTest,
    )
    .map_err(service_error)?;
    crate::review_ops::verify::run_managed(
        state.pool,
        state.review_claims,
        id,
        root,
        request.preview_token,
        build,
        test,
    )
    .await
    .map(Json)
    .map_err(service_error)
}

async fn evidence(
    State(state): State<RunnerHttpState>,
    Path(id): Path<i64>,
) -> Result<Json<Option<db::Evidence>>, ApiError> {
    authorized_task(&state, id).await?;
    db::get_evidence(&state.pool, id)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn authorized_task(
    state: &RunnerHttpState,
    id: i64,
) -> Result<(db::Task, PathBuf), ApiError> {
    let task = db::get_task(&state.pool, id)
        .await
        .map_err(internal_error)?
        .ok_or_else(|| not_found("task를 찾을 수 없습니다".into()))?;
    let root = crate::runner::auth::authorize_repository_path(
        &state.config.repository_roots,
        std::path::Path::new(&task.worktree_path),
    )
    .map_err(|_| {
        (
            StatusCode::FORBIDDEN,
            "허용되지 않는 repository 경로입니다".into(),
        )
    })?;
    Ok((task, root))
}

fn internal_error(error: anyhow::Error) -> ApiError {
    eprintln!("Runner review API 처리 실패: {error}");
    (StatusCode::INTERNAL_SERVER_ERROR, "Runner 내부 오류".into())
}

fn service_error(error: String) -> ApiError {
    (StatusCode::CONFLICT, error)
}

fn not_found(error: String) -> ApiError {
    (StatusCode::NOT_FOUND, error)
}
