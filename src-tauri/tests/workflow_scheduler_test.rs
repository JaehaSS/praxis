#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{
    runner::capacity::RunnerCapacity,
    workflow::{
        resources::{ClaimResult, ResourceDefinition},
        scheduler::WorkflowScheduler,
        store::WorkflowStore,
        AccessMode, ResourceRequest, StepKind, WorkflowSpec,
    },
};

async fn setup(name: &str) -> (WorkflowStore, std::path::PathBuf) {
    let root = temp_root::dir().join(format!("workflow-scheduler-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("workflow.sqlite");
    (WorkflowStore::open(&path).await.unwrap(), root)
}

async fn cleanup(store: WorkflowStore, root: std::path::PathBuf) {
    store.close().await;
    let _ = std::fs::remove_dir_all(root);
}

fn spec(write_path: &str, requests: Vec<ResourceRequest>) -> WorkflowSpec {
    let mut spec =
        WorkflowSpec::parse_json(include_str!("fixtures/workflow-spec-v1.json")).unwrap();
    spec.tasks[0].write_paths = vec![write_path.into()];
    spec.tasks[0].resource_requests_by_step.clear();
    if !requests.is_empty() {
        spec.tasks[0]
            .resource_requests_by_step
            .insert(StepKind::Execute, requests);
    }
    spec
}

async fn start(store: &WorkflowStore, id: &str, spec: &WorkflowSpec, now: i64) {
    store.create_run(id, "create", spec, now).await.unwrap();
    store
        .authorize_start(id, 1, &spec.digest().unwrap(), "start", now + 1)
        .await
        .unwrap();
}

fn shared_request() -> ResourceRequest {
    ResourceRequest {
        resource_id: "shared-db".into(),
        mode: AccessMode::ExclusiveWrite,
        units: 1,
    }
}

async fn register_shared(store: &WorkflowStore) {
    store
        .register_resource(&ResourceDefinition {
            id: "shared-db".into(),
            physical_identity: "fixture:shared-db".into(),
            capacity: 1,
            repository: None,
            path_prefix: None,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn blocked_candidate_does_not_prevent_later_ready_work_and_records_owner() {
    let (store, root) = setup("scan").await;
    register_shared(&store).await;
    let mut holder = spec("src/holder", vec![shared_request()]);
    holder.limits.max_concurrent_tasks = 1;
    let blocked = spec("src/blocked", vec![shared_request()]);
    let free = spec("src/free", vec![]);
    start(&store, "holder", &holder, 1).await;
    start(&store, "blocked", &blocked, 2).await;
    start(&store, "free", &free, 3).await;
    assert!(matches!(
        store
            .claim_next_step("holder", 1, "api", 4, 30)
            .await
            .unwrap(),
        ClaimResult::Claimed(_)
    ));

    let scheduler = WorkflowScheduler::new(store.clone(), RunnerCapacity::new(1));
    let scheduled = scheduler.claim_next(5, 30).await.unwrap().unwrap();
    assert_eq!(scheduled.lease.run_id, "free");
    let wait = store
        .waits("blocked")
        .await
        .unwrap()
        .into_iter()
        .find(|wait| wait.node_id == "api")
        .unwrap();
    assert_eq!(wait.reason, "resource_busy");
    assert_eq!(wait.resource_id.as_deref(), Some("shared-db"));
    assert_eq!(wait.owner_run_id.as_deref(), Some("holder"));
    assert_eq!(wait.owner_node_id.as_deref(), Some("api"));
    drop(scheduled);
    cleanup(store, root).await;
}

#[tokio::test]
async fn claims_rotate_between_runs_and_keep_fifo_order_inside_each_run() {
    let (store, root) = setup("fairness").await;
    let a = spec("src/a", vec![]);
    let b = spec("src/b", vec![]);
    start(&store, "a", &a, 1).await;
    start(&store, "b", &b, 2).await;
    let scheduler = WorkflowScheduler::new(store.clone(), RunnerCapacity::new(1));
    let first = scheduler.claim_next(3, 30).await.unwrap().unwrap();
    assert_eq!(first.lease.run_id, "a");
    assert_eq!(first.lease.node_id, "api");
    drop(first);
    let second = scheduler.claim_next(4, 30).await.unwrap().unwrap();
    assert_eq!(second.lease.run_id, "b");
    assert_eq!(second.lease.node_id, "api");
    drop(second);
    cleanup(store, root).await;
}

#[tokio::test]
async fn shared_runner_capacity_is_retained_by_the_scheduled_step() {
    let (store, root) = setup("capacity").await;
    let workflow = spec("src/capacity", vec![]);
    start(&store, "run", &workflow, 1).await;
    let capacity = RunnerCapacity::new(1);
    let scheduler = WorkflowScheduler::new(store.clone(), capacity.clone());
    let outside_owner = capacity.try_acquire().unwrap();
    assert!(scheduler.claim_next(3, 30).await.unwrap().is_none());
    assert_eq!(store.waits("run").await.unwrap()[0].reason, "capacity");
    drop(outside_owner);
    let scheduled = scheduler.claim_next(4, 30).await.unwrap().unwrap();
    assert_eq!(capacity.available(), 0);
    drop(scheduled);
    assert_eq!(capacity.available(), 1);
    cleanup(store, root).await;
}

#[tokio::test]
async fn bounded_scan_continues_past_a_blocked_prefix_on_later_ticks() {
    let (store, root) = setup("scan-cursor").await;
    register_shared(&store).await;
    let mut holder = spec("src/holder", vec![shared_request()]);
    holder.limits.max_concurrent_tasks = 1;
    let blocked_one = spec("src/blocked-one", vec![shared_request()]);
    let blocked_two = spec("src/blocked-two", vec![shared_request()]);
    let free = spec("src/free", vec![]);
    start(&store, "holder", &holder, 1).await;
    start(&store, "blocked-one", &blocked_one, 2).await;
    start(&store, "blocked-two", &blocked_two, 3).await;
    start(&store, "free", &free, 4).await;
    assert!(matches!(
        store
            .claim_next_step("holder", 1, "api", 5, 30)
            .await
            .unwrap(),
        ClaimResult::Claimed(_)
    ));
    let scheduler = WorkflowScheduler::with_scan_limit(store.clone(), RunnerCapacity::new(1), 1);
    assert!(scheduler.claim_next(6, 30).await.unwrap().is_none());
    assert!(scheduler.claim_next(7, 30).await.unwrap().is_none());
    // The holder's remaining node is also a candidate (blocked by its run cap),
    // followed by the two resource-blocked runs. A limit of one needs four ticks.
    assert!(scheduler.claim_next(8, 30).await.unwrap().is_none());
    let scheduled = scheduler.claim_next(9, 30).await.unwrap().unwrap();
    assert_eq!(scheduled.lease.run_id, "free");
    drop(scheduled);
    cleanup(store, root).await;
}

#[tokio::test]
async fn verified_predecessors_are_enqueued_when_the_scheduler_refreshes_readiness() {
    let (store, root) = setup("readiness-refresh").await;
    let mut workflow = spec("src/readiness", vec![]);
    workflow.tasks[2].resource_requests_by_step.clear();
    start(&store, "run", &workflow, 1).await;
    let raw = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        root.join("workflow.sqlite").display()
    ))
    .await
    .unwrap();
    for (node_id, output_hash) in [("api", "api-output"), ("ui", "ui-output")] {
        sqlx::query(
            "INSERT INTO workflow_attempts(run_id,node_id,revision,attempt_no,epoch,input_hash,state,output_hash,created_at) \
             VALUES('run',?,1,1,1,?,'succeeded',?,3)",
        )
        .bind(node_id)
        .bind(format!("{node_id}-input"))
        .bind(output_hash)
        .execute(&raw)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE workflow_nodes SET state='verified',accepted_attempt_id=( \
                 SELECT id FROM workflow_attempts WHERE run_id='run' AND node_id=? \
             ) WHERE run_id='run' AND node_id=?",
        )
        .bind(node_id)
        .bind(node_id)
        .execute(&raw)
        .await
        .unwrap();
    }
    raw.close().await;
    let scheduler = WorkflowScheduler::new(store.clone(), RunnerCapacity::new(1));
    let scheduled = scheduler.claim_next(4, 30).await.unwrap().unwrap();
    assert_eq!(scheduled.lease.node_id, "final-integration");
    drop(scheduled);
    cleanup(store, root).await;
}

#[tokio::test]
async fn failed_dependency_reason_propagates_to_transitive_descendants() {
    let (store, root) = setup("failed-chain").await;
    let workflow = spec("src/failed-chain", vec![]);
    start(&store, "run", &workflow, 1).await;
    let raw = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        root.join("workflow.sqlite").display()
    ))
    .await
    .unwrap();
    sqlx::query("DELETE FROM workflow_edges WHERE run_id='run' AND revision=1")
        .execute(&raw)
        .await
        .unwrap();
    for (source, target) in [("api", "ui"), ("ui", "final-integration")] {
        sqlx::query(
            "INSERT INTO workflow_edges(run_id,revision,source,target) VALUES('run',1,?,?)",
        )
        .bind(source)
        .bind(target)
        .execute(&raw)
        .await
        .unwrap();
    }
    sqlx::query("UPDATE workflow_nodes SET state='failed' WHERE run_id='run' AND node_id='api'")
        .execute(&raw)
        .await
        .unwrap();
    raw.close().await;
    let scheduler = WorkflowScheduler::new(store.clone(), RunnerCapacity::new(1));
    let _ = scheduler.claim_next(3, 30).await.unwrap();
    let waits = store.waits("run").await.unwrap();
    let descendants: Vec<_> = waits
        .iter()
        .filter(|wait| wait.node_id == "ui" || wait.node_id == "final-integration")
        .collect();
    assert_eq!(descendants.len(), 2);
    assert!(descendants
        .iter()
        .all(|wait| wait.reason == "dependency_failed"));
    cleanup(store, root).await;
}

#[tokio::test]
async fn concurrent_sqlite_schedulers_recheck_claims_under_the_write_lock() {
    let (store, root) = setup("contention").await;
    let mut workflow = spec("src/contention", vec![]);
    workflow.limits.max_concurrent_tasks = 1;
    start(&store, "run", &workflow, 1).await;
    let other = WorkflowStore::open(&root.join("workflow.sqlite"))
        .await
        .unwrap();
    let capacity = RunnerCapacity::new(2);
    let left = WorkflowScheduler::new(store.clone(), capacity.clone());
    let right = WorkflowScheduler::new(other.clone(), capacity);
    let (left, right) = tokio::join!(left.claim_next(3, 30), right.claim_next(3, 30));
    let claims = [left.unwrap(), right.unwrap()];
    assert_eq!(claims.iter().filter(|claim| claim.is_some()).count(), 1);
    assert_eq!(
        store
            .nodes("run")
            .await
            .unwrap()
            .iter()
            .map(|node| node.attempt_count)
            .sum::<i64>(),
        1
    );
    drop(claims);
    other.close().await;
    cleanup(store, root).await;
}

#[tokio::test]
async fn paused_unauthorized_failed_and_revised_runs_have_explicit_admission_state() {
    let (store, root) = setup("state").await;
    let workflow = spec("src/state", vec![]);
    start(&store, "paused", &workflow, 1).await;
    store.pause("paused", 1, "pause", 3).await.unwrap();
    start(&store, "auth", &workflow, 4).await;
    start(&store, "failed", &workflow, 5).await;
    let raw = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        root.join("workflow.sqlite").display()
    ))
    .await
    .unwrap();
    sqlx::query("UPDATE workflow_runs SET authorization_hash=NULL WHERE id='auth'")
        .execute(&raw)
        .await
        .unwrap();
    sqlx::query("UPDATE workflow_nodes SET state='failed' WHERE run_id='failed' AND node_id='api'")
        .execute(&raw)
        .await
        .unwrap();
    raw.close().await;

    let scheduler = WorkflowScheduler::new(store.clone(), RunnerCapacity::new(2));
    let _ = scheduler.claim_next(6, 30).await.unwrap();
    assert_eq!(store.waits("paused").await.unwrap()[0].reason, "paused");
    assert_eq!(
        store.waits("auth").await.unwrap()[0].reason,
        "authorization_required"
    );
    assert_eq!(
        store.waits("failed").await.unwrap()[0].reason,
        "dependency_failed"
    );
    store.resume("paused", 1, "paused-resume", 7).await.unwrap();
    assert!(store.waits("paused").await.unwrap().is_empty());
    store.pause("paused", 1, "paused-again", 8).await.unwrap();

    let revised = spec("src/revised", vec![]);
    start(&store, "revision", &workflow, 9).await;
    store
        .pause("revision", 1, "revision-pause", 9)
        .await
        .unwrap();
    store
        .apply_revision("revision", 1, "revision-apply", &revised, 10)
        .await
        .unwrap();
    store
        .reauthorize_revision(
            "revision",
            2,
            &revised.digest().unwrap(),
            "revision-authorize",
            11,
        )
        .await
        .unwrap();
    store
        .resume("revision", 2, "revision-resume", 12)
        .await
        .unwrap();
    let revised_scheduler = WorkflowScheduler::new(store.clone(), RunnerCapacity::new(1));
    let claim = revised_scheduler.claim_next(13, 30).await.unwrap().unwrap();
    assert_eq!(claim.lease.run_id, "revision");
    assert_eq!(claim.lease.revision, 2);
    drop(claim);
    cleanup(store, root).await;
}

#[tokio::test]
async fn passive_wait_observation_cannot_race_resume_and_restore_a_paused_reason() {
    let (store, root) = setup("resume-observation").await;
    let workflow = spec("src/resume-observation", vec![]);
    start(&store, "run", &workflow, 1).await;
    store.pause("run", 1, "pause", 2).await.unwrap();
    let other = WorkflowStore::open(&root.join("workflow.sqlite"))
        .await
        .unwrap();
    // No slot: a stale wait cannot be accidentally hidden by a successful claim.
    let scheduler = WorkflowScheduler::new(other.clone(), RunnerCapacity::new(0));
    let (scan, resume) = tokio::join!(
        scheduler.claim_next(3, 30),
        store.resume("run", 1, "resume", 3)
    );
    scan.unwrap();
    resume.unwrap();
    assert_eq!(store.run("run").await.unwrap().state, "running");
    assert!(store
        .waits("run")
        .await
        .unwrap()
        .iter()
        .all(|wait| wait.reason != "paused"));
    other.close().await;
    cleanup(store, root).await;
}

#[tokio::test]
async fn implicit_prefix_conflict_keeps_the_durable_blocking_owner_after_rollback() {
    let (store, root) = setup("implicit-owner").await;
    let mut holder = spec("src/parent", vec![]);
    holder.limits.max_concurrent_tasks = 1;
    let blocked = spec("src/parent/child", vec![]);
    start(&store, "holder", &holder, 1).await;
    start(&store, "blocked", &blocked, 2).await;
    assert!(matches!(
        store
            .claim_next_step("holder", 1, "api", 3, 30)
            .await
            .unwrap(),
        ClaimResult::Claimed(_)
    ));
    let scheduler = WorkflowScheduler::new(store.clone(), RunnerCapacity::new(1));
    let candidate = scheduler.claim_next(4, 30).await.unwrap();
    let wait = store
        .waits("blocked")
        .await
        .unwrap()
        .into_iter()
        .find(|wait| wait.node_id == "api")
        .unwrap();
    assert_eq!(wait.reason, "resource_busy");
    assert_eq!(wait.owner_run_id.as_deref(), Some("holder"));
    assert_eq!(wait.owner_node_id.as_deref(), Some("api"));
    assert!(wait.resource_id.is_some());
    drop(candidate);
    cleanup(store, root).await;
}
