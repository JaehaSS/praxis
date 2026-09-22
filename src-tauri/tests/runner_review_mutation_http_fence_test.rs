#[path = "support/temp_root.rs"]
mod temp_root;

use std::net::SocketAddr;

use axum::body::Body;
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode};
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

const TOKEN: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

#[tokio::test]
async fn durable_quarantine_blocks_every_runner_worktree_mutation_route() {
    let repository = paths::temporary_dir("mutation-http");
    let worktree = repository.join("managed-worktree");
    std::fs::create_dir_all(&worktree).unwrap();
    let pool = db::init_pool(&repository.join("test.sqlite").to_string_lossy())
        .await
        .unwrap();
    let task_id = db::insert_task(
        &pool,
        &repository.to_string_lossy(),
        "branch",
        "main",
        &worktree.to_string_lossy(),
        "fenced mutation",
        None,
        Some("fenced"),
        "terminal",
        1,
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, db::state::AWAITING_REVIEW, 2)
        .await
        .unwrap();
    let receipt = review_process::register(
        &pool,
        task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        999_992,
        &"b".repeat(64),
        3,
    )
    .await
    .unwrap();
    review_process::quarantine(
        &pool,
        task_id,
        ReviewOperation::Verify,
        receipt.id,
        "unresolved child",
        4,
    )
    .await
    .unwrap();
    let app = http::router(state(pool, &repository));

    let cases = [
        (
            Method::POST,
            format!("/v1/tasks/{task_id}/partial/apply"),
            r#"{"hunk_ids":[]}"#.to_string(),
        ),
        (
            Method::POST,
            format!("/v1/tasks/{task_id}/partial/rollback"),
            String::new(),
        ),
        (
            Method::POST,
            "/v1/ensembles/fenced/compose".to_string(),
            format!(r#"{{"winner_task_id":{task_id},"selections":[]}}"#),
        ),
        (
            Method::PUT,
            "/v1/files/write".to_string(),
            format!(
                r#"{{"repository":"{}","path":"managed-worktree/blocked.txt","content":"blocked"}}"#,
                repository.to_string_lossy()
            ),
        ),
    ];
    for (method, uri, body) in cases {
        let response = app
            .clone()
            .oneshot(authorized_request(method, &uri, body))
            .await
            .unwrap();
        assert!(
            matches!(
                response.status(),
                StatusCode::BAD_REQUEST | StatusCode::CONFLICT
            ),
            "unexpected mutation response for {uri}: {}",
            response.status()
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        if uri == "/v1/files/write" {
            assert_eq!(text, "유효하지 않은 파일 경로입니다");
        } else {
            assert!(
                text.contains("durable review process lease"),
                "missing durable fence for {uri}: {text}"
            );
        }
    }
    assert!(!worktree.join("blocked.txt").exists());
}

fn authorized_request(method: Method, uri: &str, body: String) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Bearer {TOKEN}"))
        .header("Content-Type", "application/json")
        .body(Body::from(body))
        .unwrap();
    request.extensions_mut().insert(ConnectInfo(
        "127.0.0.1:43123".parse::<SocketAddr>().unwrap(),
    ));
    request
}

fn state(pool: sqlx::SqlitePool, repository: &std::path::Path) -> RunnerHttpState {
    let token_path = paths::token_file(TOKEN);
    RunnerHttpState {
        auth: RunnerAuth::from_file(&token_path).unwrap(),
        events: EventHub::start(pool.clone()),
        queue: QueueWorker::new(pool.clone(), 1),
        pool,
        config: RunnerConfig {
            bind: "127.0.0.1:47831".parse().unwrap(),
            repository_roots: vec![repository.canonicalize().unwrap()],
            max_concurrent_tasks: 1,
            execution_policy: ExecutionPolicy::AlwaysApprove,
            pairing_token_file: token_path,
        },
        recovered_tasks: 0,
        started_at: 0,
        review_claims: ReviewClaims::default(),
    }
}
