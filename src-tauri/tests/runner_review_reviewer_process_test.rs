#![cfg(unix)]

#[path = "support/temp_root.rs"]
mod temp_root;

use std::os::unix::fs::PermissionsExt;

use praxis_lib::db;
use praxis_lib::runner::review_process::{self, ReviewOperation, ReviewPhase};

#[tokio::test(flavor = "multi_thread")]
async fn managed_reviewer_records_and_resolves_a_durable_process_receipt() {
    let root = temp_root::dir().join(format!("praxis-managed-reviewer-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let reviewer = root.join("claude");
    std::fs::write(&reviewer, "#!/bin/sh\ncat >/dev/null\nprintf reviewed\n").unwrap();
    std::fs::set_permissions(&reviewer, std::fs::Permissions::from_mode(0o700)).unwrap();
    let prior_path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![root.clone()];
    paths.extend(std::env::split_paths(&prior_path));
    std::env::set_var("PATH", std::env::join_paths(paths).unwrap());

    let pool = db::init_pool(&root.join("test.sqlite").to_string_lossy())
        .await
        .unwrap();
    let task_id = db::insert_task(
        &pool, "/repo", "branch", "main", "/wt", "review", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    let registrar = review_process::registrar(
        pool.clone(),
        task_id,
        ReviewOperation::Challenge,
        ReviewPhase::Reviewer,
    )
    .unwrap();
    let output = tokio::task::spawn_blocking(move || {
        praxis_lib::reviewer::run_reviewer_registered("claude", "prompt", 5, Some(&registrar))
    })
    .await
    .unwrap()
    .unwrap();

    assert_eq!(output, "reviewed");
    let receipts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM review_process_receipts \
         WHERE task_id = ? AND operation = 'challenge' AND phase = 'reviewer'",
    )
    .bind(task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(receipts, 1);
    assert!(
        review_process::lease(&pool, task_id, ReviewOperation::Challenge)
            .await
            .unwrap()
            .is_none()
    );
    std::env::set_var("PATH", prior_path);
}
