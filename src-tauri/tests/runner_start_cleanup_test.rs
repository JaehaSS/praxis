//! A blocked start must surface projection-cleanup failure and remain recoverable.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db::{self, state};
use praxis_lib::runner::queue::QueueWorker;
use praxis_lib::{evidence, memory, projector};

#[tokio::test]
async fn cleanup_conflict_is_reported_and_starting_task_is_quarantined() {
    let root = repository();
    let db_path = root.with_extension("sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let now = 2_000_000_000;
    verified_memory(&pool, &root, now).await;
    let task_id = db::insert_task(
        &pool,
        root.to_string_lossy().as_ref(),
        "branch",
        "main",
        root.to_string_lossy().as_ref(),
        "cleanup conflict evidence",
        Some("/bin/echo"),
        None,
        "terminal",
        now,
    )
    .await
    .unwrap();
    memory::inject_into_worktree(
        &pool,
        root.to_string_lossy().as_ref(),
        "cleanup conflict evidence",
        None,
        task_id,
        now + 3,
        &root,
        memory::INJECTION_LIMIT,
        &projector::project_targets(),
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, state::QUEUED, now + 4)
        .await
        .unwrap();
    std::fs::write(root.join("src/lib.rs"), "changed\n").unwrap();
    let projected = std::fs::read_to_string(root.join("CLAUDE.md")).unwrap();
    std::fs::write(
        root.join("CLAUDE.md"),
        format!("{projected}\nindependent edit\n"),
    )
    .unwrap();

    let error = QueueWorker::new(pool.clone(), 1)
        .run_next(now + 5)
        .await
        .unwrap_err();

    assert!(error.contains("projection cleanup failed"));
    assert_eq!(
        db::get_task(&pool, task_id).await.unwrap().unwrap().state,
        state::STARTING
    );
    let journal: String =
        sqlx::query_scalar("SELECT state FROM memory_projection_journal WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(journal, "degraded");
    drop(pool);
    let runtime =
        praxis_lib::runner::initialize(config_for(&root), db_path.to_str().unwrap(), now + 6)
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
    let recovered_journal: String =
        sqlx::query_scalar("SELECT state FROM memory_projection_journal WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(runtime.pool())
            .await
            .unwrap();
    assert_eq!(recovered_journal, "degraded");
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(root);
}

fn config_for(root: &std::path::Path) -> praxis_lib::runner::config::RunnerConfig {
    praxis_lib::runner::config::RunnerConfig {
        bind: "127.0.0.1:47831".parse().unwrap(),
        repository_roots: vec![root.to_path_buf()],
        max_concurrent_tasks: 1,
        execution_policy: praxis_lib::runner::config::ExecutionPolicy::AlwaysApprove,
        pairing_token_file: temp_root::dir().join("unused-runner-token"),
    }
}

fn repository() -> std::path::PathBuf {
    let root = temp_root::dir().join(format!(
        "praxis-runner-start-cleanup-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "cleanup conflict evidence\n").unwrap();
    std::fs::write(root.join("CLAUDE.md"), "# owner\n").unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "cleanup@example.test"]);
    git(&root, &["config", "user.name", "Cleanup Test"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "initial"]);
    root.canonicalize().unwrap()
}

async fn verified_memory(pool: &sqlx::SqlitePool, root: &std::path::Path, now: i64) {
    let id = memory::create_candidate(
        pool,
        memory::tier::PROJECT,
        Some(root.to_string_lossy().as_ref()),
        memory::knowledge_type::CLAIM,
        "cleanup conflict evidence",
        Some("test"),
        now,
    )
    .await
    .unwrap();
    evidence::add_code_location(
        pool,
        id,
        evidence::CodeLocationInput {
            relative_path: "src/lib.rs".into(),
            line_start: 1,
            line_end: 1,
        },
        now,
    )
    .await
    .unwrap();
    memory::submit_for_review(pool, id, now + 1).await.unwrap();
    memory::approve(pool, id, "human", now + 2).await.unwrap();
}

fn git(root: &std::path::Path, args: &[&str]) {
    assert!(std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap()
        .success());
}
