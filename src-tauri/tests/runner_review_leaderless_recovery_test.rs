#![cfg(unix)]

#[path = "support/temp_root.rs"]
mod temp_root;

use std::os::unix::process::CommandExt;

use praxis_lib::db;
use praxis_lib::runner::review_process::{self, ReviewOperation, ReviewPhase, ReviewProcessState};
use praxis_lib::runner::{self, config::RunnerConfig};

#[tokio::test]
async fn leaderless_live_review_group_is_quarantined_without_killing_descendants() {
    let root =
        temp_root::dir().join(format!("praxis-review-leaderless-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let db_path = root.with_extension("sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    let root_text = root.to_string_lossy();
    let task_id = db::insert_task(
        &pool,
        &root_text,
        "branch",
        "main",
        &root_text,
        "leaderless review",
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
    let mut command = std::process::Command::new("/bin/sh");
    command
        .args(["-c", "sleep 30 & sleep 0.2"])
        .process_group(0);
    let mut leader = command.spawn().unwrap();
    let pgid = leader.id();
    review_process::register_observed(
        &pool,
        task_id,
        ReviewOperation::Challenge,
        ReviewPhase::Reviewer,
        pgid,
        2,
    )
    .await
    .unwrap();
    assert!(leader.wait().unwrap().success());
    assert!(nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pgid as i32), None).is_ok());
    drop(pool);

    let runtime = runner::initialize(config_for(&root), db_path.to_str().unwrap(), 3)
        .await
        .unwrap();

    let lease = review_process::lease(runtime.pool(), task_id, ReviewOperation::Challenge)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.state, ReviewProcessState::Quarantined);
    assert!(nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pgid as i32), None).is_ok());
    let _ = nix::sys::signal::killpg(
        nix::unistd::Pid::from_raw(pgid as i32),
        nix::sys::signal::Signal::SIGKILL,
    );
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
