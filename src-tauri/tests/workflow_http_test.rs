#[path = "support/temp_root.rs"]
mod temp_root;
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{Request, StatusCode},
    Router,
};
use praxis_lib::{
    db,
    runner::{
        auth::RunnerAuth,
        config::{ExecutionPolicy, RunnerConfig},
        events::EventHub,
        http::{self, RunnerHttpState},
        queue::QueueWorker,
        workflow::WorkflowService,
    },
    workflow::{query::RuntimeAdmission, store::WorkflowStore, WorkflowSpec},
};
use serde_json::{json, Value};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tower::ServiceExt;
const TOKEN: &str = "abababababababababababababababababababababababababababababababab";

async fn request(
    router: &Router,
    method: &str,
    path: &str,
    body: Value,
    auth: bool,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
    if auth {
        builder = builder.header("authorization", format!("Bearer {TOKEN}"));
    }
    let response = router
        .clone()
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 400_000).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn setup(
    enabled: bool,
) -> (
    Router,
    Option<Arc<WorkflowService>>,
    sqlx::SqlitePool,
    PathBuf,
    WorkflowSpec,
) {
    let root = temp_root::dir().join(format!("workflow-http-{}-{enabled}", std::process::id()));
    std::fs::create_dir_all(root.join("repo")).unwrap();
    let token = root.join("token");
    std::fs::write(&token, TOKEN).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let db_path = root.join("runner.sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    let config = RunnerConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        repository_roots: vec![root.join("repo").canonicalize().unwrap()],
        max_concurrent_tasks: 2,
        execution_policy: ExecutionPolicy::RequireApproval,
        pairing_token_file: token.clone(),
    };
    let queue = QueueWorker::new(pool.clone(), 2);
    let mut spec: Value =
        serde_json::from_str(include_str!("fixtures/workflow-spec-v1.json")).unwrap();
    for task in spec["tasks"].as_array_mut().unwrap() {
        task["resource_requests_by_step"] = json!({});
        task["manual_acceptance"] = json!([]);
        if task["kind"] == "agent" {
            task["kind"] = json!("command");
            task["command_profile_id"] = json!("ui-build-v1");
        }
    }
    let spec = WorkflowSpec::parse_json(&spec.to_string()).unwrap();
    let profile = json!({"schema_version":1,"id":"podman-rootless-v1","image":format!("registry.example/workflow@sha256:{}","a".repeat(64)),"podman_executable":"/nonexistent-praxis-podman","cpu_limit":"1","memory_limit":"512m","pids_limit":64,"timeouts":{"probe_ms":1000,"create_ms":1000,"start_ms":1000,"inspect_ms":1000,"stop_ms":1000,"remove_ms":1000,"logs_ms":1000}});
    let mut commands = serde_json::Map::new();
    for id in ["cargo-test-v1", "vitest-v1", "ui-build-v1"] {
        commands.insert(id.into(),json!({"id":id,"runtime_profile_id":"podman-rootless-v1","vendor_id":null,"executable":"/bin/true","argv":[],"env":{}}));
    }
    let cfg = json!({"schema_version":1,"workspace_root":root.join("workflow"),"repositories":{spec.project_ref.clone():root.join("repo")},"profiles":{"podman-rootless-v1":profile},"commands":commands,"task_timeout_secs":30,"verifier_executable":"/usr/bin/python3"});
    let cfg_path = root.join("workflow.json");
    std::fs::write(&cfg_path, cfg.to_string()).unwrap();
    let service = if enabled {
        Some(
            WorkflowService::open(&cfg_path, &config, &db_path, queue.capacity())
                .await
                .unwrap(),
        )
    } else {
        None
    };
    let state = RunnerHttpState {
        auth: RunnerAuth::from_file(&token).unwrap(),
        pool: pool.clone(),
        config,
        recovered_tasks: 0,
        events: EventHub::start(pool.clone()),
        queue,
        started_at: 0,
        review_claims: Default::default(),
    };
    (
        http::router_with_workflow(state, service.clone()),
        service,
        pool,
        root,
        spec,
    )
}
#[tokio::test]
async fn workflow_routes_use_existing_auth_and_explicit_feature_gate() {
    let (router, _, pool, root, _) = setup(false).await;
    assert_eq!(
        request(&router, "GET", "/v1/workflows", Value::Null, false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, body) = request(&router, "GET", "/v1/workflows", Value::Null, true).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(body["code"], "capability_unavailable");
    drop(router);
    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn real_routes_persist_drafts_replay_requests_and_block_unverified_runtime() {
    let (router, service, pool, root, spec) = setup(true).await;
    let body = json!({"request_id":"create-1","spec":spec});
    let (status, receipt) = request(&router, "POST", "/v1/workflows", body.clone(), true).await;
    assert_eq!(status, StatusCode::OK, "{receipt}");
    let id = receipt["workflow_id"].as_str().unwrap();
    assert_eq!(
        request(&router, "POST", "/v1/workflows", body.clone(), true)
            .await
            .1,
        receipt
    );
    let mut different = body;
    different["spec"]["tasks"][0]["objective"] = json!("Different request payload");
    assert_eq!(
        request(&router, "POST", "/v1/workflows", different, true)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let snapshot = request(
        &router,
        "GET",
        &format!("/v1/workflows/{id}"),
        Value::Null,
        true,
    )
    .await
    .1;
    assert_eq!(snapshot["state"], "draft");
    assert_eq!(snapshot["nodes"].as_array().unwrap().len(), 3);
    let invalid = json!({"request_id":"validate-1","expected_revision":1});
    let (status, error) = request(
        &router,
        "POST",
        &format!("/v1/workflows/{id}/validate"),
        invalid,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{error}");
    assert_eq!(error["code"], "capability_unavailable");
    assert_eq!(
        service.as_ref().unwrap().store.run(id).await.unwrap().state,
        "draft"
    );
    let cancel = json!({"request_id":"cancel-1","expected_revision":1});
    let result = request(
        &router,
        "POST",
        &format!("/v1/workflows/{id}/cancel"),
        cancel.clone(),
        true,
    )
    .await;
    assert_eq!(result.0, StatusCode::OK, "{}", result.1);
    assert_eq!(
        request(
            &router,
            "POST",
            &format!("/v1/workflows/{id}/cancel"),
            cancel,
            true
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        service.as_ref().unwrap().store.run(id).await.unwrap().state,
        "cancelled"
    );
    assert_eq!(
        request(
            &router,
            "GET",
            &format!("/v1/workflows/{id}/artifacts/{}", "a".repeat(64)),
            Value::Null,
            true
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let artifacts = request(
        &router,
        "GET",
        &format!("/v1/workflows/{id}/artifacts"),
        Value::Null,
        true,
    )
    .await;
    assert_eq!(artifacts.0, StatusCode::OK);
    assert_eq!(artifacts.1, json!([]));
    let log_digest = praxis_lib::runner::workflow::logs::publish(
        &service.as_ref().unwrap().config.workspace_root,
        b"private log",
    )
    .unwrap();
    assert_eq!(
        request(
            &router,
            "GET",
            &format!("/v1/workflows/{id}/logs/{log_digest}"),
            Value::Null,
            true
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &router,
            "GET",
            &format!("/v1/workflows/{id}/logs/{log_digest}"),
            Value::Null,
            false
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    service.unwrap().store.close().await;
    drop(router);
    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
#[tokio::test]
async fn runtime_approval_binds_config_input_revision_and_request_identity() {
    let root = temp_root::dir().join(format!("workflow-approval-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let store = WorkflowStore::open(&root.join("db")).await.unwrap();
    store.ensure_admission_schema().await.unwrap();
    let spec = WorkflowSpec::parse_json(include_str!("fixtures/workflow-spec-v1.json")).unwrap();
    store.create_run("run", "create", &spec, 1).await.unwrap();
    let admission = RuntimeAdmission {
        config_hash: "a".repeat(64),
        base_input_hash: "b".repeat(64),
        scope_hash: "c".repeat(64),
    };
    store
        .record_admission("run", 1, "validate", &admission, 2)
        .await
        .unwrap();
    assert!(store
        .start_admitted("run", 1, "start", &admission.scope_hash, "other", false, 3)
        .await
        .is_err());
    assert!(store
        .start_admitted(
            "run",
            2,
            "start",
            &admission.scope_hash,
            &admission.config_hash,
            false,
            3
        )
        .await
        .is_err());
    assert_eq!(store.run("run").await.unwrap().state, "draft");
    store
        .start_admitted(
            "run",
            1,
            "start",
            &admission.scope_hash,
            &admission.config_hash,
            false,
            3,
        )
        .await
        .unwrap();
    assert!(
        store
            .start_admitted(
                "run",
                1,
                "start",
                &admission.scope_hash,
                &admission.config_hash,
                false,
                4
            )
            .await
            .unwrap()
            .replayed
    );
    assert!(store
        .start_admitted(
            "run",
            1,
            "start",
            "different",
            &admission.config_hash,
            false,
            4
        )
        .await
        .is_err());
    assert_eq!(
        store.authorized_runtime_scope("run", 1).await.unwrap(),
        Some(admission.scope_hash.clone())
    );
    store.pause("run", 1, "pause", 5).await.unwrap();
    store.cancel_intent("run", 1, "cancel", 6).await.unwrap();
    assert!(store
        .start_admitted(
            "run",
            1,
            "resume",
            &admission.scope_hash,
            &admission.config_hash,
            true,
            7
        )
        .await
        .is_err());
    store.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
