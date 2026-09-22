#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::db::{self, state};
use praxis_lib::runner::{self, config::RunnerConfig};

static COUNTER: AtomicU32 = AtomicU32::new(0);

#[tokio::test]
async fn initialization_marks_unowned_running_tasks_failed_with_recovery_event() {
    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let db_path = temp_root::dir()
        .join(format!(
            "praxis-runner-lifecycle-{}-{suffix}.sqlite",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned();
    let pool = db::init_pool(&db_path).await.unwrap();
    let task_id = db::insert_task(
        &pool, "/repo", "branch", "main", "/wt", "run", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, state::RUNNING, 2)
        .await
        .unwrap();
    drop(pool);

    let runtime = runner::initialize(
        RunnerConfig::from_toml(
            "repository_roots = [\"/tmp\"]\npairing_token_file = \"/tmp/praxis-token\"",
        )
        .unwrap(),
        &db_path,
        3,
    )
    .await
    .unwrap();

    assert_eq!(runtime.recovered_tasks(), 1);
    let review_ledger: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master \
         WHERE type = 'table' AND name = 'review_process_leases'",
    )
    .fetch_one(runtime.pool())
    .await
    .unwrap();
    assert_eq!(review_ledger, 1);
    assert_eq!(
        db::get_task(runtime.pool(), task_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        state::FAILED
    );
    assert_eq!(
        db::list_runner_events_after(runtime.pool(), 0, 10)
            .await
            .unwrap()[0]
            .kind,
        "recovery"
    );
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn initialization_migrates_memory_and_leaves_queued_tasks_without_receipts_alone() {
    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let db_path = temp_root::dir()
        .join(format!(
            "praxis-runner-legacy-{}-{suffix}.sqlite",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned();
    let pool = db::init_pool(&db_path).await.unwrap();
    let task_id = db::insert_task(
        &pool, "/repo", "branch", "main", "/wt", "legacy", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, state::QUEUED, 2)
        .await
        .unwrap();
    drop(pool);

    let runtime = runner::initialize(
        RunnerConfig::from_toml(
            "repository_roots = [\"/tmp\"]\npairing_token_file = \"/tmp/praxis-token\"",
        )
        .unwrap(),
        &db_path,
        3,
    )
    .await
    .unwrap();

    // 파일형 투영(설계 2026-09-13)에는 영수증이 없다 — 영수증 부재는 더 이상 결격이 아니다.
    assert_eq!(
        db::get_task(runtime.pool(), task_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        state::QUEUED
    );
    let memory_table: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'memories'",
    )
    .fetch_one(runtime.pool())
    .await
    .unwrap();
    assert_eq!(memory_table.0, 1);
    assert!(!db::list_runner_events_after(runtime.pool(), 0, 10)
        .await
        .unwrap()
        .iter()
        .any(|event| event.kind == "memory_projection_invalid"));
    let _ = std::fs::remove_file(db_path);
}
