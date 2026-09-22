//! A Runner spawn receipt must bind fresh backend checks to its own projection.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db::{self, state};
use praxis_lib::runner::queue::QueueWorker;
use praxis_lib::{evidence, memory, projector};

#[tokio::test]
async fn unchanged_source_mints_task_bound_receipt_before_spawn() {
    let root = test_repository();
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
        "task-bound evidence receipt",
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
        "task-bound evidence receipt",
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

    assert_eq!(
        QueueWorker::new(pool.clone(), 1)
            .run_next(now + 5)
            .await
            .unwrap(),
        Some(0)
    );
    let checks: String =
        sqlx::query_scalar("SELECT source_checks_json FROM task_start_receipts WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let check_ids: Vec<i64> = serde_json::from_str(&checks).unwrap();
    assert!(!check_ids.is_empty());
    let linked: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_start_receipt_checks WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(linked, check_ids.len() as i64);
    assert_eq!(
        db::get_task(&pool, task_id).await.unwrap().unwrap().state,
        state::AWAITING_REVIEW
    );
    assert!(
        sqlx::query("DELETE FROM task_start_receipts WHERE task_id = ?")
            .bind(task_id)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("DELETE FROM task_start_receipt_checks WHERE task_id = ?")
            .bind(task_id)
            .execute(&pool)
            .await
            .is_err()
    );
    db::delete_task(&pool, task_id).await.unwrap();
    let retained: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_start_receipts WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(retained, 1);
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(root);
}

fn test_repository() -> std::path::PathBuf {
    let root = temp_root::dir().join(format!(
        "praxis-runner-evidence-receipt-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "task-bound evidence receipt\n").unwrap();
    std::fs::write(root.join("CLAUDE.md"), "# owner\n").unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "receipt@example.test"]);
    git(&root, &["config", "user.name", "Receipt Test"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "initial"]);
    root.canonicalize().unwrap()
}

async fn verified_memory(pool: &sqlx::SqlitePool, root: &std::path::Path, now: i64) {
    let memory_id = memory::create_candidate(
        pool,
        memory::tier::PROJECT,
        Some(root.to_string_lossy().as_ref()),
        memory::knowledge_type::CLAIM,
        "task-bound evidence receipt",
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
}

fn git(root: &std::path::Path, args: &[&str]) {
    assert!(std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap()
        .success());
}
