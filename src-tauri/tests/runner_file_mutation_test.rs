#[path = "support/temp_root.rs"]
mod temp_root;

use std::time::Duration;

use praxis_lib::db;
use praxis_lib::review_ops::ReviewClaims;
use praxis_lib::runner::review_process::{self, ReviewOperation, ReviewPhase};
use praxis_lib::runner::worktree_lock::WorktreeLocks;

#[tokio::test]
async fn file_write_waits_for_the_worktree_lock_and_rejects_finalizing_tasks() {
    let root = temp_root::dir().join(format!(
        "praxis-runner-file-mutation-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("note.txt"), "before\n").unwrap();
    let db_path = root.with_extension("sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        root.to_str().unwrap(),
        "mutate",
        None,
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();
    let locks = WorktreeLocks::default();
    let claims = ReviewClaims::default();
    let held = locks.acquire(&root).await;
    let pending = {
        let pool = pool.clone();
        let locks = locks.clone();
        let root = root.clone();
        tokio::spawn(async move {
            praxis_lib::runner::file_mutation::write_file(
                &pool,
                &locks,
                &ReviewClaims::default(),
                &root,
                "note.txt",
                "after\n",
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        std::fs::read_to_string(root.join("note.txt")).unwrap(),
        "before\n"
    );
    drop(held);
    pending.await.unwrap().unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("note.txt")).unwrap(),
        "after\n"
    );

    db::update_state(&pool, task_id, db::state::FINALIZING, 2)
        .await
        .unwrap();
    assert!(praxis_lib::runner::file_mutation::write_file(
        &pool,
        &locks,
        &claims,
        &root,
        "note.txt",
        "forbidden\n",
    )
    .await
    .is_err());
    assert_eq!(
        std::fs::read_to_string(root.join("note.txt")).unwrap(),
        "after\n"
    );
    db::update_state(&pool, task_id, db::state::AWAITING_REVIEW, 3)
        .await
        .unwrap();
    let receipt = review_process::register(
        &pool,
        task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        999_991,
        &"a".repeat(64),
        4,
    )
    .await
    .unwrap();
    review_process::quarantine(
        &pool,
        task_id,
        ReviewOperation::Verify,
        receipt.id,
        "unresolved child",
        5,
    )
    .await
    .unwrap();
    assert!(praxis_lib::runner::file_mutation::write_file(
        &pool,
        &locks,
        &claims,
        &root,
        "note.txt",
        "quarantine bypass\n",
    )
    .await
    .is_err());
    assert_eq!(
        std::fs::read_to_string(root.join("note.txt")).unwrap(),
        "after\n"
    );
    let parent = root.parent().unwrap();
    let parent_relative = format!("{}/note.txt", root.file_name().unwrap().to_string_lossy());
    assert!(praxis_lib::runner::file_mutation::write_file(
        &pool,
        &locks,
        &claims,
        parent,
        &parent_relative,
        "parent alias bypass\n",
    )
    .await
    .is_err());
    let subdir = root.join("nested");
    std::fs::create_dir_all(&subdir).unwrap();
    assert!(praxis_lib::runner::file_mutation::write_file(
        &pool,
        &locks,
        &claims,
        &subdir,
        "note.txt",
        "subdir alias bypass\n",
    )
    .await
    .is_err());
    assert!(!subdir.join("note.txt").exists());
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_file(db_path);
}
