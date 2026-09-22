#[path = "support/temp_root.rs"]
mod temp_root;

use std::fs;
use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::{db, designmode};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_path(label: &str) -> std::path::PathBuf {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    temp_root::dir().join(format!(
        "praxis-task-deletion-failure-{label}-{}-{sequence}",
        std::process::id()
    ))
}

async fn task(pool: &sqlx::SqlitePool, worktree: &std::path::Path) -> i64 {
    db::insert_task(
        pool,
        "/repo",
        "branch",
        "main",
        worktree.to_string_lossy().as_ref(),
        "delete",
        None,
        None,
        "conversation",
        1,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn guards_and_row_errors_leave_task_and_captures_intact() {
    let root = temp_path("guards");
    let worktree = root.join("worktree");
    fs::create_dir_all(&worktree).unwrap();
    let pool = db::init_pool(root.join("tasks.sqlite").to_str().unwrap())
        .await
        .unwrap();
    let task_id = task(&pool, &worktree).await;
    designmode::save_capture(&worktree, task_id, sample_capture(), None).unwrap();
    db::set_convo_pgid(&pool, task_id, Some(99)).await.unwrap();

    assert!(db::delete_task(&pool, task_id).await.is_err());
    assert!(db::get_task(&pool, task_id).await.unwrap().is_some());
    assert!(designmode::captures_dir(&worktree, task_id).is_dir());

    db::set_convo_pgid(&pool, task_id, None).await.unwrap();
    sqlx::query(
        "INSERT INTO decision_records \
         (decision_key_hash, kind, outcome, actor_kind, task_id, summary, status, created_at) \
         VALUES (?, 'task_approval', 'approved', 'local_human', ?, \
         'Isolated worktree changes approved and merged.', 'active', 1)",
    )
    .bind("a".repeat(64))
    .bind(task_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::raw_sql(
        "CREATE TRIGGER block_decision_redaction BEFORE UPDATE ON decision_records \
         BEGIN SELECT RAISE(ABORT, 'forced decision redaction failure'); END;",
    )
    .execute(&pool)
    .await
    .unwrap();

    let error = db::delete_task(&pool, task_id).await.unwrap_err();

    assert!(error
        .to_string()
        .contains("forced decision redaction failure"));
    assert!(db::get_task(&pool, task_id).await.unwrap().is_some());
    assert!(designmode::captures_dir(&worktree, task_id).is_dir());
    drop(pool);
    let _ = fs::remove_dir_all(root);
}

#[tokio::test]
async fn absent_optional_tables_do_not_block_deletion() {
    let root = temp_path("optional");
    let worktree = root.join("worktree");
    fs::create_dir_all(&worktree).unwrap();
    let pool = db::init_pool(root.join("tasks.sqlite").to_str().unwrap())
        .await
        .unwrap();
    let task_id = task(&pool, &worktree).await;
    designmode::save_capture(&worktree, task_id, sample_capture(), None).unwrap();

    db::delete_task(&pool, task_id).await.unwrap();

    assert!(db::get_task(&pool, task_id).await.unwrap().is_none());
    assert!(!designmode::captures_dir(&worktree, task_id).exists());
    drop(pool);
    let _ = fs::remove_dir_all(root);
}

fn sample_capture() -> designmode::ElementCapture {
    designmode::ElementCapture {
        outer_html: "<main />".into(),
        computed_css: Default::default(),
        bounding_rect: designmode::BoundingRect {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        },
    }
}
