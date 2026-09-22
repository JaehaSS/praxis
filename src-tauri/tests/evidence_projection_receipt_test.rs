//! Projection receipts keep per-memory check provenance while the journal keeps the union.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory, projector};

#[tokio::test]
async fn each_injection_receipt_contains_only_its_memory_checks() {
    let root =
        temp_root::dir().join(format!("praxis-projection-receipt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("CLAUDE.md"), "# owner\n").unwrap();
    let root = root.canonicalize().unwrap();
    let db_path = root.with_extension("sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let now = 2_000_000_000;
    verified_memory(&pool, &root, "provenance alpha", now).await;
    verified_memory(&pool, &root, "provenance beta", now + 10).await;
    let task_id = db::insert_task(
        &pool,
        root.to_string_lossy().as_ref(),
        "branch",
        "main",
        root.to_string_lossy().as_ref(),
        "provenance alpha beta",
        None,
        None,
        "terminal",
        now + 20,
    )
    .await
    .unwrap();

    assert_eq!(
        memory::inject_into_worktree(
            &pool,
            root.to_string_lossy().as_ref(),
            "provenance alpha beta",
            None,
            task_id,
            now + 21,
            &root,
            8,
            &projector::project_targets(),
        )
        .await
        .unwrap(),
        2
    );
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT source_check_ids_json FROM memory_injections WHERE task_id = ? ORDER BY memory_id",
    )
    .bind(task_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    let first: Vec<i64> = serde_json::from_str(&rows[0]).unwrap();
    let second: Vec<i64> = serde_json::from_str(&rows[1]).unwrap();
    let journal: String = sqlx::query_scalar(
        "SELECT source_check_ids_json FROM memory_projection_journal WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let union: Vec<i64> = serde_json::from_str(&journal).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(second.len(), 1);
    assert_ne!(first, second);
    assert_eq!(union.len(), 2);
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(root);
}

async fn verified_memory(pool: &sqlx::SqlitePool, root: &std::path::Path, text: &str, now: i64) {
    let id = memory::create_candidate(
        pool,
        memory::tier::PROJECT,
        Some(root.to_string_lossy().as_ref()),
        memory::knowledge_type::CLAIM,
        text,
        Some("test"),
        now,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(pool, id, now + 1, None)
        .await
        .unwrap();
    memory::submit_for_review(pool, id, now + 2).await.unwrap();
    memory::approve(pool, id, "human", now + 3).await.unwrap();
}
