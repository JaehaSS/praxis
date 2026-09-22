//! Projection finalization must never succeed after a task leaves Created.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db;
use praxis_lib::memory::{self, knowledge_type, tier};
use praxis_lib::projector;

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

#[tokio::test]
async fn task_state_race_rolls_projection_back_without_a_receipt() {
    let root = temp_root::dir().join(format!("praxis-state-race-{}", std::process::id()));
    let db_path = root.with_extension("sqlite");
    let _ = std::fs::remove_file(&db_path);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("CLAUDE.md"), "# owner content\n").unwrap();
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let now = now_ts();
    let memory_id = memory::create_candidate(
        &pool,
        tier::PROJECT,
        Some("/repo"),
        knowledge_type::DECISION,
        "only Created tasks can finalize memory",
        Some("test"),
        now,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(&pool, memory_id, now, None)
        .await
        .unwrap();
    memory::submit_for_review(&pool, memory_id, now)
        .await
        .unwrap();
    memory::approve(&pool, memory_id, "human", now)
        .await
        .unwrap();
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        root.to_str().unwrap(),
        "race",
        Some("claude"),
        None,
        "terminal",
        now,
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, db::state::RUNNING, now)
        .await
        .unwrap();

    let targets = projector::project_targets();
    let result = memory::inject_into_worktree(
        &pool, "/repo", "race", None, task_id, now, &root, 8, &targets,
    )
    .await;
    assert!(result.is_err());
    assert_eq!(
        std::fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
        "# owner content\n"
    );
    let rows: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM memory_injections WHERE task_id = ?")
        .bind(task_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows.0, 0);
    let state: (String,) =
        sqlx::query_as("SELECT state FROM memory_projection_journal WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state.0, "rolled_back");
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_file(db_path);
}
