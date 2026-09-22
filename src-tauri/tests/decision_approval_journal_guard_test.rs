#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::db;
use praxis_lib::decision::approval_journal::{self, Stage};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_db(label: &str) -> String {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    temp_root::dir()
        .join(format!(
            "praxis-decision-journal-guard-{label}-{}-{sequence}.sqlite",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

async fn review_task(pool: &sqlx::SqlitePool, label: &str) -> i64 {
    let id = db::insert_task(
        pool,
        "/repo",
        &format!("praxis/{label}"),
        "main",
        &format!("/repo/.praxis/worktrees/{label}"),
        label,
        None,
        None,
        "terminal",
        10,
    )
    .await
    .unwrap();
    db::update_state(pool, id, db::state::AWAITING_REVIEW, 11)
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn claim_refuses_a_finalizing_task_without_a_local_journal() {
    let path = temp_db("foreign-owner");
    let pool = db::init_pool(&path).await.unwrap();
    let task_id = review_task(&pool, "foreign-owner").await;
    db::update_state(&pool, task_id, db::state::FINALIZING, 19)
        .await
        .unwrap();

    let error = approval_journal::claim(&pool, task_id, false, 20)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("owned by another finalizer"));
    let journals: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM local_approval_finalizations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(journals, 0);
    drop(pool);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn committed_stage_rejects_revision_expressions() {
    let path = temp_db("commit-identity");
    let pool = db::init_pool(&path).await.unwrap();
    let task_id = review_task(&pool, "commit-identity").await;
    approval_journal::claim(&pool, task_id, false, 20)
        .await
        .unwrap();
    approval_journal::stage(&pool, task_id, Stage::ProjectionRetired, None, 21)
        .await
        .unwrap();
    let expression = format!("HEAD~1^{{commit}}{}", "a".repeat(26));

    let error = approval_journal::stage(&pool, task_id, Stage::Committed, Some(&expression), 22)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("full hexadecimal commit SHA"));
    let journal = approval_journal::load(&pool, task_id).await.unwrap();
    assert_eq!(journal.state, "projection_retired");
    drop(pool);
    let _ = std::fs::remove_file(path);
}
