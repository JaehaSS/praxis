//! A pending external task must revalidate its immutable projection before agent start.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db;
use praxis_lib::memory::{self, knowledge_type, tier};
use praxis_lib::projector;

struct Fixture {
    pool: sqlx::SqlitePool,
    root: std::path::PathBuf,
    db_path: std::path::PathBuf,
    task_id: i64,
    now: i64,
}

async fn verified_memory(pool: &sqlx::SqlitePool, now: i64) {
    let memory_id = memory::create_candidate(
        pool,
        tier::PROJECT,
        Some("/repo"),
        knowledge_type::DECISION,
        "revalidate evidence immediately before start",
        Some("test"),
        now,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(pool, memory_id, now, Some(now + 3))
        .await
        .unwrap();
    memory::submit_for_review(pool, memory_id, now)
        .await
        .unwrap();
    memory::approve(pool, memory_id, "human", now)
        .await
        .unwrap();
}

async fn created_task(pool: &sqlx::SqlitePool, root: &std::path::Path, now: i64) -> i64 {
    db::insert_task(
        pool,
        "/repo",
        "branch",
        "main",
        root.to_str().unwrap(),
        "pending external task",
        Some("claude"),
        None,
        "telegram",
        now,
    )
    .await
    .unwrap()
}

async fn projected_fixture() -> Fixture {
    let root = temp_root::dir().join(format!("praxis-start-gate-{}", std::process::id()));
    let db_path = root.with_extension("sqlite");
    let _ = std::fs::remove_file(&db_path);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("AGENTS.md"), "# owner\n").unwrap();
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    verified_memory(&pool, now).await;
    let task_id = created_task(&pool, &root, now).await;
    let targets = projector::project_targets();
    memory::inject_into_worktree(
        &pool,
        "/repo",
        "revalidate",
        None,
        task_id,
        now + 1,
        &root,
        8,
        &targets,
    )
    .await
    .unwrap();
    Fixture {
        pool,
        root,
        db_path,
        task_id,
        now,
    }
}

#[tokio::test]
async fn expired_evidence_and_tampering_block_start_revalidation() {
    let fixture = projected_fixture().await;
    db::update_state(
        &fixture.pool,
        fixture.task_id,
        db::state::PENDING_APPROVAL,
        fixture.now + 2,
    )
    .await
    .unwrap();
    assert!(
        memory::verify_task_projection(&fixture.pool, fixture.task_id, fixture.now + 4)
            .await
            .is_err()
    );
    let state: (String,) = journal_state(&fixture).await;
    assert_eq!(
        state.0, "applied",
        "expiry does not corrupt the file receipt"
    );
    std::fs::write(fixture.root.join("AGENTS.md"), "# independently changed\n").unwrap();
    assert!(
        memory::verify_task_projection(&fixture.pool, fixture.task_id, fixture.now + 2)
            .await
            .is_err()
    );
    assert_eq!(journal_state(&fixture).await.0, "degraded");
    let _ = std::fs::remove_dir_all(fixture.root);
    let _ = std::fs::remove_file(fixture.db_path);
}

async fn journal_state(fixture: &Fixture) -> (String,) {
    sqlx::query_as("SELECT state FROM memory_projection_journal WHERE task_id = ?")
        .bind(fixture.task_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap()
}
