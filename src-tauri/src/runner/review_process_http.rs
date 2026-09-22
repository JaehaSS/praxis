use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use crate::runner::http::RunnerHttpState;
use crate::runner::review_process::{
    ReviewProcessQuarantine, ReviewProcessRepairError, ReviewProcessRepairResult,
};

type ApiError = (StatusCode, String);

pub fn routes() -> Router<RunnerHttpState> {
    Router::new()
        .route(
            "/v1/review-processes/quarantined",
            get(quarantined_processes),
        )
        .route(
            "/v1/review-processes/:receipt_id/reconcile",
            post(reconcile_process),
        )
}

async fn quarantined_processes(
    State(state): State<RunnerHttpState>,
) -> Result<Json<Vec<ReviewProcessQuarantine>>, ApiError> {
    crate::runner::review_process::list_quarantined(&state.pool)
        .await
        .map(Json)
        .map_err(internal_error)
}

async fn reconcile_process(
    State(state): State<RunnerHttpState>,
    Path(receipt_id): Path<i64>,
) -> Result<Json<ReviewProcessRepairResult>, ApiError> {
    let task_id = crate::runner::review_process::receipt_task_id(&state.pool, receipt_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(receipt_not_found)?;
    let task = crate::db::get_task(&state.pool, task_id)
        .await
        .map_err(internal_error)?
        .ok_or_else(task_gone)?;
    let root = reconciliation_lock_path(
        &state.config.repository_roots,
        &task.repo,
        &task.worktree_path,
    )?;
    let locks = state.queue.worktree_locks();
    let guard = crate::runner::review_process::claim_task_reconciliation(
        &state.review_claims,
        &locks,
        task_id,
        &root,
    )
    .await
    .map_err(conflict)?;
    crate::runner::review_process::repair_quarantined(&state.pool, receipt_id, now(), &guard)
        .await
        .map(Json)
        .map_err(repair_error)
}

fn reconciliation_lock_path(
    roots: &[std::path::PathBuf],
    repo: &str,
    worktree: &str,
) -> Result<std::path::PathBuf, ApiError> {
    crate::runner::auth::authorize_repository_path(roots, std::path::Path::new(repo))
        .map_err(|_| forbidden_repository())?;
    let stored = std::path::PathBuf::from(worktree);
    if !stored.exists() {
        return Ok(stored);
    }
    crate::runner::auth::authorize_repository_path(roots, &stored)
        .map_err(|_| forbidden_repository())
}

fn repair_error(error: ReviewProcessRepairError) -> ApiError {
    match error {
        ReviewProcessRepairError::ReceiptNotFound => receipt_not_found(),
        ReviewProcessRepairError::NotQuarantined => {
            conflict("격리된 review process 영수증이 아닙니다".into())
        }
        ReviewProcessRepairError::StaleReceipt => {
            conflict("현재 lease와 일치하지 않는 오래된 영수증입니다".into())
        }
        ReviewProcessRepairError::Internal(error) => internal_error(error),
    }
}

fn internal_error(error: anyhow::Error) -> ApiError {
    eprintln!("Runner review process repair API 처리 실패: {error}");
    (StatusCode::INTERNAL_SERVER_ERROR, "Runner 내부 오류".into())
}

fn conflict(error: String) -> ApiError {
    (StatusCode::CONFLICT, error)
}

fn forbidden_repository() -> ApiError {
    (
        StatusCode::FORBIDDEN,
        "허용되지 않는 repository 경로입니다".into(),
    )
}

fn receipt_not_found() -> ApiError {
    (
        StatusCode::NOT_FOUND,
        "review process 영수증을 찾을 수 없습니다".into(),
    )
}

fn task_gone() -> ApiError {
    (
        StatusCode::GONE,
        "영수증의 작업이 이미 삭제되었습니다".into(),
    )
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
