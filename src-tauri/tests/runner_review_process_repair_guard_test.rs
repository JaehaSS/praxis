#[path = "support/temp_root.rs"]
mod temp_root;

use axum::http::{Method, StatusCode};
use praxis_lib::db;
use praxis_lib::review_ops::ReviewClaims;
use praxis_lib::runner::auth::RunnerAuth;
use praxis_lib::runner::config::{ExecutionPolicy, RunnerConfig};
use praxis_lib::runner::events::EventHub;
use praxis_lib::runner::http::{self, RunnerHttpState};
use praxis_lib::runner::queue::QueueWorker;
use praxis_lib::runner::review_process::{self, ReviewOperation, ReviewPhase};
use tower::ServiceExt;

#[allow(dead_code)]
#[path = "support/runner_review_paths.rs"]
mod paths;

const TOKEN: &str = "abababababababababababababababababababababababababababababababab";

#[tokio::test]
async fn active_receipt_returns_conflict_without_changing_the_lease() {
    let (state, task_id, receipt_id) = state_with_receipt("active").await;
    let app = http::router(state.clone());

    let response = repair_request(app, receipt_id).await;

    assert_eq!(response, StatusCode::CONFLICT);
    assert_eq!(
        review_process::lease(&state.pool, task_id, ReviewOperation::Verify)
            .await
            .unwrap()
            .unwrap()
            .receipt_id,
        receipt_id
    );
}

#[tokio::test]
async fn active_review_claim_blocks_operator_reconciliation_before_os_work() {
    let (state, task_id, receipt_id) = state_with_receipt("claimed").await;
    review_process::quarantine(
        &state.pool,
        task_id,
        ReviewOperation::Verify,
        receipt_id,
        "uncertain",
        3,
    )
    .await
    .unwrap();
    let _claim = state.review_claims.claim_verify(task_id).unwrap();
    let app = http::router(state.clone());

    assert_eq!(repair_request(app, receipt_id).await, StatusCode::CONFLICT);
    assert!(
        review_process::lease(&state.pool, task_id, ReviewOperation::Verify)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn deleted_task_receipt_returns_gone_without_attempting_repair() {
    let (state, task_id, receipt_id) = state_with_receipt("deleted").await;
    sqlx::query("DELETE FROM review_process_leases WHERE receipt_id = ?")
        .bind(receipt_id)
        .execute(&state.pool)
        .await
        .unwrap();
    db::delete_task(&state.pool, task_id).await.unwrap();
    let app = http::router(state);

    assert_eq!(repair_request(app, receipt_id).await, StatusCode::GONE);
}

#[tokio::test]
async fn missing_worktree_can_reconcile_after_authorizing_the_durable_repo() {
    let (state, task_id, receipt_id) = state_with_receipt("missing-worktree").await;
    let missing = state.config.repository_roots[0].join("already-removed-worktree");
    sqlx::query("UPDATE tasks SET worktree_path = ? WHERE id = ?")
        .bind(missing.to_string_lossy().as_ref())
        .bind(task_id)
        .execute(&state.pool)
        .await
        .unwrap();
    review_process::quarantine(
        &state.pool,
        task_id,
        ReviewOperation::Verify,
        receipt_id,
        "uncertain",
        3,
    )
    .await
    .unwrap();
    let app = http::router(state.clone());

    assert_eq!(repair_request(app, receipt_id).await, StatusCode::OK);
    assert!(
        review_process::lease(&state.pool, task_id, ReviewOperation::Verify)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn existing_worktree_outside_configured_roots_is_forbidden() {
    let (state, task_id, receipt_id) = state_with_receipt("outside-worktree").await;
    let outside = paths::temporary_dir("outside-root").canonicalize().unwrap();
    sqlx::query("UPDATE tasks SET worktree_path = ? WHERE id = ?")
        .bind(outside.to_string_lossy().as_ref())
        .bind(task_id)
        .execute(&state.pool)
        .await
        .unwrap();
    let app = http::router(state);

    assert_eq!(repair_request(app, receipt_id).await, StatusCode::FORBIDDEN);
}

async fn repair_request(app: axum::Router, receipt_id: i64) -> StatusCode {
    app.oneshot(paths::request(
        TOKEN,
        Method::POST,
        &format!("/v1/review-processes/{receipt_id}/reconcile"),
        None,
        true,
    ))
    .await
    .unwrap()
    .status()
}

async fn state_with_receipt(label: &str) -> (RunnerHttpState, i64, i64) {
    let root = paths::temporary_dir(label).canonicalize().unwrap();
    let pool = db::init_pool(&root.join("test.sqlite").to_string_lossy())
        .await
        .unwrap();
    review_process::migrate(&pool).await.unwrap();
    let root_text = root.to_string_lossy();
    let task_id = db::insert_task(
        &pool,
        &root_text,
        "branch",
        "main",
        &root_text,
        "repair guard",
        None,
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();
    let receipt = review_process::register(
        &pool,
        task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        999_997,
        &"c".repeat(64),
        2,
    )
    .await
    .unwrap();
    let token_path = paths::token_file(TOKEN);
    let claims = ReviewClaims::default();
    let state = RunnerHttpState {
        auth: RunnerAuth::from_file(&token_path).unwrap(),
        events: EventHub::start(pool.clone()),
        queue: QueueWorker::new(pool.clone(), 1),
        pool,
        config: RunnerConfig {
            bind: "127.0.0.1:47831".parse().unwrap(),
            repository_roots: vec![root],
            max_concurrent_tasks: 1,
            execution_policy: ExecutionPolicy::AlwaysApprove,
            pairing_token_file: token_path,
        },
        recovered_tasks: 0,
        started_at: 0,
        review_claims: claims,
    };
    (state, task_id, receipt.id)
}
