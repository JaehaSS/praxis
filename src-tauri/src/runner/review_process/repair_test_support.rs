use super::{ReviewOperation, ReviewPhase};

pub(super) struct Fixture {
    pub pool: sqlx::SqlitePool,
    pub task_id: i64,
}

pub(super) async fn fixture(label: &str) -> Fixture {
    let path = crate::testtmp::dir().join(format!(
        "praxis-review-repair-{label}-{}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = crate::db::init_pool(path.to_str().unwrap()).await.unwrap();
    super::migrate(&pool).await.unwrap();
    let task_id = crate::db::insert_task(
        &pool, "/repo", "branch", "main", "/wt", "review", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    Fixture { pool, task_id }
}

pub(super) async fn quarantined_receipt(
    fixture: &Fixture,
    pgid: i64,
) -> super::ReviewProcessReceipt {
    let receipt = super::register(
        &fixture.pool,
        fixture.task_id,
        ReviewOperation::Verify,
        ReviewPhase::VerifyBuild,
        pgid,
        &"a".repeat(64),
        2,
    )
    .await
    .unwrap();
    super::quarantine(
        &fixture.pool,
        fixture.task_id,
        ReviewOperation::Verify,
        receipt.id,
        "unverified ownership",
        3,
    )
    .await
    .unwrap();
    receipt
}

pub(super) async fn latest_event(pool: &sqlx::SqlitePool, task_id: i64) -> (String, String) {
    sqlx::query_as(
        "SELECT kind, detail FROM runner_events \
         WHERE task_id = ? ORDER BY sequence DESC LIMIT 1",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .unwrap()
}
