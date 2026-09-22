#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db::{self, state};
use praxis_lib::runner::{self, config::RunnerConfig};

#[tokio::test]
async fn initialization_discards_an_applied_projection_left_in_created() {
    let repo = temp_root::dir().join(format!("praxis-runner-created-{}", std::process::id()));
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "runner@example.test"]);
    git(&repo, &["config", "user.name", "Runner Test"]);
    std::fs::write(repo.join("README.md"), "base\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-qm", "initial"]);
    let worktree = praxis_lib::worktree::create_plain(&repo, "praxis/orphan-created", None).unwrap();
    let db_path = repo.with_extension("sqlite").to_string_lossy().into_owned();
    let pool = db::init_pool(&db_path).await.unwrap();
    praxis_lib::memory::migrate(&pool).await.unwrap();
    let task_id = db::insert_task(
        &pool,
        repo.to_str().unwrap(),
        &worktree.branch,
        &worktree.base,
        worktree.path.to_str().unwrap(),
        "orphan created",
        Some("claude"),
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();
    praxis_lib::memory::inject_into_worktree(
        &pool,
        repo.to_str().unwrap(),
        "orphan created",
        None,
        task_id,
        2,
        &worktree.path,
        praxis_lib::memory::INJECTION_LIMIT,
        &praxis_lib::projector::project_targets(),
    )
    .await
    .unwrap();
    drop(pool);

    let runtime = runner::initialize(config_for(&repo), &db_path, 3)
        .await
        .unwrap();
    assert_eq!(
        db::get_task(runtime.pool(), task_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        state::FAILED
    );
    let journal: (String, Option<String>) = sqlx::query_as(
        "SELECT state, preimages_json FROM memory_projection_journal WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_one(runtime.pool())
    .await
    .unwrap();
    assert_eq!(journal, ("retired".into(), None));
    assert!(!worktree.path.exists());
    assert!(db::list_runner_events_after(runtime.pool(), 0, 10)
        .await
        .unwrap()
        .iter()
        .any(|event| event.kind == "memory_projection_orphaned"));
    let _ = std::fs::remove_dir_all(repo);
    let _ = std::fs::remove_file(db_path);
}

fn config_for(repo: &std::path::Path) -> RunnerConfig {
    RunnerConfig {
        bind: "127.0.0.1:47831".parse().unwrap(),
        repository_roots: vec![repo.canonicalize().unwrap()],
        max_concurrent_tasks: 1,
        execution_policy: praxis_lib::runner::config::ExecutionPolicy::AlwaysApprove,
        pairing_token_file: temp_root::dir().join("unused-runner-token"),
    }
}

fn git(root: &std::path::Path, args: &[&str]) {
    assert!(std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap()
        .success());
}
