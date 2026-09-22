use sqlx::SqlitePool;

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS review_process_receipts (\
           id INTEGER PRIMARY KEY AUTOINCREMENT, task_id INTEGER NOT NULL, \
           operation TEXT NOT NULL, phase TEXT NOT NULL, pgid INTEGER NOT NULL, \
           identity_hash TEXT NOT NULL, created_at INTEGER NOT NULL)",
    )
    .execute(pool)
    .await?;
    // Replace the old validator atomically so existing databases admit repair receipts.
    let mut tx = pool.begin().await?;
    sqlx::query("DROP TRIGGER IF EXISTS review_process_receipts_valid").execute(&mut *tx).await?;
    sqlx::query("CREATE TRIGGER review_process_receipts_valid BEFORE INSERT ON review_process_receipts WHEN NEW.pgid <= 0 OR length(NEW.identity_hash) != 64 OR NOT ((NEW.operation = 'verify' AND NEW.phase IN ('verify_build','verify_test')) OR (NEW.operation = 'challenge' AND NEW.phase = 'reviewer') OR (NEW.operation = 'repair' AND NEW.phase IN ('repair_agent','repair_check'))) BEGIN SELECT RAISE(ABORT, 'invalid review process receipt'); END")
        .execute(&mut *tx).await?;
    tx.commit().await?;
    execute_statements(pool, CORE_OBJECTS).await?;
    ensure_reason_column(pool).await?;
    execute_statements(pool, REASON_TRIGGERS).await
}

const CORE_OBJECTS: &[&str] = &[
    "CREATE TRIGGER IF NOT EXISTS review_process_receipts_immutable \
         BEFORE UPDATE ON review_process_receipts BEGIN \
         SELECT RAISE(ABORT, 'review process receipt is immutable'); END",
    "CREATE TRIGGER IF NOT EXISTS review_process_receipts_no_delete \
         BEFORE DELETE ON review_process_receipts BEGIN \
         SELECT RAISE(ABORT, 'review process receipt cannot be deleted'); END",
    "CREATE TRIGGER IF NOT EXISTS review_process_receipts_pgid_range \
         BEFORE INSERT ON review_process_receipts WHEN NEW.pgid > 2147483647 BEGIN \
         SELECT RAISE(ABORT, 'invalid review process pgid'); END",
    "CREATE TABLE IF NOT EXISTS review_process_leases (\
           task_id INTEGER NOT NULL, operation TEXT NOT NULL, receipt_id INTEGER NOT NULL UNIQUE, \
           state TEXT NOT NULL, detail TEXT, reason TEXT, updated_at INTEGER NOT NULL, \
           PRIMARY KEY (task_id, operation))",
    "CREATE TRIGGER IF NOT EXISTS review_process_leases_valid_insert \
         BEFORE INSERT ON review_process_leases WHEN NEW.state NOT IN ('active', 'quarantined') OR \
         NOT EXISTS (SELECT 1 FROM review_process_receipts r WHERE r.id = NEW.receipt_id \
           AND r.task_id = NEW.task_id AND r.operation = NEW.operation) BEGIN \
         SELECT RAISE(ABORT, 'invalid review process lease'); END",
    "CREATE TRIGGER IF NOT EXISTS review_process_leases_valid_update \
         BEFORE UPDATE ON review_process_leases WHEN NEW.state NOT IN ('active', 'quarantined') OR \
         NEW.task_id != OLD.task_id OR NEW.operation != OLD.operation OR \
         NEW.receipt_id != OLD.receipt_id BEGIN \
         SELECT RAISE(ABORT, 'invalid review process lease update'); END",
];

const REASON_TRIGGERS: &[&str] = &[
    "CREATE TRIGGER IF NOT EXISTS review_process_leases_reason_insert \
         BEFORE INSERT ON review_process_leases WHEN NEW.reason IS NOT NULL AND \
         NEW.reason NOT IN ('identity_mismatch', 'leaderless_group', \
           'invalid_process_group_id', 'verification_uncertain') BEGIN \
         SELECT RAISE(ABORT, 'invalid review process quarantine reason'); END",
    "CREATE TRIGGER IF NOT EXISTS review_process_leases_reason_update \
         BEFORE UPDATE ON review_process_leases WHEN NEW.reason IS NOT NULL AND \
         NEW.reason NOT IN ('identity_mismatch', 'leaderless_group', \
           'invalid_process_group_id', 'verification_uncertain') BEGIN \
         SELECT RAISE(ABORT, 'invalid review process quarantine reason'); END",
];

async fn execute_statements(pool: &SqlitePool, statements: &[&str]) -> anyhow::Result<()> {
    for statement in statements {
        sqlx::query(statement).execute(pool).await?;
    }
    Ok(())
}

async fn ensure_reason_column(pool: &SqlitePool) -> anyhow::Result<()> {
    let columns = sqlx::query_scalar::<_, String>(
        "SELECT name FROM pragma_table_info('review_process_leases')",
    )
    .fetch_all(pool)
    .await?;
    if columns.iter().any(|column| column == "reason") {
        return Ok(());
    }
    sqlx::query("ALTER TABLE review_process_leases ADD COLUMN reason TEXT")
        .execute(pool)
        .await?;
    Ok(())
}
