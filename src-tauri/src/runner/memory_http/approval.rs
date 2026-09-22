use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;

use super::{now, scope, ApiError, RunnerHttpState};

#[derive(Deserialize)]
struct ConfirmApprovalRequest {
    expected_version: i64,
}

pub(super) fn routes() -> Router<RunnerHttpState> {
    Router::new().route(
        "/v1/memories/:id/confirm-and-approve",
        post(confirm_and_approve),
    )
}

async fn confirm_and_approve(
    State(state): State<RunnerHttpState>,
    AxumPath(id): AxumPath<i64>,
    Json(request): Json<ConfirmApprovalRequest>,
) -> Result<Json<crate::memory::ConfirmedApproval>, ApiError> {
    scope::authorize_memory(&state, id).await?;
    crate::memory::confirm_and_approve(&state.pool, id, request.expected_version, now())
        .await
        .map(Json)
        .map_err(approval_error)
}

fn approval_error(error: crate::memory::ConfirmApprovalFailure) -> ApiError {
    use crate::memory::ConfirmApprovalFailureKind;

    let status = match error.kind() {
        ConfirmApprovalFailureKind::NotFound => StatusCode::NOT_FOUND,
        ConfirmApprovalFailureKind::Invalid => StatusCode::BAD_REQUEST,
        ConfirmApprovalFailureKind::Conflict => StatusCode::CONFLICT,
        ConfirmApprovalFailureKind::Storage => StatusCode::INTERNAL_SERVER_ERROR,
    };
    let message = if error.kind() == ConfirmApprovalFailureKind::Storage {
        "메모리 원자 승인 저장소 오류".to_string()
    } else {
        error.to_string()
    };
    (status, message)
}
