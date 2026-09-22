//! Crash reconciliation for durable prepared memory projections.

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::{db, memory, projector};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_path(label: &str) -> std::path::PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    temp_root::dir().join(format!("praxis-{label}-{}-{n}", std::process::id()))
}

async fn setup() -> (sqlx::SqlitePool, std::path::PathBuf, String, i64) {
    let db_path = temp_path("recovery-db").with_extension("sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let worktree = temp_path("recovery-wt");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join("CLAUDE.md"), "# owner content\n").unwrap();
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        worktree.to_str().unwrap(),
        "recover projection",
        Some("claude"),
        None,
        "terminal",
        100,
    )
    .await
    .unwrap();
    (
        pool,
        worktree,
        db_path.to_string_lossy().into_owned(),
        task_id,
    )
}

async fn insert_prepared(
    pool: &sqlx::SqlitePool,
    task_id: i64,
    worktree: &std::path::Path,
    preimages: &[projector::TargetPreimage],
) {
    let receipts = serde_json::json!([{
        "memory_id": 1,
        "version": 1,
        "content": "recoverable memory",
        "knowledge_type": "decision",
        "evidence": []
    }]);
    sqlx::query(
        "INSERT INTO memory_projection_journal \
         (task_id, state, worktree_path, target_paths_json, target_hash, renderer_version, \
          ordered_memories_json, preimages_json, created_at, updated_at) \
         VALUES (?, 'prepared', ?, '[\"CLAUDE.md\"]', 'hash', 1, ?, ?, 100, 100)",
    )
    .bind(task_id)
    .bind(worktree.to_string_lossy().as_ref())
    .bind(receipts.to_string())
    .bind(serde_json::to_string(preimages).unwrap())
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn restart_restores_preimages_and_fails_unstarted_task() {
    let (pool, worktree, db_path, task_id) = setup().await;
    let preimages = projector::capture_targets(&worktree, &["CLAUDE.md"]).unwrap();
    insert_prepared(&pool, task_id, &worktree, &preimages).await;
    let block = "<!-- PRAXIS MEMORY START -->\n# Project Memory (Praxis 자동 삽입)\n\n\
- [M-1 · decision · verified · v1 · evidence 0] recoverable memory\n\
\n위 항목을 실제로 활용했다면 응답에 해당 ID(예: M-123)를 표기할 것.\n\
<!-- PRAXIS MEMORY END -->";
    std::fs::write(
        worktree.join("CLAUDE.md"),
        format!("{block}\n\n# owner content\n"),
    )
    .unwrap();

    let recovered = memory::reconcile_prepared_projections(&pool, 200)
        .await
        .unwrap();
    assert_eq!(recovered, 1);
    assert_eq!(
        std::fs::read_to_string(worktree.join("CLAUDE.md")).unwrap(),
        "# owner content\n"
    );
    let journal: (String, Option<String>) = sqlx::query_as(
        "SELECT state, preimages_json FROM memory_projection_journal WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(journal, ("rolled_back".to_string(), None));
    assert_eq!(
        db::get_task(&pool, task_id).await.unwrap().unwrap().state,
        db::state::FAILED
    );
    let _ = std::fs::remove_dir_all(worktree);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn failed_restart_restore_is_quarantined_as_degraded() {
    let (pool, worktree, db_path, task_id) = setup().await;
    let preimages = projector::capture_targets(&worktree, &["CLAUDE.md"]).unwrap();
    insert_prepared(&pool, task_id, &worktree, &preimages).await;
    std::fs::remove_dir_all(&worktree).unwrap();

    assert!(memory::reconcile_prepared_projections(&pool, 200)
        .await
        .is_err());
    let journal: (String, Option<String>) = sqlx::query_as(
        "SELECT state, preimages_json FROM memory_projection_journal WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(journal.0, "degraded");
    assert!(
        journal.1.is_some(),
        "degraded recovery keeps repair material"
    );
    assert_eq!(
        db::get_task(&pool, task_id).await.unwrap().unwrap().state,
        db::state::FAILED
    );
    let _ = std::fs::remove_file(db_path);
}
