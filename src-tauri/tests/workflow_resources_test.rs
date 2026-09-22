#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::workflow::resources::{ClaimResult, ResourceDefinition, StepLease};
use praxis_lib::workflow::store::WorkflowStore;
use praxis_lib::workflow::{AccessMode, ResourceRequest, StepKind, WorkflowSpec};

async fn setup(name: &str) -> (WorkflowStore, std::path::PathBuf) {
    let dir = temp_root::dir().join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let store = WorkflowStore::open(&dir.join("workflow.sqlite"))
        .await
        .unwrap();
    (store, dir)
}

fn resource(id: &str, capacity: i64) -> ResourceDefinition {
    ResourceDefinition {
        id: id.into(),
        physical_identity: format!("fixture:{id}"),
        capacity,
        repository: None,
        path_prefix: None,
    }
}

async fn start(store: &WorkflowStore, id: &str, requests: Vec<ResourceRequest>, write_path: &str) {
    let mut spec =
        WorkflowSpec::parse_json(include_str!("fixtures/workflow-spec-v1.json")).unwrap();
    spec.tasks[0].write_paths = vec![write_path.into()];
    spec.tasks[0]
        .resource_requests_by_step
        .insert(StepKind::Execute, requests);
    store.create_run(id, "create", &spec, 1).await.unwrap();
    store
        .authorize_start(id, 1, &spec.digest().unwrap(), "start", 2)
        .await
        .unwrap();
}

fn request(id: &str, mode: AccessMode) -> ResourceRequest {
    ResourceRequest {
        resource_id: id.into(),
        mode,
        units: 1,
    }
}
fn claimed(result: ClaimResult) -> StepLease {
    match result {
        ClaimResult::Claimed(lease) => lease,
        other => panic!("expected claim, got {other:?}"),
    }
}

#[tokio::test]
async fn code_identity_cannot_be_registered_as_a_generic_resource() {
    let (store, dir) = setup("resource-code-identity").await;
    let malformed = ResourceDefinition {
        id: "generic".into(),
        physical_identity: "repo:github.com/example/praxis:path:src".into(),
        capacity: 1,
        repository: None,
        path_prefix: None,
    };
    assert!(store
        .register_resource(&malformed)
        .await
        .unwrap_err()
        .to_string()
        .contains("metadata"));
    // Also fail closed if a malformed row came from a previous/corrupt writer.
    let raw = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new().filename(dir.join("workflow.sqlite")),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO workflow_resources(id,physical_identity,capacity) VALUES(?,?,1)")
        .bind(&malformed.id)
        .bind(&malformed.physical_identity)
        .execute(&raw)
        .await
        .unwrap();
    start(&store, "parent", vec![], "src").await;
    assert!(store
        .claim_next_step("parent", 1, "api", 3, 10)
        .await
        .unwrap_err()
        .to_string()
        .contains("metadata"));
    assert_eq!(store.claim_count().await.unwrap(), 0);
    raw.close().await;
    store.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn competing_ready_nodes_respect_the_run_attempt_capacity() {
    let (store, dir) = setup("resource-run-capacity").await;
    let mut spec =
        WorkflowSpec::parse_json(include_str!("fixtures/workflow-spec-v1.json")).unwrap();
    spec.tasks[0].resource_requests_by_step.clear();
    spec.limits.max_concurrent_tasks = 1;
    store.create_run("run", "create", &spec, 1).await.unwrap();
    store
        .authorize_start("run", 1, &spec.digest().unwrap(), "start", 2)
        .await
        .unwrap();
    let (api, ui) = tokio::join!(
        store.claim_next_step("run", 1, "api", 3, 10),
        store.claim_next_step("run", 1, "ui", 3, 10)
    );
    let results = [api.unwrap(), ui.unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, ClaimResult::Claimed(_)))
            .count(),
        1
    );
    assert!(results
        .iter()
        .any(|r| matches!(r,ClaimResult::Waiting { reason,.. } if reason=="capacity")));
    assert_eq!(
        store
            .nodes("run")
            .await
            .unwrap()
            .iter()
            .map(|n| n.attempt_count)
            .sum::<i64>(),
        1
    );
    store.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn competing_connections_never_claim_the_same_writer_or_node_twice() {
    let (store, dir) = setup("resource-race").await;
    store
        .register_resource(&resource("shared-db", 1))
        .await
        .unwrap();
    start(
        &store,
        "a",
        vec![request("shared-db", AccessMode::ExclusiveWrite)],
        "src/a",
    )
    .await;
    start(
        &store,
        "b",
        vec![request("shared-db", AccessMode::ExclusiveWrite)],
        "src/b",
    )
    .await;
    let other = WorkflowStore::open(&dir.join("workflow.sqlite"))
        .await
        .unwrap();
    let (a, b) = tokio::join!(
        store.claim_next_step("a", 1, "api", 3, 10),
        other.claim_next_step("b", 1, "api", 3, 10)
    );
    let results = [a.unwrap(), b.unwrap()];
    assert_eq!(
        results
            .iter()
            .filter(|r| matches!(r, ClaimResult::Claimed(_)))
            .count(),
        1
    );
    assert_eq!(store.claim_count().await.unwrap(), 2); // shared DB and implicit code scope
    let winner = results
        .into_iter()
        .find_map(|r| {
            if let ClaimResult::Claimed(l) = r {
                Some(l)
            } else {
                None
            }
        })
        .unwrap();
    assert!(matches!(
        store
            .claim_next_step(&winner.run_id, 1, "api", 4, 10)
            .await
            .unwrap(),
        ClaimResult::Waiting { .. }
    ));
    assert_eq!(
        store
            .nodes(&winner.run_id)
            .await
            .unwrap()
            .iter()
            .find(|n| n.node_id == "api")
            .unwrap()
            .attempt_count,
        1
    );
    other.close().await;
    store.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn readers_share_but_writer_and_expired_quarantined_claims_block() {
    let (store, dir) = setup("resource-readers").await;
    store
        .register_resource(&resource("shared-db", 1))
        .await
        .unwrap();
    for id in ["a", "b"] {
        start(
            &store,
            id,
            vec![request("shared-db", AccessMode::SharedRead)],
            &format!("src/{id}"),
        )
        .await;
    }
    start(
        &store,
        "writer",
        vec![request("shared-db", AccessMode::ExclusiveWrite)],
        "src/w",
    )
    .await;
    let a = claimed(store.claim_next_step("a", 1, "api", 3, 1).await.unwrap());
    claimed(store.claim_next_step("b", 1, "api", 3, 1).await.unwrap());
    assert!(
        matches!(store.claim_next_step("writer",1,"api",10000,1).await.unwrap(),ClaimResult::Waiting { reason,.. } if reason=="resource_busy")
    );
    store.fence_recovery("a", 10001).await.unwrap();
    assert!(store.assert_lease_current(&a).await.is_err());
    assert_eq!(store.run("a").await.unwrap().state, "quarantined");
    assert!(matches!(
        store
            .claim_next_step("writer", 1, "api", 10002, 1)
            .await
            .unwrap(),
        ClaimResult::Waiting { .. }
    ));
    assert_eq!(store.claim_count().await.unwrap(), 4);
    store.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn failed_multi_resource_claim_leaves_no_partial_reservation() {
    let (store, dir) = setup("resource-atomic").await;
    for id in ["a", "b"] {
        store.register_resource(&resource(id, 1)).await.unwrap();
    }
    start(
        &store,
        "holder",
        vec![request("b", AccessMode::ExclusiveWrite)],
        "src/holder",
    )
    .await;
    start(
        &store,
        "waiter",
        vec![
            request("a", AccessMode::ExclusiveWrite),
            request("b", AccessMode::ExclusiveWrite),
        ],
        "src/waiter",
    )
    .await;
    start(
        &store,
        "free",
        vec![request("a", AccessMode::ExclusiveWrite)],
        "src/free",
    )
    .await;
    claimed(
        store
            .claim_next_step("holder", 1, "api", 3, 10)
            .await
            .unwrap(),
    );
    assert!(matches!(
        store
            .claim_next_step("waiter", 1, "api", 3, 10)
            .await
            .unwrap(),
        ClaimResult::Waiting { .. }
    ));
    assert_eq!(store.claim_count().await.unwrap(), 2);
    assert_eq!(
        store
            .nodes("waiter")
            .await
            .unwrap()
            .iter()
            .find(|n| n.node_id == "api")
            .unwrap()
            .attempt_count,
        0
    );
    claimed(
        store
            .claim_next_step("free", 1, "api", 3, 10)
            .await
            .unwrap(),
    );
    store.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn implicit_parent_child_write_scopes_conflict_without_explicit_resources() {
    let (store, dir) = setup("resource-prefix").await;
    start(&store, "parent", vec![], "src/domain").await;
    start(&store, "child", vec![], "src/domain/model.rs").await;
    start(&store, "sibling", vec![], "src/domain-other").await;
    claimed(
        store
            .claim_next_step("parent", 1, "api", 3, 10)
            .await
            .unwrap(),
    );
    assert!(
        matches!(store.claim_next_step("child",1,"api",3,10).await.unwrap(),ClaimResult::Waiting { reason,.. } if reason=="resource_busy")
    );
    claimed(
        store
            .claim_next_step("sibling", 1, "api", 3, 10)
            .await
            .unwrap(),
    );
    store.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn alias_registration_and_overcapacity_do_not_bypass_claims() {
    let (store, dir) = setup("resource-capacity").await;
    store.register_resource(&resource("cpu", 2)).await.unwrap();
    let mut alias = resource("cpu-alias", 2);
    alias.physical_identity = "fixture:cpu".into();
    assert!(store.register_resource(&alias).await.is_err());
    for id in ["a", "b", "c"] {
        start(
            &store,
            id,
            vec![request("cpu", AccessMode::Capacity)],
            &format!("src/{id}"),
        )
        .await;
    }
    claimed(store.claim_next_step("a", 1, "api", 3, 10).await.unwrap());
    claimed(store.claim_next_step("b", 1, "api", 3, 10).await.unwrap());
    assert!(matches!(
        store.claim_next_step("c", 1, "api", 3, 10).await.unwrap(),
        ClaimResult::Waiting { .. }
    ));
    store.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn missing_resources_and_unverified_dependencies_never_create_attempts() {
    let (store, dir) = setup("resource-admission").await;
    start(
        &store,
        "run",
        vec![request("missing-db", AccessMode::ExclusiveWrite)],
        "src/a",
    )
    .await;
    assert!(
        matches!(store.claim_next_step("run",1,"api",3,10).await.unwrap(),ClaimResult::Waiting { reason,.. } if reason=="resource_unavailable")
    );
    assert!(
        matches!(store.claim_next_step("run",1,"final-integration",3,10).await.unwrap(),ClaimResult::Waiting { reason,.. } if reason=="dependency_pending")
    );
    assert_eq!(store.claim_count().await.unwrap(), 0);
    assert!(store
        .nodes("run")
        .await
        .unwrap()
        .iter()
        .all(|n| n.attempt_count == 0));
    store.close().await;
    std::fs::remove_dir_all(dir).unwrap();
}
