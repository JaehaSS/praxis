use sqlx::SqlitePool;

const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS runner_finalizations (
  task_id        INTEGER PRIMARY KEY,
  decision       TEXT NOT NULL,
  state          TEXT NOT NULL,
  commit_sha     TEXT,
  failure_reason TEXT,
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL
);
DROP TRIGGER IF EXISTS runner_finalization_identity_immutable;
DROP TRIGGER IF EXISTS runner_finalization_decision_locked;
CREATE TRIGGER runner_finalization_identity_immutable
BEFORE UPDATE OF task_id, created_at ON runner_finalizations
BEGIN SELECT RAISE(ABORT, 'runner finalization identity is immutable'); END;
CREATE TRIGGER runner_finalization_decision_locked
BEFORE UPDATE OF decision ON runner_finalizations
WHEN OLD.state IN ('merged', 'cleaned', 'completed')
BEGIN SELECT RAISE(ABORT, 'runner finalization decision is locked after merge'); END;
"#;

pub(super) async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::raw_sql(MIGRATION).execute(pool).await?;
    Ok(())
}
