use super::{ReviewOperation, ReviewPhase, ReviewProcessState};

#[tokio::test]
async fn receipt_cas_and_quarantine_state_prevent_unsafe_resolution() {
    let path = crate::testtmp::dir().join(format!(
        "praxis-review-ledger-unit-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = crate::db::init_pool(path.to_str().unwrap()).await.unwrap();
    let task_id = crate::db::insert_task(
        &pool, "/repo", "branch", "main", "/wt", "review", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    let first = super::register(
        &pool,
        task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        201,
        &"a".repeat(64),
        2,
    )
    .await
    .unwrap();
    assert!(!super::resolve(
        &pool,
        task_id,
        ReviewOperation::Verify,
        first.id + 1,
        3,
        "review_process_completed",
    )
    .await
    .unwrap());
    assert!(super::resolve(
        &pool,
        task_id,
        ReviewOperation::Verify,
        first.id,
        4,
        "review_process_completed",
    )
    .await
    .unwrap());

    let second = super::register(
        &pool,
        task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyTest,
        202,
        &"b".repeat(64),
        5,
    )
    .await
    .unwrap();
    assert!(!super::resolve(
        &pool,
        task_id,
        ReviewOperation::Verify,
        first.id,
        6,
        "review_process_completed",
    )
    .await
    .unwrap());
    super::quarantine(
        &pool,
        task_id,
        ReviewOperation::Verify,
        second.id,
        "uncertain",
        7,
    )
    .await
    .unwrap();
    assert!(!super::resolve(
        &pool,
        task_id,
        ReviewOperation::Verify,
        second.id,
        8,
        "review_process_completed",
    )
    .await
    .unwrap());
    let lease = super::lease(&pool, task_id, ReviewOperation::Verify)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(lease.state, ReviewProcessState::Quarantined);
    assert_eq!(lease.receipt_id, second.id);
}

#[tokio::test]
async fn registration_rejects_process_group_ids_that_cannot_reach_killpg() {
    let path = crate::testtmp::dir().join(format!(
        "praxis-review-ledger-pgid-unit-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = crate::db::init_pool(path.to_str().unwrap()).await.unwrap();
    let task_id = crate::db::insert_task(
        &pool, "/repo", "branch", "main", "/wt", "review", None, None, "terminal", 1,
    )
    .await
    .unwrap();

    assert!(super::register(
        &pool,
        task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        i64::from(i32::MAX) + 1,
        &"a".repeat(64),
        2,
    )
    .await
    .is_err());
    assert!(sqlx::query(
        "INSERT INTO review_process_receipts \
             (task_id, operation, phase, pgid, identity_hash, created_at) \
             VALUES (?, 'verify', 'verify_build', ?, ?, ?)",
    )
    .bind(task_id)
    .bind(i64::from(i32::MAX) + 1)
    .bind("b".repeat(64))
    .bind(3_i64)
    .execute(&pool)
    .await
    .is_err());
}

#[tokio::test]
async fn migration_adds_validated_reason_column_to_legacy_leases() {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query(
        "CREATE TABLE review_process_leases (\
         task_id INTEGER NOT NULL, operation TEXT NOT NULL, receipt_id INTEGER NOT NULL UNIQUE, \
         state TEXT NOT NULL, detail TEXT, updated_at INTEGER NOT NULL, \
         PRIMARY KEY (task_id, operation))",
    )
    .execute(&pool)
    .await
    .unwrap();

    super::schema::migrate(&pool).await.unwrap();

    let reason_columns: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pragma_table_info('review_process_leases') \
         WHERE name = 'reason'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reason_columns, 1);
}

#[cfg(unix)]
#[tokio::test]
async fn observed_registration_rejects_a_process_that_is_not_group_leader() {
    let path = crate::testtmp::dir().join(format!(
        "praxis-review-nonleader-unit-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = crate::db::init_pool(path.to_str().unwrap()).await.unwrap();
    let task_id = crate::db::insert_task(
        &pool, "/repo", "branch", "main", "/wt", "review", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    let mut child = std::process::Command::new("/bin/sleep")
        .arg("5")
        .spawn()
        .unwrap();

    let result = super::register_observed(
        &pool,
        task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        child.id(),
        2,
    )
    .await;

    let _ = child.kill();
    let _ = child.wait();
    assert!(result.is_err());
}
