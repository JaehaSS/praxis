use sqlx::SqlitePool;

pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::raw_sql(MIGRATION).execute(pool).await?;
    let preview_columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('vault_reference_preview_items')")
            .fetch_all(pool)
            .await?;
    if !preview_columns.is_empty()
        && !preview_columns
            .iter()
            .any(|column| column == "stale_reason")
    {
        sqlx::query("ALTER TABLE vault_reference_preview_items ADD COLUMN stale_reason TEXT")
            .execute(pool)
            .await?;
    }
    let policy_columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('vault_draft_policy_sources')")
            .fetch_all(pool)
            .await?;
    if !policy_columns.is_empty()
        && !policy_columns
            .iter()
            .any(|column| column == "grant_fingerprint")
    {
        sqlx::query("ALTER TABLE vault_draft_policy_sources ADD COLUMN grant_fingerprint TEXT NOT NULL DEFAULT ''")
            .execute(pool)
            .await?;
    }
    if !policy_columns.is_empty()
        && !policy_columns.iter().any(|column| column == "stale_reason")
    {
        sqlx::query("ALTER TABLE vault_draft_policy_sources ADD COLUMN stale_reason TEXT")
            .execute(pool)
            .await?;
    }
    let attempt_columns: Vec<String> =
        sqlx::query_scalar("SELECT name FROM pragma_table_info('vault_task_attempts')")
            .fetch_all(pool)
            .await?;
    if !attempt_columns.is_empty()
        && !attempt_columns
            .iter()
            .any(|column| column == "provenance_failed")
    {
        sqlx::query("ALTER TABLE vault_task_attempts ADD COLUMN provenance_failed INTEGER NOT NULL DEFAULT 0")
            .execute(pool)
            .await?;
    }
    if !attempt_columns.is_empty()
        && !attempt_columns
            .iter()
            .any(|column| column == "provenance_complete")
    {
        sqlx::query(
            "ALTER TABLE vault_task_attempts ADD COLUMN provenance_complete INTEGER NOT NULL DEFAULT 0",
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS vault_capture_consents (
  id TEXT PRIMARY KEY,
  binding_id TEXT NOT NULL REFERENCES vault_project_bindings(id),
  binding_epoch TEXT NOT NULL,
  provider TEXT NOT NULL,
  input_kinds_json TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  revoked_at INTEGER
);
CREATE UNIQUE INDEX IF NOT EXISTS vault_active_capture_consent
  ON vault_capture_consents(binding_id, binding_epoch, provider)
  WHERE revoked_at IS NULL;
CREATE TABLE IF NOT EXISTS vault_task_attempts (
  id TEXT PRIMARY KEY,
  task_id INTEGER NOT NULL REFERENCES tasks(id),
  vault_id TEXT NOT NULL REFERENCES vaults(id),
  vault_root TEXT NOT NULL,
  vault_device INTEGER NOT NULL,
  vault_inode INTEGER NOT NULL,
  binding_id TEXT NOT NULL REFERENCES vault_project_bindings(id),
  binding_epoch TEXT NOT NULL,
  provider TEXT NOT NULL,
  client_ref TEXT,
  provenance_failed INTEGER NOT NULL DEFAULT 0,
  provenance_complete INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS vault_attempt_inputs (
  id TEXT PRIMARY KEY,
  attempt_id TEXT NOT NULL REFERENCES vault_task_attempts(id) ON DELETE CASCADE,
  origin_kind TEXT NOT NULL,
  payload_hash TEXT NOT NULL,
  declared_scope TEXT NOT NULL,
  capture_purpose TEXT NOT NULL,
  consent_id TEXT REFERENCES vault_capture_consents(id),
  document_id TEXT REFERENCES vault_documents(id),
  revision_id TEXT REFERENCES vault_revisions(id),
  revision_hash TEXT,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS vault_terminal_snapshots (
  id TEXT PRIMARY KEY,
  attempt_id TEXT NOT NULL UNIQUE REFERENCES vault_task_attempts(id),
  task_id INTEGER NOT NULL REFERENCES tasks(id),
  vault_id TEXT NOT NULL REFERENCES vaults(id),
  vault_root TEXT NOT NULL,
  vault_device INTEGER NOT NULL,
  vault_inode INTEGER NOT NULL,
  binding_id TEXT NOT NULL REFERENCES vault_project_bindings(id),
  binding_epoch TEXT NOT NULL,
  scope TEXT NOT NULL,
  bounded_body TEXT NOT NULL,
  body_hash TEXT NOT NULL,
  input_snapshot_id TEXT NOT NULL REFERENCES vault_input_snapshots(id),
  terminal_hash TEXT NOT NULL,
  terminal_state TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS vault_reference_previews (
  id TEXT PRIMARY KEY,
  binding_id TEXT NOT NULL REFERENCES vault_project_bindings(id),
  binding_epoch TEXT NOT NULL,
  query_hash TEXT NOT NULL,
  client_ref TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('pending','consumed','invalidated')),
  created_at INTEGER NOT NULL,
  consumed_task_id INTEGER REFERENCES tasks(id)
);
CREATE UNIQUE INDEX IF NOT EXISTS vault_one_pending_preview
  ON vault_reference_previews(binding_id, binding_epoch, client_ref)
  WHERE state = 'pending';
CREATE TABLE IF NOT EXISTS vault_draft_policies (
  id TEXT PRIMARY KEY,
  binding_id TEXT NOT NULL REFERENCES vault_project_bindings(id),
  binding_epoch TEXT NOT NULL,
  client_ref TEXT NOT NULL,
  query_hash TEXT NOT NULL,
  input_mode TEXT NOT NULL CHECK(input_mode IN ('default','task_only','private_attachment')),
  state TEXT NOT NULL CHECK(state IN ('pending','consumed','invalidated')),
  created_at INTEGER NOT NULL,
  consumed_task_id INTEGER REFERENCES tasks(id)
);
CREATE UNIQUE INDEX IF NOT EXISTS vault_one_pending_draft_policy
  ON vault_draft_policies(binding_id, binding_epoch, client_ref)
  WHERE state = 'pending';
CREATE TABLE IF NOT EXISTS vault_draft_policy_sources (
  policy_id TEXT NOT NULL REFERENCES vault_draft_policies(id) ON DELETE CASCADE,
  document_id TEXT NOT NULL REFERENCES vault_documents(id),
  revision_id TEXT NOT NULL REFERENCES vault_revisions(id),
  revision_hash TEXT NOT NULL,
  grant_fingerprint TEXT NOT NULL,
  scope TEXT NOT NULL,
  stale_reason TEXT,
  PRIMARY KEY(policy_id, revision_id)
);
CREATE TABLE IF NOT EXISTS vault_input_snapshots (
  id TEXT PRIMARY KEY,
  attempt_id TEXT NOT NULL UNIQUE REFERENCES vault_task_attempts(id) ON DELETE CASCADE,
  vault_id TEXT NOT NULL REFERENCES vaults(id),
  vault_root TEXT NOT NULL,
  vault_device INTEGER NOT NULL,
  vault_inode INTEGER NOT NULL,
  body_hash TEXT NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS vault_reference_preview_items (
  preview_id TEXT NOT NULL REFERENCES vault_reference_previews(id) ON DELETE CASCADE,
  revision_id TEXT NOT NULL REFERENCES vault_revisions(id),
  revision_hash TEXT NOT NULL,
  snippet TEXT NOT NULL,
  reason TEXT NOT NULL,
  excluded INTEGER NOT NULL DEFAULT 0,
  stale_reason TEXT,
  PRIMARY KEY(preview_id, revision_id)
);
CREATE TABLE IF NOT EXISTS vault_usages (
  id TEXT PRIMARY KEY,
  task_id INTEGER NOT NULL REFERENCES tasks(id),
  attempt_id TEXT NOT NULL REFERENCES vault_task_attempts(id),
  revision_id TEXT NOT NULL REFERENCES vault_revisions(id),
  revision_hash TEXT NOT NULL,
  snippet TEXT NOT NULL,
  snippet_hash TEXT NOT NULL,
  delivery_state TEXT NOT NULL CHECK(delivery_state IN ('pending','delivered','not_delivered')),
  citation_state TEXT NOT NULL CHECK(citation_state IN ('cited','unknown')),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS vault_usage_once
  ON vault_usages(task_id, attempt_id, revision_id);
"#;
