#![cfg(unix)]

#[path = "support/temp_root.rs"]
mod temp_root;

use std::os::unix::process::CommandExt;

use axum::http::{Method, StatusCode};
use praxis_lib::db;
use praxis_lib::runner::auth::RunnerAuth;
use praxis_lib::runner::config::{ExecutionPolicy, RunnerConfig};
use praxis_lib::runner::events::EventHub;
use praxis_lib::runner::http::{self, RunnerHttpState};
use praxis_lib::runner::queue::QueueWorker;
use praxis_lib::runner::review_process::{self, ReviewOperation, ReviewPhase, ReviewProcessState};
use tower::ServiceExt;

#[allow(dead_code)]
#[path = "support/runner_review_paths.rs"]
mod paths;

const TOKEN: &str = "abababababababababababababababababababababababababababababababab";

#[tokio::test]
async fn quarantine_routes_require_auth_and_withhold_private_receipt_fields() {
    let (state, _, receipt_id) = quarantined_state("list", 999_999, &"a".repeat(64)).await;
    let app = http::router(state);
    let unauthorized = app
        .clone()
        .oneshot(paths::request(
            TOKEN,
            Method::GET,
            "/v1/review-processes/quarantined",
            None,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    let unauthorized_repair = app
        .clone()
        .oneshot(paths::request(
            TOKEN,
            Method::POST,
            &format!("/v1/review-processes/{receipt_id}/reconcile"),
            None,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(unauthorized_repair.status(), StatusCode::UNAUTHORIZED);

    let response = app
        .oneshot(paths::request(
            TOKEN,
            Method::GET,
            "/v1/review-processes/quarantined",
            None,
            true,
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = paths::json_body(response).await;
    assert_eq!(body[0]["receipt_id"], receipt_id);
    assert_eq!(body[0]["pgid"], 999_999);
    assert_eq!(body[0]["state"], "quarantined");
    let encoded = body.to_string();
    assert!(!encoded.contains("identity_hash"));
    assert!(!encoded.contains(&"a".repeat(64)));
    assert!(!encoded.contains("raw quarantine detail"));
}

#[tokio::test]
async fn absent_process_repair_releases_the_fence_through_the_http_contract() {
    let (state, task_id, receipt_id) = quarantined_state("absent", 999_998, &"b".repeat(64)).await;
    let app = http::router(state.clone());

    let response = app
        .oneshot(paths::request(
            TOKEN,
            Method::POST,
            &format!("/v1/review-processes/{receipt_id}/reconcile"),
            None,
            true,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = paths::json_body(response).await;
    assert_eq!(body["status"], "resolved_absent");
    assert!(
        review_process::lease(&state.pool, task_id, ReviewOperation::Verify)
            .await
            .unwrap()
            .is_none()
    );
    review_process::assert_task_unfenced(&state.pool, task_id)
        .await
        .unwrap();
}

#[tokio::test]
async fn identity_mismatch_remains_quarantined_without_killing_the_process() {
    let mut command = std::process::Command::new("/bin/sleep");
    command.arg("30").process_group(0);
    let mut unrelated = command.spawn().unwrap();
    let (state, task_id, receipt_id) =
        quarantined_state("mismatch", unrelated.id() as i64, &"0".repeat(64)).await;
    let app = http::router(state.clone());

    let response = app
        .oneshot(paths::request(
            TOKEN,
            Method::POST,
            &format!("/v1/review-processes/{receipt_id}/reconcile"),
            None,
            true,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = paths::json_body(response).await;
    assert_eq!(body["status"], "still_quarantined");
    assert_eq!(body["reason"], "identity_mismatch");
    assert!(unrelated.try_wait().unwrap().is_none());
    let lease = review_process::lease(&state.pool, task_id, ReviewOperation::Verify)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.state, ReviewProcessState::Quarantined);
    unrelated.kill().unwrap();
    let _ = unrelated.wait();
}

async fn quarantined_state(
    label: &str,
    pgid: i64,
    identity_hash: &str,
) -> (RunnerHttpState, i64, i64) {
    let root = paths::temporary_dir(label).canonicalize().unwrap();
    let pool = db::init_pool(&root.join("test.sqlite").to_string_lossy())
        .await
        .unwrap();
    review_process::migrate(&pool).await.unwrap();
    let root_text = root.to_string_lossy();
    let task_id = db::insert_task(
        &pool, &root_text, "branch", "main", &root_text, "repair", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    let receipt = review_process::register(
        &pool,
        task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        pgid,
        identity_hash,
        2,
    )
    .await
    .unwrap();
    review_process::quarantine(
        &pool,
        task_id,
        ReviewOperation::Verify,
        receipt.id,
        "raw quarantine detail",
        3,
    )
    .await
    .unwrap();
    let token_path = paths::token_file(TOKEN);
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
        review_claims: Default::default(),
        started_at: 0,
    };
    (state, task_id, receipt.id)
}
