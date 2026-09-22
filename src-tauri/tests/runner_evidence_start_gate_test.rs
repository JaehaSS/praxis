//! Runner must verify source evidence while Starting and never spawn on drift.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db::{self, state};
use praxis_lib::runner::queue::QueueWorker;
use praxis_lib::{evidence, memory, projector};

#[tokio::test]
async fn changed_source_fails_starting_task_and_retires_projection() {
    let root = temp_root::dir().join(format!(
        "praxis-runner-evidence-start-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "original\n").unwrap();
    std::fs::write(root.join("CLAUDE.md"), "# owner\n").unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "start@example.test"]);
    git(&root, &["config", "user.name", "Start Test"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "initial"]);
    let root = root.canonicalize().unwrap();
    let db_path = root.with_extension("sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let now = 2_000_000_000;
    let memory_id = verified_memory(&pool, &root, now).await;
    let task_id = db::insert_task(
        &pool,
        root.to_string_lossy().as_ref(),
        "branch",
        "main",
        root.to_string_lossy().as_ref(),
        "source-backed runner gate",
        Some("/bin/echo"),
        None,
        "terminal",
        now,
    )
    .await
    .unwrap();
    let targets = projector::project_targets();
    memory::inject_into_worktree(
        &pool,
        root.to_string_lossy().as_ref(),
        "source-backed runner gate",
        None,
        task_id,
        now + 3,
        &root,
        8,
        &targets,
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, state::QUEUED, now + 4)
        .await
        .unwrap();
    std::fs::write(root.join("src/lib.rs"), "changed\n").unwrap();

    assert!(QueueWorker::new(pool.clone(), 1)
        .run_next(now + 5)
        .await
        .is_err());
    assert_eq!(
        db::get_task(&pool, task_id).await.unwrap().unwrap().state,
        state::FAILED
    );
    let receipts: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_start_receipts WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let journal: String =
        sqlx::query_scalar("SELECT state FROM memory_projection_journal WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(receipts, 0);
    assert_eq!(journal, "retired");
    let memory_status: String = sqlx::query_scalar("SELECT status FROM memories WHERE id = ?")
        .bind(memory_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(memory_status, memory::knowledge_status::STALE);
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(root);
}

async fn verified_memory(pool: &sqlx::SqlitePool, root: &std::path::Path, now: i64) -> i64 {
    let memory_id = memory::create_candidate(
        pool,
        memory::tier::PROJECT,
        Some(root.to_string_lossy().as_ref()),
        memory::knowledge_type::CLAIM,
        "source-backed runner gate",
        Some("test"),
        now,
    )
    .await
    .unwrap();
    evidence::add_code_location(
        pool,
        memory_id,
        evidence::CodeLocationInput {
            relative_path: "src/lib.rs".into(),
            line_start: 1,
            line_end: 1,
        },
        now,
    )
    .await
    .unwrap();
    memory::submit_for_review(pool, memory_id, now + 1)
        .await
        .unwrap();
    memory::approve(pool, memory_id, "human", now + 2)
        .await
        .unwrap();
    memory_id
}

fn git(root: &std::path::Path, args: &[&str]) {
    assert!(std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap()
        .success());
}
