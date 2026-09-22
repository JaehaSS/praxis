#[path = "support/temp_root.rs"]
mod temp_root;

use std::collections::BTreeMap;

use praxis_lib::runner::workflow::test_db::{prepare, DbProfile};
use praxis_lib::workflow::{
    lifecycle::{CleanupProof, StepExit},
    resources::{ClaimResult, ResourceDefinition},
    store::WorkflowStore,
    AccessMode, ResourceRequest, StepKind, WorkflowSpec,
};

async fn sqlite_fixture(root: &std::path::Path) -> std::path::PathBuf {
    let path = root.join("seed.sqlite");
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(&path)
            .create_if_missing(true),
    )
    .await
    .unwrap();
    sqlx::query("PRAGMA journal_mode=DELETE")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE seed (value TEXT NOT NULL)")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO seed(value) VALUES('fixed')")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    path
}

fn spec(mode: AccessMode, capacity: i64) -> WorkflowSpec {
    let mut spec =
        WorkflowSpec::parse_json(include_str!("fixtures/workflow-spec-v1.json")).unwrap();
    let task = &mut spec.tasks[0];
    task.write_paths.clear();
    task.resource_requests_by_step = BTreeMap::from([(
        StepKind::Execute,
        vec![ResourceRequest {
            resource_id: "fixture-db".into(),
            mode,
            units: 1,
        }],
    )]);
    if capacity > 1 {
        task.resource_requests_by_step
            .get_mut(&StepKind::Execute)
            .unwrap()[0]
            .mode = AccessMode::Capacity;
    }
    spec
}

async fn setup(
    label: &str,
    mode: AccessMode,
    capacity: i64,
) -> (WorkflowStore, std::path::PathBuf, WorkflowSpec) {
    let root = temp_root::dir().join(format!("workflow-test-db-{label}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let store = WorkflowStore::open(&root.join("workflow.sqlite"))
        .await
        .unwrap();
    store
        .register_resource(&ResourceDefinition {
            id: "fixture-db".into(),
            physical_identity: "fixture:workflow-test-db".into(),
            capacity,
            repository: None,
            path_prefix: None,
        })
        .await
        .unwrap();
    let spec = spec(mode, capacity);
    store
        .create_run("writer", "create", &spec, 1)
        .await
        .unwrap();
    store
        .authorize_start("writer", 1, &spec.digest().unwrap(), "start", 2)
        .await
        .unwrap();
    (store, root, spec)
}

async fn claim(
    store: &WorkflowStore,
    run: &str,
    now: i64,
) -> praxis_lib::workflow::resources::StepLease {
    match store.claim_next_step(run, 1, "api", now, 30).await.unwrap() {
        ClaimResult::Claimed(lease) => lease,
        value => panic!("expected claim, got {value:?}"),
    }
}

async fn release_failed(
    store: &WorkflowStore,
    lease: &praxis_lib::workflow::resources::StepLease,
    request: &str,
) {
    store
        .finish_step(
            lease,
            request,
            &StepExit {
                exit_code: 1,
                log_hash: "a".repeat(64),
                cleanup: CleanupProof::Absent,
            },
            20,
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn fixture_mount_requires_the_exact_claimed_resource_and_rejects_capacity() {
    let (store, root, _spec) = setup("claim", AccessMode::ExclusiveWrite, 1).await;
    let seed = sqlite_fixture(&root).await;
    let lease = claim(&store, "writer", 3).await;
    assert!(prepare(
        &store,
        &lease,
        &DbProfile {
            resource_id: "not-claimed".into(),
            fixture_path: seed.clone(),
        },
        &root.join("private"),
    )
    .await
    .is_err());
    store.close().await;
    let _ = std::fs::remove_dir_all(root);

    let (store, root, _spec) = setup("capacity", AccessMode::Capacity, 2).await;
    let seed = sqlite_fixture(&root).await;
    let lease = claim(&store, "writer", 3).await;
    assert!(prepare(
        &store,
        &lease,
        &DbProfile {
            resource_id: "fixture-db".into(),
            fixture_path: seed,
        },
        &root.join("private"),
    )
    .await
    .is_err());
    store.close().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn exclusive_reset_has_a_new_generation_after_a_bounded_retry() {
    let (store, root, _spec) = setup("reset", AccessMode::ExclusiveWrite, 1).await;
    let seed = sqlite_fixture(&root).await;
    let profile = DbProfile {
        resource_id: "fixture-db".into(),
        fixture_path: seed,
    };
    let first = claim(&store, "writer", 3).await;
    let first_mount = prepare(&store, &first, &profile, &root.join("private"))
        .await
        .unwrap();
    assert!(!first_mount.read_only);
    release_failed(&store, &first, "writer-failed").await;
    store
        .retry_node("writer", 1, "api", "writer-retry", 21)
        .await
        .unwrap();
    let second = claim(&store, "writer", 22).await;
    let second_mount = prepare(&store, &second, &profile, &root.join("private"))
        .await
        .unwrap();
    assert_ne!(first_mount.generation, second_mount.generation);
    assert_ne!(first_mount.host_path, second_mount.host_path);
    assert_ne!(first_mount.environment_hash, second_mount.environment_hash);
    store.close().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn shared_read_uses_the_frozen_receipt_after_writer_release() {
    let (store, root, _writer_spec) = setup("reader", AccessMode::ExclusiveWrite, 1).await;
    let seed = sqlite_fixture(&root).await;
    let profile = DbProfile {
        resource_id: "fixture-db".into(),
        fixture_path: seed,
    };
    let writer = claim(&store, "writer", 3).await;
    let writer_mount = prepare(&store, &writer, &profile, &root.join("private"))
        .await
        .unwrap();
    release_failed(&store, &writer, "writer-release").await;

    let reader_spec = spec(AccessMode::SharedRead, 1);
    store
        .create_run("reader", "reader-create", &reader_spec, 30)
        .await
        .unwrap();
    store
        .authorize_start(
            "reader",
            1,
            &reader_spec.digest().unwrap(),
            "reader-start",
            31,
        )
        .await
        .unwrap();
    let reader = claim(&store, "reader", 32).await;
    let reader_mount = prepare(&store, &reader, &profile, &root.join("private"))
        .await
        .unwrap();
    assert!(reader_mount.read_only);
    assert_ne!(writer_mount.host_path, reader_mount.host_path);
    assert_eq!(reader_mount.generation, writer_mount.generation);
    store.close().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn first_shared_read_atomically_publishes_a_private_frozen_seed() {
    let (store, root, _spec) = setup("first-reader", AccessMode::SharedRead, 1).await;
    let seed = sqlite_fixture(&root).await;
    let profile = DbProfile {
        resource_id: "fixture-db".into(),
        fixture_path: seed,
    };
    let lease = claim(&store, "writer", 3).await;
    let private = root.join("private");
    let (first, second) = tokio::join!(
        prepare(&store, &lease, &profile, &private),
        prepare(&store, &lease, &profile, &private)
    );
    let first = first.unwrap();
    let second = second.unwrap();
    assert!(first.read_only);
    assert_eq!(first.generation, 0);
    assert_eq!(first.host_path, second.host_path);
    assert_eq!(first.environment_hash, second.environment_hash);
    assert!(first.host_path.join("database.sqlite").is_file());
    assert_ne!(first.host_path, root.join("seed.sqlite"));
    store.close().await;
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn source_fixture_rejects_wal_and_symlink_inputs() {
    let (store, root, _spec) = setup("unsafe-source", AccessMode::ExclusiveWrite, 1).await;
    let seed = sqlite_fixture(&root).await;
    let lease = claim(&store, "writer", 3).await;
    std::fs::write(format!("{}-wal", seed.display()), b"unexpected").unwrap();
    assert!(prepare(
        &store,
        &lease,
        &DbProfile {
            resource_id: "fixture-db".into(),
            fixture_path: seed.clone(),
        },
        &root.join("private"),
    )
    .await
    .is_err());
    std::fs::remove_file(format!("{}-wal", seed.display())).unwrap();
    #[cfg(unix)]
    {
        let link = root.join("seed-link.sqlite");
        std::os::unix::fs::symlink(&seed, &link).unwrap();
        assert!(prepare(
            &store,
            &lease,
            &DbProfile {
                resource_id: "fixture-db".into(),
                fixture_path: link,
            },
            &root.join("private"),
        )
        .await
        .is_err());
    }
    store.close().await;
    let _ = std::fs::remove_dir_all(root);
}
