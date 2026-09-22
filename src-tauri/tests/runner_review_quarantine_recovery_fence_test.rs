#![cfg(unix)]

#[path = "support/temp_root.rs"]
mod temp_root;

use std::os::unix::process::CommandExt;

use praxis_lib::db;
use praxis_lib::runner::review_process::{self, ReviewOperation, ReviewPhase};
use praxis_lib::runner::{self, config::RunnerConfig};

#[tokio::test]
async fn quarantined_review_child_preserves_created_task_worktree_and_state() {
    let root = temp_root::dir().join(format!(
        "praxis-review-recovery-fence-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let db_path = root.with_extension("sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    let task_id = db::insert_task(
        &pool,
        root.to_string_lossy().as_ref(),
        "branch",
        "main",
        root.to_string_lossy().as_ref(),
        "created with review child",
        None,
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();
    let mut command = std::process::Command::new("/bin/sleep");
    command.arg("30").process_group(0);
    let mut unrelated = command.spawn().unwrap();
    review_process::register(
        &pool,
        task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        unrelated.id() as i64,
        &"0".repeat(64),
        2,
    )
    .await
    .unwrap();
    drop(pool);

    let runtime = runner::initialize(config_for(&root), db_path.to_str().unwrap(), 3)
        .await
        .unwrap();

    assert!(unrelated.try_wait().unwrap().is_none());
    assert!(root.exists());
    assert_eq!(
        db::get_task(runtime.pool(), task_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        db::state::CREATED
    );
    let _ = nix::sys::signal::killpg(
        nix::unistd::Pid::from_raw(unrelated.id() as i32),
        nix::sys::signal::Signal::SIGKILL,
    );
    let _ = unrelated.wait();
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
