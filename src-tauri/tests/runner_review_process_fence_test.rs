#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db;
use praxis_lib::runner::review_process::{self, ReviewOperation, ReviewPhase};

#[tokio::test]
async fn active_review_lease_excludes_a_queued_task_from_worker_claiming() {
    let path = temp_root::dir().join(format!(
        "praxis-review-queue-fence-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = db::init_pool(path.to_str().unwrap()).await.unwrap();
    review_process::migrate(&pool).await.unwrap();
    let task_id = db::insert_task(
        &pool, "/repo", "branch", "main", "/wt", "review", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, db::state::QUEUED, 2)
        .await
        .unwrap();
    review_process::register(
        &pool,
        task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        401,
        &"d".repeat(64),
        3,
    )
    .await
    .unwrap();

    assert!(db::claim_oldest_queued_task(&pool, 4)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        db::get_task(&pool, task_id).await.unwrap().unwrap().state,
        db::state::QUEUED
    );
}

#[tokio::test]
async fn execution_transition_and_review_registration_are_mutually_exclusive() {
    let path = temp_root::dir().join(format!(
        "praxis-review-transition-fence-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = db::init_pool(path.to_str().unwrap()).await.unwrap();
    review_process::migrate(&pool).await.unwrap();

    let starting_id = db::insert_task(
        &pool, "/repo", "starting", "main", "/wt-a", "start", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    db::update_state(&pool, starting_id, db::state::STARTING, 2)
        .await
        .unwrap();
    assert!(review_process::register(
        &pool,
        starting_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        402,
        &"e".repeat(64),
        3,
    )
    .await
    .is_err());

    let review_id = db::insert_task(
        &pool,
        "/repo",
        "review",
        "main",
        "/wt-b",
        "resume",
        None,
        None,
        "conversation",
        4,
    )
    .await
    .unwrap();
    db::update_state(&pool, review_id, db::state::AWAITING_REVIEW, 5)
        .await
        .unwrap();
    review_process::register(
        &pool,
        review_id,
        ReviewOperation::Challenge,
        ReviewPhase::Reviewer,
        403,
        &"f".repeat(64),
        6,
    )
    .await
    .unwrap();
    assert!(!db::mark_running_from_review(&pool, review_id, 7)
        .await
        .unwrap());
    assert_eq!(
        db::get_task(&pool, review_id).await.unwrap().unwrap().state,
        db::state::AWAITING_REVIEW
    );
}
