//! Legacy or unverifiable process leases must quarantine one task without blocking Runner startup.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db::{self, state};
use praxis_lib::runner::{self, config::RunnerConfig};

#[tokio::test]
async fn legacy_process_lease_is_quarantined_and_retained() {
    let root = temp_root::dir().join(format!(
        "praxis-runner-process-quarantine-{}",
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
        "legacy process lease",
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
    db::set_convo_pgid(&pool, task_id, Some(424_242))
        .await
        .unwrap();
    drop(pool);

    let runtime = runner::initialize(config_for(&root), db_path.to_str().unwrap(), 3)
        .await
        .unwrap();
    let task = db::get_task(runtime.pool(), task_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(task.state, state::FAILED);
    assert_eq!(task.convo_pgid, Some(424_242));
    let kind: String = sqlx::query_scalar(
        "SELECT kind FROM runner_events WHERE task_id = ? ORDER BY sequence DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_one(runtime.pool())
    .await
    .unwrap();
    assert_eq!(kind, "process_quarantined");
    assert!(db::delete_task(runtime.pool(), task_id).await.is_err());

    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(root);
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
