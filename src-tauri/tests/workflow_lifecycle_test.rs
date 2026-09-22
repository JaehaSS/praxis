#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::workflow::{
    lifecycle::{
        deterministic_container_name, CleanupProof, ContainerRegistration, FinishStep,
        LaunchIntent, StepCleanup, StepExit,
    },
    resources::ClaimResult,
    store::WorkflowStore,
    verification::{ArtifactReceipt, CheckReceipt, ManualAcceptance},
    WorkflowSpec,
};

fn hash(byte: char) -> String {
    std::iter::repeat_n(byte, 64).collect()
}

#[tokio::test]
async fn independent_verifier_respects_the_workflow_concurrency_limit() {
    let (store, root, mut spec) = setup("verify-capacity").await;
    spec.limits.max_concurrent_tasks = 1;
    store
        .create_run("bounded", "create-bounded", &spec, 3)
        .await
        .unwrap();
    store
        .authorize_start("bounded", 1, &spec.digest().unwrap(), "start-bounded", 4)
        .await
        .unwrap();
    let execute = match store
        .claim_next_step("bounded", 1, "api", 5, 30)
        .await
        .unwrap()
    {
        ClaimResult::Claimed(lease) => lease,
        other => panic!("{other:?}"),
    };
    let container = launch(&store, &execute, "bounded-api", 6).await;
    store
        .record_artifact(
            &execute,
            "bounded-artifact",
            &ArtifactReceipt {
                artifact_id: "bounded-artifact".into(),
                input_tree_hash: hash('1'),
                parent_input_hash: hash('1'),
                output_tree_hash: hash('2'),
                delta_hash: hash('3'),
                manifest_hash: hash('4'),
                task_spec_hash: spec.task_execution_hash("api").unwrap(),
                config_hash: hash('5'),
            },
            10,
        )
        .await
        .unwrap();
    store
        .finish_step(
            &execute,
            "bounded-exit",
            &StepExit {
                exit_code: 0,
                log_hash: hash('6'),
                cleanup: CleanupProof::Terminated {
                    observed_identity: container,
                },
            },
            11,
        )
        .await
        .unwrap();
    let other = match store
        .claim_next_step("bounded", 1, "ui", 12, 30)
        .await
        .unwrap()
    {
        ClaimResult::Claimed(lease) => lease,
        value => panic!("{value:?}"),
    };
    assert!(
        matches!(store.claim_verify_step("bounded",1,"api",execute.attempt_id,13,30).await.unwrap(),ClaimResult::Waiting{reason,..} if reason=="capacity")
    );
    store
        .finish_step(
            &other,
            "bounded-ui-exit",
            &StepExit {
                exit_code: 1,
                log_hash: hash('7'),
                cleanup: CleanupProof::Absent,
            },
            14,
        )
        .await
        .unwrap();
    assert_eq!(store.claim_count().await.unwrap(), 0);
    assert!(matches!(
        store
            .claim_verify_step("bounded", 1, "api", execute.attempt_id, 15, 30)
            .await
            .unwrap(),
        ClaimResult::Claimed(_)
    ));
    store.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

async fn setup(label: &str) -> (WorkflowStore, std::path::PathBuf, WorkflowSpec) {
    let root = temp_root::dir().join(format!("workflow-lifecycle-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let mut spec =
        WorkflowSpec::parse_json(include_str!("fixtures/workflow-spec-v1.json")).unwrap();
    for task in &mut spec.tasks {
        task.resource_requests_by_step.clear();
    }
    let store = WorkflowStore::open(&root.join("workflow.sqlite"))
        .await
        .unwrap();
    store.create_run("run", "create", &spec, 1).await.unwrap();
    store
        .authorize_start("run", 1, &spec.digest().unwrap(), "start", 2)
        .await
        .unwrap();
    (store, root, spec)
}

async fn claim(
    store: &WorkflowStore,
    node_id: &str,
    now: i64,
) -> praxis_lib::workflow::resources::StepLease {
    let revision = store.run("run").await.unwrap().active_revision;
    match store
        .claim_next_step("run", revision, node_id, now, 30)
        .await
        .unwrap()
    {
        ClaimResult::Claimed(lease) => lease,
        result => panic!("expected claim, got {result:?}"),
    }
}

async fn launch(
    store: &WorkflowStore,
    lease: &praxis_lib::workflow::resources::StepLease,
    tag: &str,
    now: i64,
) -> String {
    let id = format!("container-{tag}");
    let intent = LaunchIntent {
        container_name: deterministic_container_name(lease),
        ownership_nonce: hash(if tag.as_bytes()[0] % 2 == 0 { 'b' } else { 'c' })[..32].into(),
        adapter_profile_hash: hash('a'),
    };
    store
        .begin_launch(lease, &format!("intent-{tag}"), &intent, now)
        .await
        .unwrap();
    store
        .register_container(
            lease,
            &format!("register-{tag}"),
            &ContainerRegistration {
                container_id: id.clone(),
                ownership_nonce: intent.ownership_nonce,
            },
            now + 1,
        )
        .await
        .unwrap();
    store
        .mark_step_running(lease, &format!("running-{tag}"), now + 2)
        .await
        .unwrap();
    id
}

async fn complete_node(
    store: &WorkflowStore,
    spec: &WorkflowSpec,
    node: &str,
    ordinal: i64,
    accept_manual: bool,
) {
    let execute = claim(store, node, ordinal * 100).await;
    let revision = execute.revision;
    let container = launch(
        store,
        &execute,
        &format!("{node}-execute"),
        ordinal * 100 + 1,
    )
    .await;
    let task_hash = spec.task_execution_hash(node).unwrap();
    let artifact = ArtifactReceipt {
        artifact_id: format!("content-{node}"),
        input_tree_hash: hash('1'),
        parent_input_hash: hash('1'),
        output_tree_hash: hash(char::from(b'2' + ordinal as u8)),
        delta_hash: hash('3'),
        manifest_hash: hash('4'),
        task_spec_hash: task_hash.clone(),
        config_hash: hash('5'),
    };
    store
        .record_artifact(
            &execute,
            &format!("artifact-{node}"),
            &artifact,
            ordinal * 100 + 4,
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .finish_step(
                &execute,
                &format!("execute-exit-{node}"),
                &StepExit {
                    exit_code: 0,
                    log_hash: hash('6'),
                    cleanup: CleanupProof::Terminated {
                        observed_identity: container
                    }
                },
                ordinal * 100 + 5
            )
            .await
            .unwrap(),
        FinishStep::Verifying
    );
    let verify = match store
        .claim_verify_step(
            "run",
            revision,
            node,
            execute.attempt_id,
            ordinal * 100 + 6,
            30,
        )
        .await
        .unwrap()
    {
        ClaimResult::Claimed(lease) => lease,
        result => panic!("expected verify claim, got {result:?}"),
    };
    let container = launch(store, &verify, &format!("{node}-verify"), ordinal * 100 + 7).await;
    let task = spec.tasks.iter().find(|task| task.id == node).unwrap();
    for check in &task.checks {
        store
            .record_check(
                &verify,
                &format!("check-{node}-{}", check.id),
                &CheckReceipt {
                    check_id: check.id.clone(),
                    profile_id: check.profile_id.clone(),
                    snapshot_hash: artifact.output_tree_hash.clone(),
                    input_hash: verify.input_hash.clone(),
                    task_spec_hash: task_hash.clone(),
                    check_profile_hash: hash('7'),
                    image_digest: format!("sha256:{}", hash('8')),
                    environment_hash: hash('9'),
                    exit_code: 0,
                    log_hash: hash('a'),
                },
                ordinal * 100 + 8,
            )
            .await
            .unwrap();
    }
    assert_eq!(
        store
            .finish_step(
                &verify,
                &format!("verify-exit-{node}"),
                &StepExit {
                    exit_code: 0,
                    log_hash: hash('b'),
                    cleanup: CleanupProof::Terminated {
                        observed_identity: container
                    }
                },
                ordinal * 100 + 9
            )
            .await
            .unwrap(),
        FinishStep::Verifying
    );
    store
        .finalize_verification(
            "run",
            revision,
            node,
            execute.attempt_id,
            &format!("finalize-pre-{node}"),
            ordinal * 100 + 10,
        )
        .await
        .unwrap();
    for criterion in task.manual_acceptance.iter().filter(|_| accept_manual) {
        assert!(store
            .accept_manual(
                "run",
                revision,
                node,
                execute.attempt_id,
                &format!("wrong-accept-{node}"),
                &ManualAcceptance {
                    criterion: criterion.clone(),
                    output_tree_hash: hash('f'),
                },
                ordinal * 100 + 11,
            )
            .await
            .is_err());
        store
            .accept_manual(
                "run",
                revision,
                node,
                execute.attempt_id,
                &format!("accept-{node}"),
                &ManualAcceptance {
                    criterion: criterion.clone(),
                    output_tree_hash: artifact.output_tree_hash.clone(),
                },
                ordinal * 100 + 11,
            )
            .await
            .unwrap();
    }
    if accept_manual && !task.manual_acceptance.is_empty() {
        store
            .finalize_verification(
                "run",
                revision,
                node,
                execute.attempt_id,
                &format!("finalize-{node}"),
                ordinal * 100 + 12,
            )
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn stale_result_and_cancel_intent_cannot_accept_an_active_step() {
    let (store, root, _spec) = setup("stale-cancel").await;
    let lease = claim(&store, "api", 3).await;
    let container = launch(&store, &lease, "cancel", 4).await;
    store.cancel_intent("run", 1, "cancel", 8).await.unwrap();
    assert!(store
        .finish_step(
            &lease,
            "late-result",
            &StepExit {
                exit_code: 0,
                log_hash: hash('a'),
                cleanup: CleanupProof::Terminated {
                    observed_identity: container.clone()
                }
            },
            9
        )
        .await
        .is_err());
    store
        .finish_cancel(
            "run",
            1,
            "cancel-finish",
            &[StepCleanup {
                step_id: lease.step_id,
                cleanup: CleanupProof::Terminated {
                    observed_identity: container,
                },
            }],
            10,
        )
        .await
        .unwrap();
    assert_eq!(store.run("run").await.unwrap().state, "cancelled");
    store.close().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn recovery_epoch_fences_old_leases_before_launch_or_result() {
    let (store, root, _spec) = setup("recovery-fence").await;
    let lease = claim(&store, "api", 3).await;
    store.fence_recovery("run", 4).await.unwrap();
    assert!(store
        .begin_launch(
            &lease,
            "stale-launch",
            &LaunchIntent {
                container_name: deterministic_container_name(&lease),
                ownership_nonce: hash('d')[..32].into(),
                adapter_profile_hash: hash('a'),
            },
            5,
        )
        .await
        .is_err());
    store.close().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn recovery_fails_quiescent_manual_acceptance_without_quarantining_a_zombie_attempt() {
    let (store, root, spec) = setup("quiescent-recovery").await;
    complete_node(&store, &spec, "api", 1, false).await;
    let before = store
        .attempts("run")
        .await
        .unwrap()
        .into_iter()
        .find(|attempt| attempt.node_id == "api")
        .unwrap();
    assert_eq!(before.state, "active");
    assert_eq!(
        store
            .nodes("run")
            .await
            .unwrap()
            .into_iter()
            .find(|node| node.node_id == "api")
            .unwrap()
            .state,
        "awaiting_acceptance"
    );
    store.fence_recovery("run", 200).await.unwrap();
    assert_eq!(
        store
            .attempts("run")
            .await
            .unwrap()
            .into_iter()
            .find(|attempt| attempt.id == before.id)
            .unwrap()
            .state,
        "failed"
    );
    assert_eq!(
        store
            .nodes("run")
            .await
            .unwrap()
            .into_iter()
            .find(|node| node.node_id == "api")
            .unwrap()
            .state,
        "failed"
    );
    assert_eq!(store.run("run").await.unwrap().state, "failed");
    assert!(store.recovery_steps("run").await.unwrap().is_empty());
    store.close().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn verification_failure_is_terminal_and_retry_stays_bounded() {
    let (store, root, _spec) = setup("failure").await;
    let lease = claim(&store, "api", 3).await;
    let container = launch(&store, &lease, "failure", 4).await;
    assert_eq!(
        store
            .finish_step(
                &lease,
                "failed",
                &StepExit {
                    exit_code: 17,
                    log_hash: hash('a'),
                    cleanup: CleanupProof::Terminated {
                        observed_identity: container
                    }
                },
                9
            )
            .await
            .unwrap(),
        FinishStep::Failed
    );
    assert_eq!(
        store
            .nodes("run")
            .await
            .unwrap()
            .into_iter()
            .find(|node| node.node_id == "api")
            .unwrap()
            .state,
        "failed"
    );
    store
        .retry_node("run", 1, "api", "retry", 10)
        .await
        .unwrap();
    assert!(store
        .retry_node("run", 1, "api", "retry-again", 11)
        .await
        .is_err());
    store.close().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn manual_acceptance_requires_the_exact_artifact_hash_and_final_completion_is_atomic() {
    let (store, root, spec) = setup("complete").await;
    complete_node(&store, &spec, "api", 1, true).await;
    complete_node(&store, &spec, "ui", 2, true).await;
    complete_node(&store, &spec, "final-integration", 3, true).await;
    assert_eq!(store.run("run").await.unwrap().state, "completed");
    let final_artifact = store
        .accepted_artifact("run", "final-integration")
        .await
        .unwrap();
    assert_eq!(
        store
            .artifact_for_attempt("run", final_artifact.attempt_id)
            .await
            .unwrap(),
        final_artifact
    );
    store.close().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn retired_tasks_do_not_block_current_revision_completion() {
    let (store, root, mut spec) = setup("retired-completion").await;
    store.pause("run", 1, "pause", 3).await.unwrap();
    spec.tasks.retain(|task| task.id != "ui");
    spec.edges
        .retain(|edge| edge.from != "ui" && edge.to != "ui");
    for task in &mut spec.tasks {
        task.input_artifacts.retain(|input| input.task_id != "ui");
    }
    store
        .apply_revision("run", 1, "revise", &spec, 4)
        .await
        .unwrap();
    store
        .reauthorize_revision("run", 2, &spec.digest().unwrap(), "approve", 5)
        .await
        .unwrap();
    store.resume("run", 2, "resume", 6).await.unwrap();
    complete_node(&store, &spec, "api", 1, true).await;
    complete_node(&store, &spec, "final-integration", 2, true).await;
    assert_eq!(store.run("run").await.unwrap().state, "completed");
    store.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
