use std::path::{Path, PathBuf};

use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::routing::get;
use axum::{Json, Router};
use serde::Deserialize;

use crate::runner::http::RunnerHttpState;

type ApiError = (StatusCode, String);

#[derive(Deserialize)]
struct ContextFileQuery {
    path: String,
}

pub(super) fn routes() -> Router<RunnerHttpState> {
    Router::new()
        .route("/v1/tasks/:id/context", get(context_report))
        .route("/v1/tasks/:id/context/file", get(context_file))
}

async fn context_report(
    State(state): State<RunnerHttpState>,
    AxumPath(id): AxumPath<i64>,
) -> Result<Json<crate::memory::context_audit::ContextReport>, ApiError> {
    authorize_task(&state, id).await?;
    // runner에는 캡처·회고 파이프라인이 없다 — 둘 다 꺼진 것으로 보고한다.
    crate::memory::context_audit::report(&state.pool, id, &runner_home(), false, false)
        .await
        .map(Json)
        .map_err(internal)
}

async fn context_file(
    State(state): State<RunnerHttpState>,
    AxumPath(id): AxumPath<i64>,
    Query(query): Query<ContextFileQuery>,
) -> Result<Json<String>, ApiError> {
    authorize_task(&state, id).await?;
    crate::memory::context_audit::read_file(&state.pool, id, &runner_home(), Path::new(&query.path))
        .await
        .map(Json)
        .map_err(invalid)
}

async fn authorize_task(state: &RunnerHttpState, id: i64) -> Result<(), ApiError> {
    let task = crate::db::get_task(&state.pool, id)
        .await
        .map_err(internal)?
        .ok_or_else(|| (StatusCode::NOT_FOUND, "task를 찾을 수 없습니다".to_string()))?;
    crate::runner::auth::authorize_repository_path(
        &state.config.repository_roots,
        Path::new(&task.repo),
    )
    .map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            "허용되지 않는 repository 경로입니다".into(),
        )
    })?;
    Ok(())
}

fn runner_home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn invalid(error: impl std::fmt::Display) -> ApiError {
    (StatusCode::BAD_REQUEST, error.to_string())
}

fn internal(error: impl std::fmt::Display) -> ApiError {
    (StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}
