//! 모바일 표면(`/m/*` + `/v1/*`)이 부르는 **작업 행위**의 추상 — 설계 2026-09-13 D4.
//!
//! 같은 HTTP 핸들러가 두 호스트에서 돈다. Runner에서는 큐·finalization이 행위를 수행하고,
//! 데스크톱에서는 IPC 명령(`task_approve` 등)이 수행한다. 핸들러는 요청 파싱·응답 코드만
//! 알고, "누가 어떻게 승인하는가"는 이 트레이트 뒤로 숨긴다.
//!
//! 상태 구조체(`RunnerHttpState`)에 필드를 더하지 않고 `Extension`으로 주입한다 — 그 구조체를
//! 리터럴로 만드는 테스트가 열 곳이 넘고, 그 테스트들은 Runner 의미론을 검증하므로 기본값이
//! Runner 구현이어야 한다.

use std::sync::Arc;

use async_trait::async_trait;
use axum::http::StatusCode;

use crate::db;
use crate::runner::http::RunnerHttpState;
use crate::runner::QueuedTaskRequest;

pub type ApiError = (StatusCode, String);

/// 핸들러가 `Extension`으로 꺼내 쓰는 공유 핸들.
pub type SharedTaskActions = Arc<dyn TaskActions>;

#[async_trait]
pub trait TaskActions: Send + Sync {
    /// 새 작업 생성. Runner는 큐에 넣고, 데스크톱은 즉시 대화 턴을 시작한다.
    async fn create(
        &self,
        cx: &RunnerHttpState,
        request: QueuedTaskRequest,
    ) -> Result<db::Task, ApiError>;
    /// 외부기원 승인 대기(PendingApproval) 작업의 실행 허가.
    async fn run_approve(&self, cx: &RunnerHttpState, id: i64) -> Result<db::Task, ApiError>;
    /// 검토 대기 작업을 승인(머지)한다.
    async fn approve(&self, cx: &RunnerHttpState, id: i64) -> Result<(), ApiError>;
    /// 검토 대기 작업을 버린다.
    async fn discard(&self, cx: &RunnerHttpState, id: i64) -> Result<(), ApiError>;
    /// 검토 대기 중인 대화 작업에 후속 메시지를 보낸다.
    async fn message(&self, cx: &RunnerHttpState, id: i64, message: &str)
        -> Result<(), ApiError>;
    /// Verify(빌드·테스트)를 돌리고 보고서를 돌려준다.
    async fn verify(
        &self,
        cx: &RunnerHttpState,
        id: i64,
        preview_token: String,
    ) -> Result<crate::verify::VerifyReport, ApiError>;
    /// 작업 출력 replay — `after` 뒤의 조각을 task별로 자른다.
    async fn output(
        &self,
        cx: &RunnerHttpState,
        id: i64,
        after: i64,
        limit: i64,
    ) -> Result<Vec<db::TaskOutput>, ApiError>;
}

/// Runner 구현 — 종전 핸들러 본문을 그대로 옮겼다. 동작 변화 없음.
pub struct RunnerTaskActions;

fn invalid_request(error: String) -> ApiError {
    (StatusCode::BAD_REQUEST, error)
}

fn internal_error(error: anyhow::Error) -> ApiError {
    eprintln!("Runner API 처리 실패: {error}");
    (StatusCode::INTERNAL_SERVER_ERROR, "Runner 내부 오류".to_string())
}

fn service_error(error: String) -> ApiError {
    (StatusCode::SERVICE_UNAVAILABLE, error)
}

fn now() -> i64 {
    crate::runner::now_secs()
}

#[async_trait]
impl TaskActions for RunnerTaskActions {
    async fn create(
        &self,
        cx: &RunnerHttpState,
        request: QueuedTaskRequest,
    ) -> Result<db::Task, ApiError> {
        crate::runner::create_queued_task(
            &cx.config,
            &cx.pool,
            &cx.queue.worktree_locks(),
            request,
            now(),
        )
        .await
        .map_err(crate::runner::http::create_task_error_response)
    }

    async fn run_approve(&self, cx: &RunnerHttpState, id: i64) -> Result<db::Task, ApiError> {
        let _review_claim = cx
            .review_claims
            .claim_finalization(id)
            .map_err(invalid_request)?;
        crate::runner::review_process::assert_task_unfenced(&cx.pool, id)
            .await
            .map_err(invalid_request)?;
        crate::runner::approve_pending_task(&cx.pool, &cx.queue.worktree_locks(), id, now())
            .await
            .map_err(invalid_request)
    }

    async fn approve(&self, cx: &RunnerHttpState, id: i64) -> Result<(), ApiError> {
        finalize(cx, id, true).await
    }

    async fn discard(&self, cx: &RunnerHttpState, id: i64) -> Result<(), ApiError> {
        finalize(cx, id, false).await
    }

    async fn message(
        &self,
        cx: &RunnerHttpState,
        id: i64,
        message: &str,
    ) -> Result<(), ApiError> {
        let task = db::get_task(&cx.pool, id)
            .await
            .map_err(internal_error)?
            .ok_or_else(|| (StatusCode::NOT_FOUND, "task를 찾을 수 없습니다".to_string()))?;
        if task.mode != "conversation" {
            return Err((
                StatusCode::CONFLICT,
                "conversation 작업에만 후속 메시지를 보낼 수 있습니다".to_string(),
            ));
        }
        if db::requeue_conversation_followup(&cx.pool, id, message, now())
            .await
            .map_err(internal_error)?
        {
            Ok(())
        } else {
            Err((
                StatusCode::CONFLICT,
                "검토 대기 상태의 대화 작업만 이어갈 수 있습니다".to_string(),
            ))
        }
    }

    async fn verify(
        &self,
        cx: &RunnerHttpState,
        id: i64,
        preview_token: String,
    ) -> Result<crate::verify::VerifyReport, ApiError> {
        let task = db::get_task(&cx.pool, id)
            .await
            .map_err(internal_error)?
            .ok_or_else(|| (StatusCode::NOT_FOUND, "task를 찾을 수 없습니다".to_string()))?;
        let root = crate::runner::auth::authorize_repository_path(
            &cx.config.repository_roots,
            std::path::Path::new(&task.worktree_path),
        )
        .map_err(|_| {
            (
                StatusCode::FORBIDDEN,
                "허용되지 않는 repository 경로입니다".to_string(),
            )
        })?;
        crate::runner::review_process::assert_task_not_quarantined(&cx.pool, id)
            .await
            .map_err(service_error)?;
        let build = crate::runner::review_process::registrar(
            cx.pool.clone(),
            id,
            crate::runner::review_process::ReviewOperation::Verify,
            crate::runner::review_process::ReviewPhase::VerifyBuild,
        )
        .map_err(service_error)?;
        let test = crate::runner::review_process::registrar(
            cx.pool.clone(),
            id,
            crate::runner::review_process::ReviewOperation::Verify,
            crate::runner::review_process::ReviewPhase::VerifyTest,
        )
        .map_err(service_error)?;
        crate::review_ops::verify::run_managed(
            cx.pool.clone(),
            cx.review_claims.clone(),
            id,
            root,
            preview_token,
            build,
            test,
        )
        .await
        .map_err(service_error)
    }

    async fn output(
        &self,
        cx: &RunnerHttpState,
        id: i64,
        after: i64,
        limit: i64,
    ) -> Result<Vec<db::TaskOutput>, ApiError> {
        db::list_task_output_for_task_after(&cx.pool, id, after, limit)
            .await
            .map_err(internal_error)
    }
}

async fn finalize(cx: &RunnerHttpState, id: i64, approved: bool) -> Result<(), ApiError> {
    let _review_claim = cx
        .review_claims
        .claim_finalization(id)
        .map_err(invalid_request)?;
    crate::runner::review_process::assert_task_unfenced(&cx.pool, id)
        .await
        .map_err(invalid_request)?;
    crate::runner::finalize_task(
        &cx.config,
        &cx.pool,
        &cx.queue.worktree_locks(),
        id,
        approved,
        now(),
    )
    .await
    .map_err(invalid_request)
}
