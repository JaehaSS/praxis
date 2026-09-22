//! Start promotion must cover each projected evidence row exactly once.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db::{self, state};
use praxis_lib::{evidence, memory};

#[tokio::test]
async fn duplicate_generations_for_one_evidence_cannot_replace_another() {
    let path = temp_root::dir().join(format!(
        "praxis-runner-receipt-security-{}.sqlite",
        std::process::id()
    ));
    let pool = db::init_pool(path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let memory_id = memory::create_candidate(
        &pool,
        memory::tier::GLOBAL,
        None,
        memory::knowledge_type::CLAIM,
        "exact evidence coverage",
        Some("test"),
        99,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(&pool, memory_id, 100, None)
        .await
        .unwrap();
    memory::add_user_confirmation(&pool, memory_id, 101, None)
        .await
        .unwrap();
    memory::submit_for_review(&pool, memory_id, 102)
        .await
        .unwrap();
    memory::approve(&pool, memory_id, "human", 103)
        .await
        .unwrap();
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        "/worktree",
        "run",
        None,
        None,
        "terminal",
        104,
    )
    .await
    .unwrap();
    let projection_id = insert_projection(&pool, task_id, memory_id).await;
    db::update_state(&pool, task_id, state::QUEUED, 105)
        .await
        .unwrap();
    db::claim_oldest_queued_task(&pool, 106).await.unwrap();
    let first = evidence::revalidate_memory(&pool, memory_id, 200)
        .await
        .unwrap();
    let second = evidence::revalidate_memory(&pool, memory_id, 200)
        .await
        .unwrap();
    let forged = serde_json::to_string(&[first.check_ids[0], second.check_ids[0]]).unwrap();

    assert!(
        db::promote_starting_task(&pool, task_id, Some((projection_id, forged.as_str())), 200)
            .await
            .is_err()
    );
    assert_eq!(
        db::get_task(&pool, task_id).await.unwrap().unwrap().state,
        state::STARTING
    );
    let _ = std::fs::remove_file(path);
}

async fn insert_projection(pool: &sqlx::SqlitePool, task_id: i64, memory_id: i64) -> i64 {
    let projection_id = sqlx::query(
        "INSERT INTO memory_projection_journal \
         (task_id, state, worktree_path, target_paths_json, target_hash, renderer_version, \
          ordered_memories_json, source_check_ids_json, created_at, updated_at) \
         VALUES (?, 'applied', '/worktree', '[]', 'hash', 1, '[]', '[]', 104, 104)",
    )
    .bind(task_id)
    .execute(pool)
    .await
    .unwrap()
    .last_insert_rowid();
    sqlx::query(
        "INSERT INTO memory_injections \
         (memory_id, version, task_id, target_hash, injected_at, projection_id, \
          evidence_snapshot_json, source_check_ids_json, target_paths_json, renderer_version) \
         VALUES (?, 1, ?, 'hash', 104, ?, '[]', '[]', '[]', 1)",
    )
    .bind(memory_id)
    .bind(task_id)
    .bind(projection_id)
    .execute(pool)
    .await
    .unwrap();
    projection_id
}
