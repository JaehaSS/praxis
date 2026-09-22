#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::decision::approval_journal::{self, FailureCode, Stage};
use praxis_lib::{db, decision};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_db(label: &str) -> String {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    temp_root::dir()
        .join(format!(
            "praxis-decision-journal-{label}-{}-{sequence}.sqlite",
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
async fn claim_atomically_prepares_the_journal_and_task() {
    let path = temp_db("claim");
    let pool = db::init_pool(&path).await.unwrap();
    let task_id = review_task(&pool, "claim").await;

    let task = approval_journal::claim(&pool, task_id, true, 20)
        .await
        .unwrap();
    assert_eq!(task.state, db::state::FINALIZING);
    let row = approval_journal::load(&pool, task_id).await.unwrap();
    assert_eq!(row.state, "prepared");
    assert!(row.exclude_generated_mcp);
    assert_eq!(row.commit_sha, None);
    assert_eq!(row.failure_code, None);

    let second = review_task(&pool, "rollback").await;
    sqlx::raw_sql(
        "CREATE TRIGGER fail_local_journal BEFORE INSERT ON local_approval_finalizations \
         BEGIN SELECT RAISE(ABORT, 'forced journal failure'); END;",
    )
    .execute(&pool)
    .await
    .unwrap();
    assert!(approval_journal::claim(&pool, second, false, 30)
        .await
        .unwrap_err()
        .to_string()
        .contains("forced journal failure"));
    assert_eq!(
        db::get_task(&pool, second).await.unwrap().unwrap().state,
        db::state::AWAITING_REVIEW
    );
    drop(pool);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn claim_is_idempotent_only_for_the_frozen_commit_policy() {
    let path = temp_db("policy");
    let pool = db::init_pool(&path).await.unwrap();
    let task_id = review_task(&pool, "policy").await;

    approval_journal::claim(&pool, task_id, false, 20)
        .await
        .unwrap();
    approval_journal::claim(&pool, task_id, false, 21)
        .await
        .unwrap();
    let conflict = approval_journal::claim(&pool, task_id, true, 22)
        .await
        .unwrap_err();
    assert!(conflict.to_string().contains("commit policy conflicts"));
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM local_approval_finalizations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1);
    drop(pool);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn stage_and_failure_accept_only_closed_bounded_values() {
    let path = temp_db("stage");
    let pool = db::init_pool(&path).await.unwrap();
    let task_id = review_task(&pool, "stage").await;
    approval_journal::claim(&pool, task_id, false, 20)
        .await
        .unwrap();

    approval_journal::failure(&pool, task_id, FailureCode::GitMergeFailed, 21)
        .await
        .unwrap();
    approval_journal::stage(&pool, task_id, Stage::ProjectionRetired, None, 22)
        .await
        .unwrap();
    approval_journal::stage(&pool, task_id, Stage::ProjectionRetired, None, 22)
        .await
        .unwrap();
    approval_journal::stage(
        &pool,
        task_id,
        Stage::Committed,
        Some("0123456789abcdef0123456789abcdef01234567"),
        23,
    )
    .await
    .unwrap();
    let row = approval_journal::load(&pool, task_id).await.unwrap();
    assert_eq!(row.state, "committed");
    assert_eq!(row.failure_code, None);
    assert_eq!(
        row.commit_sha.as_deref(),
        Some("0123456789abcdef0123456789abcdef01234567")
    );

    let raw = sqlx::query(
        "UPDATE local_approval_finalizations SET failure_code = 'PRIVATE_RAW_STDERR' WHERE task_id = ?",
    )
    .bind(task_id)
    .execute(&pool)
    .await
    .unwrap_err();
    assert!(raw.to_string().contains("CHECK constraint failed"));
    assert!(!format!("{row:?}").contains("PRIVATE_RAW_STDERR"));
    drop(pool);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn feature_flag_defaults_off_and_can_be_enabled_explicitly() {
    let path = temp_db("flag");
    let pool = db::init_pool(&path).await.unwrap();
    assert!(!decision::is_enabled(&pool).await.unwrap());
    db::set_setting(&pool, decision::FLAG_KEY, "true")
        .await
        .unwrap();
    assert!(decision::is_enabled(&pool).await.unwrap());
    drop(pool);
    let _ = std::fs::remove_file(path);
}
