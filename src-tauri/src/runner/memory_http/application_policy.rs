//! 항상-적용 정책 변경 엔드포인트.
//!
//! Desktop의 Tauri command와 **같은 도메인 함수**(`memory::application_policy::set_policy`)를
//! 부른다. 판정을 여기에 복제하면 한쪽 경로에서만 지정되는 상태가 생긴다.

use axum::extract::{Path as AxumPath, State};
use axum::http::StatusCode;
use axum::routing::put;
use axum::{Json, Router};
use serde::Deserialize;

use super::{internal, now, scope, ApiError, RunnerHttpState};
use crate::memory::application_policy::{self, PolicyFailure};

#[derive(Deserialize)]
struct ApplicationPolicyRequest {
    policy: String,
    /// CAS 기대값 — 본문이 바뀌었으면 승인 대상이 달라진 것이다.
    expected_version: i64,
    expected_policy: String,
}

pub(super) fn routes() -> Router<RunnerHttpState> {
    Router::new().route("/v1/memories/:id/application-policy", put(set))
}

/// 반환값은 "실제로 바뀌었는가". `false`는 이미 목표 상태였다는 뜻이라
/// 응답을 잃은 클라이언트가 재시도해도 안전하다.
async fn set(
    State(state): State<RunnerHttpState>,
    AxumPath(id): AxumPath<i64>,
    Json(request): Json<ApplicationPolicyRequest>,
) -> Result<Json<bool>, ApiError> {
    scope::authorize_memory(&state, id).await?;
    application_policy::set_policy(
        &state.pool,
        id,
        &request.policy,
        request.expected_version,
        &request.expected_policy,
        now(),
    )
    .await
    .map(Json)
    .map_err(policy_api_error)
}

fn policy_api_error(failure: PolicyFailure) -> ApiError {
    match failure {
        PolicyFailure::NotFound => (StatusCode::NOT_FOUND, failure.to_string()),
        PolicyFailure::Conflict => (StatusCode::CONFLICT, failure.to_string()),
        // 저장소 오류 원문(경로·SQL)은 호출자에게 내보내지 않는다.
        PolicyFailure::Storage => internal("메모리 적용 정책 저장소 오류"),
        other => (StatusCode::BAD_REQUEST, other.to_string()),
    }
}
