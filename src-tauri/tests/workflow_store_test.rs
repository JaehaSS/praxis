#[path = "support/temp_root.rs"]
mod temp_root;

use std::path::PathBuf;

use praxis_lib::{
    db,
    workflow::{resources::ResourceDefinition, store::WorkflowStore, WorkflowSpec},
};

const FIXTURE: &str = include_str!("fixtures/workflow-spec-v1.json");

fn spec() -> WorkflowSpec {
    WorkflowSpec::parse_json(FIXTURE).unwrap()
}

fn revised_spec(mutator: impl FnOnce(&mut serde_json::Value)) -> WorkflowSpec {
    let mut value: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    mutator(&mut value);
    WorkflowSpec::parse_json(&serde_json::to_string(&value).unwrap()).unwrap()
}

async fn store(label: &str) -> (WorkflowStore, PathBuf) {
    let root = temp_root::dir().join(format!("workflow-store-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("workflow.sqlite");
    (WorkflowStore::open(&path).await.unwrap(), root)
}

async fn cleanup(store: WorkflowStore, root: PathBuf) {
    store.close().await;
    let _ = std::fs::remove_dir_all(root);
}

async fn create_and_start(store: &WorkflowStore, id: &str, now: i64) -> WorkflowSpec {
    let spec = spec();
    store.create_run(id, "create", &spec, now).await.unwrap();
    store
        .authorize_start(id, 1, &spec.digest().unwrap(), "start", now + 1)
        .await
        .unwrap();
    spec
}

#[tokio::test]
async fn workflow_pool_uses_full_and_foreign_keys_without_changing_legacy_normal() {
    let (store, root) = store("durability").await;
    assert_eq!(store.durability_settings().await.unwrap(), vec![(2, 1); 4]);

    let legacy_path = root.join("legacy.sqlite");
    let legacy = db::init_pool(legacy_path.to_str().unwrap()).await.unwrap();
    let normal: i64 = sqlx::query_scalar("PRAGMA synchronous")
        .fetch_one(&legacy)
        .await
        .unwrap();
    assert_eq!(
        normal, 1,
        "workflow FULL must not mutate the legacy pool policy"
    );
    legacy.close().await;
    cleanup(store, root).await;
}

#[tokio::test]
async fn creation_and_start_requests_replay_only_the_same_payload() {
    let (store, root) = store("receipts").await;
    let spec = spec();
    assert!(
        !store
            .create_run("run", "create", &spec, 10)
            .await
            .unwrap()
            .replayed
    );
    assert!(
        store
            .create_run("run", "create", &spec, 11)
            .await
            .unwrap()
            .replayed
    );

    let changed =
        revised_spec(|value| value["tasks"][0]["objective"] = serde_json::json!("Changed."));
    assert!(store
        .create_run("run", "create", &changed, 12)
        .await
        .is_err());

    let hash = spec.digest().unwrap();
    assert!(
        !store
            .authorize_start("run", 1, &hash, "start", 13)
            .await
            .unwrap()
            .replayed
    );
    assert!(
        store
            .authorize_start("run", 1, &hash, "start", 14)
            .await
            .unwrap()
            .replayed
    );
    assert!(store
        .authorize_start("run", 1, &"0".repeat(64), "start", 15)
        .await
        .is_err());
    cleanup(store, root).await;
}

#[tokio::test]
async fn changed_graph_revokes_authorization_until_exact_reauthorization_then_resume() {
    let (store, root) = store("authorization").await;
    let _ = create_and_start(&store, "run", 20).await;
    store.pause("run", 1, "pause", 22).await.unwrap();
    let revised = revised_spec(|value| {
        value["tasks"][2]["input_artifacts"] = serde_json::json!([
            { "task_id": "ui", "artifact": "delta" }
        ]);
        value["edges"] = serde_json::json!([
            { "from": "api", "to": "ui" },
            { "from": "ui", "to": "final-integration" }
        ]);
    });
    assert_eq!(
        store
            .apply_revision("run", 1, "revision", &revised, 23)
            .await
            .unwrap()
            .revision,
        2
    );
    assert!(store.run("run").await.unwrap().authorization_hash.is_none());
    assert!(store.resume("run", 2, "resume", 24).await.is_err());
    assert!(store
        .reauthorize_revision("run", 2, &"f".repeat(64), "reauthorize", 25)
        .await
        .is_err());
    store
        .reauthorize_revision("run", 2, &revised.digest().unwrap(), "reauthorize", 26)
        .await
        .unwrap();
    store.resume("run", 2, "resume", 27).await.unwrap();
    assert_eq!(store.run("run").await.unwrap().state, "running");
    cleanup(store, root).await;
}

#[tokio::test]
async fn display_only_revision_preserves_unattempted_readiness_and_approval() {
    let (store, root) = store("display").await;
    let old = create_and_start(&store, "run", 30).await;
    store.pause("run", 1, "pause", 32).await.unwrap();
    let revised =
        revised_spec(|value| value["phases"][0]["name"] = serde_json::json!("Implementation"));
    store
        .apply_revision("run", 1, "revision", &revised, 33)
        .await
        .unwrap();
    let run = store.run("run").await.unwrap();
    assert_eq!(run.active_revision, 2);
    let revised_digest = revised.digest().unwrap();
    assert_eq!(
        run.authorization_hash.as_deref(),
        Some(revised_digest.as_str())
    );
    let nodes = store.nodes("run").await.unwrap();
    assert_eq!(
        nodes
            .iter()
            .find(|node| node.node_id == "api")
            .unwrap()
            .state,
        "ready"
    );
    let applied = store.events_after("run", 0).await.unwrap();
    let event = applied
        .iter()
        .find(|event| event.kind == "revision_applied")
        .unwrap();
    assert!(event.detail.contains("\"impacted_nodes\":[]"));
    assert_ne!(old.digest().unwrap(), revised_digest);
    cleanup(store, root).await;
}

#[tokio::test]
async fn revision_rejects_accepted_output_until_evidence_rebinding_exists() {
    let (store, root) = store("accepted-result").await;
    create_and_start(&store, "run", 35).await;
    store.pause("run", 1, "pause", 37).await.unwrap();
    let options = sqlx::sqlite::SqliteConnectOptions::new()
        .filename(root.join("workflow.sqlite"))
        .foreign_keys(true);
    let raw = sqlx::SqlitePool::connect_with(options).await.unwrap();
    sqlx::query("INSERT INTO workflow_attempts(id,run_id,node_id,revision,attempt_no,epoch,input_hash,state,output_hash,created_at) VALUES(42,'run','api',1,1,1,'input','succeeded','output',36)")
        .execute(&raw).await.unwrap();
    sqlx::query(
        "UPDATE workflow_nodes SET accepted_attempt_id=42 WHERE run_id='run' AND node_id='api'",
    )
    .execute(&raw)
    .await
    .unwrap();
    raw.close().await;

    let revised =
        revised_spec(|value| value["phases"][0]["name"] = serde_json::json!("Implementation"));
    assert!(store
        .apply_revision("run", 1, "revision", &revised, 38)
        .await
        .unwrap_err()
        .to_string()
        .contains("rebinding not yet supported"));
    assert_eq!(store.run("run").await.unwrap().active_revision, 1);
    cleanup(store, root).await;
}

#[tokio::test]
async fn repository_and_capacity_changes_require_new_authorization() {
    for changed_field in ["repository", "capacity"] {
        let (store, root) = store(changed_field).await;
        let old = create_and_start(&store, "run", 1).await;
        store.pause("run", 1, "pause", 3).await.unwrap();
        let mut changed = old.clone();
        if changed_field == "repository" {
            changed.project_ref = "other/project".into();
        } else {
            changed.limits.max_concurrent_tasks = 3;
        }
        assert_ne!(
            old.task_execution_hash("api").unwrap(),
            changed.task_execution_hash("api").unwrap()
        );
        store
            .apply_revision("run", 1, "revision", &changed, 4)
            .await
            .unwrap();
        assert!(store.run("run").await.unwrap().authorization_hash.is_none());
        assert!(store.resume("run", 2, "resume", 5).await.is_err());
        cleanup(store, root).await;
    }
}

#[tokio::test]
async fn accepted_attempt_foreign_keys_reject_cross_node_and_cross_run_results() {
    let (store, root) = store("accepted-owner").await;
    create_and_start(&store, "one", 1).await;
    create_and_start(&store, "two", 1).await;
    let raw = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(root.join("workflow.sqlite"))
            .foreign_keys(true),
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO workflow_attempts(id,run_id,node_id,revision,attempt_no,epoch,input_hash,state,output_hash,created_at) VALUES(42,'one','api',1,1,1,'input','succeeded','output',2)")
        .execute(&raw).await.unwrap();
    for (run, node) in [("one", "ui"), ("two", "api")] {
        assert!(sqlx::query(
            "UPDATE workflow_nodes SET accepted_attempt_id=42 WHERE run_id=? AND node_id=?"
        )
        .bind(run)
        .bind(node)
        .execute(&raw)
        .await
        .is_err());
        assert!(sqlx::query(
            "UPDATE workflow_node_bindings SET accepted_attempt_id=42 WHERE run_id=? AND node_id=?"
        )
        .bind(run)
        .bind(node)
        .execute(&raw)
        .await
        .is_err());
    }
    sqlx::query(
        "UPDATE workflow_nodes SET accepted_attempt_id=42 WHERE run_id='one' AND node_id='api'",
    )
    .execute(&raw)
    .await
    .unwrap();
    raw.close().await;
    cleanup(store, root).await;
}

#[tokio::test]
async fn revisions_can_add_nodes_before_edges_and_never_reuse_retired_ids() {
    let (store, root) = store("revision-new-node").await;
    let original = spec();
    store
        .create_run("run", "create", &original, 1)
        .await
        .unwrap();
    let mut added = original.clone();
    let mut task = added.tasks[0].clone();
    task.id = "repair".into();
    added.tasks.push(task);
    added.edges.push(praxis_lib::workflow::EdgeSpec {
        from: "repair".into(),
        to: "final-integration".into(),
    });
    added.limits.max_tasks += 1;
    added.limits.max_edges += 1;
    store
        .apply_revision("run", 1, "add", &added, 2)
        .await
        .unwrap();
    assert!(store
        .nodes("run")
        .await
        .unwrap()
        .iter()
        .any(|node| node.node_id == "repair" && node.state == "pending"));
    assert_eq!(store.spec("run", 1).await.unwrap(), original);
    store
        .apply_revision("run", 2, "remove", &original, 3)
        .await
        .unwrap();
    assert!(store
        .nodes("run")
        .await
        .unwrap()
        .iter()
        .any(|node| node.node_id == "repair" && node.state == "retired"));
    assert!(store
        .apply_revision("run", 3, "reuse", &added, 4)
        .await
        .unwrap_err()
        .to_string()
        .contains("retired"));
    assert_eq!(store.run("run").await.unwrap().active_revision, 3);
    cleanup(store, root).await;
}

#[tokio::test]
async fn revision_compare_and_swap_allows_only_one_concurrent_writer() {
    let (store, root) = store("cas").await;
    store
        .create_run("run", "create", &spec(), 40)
        .await
        .unwrap();
    let first = revised_spec(|value| value["phases"][0]["name"] = serde_json::json!("First"));
    let second = revised_spec(|value| value["phases"][0]["name"] = serde_json::json!("Second"));
    let left = store.clone();
    let right = store.clone();
    let (one, two) = tokio::join!(
        async move { left.apply_revision("run", 1, "first", &first, 41).await },
        async move { right.apply_revision("run", 1, "second", &second, 42).await }
    );
    assert_eq!(usize::from(one.is_ok()) + usize::from(two.is_ok()), 1);
    assert_eq!(store.run("run").await.unwrap().active_revision, 2);
    cleanup(store, root).await;
}

#[tokio::test]
async fn recovery_fence_keeps_quarantined_claims_as_revision_blockers() {
    let (store, root) = store("fence").await;
    let claimed_spec = revised_spec(|value| {
        value["tasks"][0]["resource_requests_by_step"]["execute"][0]["resource_id"] =
            serde_json::json!("resource.api");
    });
    store
        .create_run("run", "create", &claimed_spec, 50)
        .await
        .unwrap();
    store
        .authorize_start("run", 1, &claimed_spec.digest().unwrap(), "start", 51)
        .await
        .unwrap();
    store
        .register_resource(&ResourceDefinition {
            id: "resource.api".into(),
            physical_identity: "repo:example:path:src/api".into(),
            capacity: 1,
            repository: Some("example".into()),
            path_prefix: Some("src/api".into()),
        })
        .await
        .unwrap();
    assert!(matches!(
        store
            .claim_next_step("run", 1, "api", 52, 30)
            .await
            .unwrap(),
        praxis_lib::workflow::resources::ClaimResult::Claimed(_)
    ));
    store.fence_recovery("run", 53).await.unwrap();
    // Explicit fixture identity and the task's implicit project write scope.
    assert_eq!(store.claim_count().await.unwrap(), 2);
    assert!(store
        .apply_revision("run", 1, "revision", &spec(), 54)
        .await
        .is_err());
    cleanup(store, root).await;
}
