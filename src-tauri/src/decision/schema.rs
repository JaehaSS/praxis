use sqlx::SqlitePool;

const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS decision_records (
  id                INTEGER PRIMARY KEY AUTOINCREMENT,
  decision_key_hash TEXT NOT NULL UNIQUE
                    CHECK(length(decision_key_hash) = 64
                      AND decision_key_hash NOT GLOB '*[^0-9a-f]*'),
  kind              TEXT,
  outcome           TEXT,
  actor_kind        TEXT,
  task_id           INTEGER,
  summary           TEXT,
  status            TEXT NOT NULL DEFAULT 'active'
                    CHECK(status IN ('active', 'redacted')),
  created_at        INTEGER NOT NULL,
  redacted_at       INTEGER,
  CHECK(
    (status = 'active'
      AND kind = 'task_approval'
      AND outcome = 'approved'
      AND actor_kind = 'local_human'
      AND task_id IS NOT NULL
      AND summary = 'Isolated worktree changes approved and merged.'
      AND redacted_at IS NULL)
    OR
    (status = 'redacted'
      AND kind IS NULL
      AND outcome IS NULL
      AND actor_kind IS NULL
      AND task_id IS NULL
      AND summary IS NULL
      AND redacted_at IS NOT NULL)
  )
);
CREATE INDEX IF NOT EXISTS decision_records_task_status
  ON decision_records(task_id, status);

CREATE TABLE IF NOT EXISTS decision_artifact_links (
  decision_id  INTEGER NOT NULL,
  relation     TEXT NOT NULL CHECK(relation IN (
    'used', 'generated', 'derived_from', 'approved_by', 'supersedes', 'blocked_by'
  )),
  artifact_kind TEXT NOT NULL CHECK(artifact_kind IN (
    'task', 'instruction_digest', 'task_start_receipt', 'memory_version',
    'evidence_check', 'verification_run', 'git_commit', 'actor'
  )),
  artifact_ref TEXT NOT NULL CHECK(length(artifact_ref) BETWEEN 1 AND 128),
  created_at   INTEGER NOT NULL,
  PRIMARY KEY(decision_id, relation, artifact_kind, artifact_ref)
);
CREATE INDEX IF NOT EXISTS decision_links_artifact
  ON decision_artifact_links(artifact_kind, artifact_ref, decision_id);

CREATE TABLE IF NOT EXISTS local_approval_finalizations (
  task_id               INTEGER PRIMARY KEY,
  state                 TEXT NOT NULL CHECK(state IN (
    'prepared', 'projection_retired', 'committed', 'merged', 'cleaned', 'completed'
  )),
  commit_sha            TEXT,
  exclude_generated_mcp INTEGER NOT NULL CHECK(exclude_generated_mcp IN (0, 1)),
  failure_code          TEXT CHECK(failure_code IS NULL OR failure_code IN (
    'projection_retirement_failed', 'protected_path_changed', 'git_commit_failed',
    'git_merge_failed', 'git_cleanup_failed', 'ledger_commit_failed'
  )),
  created_at            INTEGER NOT NULL,
  updated_at            INTEGER NOT NULL,
  CHECK(
    (state IN ('prepared', 'projection_retired') AND commit_sha IS NULL)
    OR
    (state IN ('committed', 'merged', 'cleaned', 'completed')
      AND commit_sha IS NOT NULL
      AND length(commit_sha) IN (40, 64)
      AND commit_sha NOT GLOB '*[^0-9a-f]*')
  )
);

DROP TRIGGER IF EXISTS decision_records_immutable;
DROP TRIGGER IF EXISTS decision_records_no_delete;
DROP TRIGGER IF EXISTS decision_artifact_links_immutable;
DROP TRIGGER IF EXISTS decision_artifact_links_parent_guard;
DROP TRIGGER IF EXISTS decision_artifact_links_sealed_guard;
DROP TRIGGER IF EXISTS decision_artifact_links_delete_guard;
DROP TRIGGER IF EXISTS local_approval_identity_immutable;
DROP TRIGGER IF EXISTS local_approval_commit_immutable;
DROP TRIGGER IF EXISTS local_approval_state_guard;

CREATE TRIGGER decision_records_immutable
BEFORE UPDATE ON decision_records
WHEN NOT (
  OLD.status = 'active' AND NEW.status = 'redacted'
  AND NEW.id = OLD.id
  AND NEW.decision_key_hash = OLD.decision_key_hash
  AND NEW.created_at = OLD.created_at
  AND NEW.kind IS NULL AND NEW.outcome IS NULL AND NEW.actor_kind IS NULL
  AND NEW.task_id IS NULL AND NEW.summary IS NULL AND NEW.redacted_at IS NOT NULL
)
BEGIN SELECT RAISE(ABORT, 'decision record is immutable'); END;

CREATE TRIGGER decision_records_no_delete
BEFORE DELETE ON decision_records
BEGIN SELECT RAISE(ABORT, 'decision record cannot be deleted'); END;

CREATE TRIGGER decision_artifact_links_immutable
BEFORE UPDATE ON decision_artifact_links
BEGIN SELECT RAISE(ABORT, 'decision artifact link is immutable'); END;

CREATE TRIGGER decision_artifact_links_parent_guard
BEFORE INSERT ON decision_artifact_links
WHEN NOT EXISTS (
  SELECT 1 FROM decision_records
  WHERE id = NEW.decision_id AND status = 'active'
)
BEGIN SELECT RAISE(ABORT, 'decision link requires an active decision'); END;

CREATE TRIGGER decision_artifact_links_sealed_guard
BEFORE INSERT ON decision_artifact_links
WHEN EXISTS (
  SELECT 1 FROM decision_records decision
  JOIN local_approval_finalizations journal ON journal.task_id = decision.task_id
  WHERE decision.id = NEW.decision_id AND journal.state = 'completed'
)
BEGIN SELECT RAISE(ABORT, 'completed decision links are sealed'); END;

CREATE TRIGGER decision_artifact_links_delete_guard
BEFORE DELETE ON decision_artifact_links
WHEN NOT EXISTS (
  SELECT 1 FROM decision_records
  WHERE id = OLD.decision_id AND status = 'redacted'
)
BEGIN SELECT RAISE(ABORT, 'decision links may only be deleted during redaction'); END;

CREATE TRIGGER local_approval_identity_immutable
BEFORE UPDATE OF task_id, exclude_generated_mcp, created_at
ON local_approval_finalizations
BEGIN SELECT RAISE(ABORT, 'local approval identity is immutable'); END;

CREATE TRIGGER local_approval_commit_immutable
BEFORE UPDATE OF commit_sha ON local_approval_finalizations
WHEN OLD.commit_sha IS NOT NULL AND NEW.commit_sha IS NOT OLD.commit_sha
BEGIN SELECT RAISE(ABORT, 'local approval commit is immutable'); END;

CREATE TRIGGER local_approval_state_guard
BEFORE UPDATE OF state ON local_approval_finalizations
WHEN NEW.state != OLD.state AND NOT (
  (OLD.state = 'prepared' AND NEW.state = 'projection_retired')
  OR (OLD.state = 'projection_retired' AND NEW.state = 'committed')
  OR (OLD.state = 'committed' AND NEW.state = 'merged')
  OR (OLD.state = 'merged' AND NEW.state = 'cleaned')
  OR (OLD.state = 'cleaned' AND NEW.state = 'completed')
)
BEGIN SELECT RAISE(ABORT, 'invalid local approval transition'); END;
"#;

pub(super) async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::raw_sql(MIGRATION).execute(pool).await?;
    Ok(())
}
