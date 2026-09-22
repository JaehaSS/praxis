#[path = "support/temp_root.rs"]
mod temp_root;

#[path = "support/runner_memory_atomic_approval.rs"]
mod runner_support;

use axum::http::StatusCode;
use runner_support::{
    request, request_with_auth, response_json, response_text, RunnerApprovalFixture,
};
use tower::ServiceExt;

#[tokio::test]
async fn runner_atomic_approval_is_idempotent_and_version_guarded() {
    let fixture = RunnerApprovalFixture::new().await;

    let first = response_json(
        fixture
            .app
            .clone()
            .oneshot(request(fixture.memory_id, 1))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(first["version"], 1);
    assert_eq!(first["already_approved"], false);

    let retry = response_json(
        fixture
            .app
            .clone()
            .oneshot(request(fixture.memory_id, 1))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(retry["receipt_id"], first["receipt_id"]);
    assert_eq!(retry["already_approved"], true);

    let conflict = fixture
        .app
        .clone()
        .oneshot(request(fixture.memory_id, 2))
        .await
        .unwrap();
    assert_eq!(conflict.status(), StatusCode::CONFLICT);

    fixture.cleanup().await;
}

#[tokio::test]
async fn runner_atomic_approval_enforces_auth_scope_and_not_found() {
    let fixture = RunnerApprovalFixture::new().await;
    let unauthenticated = fixture
        .app
        .clone()
        .oneshot(request_with_auth(fixture.memory_id, 1, false))
        .await
        .unwrap();
    assert_eq!(unauthenticated.status(), StatusCode::UNAUTHORIZED);

    fixture.move_out_of_scope().await;
    let outside = fixture
        .app
        .clone()
        .oneshot(request(fixture.memory_id, 1))
        .await
        .unwrap();
    assert_eq!(outside.status(), StatusCode::BAD_REQUEST);

    let missing = fixture
        .app
        .clone()
        .oneshot(request(fixture.memory_id + 999, 1))
        .await
        .unwrap();
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    fixture.cleanup().await;
}

#[tokio::test]
async fn runner_atomic_approval_maps_invalid_evidence_to_bad_request() {
    let fixture = RunnerApprovalFixture::new().await;
    fixture.add_expired_confirmation().await;
    let response = fixture
        .app
        .clone()
        .oneshot(request(fixture.memory_id, 1))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    fixture.cleanup().await;
}

#[tokio::test]
async fn runner_atomic_approval_redacts_scope_storage_errors() {
    let fixture = RunnerApprovalFixture::new().await;
    fixture.break_memory_lookup().await;
    let response = fixture
        .app
        .clone()
        .oneshot(request(fixture.memory_id, 1))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response_text(response).await,
        "메모리 범위 조회 저장소 오류"
    );
    fixture.cleanup().await;
}
