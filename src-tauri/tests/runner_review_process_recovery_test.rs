#![cfg(unix)]

#[path = "support/temp_root.rs"]
mod temp_root;

use std::os::unix::process::CommandExt;

use praxis_lib::db;
use praxis_lib::runner::review_process::{self, ReviewOperation, ReviewPhase, ReviewProcessState};
use praxis_lib::runner::{self, config::RunnerConfig};

#[tokio::test]
async fn startup_terminates_only_an_exact_review_process_identity() {
    let fixture = fixture("exact").await;
    let mut command = std::process::Command::new("/bin/sleep");
    command.arg("30").process_group(0);
    let mut child = command.spawn().unwrap();
    let pid = child.id();
    review_process::register_observed(
        &fixture.pool,
        fixture.task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyTest,
        pid,
        2,
    )
    .await
    .unwrap();
    drop(fixture.pool);

    let runtime = runner::initialize(config_for(&fixture.root), &fixture.db_path, 3)
        .await
        .unwrap();

    let _ = child.wait();
    assert!(
        review_process::lease(runtime.pool(), fixture.task_id, ReviewOperation::Verify)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        latest_event(runtime.pool(), fixture.task_id).await,
        "review_process_recovered"
    );
}

#[tokio::test]
async fn startup_resolves_an_absent_review_process_group() {
    let fixture = fixture("absent").await;
    review_process::register(
        &fixture.pool,
        fixture.task_id,
        ReviewOperation::Challenge,
        ReviewPhase::Reviewer,
        999_999,
        &"a".repeat(64),
        2,
    )
    .await
    .unwrap();
    drop(fixture.pool);

    let runtime = runner::initialize(config_for(&fixture.root), &fixture.db_path, 3)
        .await
        .unwrap();

    assert!(
        review_process::lease(runtime.pool(), fixture.task_id, ReviewOperation::Challenge)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        latest_event(runtime.pool(), fixture.task_id).await,
        "review_process_absent"
    );
}

#[tokio::test]
async fn startup_quarantines_identity_mismatch_without_blocking_runner() {
    let fixture = fixture("mismatch").await;
    let mut command = std::process::Command::new("/bin/sleep");
    command.arg("30").process_group(0);
    let mut unrelated = command.spawn().unwrap();
    review_process::register(
        &fixture.pool,
        fixture.task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        unrelated.id() as i64,
        &"0".repeat(64),
        2,
    )
    .await
    .unwrap();
    drop(fixture.pool);

    let runtime = runner::initialize(config_for(&fixture.root), &fixture.db_path, 3)
        .await
        .unwrap();

    assert!(unrelated.try_wait().unwrap().is_none());
    let lease = review_process::lease(runtime.pool(), fixture.task_id, ReviewOperation::Verify)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.state, ReviewProcessState::Quarantined);
    assert_eq!(
        latest_event(runtime.pool(), fixture.task_id).await,
        "review_process_quarantined"
    );
    let _ = nix::sys::signal::killpg(
        nix::unistd::Pid::from_raw(unrelated.id() as i32),
        nix::sys::signal::Signal::SIGKILL,
    );
    let _ = unrelated.wait();
}

struct Fixture {
    pool: sqlx::SqlitePool,
    task_id: i64,
    root: std::path::PathBuf,
    db_path: String,
}

async fn fixture(label: &str) -> Fixture {
    let root = temp_root::dir().join(format!(
        "praxis-review-recovery-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let db_path = root.with_extension("sqlite").to_string_lossy().into_owned();
    let pool = db::init_pool(&db_path).await.unwrap();
    review_process::migrate(&pool).await.unwrap();
    let task_id = db::insert_task(
        &pool,
        root.to_string_lossy().as_ref(),
        "branch",
        "main",
        root.to_string_lossy().as_ref(),
        "review",
        None,
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, db::state::AWAITING_REVIEW, 1)
        .await
        .unwrap();
    Fixture {
        pool,
        task_id,
        root,
        db_path,
    }
}

async fn latest_event(pool: &sqlx::SqlitePool, task_id: i64) -> String {
    sqlx::query_scalar(
        "SELECT kind FROM runner_events WHERE task_id = ? ORDER BY sequence DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .unwrap()
}

fn config_for(root: &std::path::Path) -> RunnerConfig {
    RunnerConfig {
        bind: "127.0.0.1:47831".parse().unwrap(),
        repository_roots: vec![root.to_path_buf()],
        max_concurrent_tasks: 1,
        execution_policy: praxis_lib::runner::config::ExecutionPolicy::AlwaysApprove,
        pairing_token_file: root.join("token"),
    }
}
