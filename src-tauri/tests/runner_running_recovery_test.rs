//! Restart recovery must remove ephemeral memory context before failing an unowned task.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db::{self, state};
use praxis_lib::runner::{self, config::RunnerConfig};
use praxis_lib::{memory, projector};

#[tokio::test]
async fn initialization_retires_projection_for_unowned_running_task() {
    let root = temp_root::dir().join(format!(
        "praxis-runner-running-recovery-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("CLAUDE.md"), "# owner\n").unwrap();
    let root = root.canonicalize().unwrap();
    let db_path = root.with_extension("sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    verified_memory(&pool, &root).await;
    let task_id = db::insert_task(
        &pool,
        root.to_string_lossy().as_ref(),
        "branch",
        "main",
        root.to_string_lossy().as_ref(),
        "running recovery context",
        None,
        None,
        "terminal",
        100,
    )
    .await
    .unwrap();
    memory::inject_into_worktree(
        &pool,
        root.to_string_lossy().as_ref(),
        "running recovery context",
        None,
        task_id,
        104,
        &root,
        memory::INJECTION_LIMIT,
        &projector::project_targets(),
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, state::RUNNING, 105)
        .await
        .unwrap();
    drop(pool);

    let runtime = runner::initialize(config_for(&root), db_path.to_str().unwrap(), 106)
        .await
        .unwrap();

    let journal: String =
        sqlx::query_scalar("SELECT state FROM memory_projection_journal WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(runtime.pool())
            .await
            .unwrap();
    assert_eq!(journal, "retired");
    assert_eq!(
        std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
        "# owner\n"
    );
    assert_eq!(
        db::get_task(runtime.pool(), task_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        state::FAILED
    );
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn recovery_never_kills_a_reused_process_identity() {
    use std::os::unix::process::CommandExt;

    let root =
        temp_root::dir().join(format!("praxis-runner-reused-pid-{}", std::process::id()));
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
        "reused process identity",
        None,
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, state::RUNNING, 2)
        .await
        .unwrap();
    let mut command = std::process::Command::new("/bin/sleep");
    command.arg("5").process_group(0);
    let mut unrelated = command.spawn().unwrap();
    let pgid = unrelated.id();
    db::record_task_process_start(&pool, task_id, pgid as i64, &"0".repeat(64), "terminal", 3)
        .await
        .unwrap();
    drop(pool);

    let runtime = runner::initialize(config_for(&root), db_path.to_str().unwrap(), 4)
        .await
        .unwrap();

    assert!(unrelated.try_wait().unwrap().is_none());
    let recovered = db::get_task(runtime.pool(), task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(recovered.state, state::FAILED);
    assert_eq!(recovered.convo_pgid, Some(pgid as i64));
    assert_eq!(
        db::list_runner_events_after(runtime.pool(), 0, 10)
            .await
            .unwrap()
            .last()
            .unwrap()
            .kind,
        "process_quarantined"
    );
    let _ = nix::sys::signal::killpg(
        nix::unistd::Pid::from_raw(pgid as i32),
        nix::sys::signal::Signal::SIGKILL,
    );
    let _ = unrelated.wait();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(root);
}

async fn verified_memory(pool: &sqlx::SqlitePool, root: &std::path::Path) {
    let id = memory::create_candidate(
        pool,
        memory::tier::PROJECT,
        Some(root.to_string_lossy().as_ref()),
        memory::knowledge_type::CLAIM,
        "running recovery context",
        Some("test"),
        100,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(pool, id, 101, None)
        .await
        .unwrap();
    memory::submit_for_review(pool, id, 102).await.unwrap();
    memory::approve(pool, id, "human", 103).await.unwrap();
}

fn config_for(root: &std::path::Path) -> RunnerConfig {
    RunnerConfig {
        bind: "127.0.0.1:47831".parse().unwrap(),
        repository_roots: vec![root.to_path_buf()],
        max_concurrent_tasks: 1,
        execution_policy: praxis_lib::runner::config::ExecutionPolicy::AlwaysApprove,
        pairing_token_file: temp_root::dir().join("unused-runner-token"),
    }
}
