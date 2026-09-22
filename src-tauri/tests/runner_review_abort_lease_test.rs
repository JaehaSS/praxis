#![cfg(unix)]

#[path = "support/temp_root.rs"]
mod temp_root;

use std::time::Duration;

use praxis_lib::db;
use praxis_lib::review_ops::{verify, ReviewClaims};
use praxis_lib::runner::review_process::{self, ReviewOperation, ReviewPhase};

#[tokio::test(flavor = "multi_thread")]
async fn aborted_verify_request_keeps_durable_lease_until_worker_reaps() {
    let root =
        temp_root::dir().join(format!("praxis-review-abort-lease-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".praxis")).unwrap();
    std::fs::write(
        root.join(".praxis").join("validate.toml"),
        "test = \"sleep 1; printf '1 passed'\"\n",
    )
    .unwrap();
    let root = root.canonicalize().unwrap();
    let pool = db::init_pool(&root.join("test.sqlite").to_string_lossy())
        .await
        .unwrap();
    let root_text = root.to_string_lossy();
    let task_id = db::insert_task(
        &pool, &root_text, "branch", "main", &root_text, "verify", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    let preview = verify::preview(&pool, task_id, &root).await.unwrap();
    let build = review_process::registrar(
        pool.clone(),
        task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
    )
    .unwrap();
    let test = review_process::registrar(
        pool.clone(),
        task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyTest,
    )
    .unwrap();
    let worker = tokio::spawn(verify::run_managed(
        pool.clone(),
        ReviewClaims::default(),
        task_id,
        root,
        preview.preview_token,
        build,
        test,
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    worker.abort();

    assert!(
        review_process::lease(&pool, task_id, ReviewOperation::Verify)
            .await
            .unwrap()
            .is_some()
    );
    tokio::time::sleep(Duration::from_millis(1_200)).await;
    assert!(
        review_process::lease(&pool, task_id, ReviewOperation::Verify)
            .await
            .unwrap()
            .is_none()
    );
    assert!(db::get_evidence(&pool, task_id).await.unwrap().is_some());
}
