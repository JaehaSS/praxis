#[path = "support/temp_root.rs"]
mod temp_root;

use std::fs;
use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::{annotations, db, designmode, partial, runner};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_path(label: &str) -> std::path::PathBuf {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    temp_root::dir().join(format!(
        "praxis-task-deletion-{label}-{}-{sequence}",
        std::process::id()
    ))
}

async fn task(pool: &sqlx::SqlitePool, worktree: &std::path::Path, instruction: &str) -> i64 {
    db::insert_task(
        pool,
        "/repo",
        "branch",
        "main",
        worktree.to_string_lossy().as_ref(),
        instruction,
        None,
        None,
        "conversation",
        1,
    )
    .await
    .unwrap()
}

async fn task_row_count(pool: &sqlx::SqlitePool, table: &str, task_id: i64) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE task_id = ?"))
        .bind(task_id)
        .fetch_one(pool)
        .await
        .unwrap()
}

#[tokio::test]
async fn deletion_removes_only_the_target_session_originals() {
    let root = temp_path("isolation");
    fs::create_dir_all(&root).unwrap();
    let db_path = root.join("tasks.sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    partial::migrate(&pool).await.unwrap();
    annotations::migrate(&pool).await.unwrap();
    let worktree = root.join("worktree");
    fs::create_dir_all(&worktree).unwrap();
    let first = task(&pool, &worktree, "first").await;
    let second = task(&pool, &worktree, "second").await;
    for task_id in [first, second] {
        sqlx::query("INSERT INTO convo_events (task_id, ts, event) VALUES (?, 1, '{}')")
            .bind(task_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO convo_events (task_id, ts, event, rewound_at) VALUES (?, 2, 'rewound', 2)",
        )
        .bind(task_id)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO convo_checkpoints (task_id, label, worktree_commit, convo_event_max_id, ts) VALUES (?, 'checkpoint', 'abc', 1, 1)")
            .bind(task_id)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO preview_commands (id, ts, task_id, op, ok, elapsed_ms) VALUES (?, 1, ?, 'run', 1, 1)")
            .bind(task_id)
            .bind(task_id)
            .execute(&pool)
            .await
            .unwrap();
        db::append_task_output(&pool, task_id, 1, "original")
            .await
            .unwrap();
        partial::save_checkpoint(&pool, task_id, "abc", 1)
            .await
            .unwrap();
        annotations::create_draft(&pool, task_id, "hunk", "a.rs", 1, "new", "draft", 1)
            .await
            .unwrap();
    }
    db::set_pending_capsule(&pool, second, "pending")
        .await
        .unwrap();
    designmode::save_capture(&worktree, first, sample_capture(), None).unwrap();
    designmode::save_capture(&worktree, second, sample_capture(), None).unwrap();
    db::update_state(&pool, first, db::state::DONE, 2)
        .await
        .unwrap();

    runner::delete_finished_task(&pool, first).await.unwrap();

    assert!(db::get_task(&pool, first).await.unwrap().is_none());
    assert!(db::get_task(&pool, second).await.unwrap().is_some());
    for table in [
        "convo_events",
        "convo_checkpoints",
        "preview_commands",
        "task_output",
        "partial_checkpoints",
        "review_annotations",
    ] {
        assert_eq!(task_row_count(&pool, table, first).await, 0, "{table}");
        let expected = if table == "convo_events" { 2 } else { 1 };
        assert_eq!(
            task_row_count(&pool, table, second).await,
            expected,
            "{table}"
        );
    }
    assert_eq!(
        db::peek_pending_capsule(&pool, second).await.unwrap(),
        Some("pending".into())
    );
    assert!(!designmode::captures_dir(&worktree, first).exists());
    assert!(designmode::captures_dir(&worktree, second).is_dir());
    drop(pool);
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[tokio::test]
async fn unsafe_capture_parent_rolls_back_task_deletion() {
    let root = temp_path("rollback");
    let worktree = root.join("worktree");
    let outside = root.join("outside");
    fs::create_dir_all(&worktree).unwrap();
    fs::create_dir_all(&outside).unwrap();
    let db_path = root.join("tasks.sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    let task_id = task(&pool, &worktree, "keep").await;
    std::os::unix::fs::symlink(&outside, worktree.join(".praxis")).unwrap();

    let error = db::delete_task(&pool, task_id).await.unwrap_err();

    assert!(error.to_string().contains("심볼릭 링크"));
    assert!(db::get_task(&pool, task_id).await.unwrap().is_some());
    assert!(outside.is_dir());
    drop(pool);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn checked_capture_cleanup_handles_missing_and_symlink_paths() {
    let root = temp_path("paths");
    let missing = root.join("missing");
    assert!(designmode::delete_task_captures(&missing, 1).is_ok());
    let outside = root.join("outside");
    fs::create_dir_all(&outside).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, root.join("linked-worktree")).unwrap();
        assert!(designmode::delete_task_captures(&root.join("linked-worktree"), 1).is_err());
    }
    fs::create_dir_all(root.join("worktree/.praxis/captures")).unwrap();
    fs::write(outside.join("keep"), "keep").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, root.join("worktree/.praxis/captures/1")).unwrap();

    designmode::delete_task_captures(&root.join("worktree"), 1).unwrap();

    assert!(outside.join("keep").is_file());
    assert!(!root.join("worktree/.praxis/captures/1").exists());
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
