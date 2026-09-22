pub const MIGRATION: &str = r#"
CREATE TABLE IF NOT EXISTS vaults (
  id TEXT PRIMARY KEY,
  canonical_root TEXT NOT NULL UNIQUE,
  root_device INTEGER NOT NULL,
  root_inode INTEGER NOT NULL,
  registered_at INTEGER NOT NULL,
  writable INTEGER NOT NULL DEFAULT 1,
  enabled INTEGER NOT NULL DEFAULT 1,
  UNIQUE(root_device, root_inode)
);
CREATE UNIQUE INDEX IF NOT EXISTS vault_one_active_writable
  ON vaults(writable) WHERE enabled = 1 AND writable = 1;
CREATE TABLE IF NOT EXISTS vault_project_bindings (
  id TEXT PRIMARY KEY,
  canonical_root TEXT NOT NULL,
  root_device INTEGER NOT NULL,
  root_inode INTEGER NOT NULL,
  epoch TEXT NOT NULL,
  registered_at INTEGER NOT NULL,
  active INTEGER NOT NULL DEFAULT 1
);
CREATE UNIQUE INDEX IF NOT EXISTS vault_active_project_binding
  ON vault_project_bindings(root_device, root_inode) WHERE active = 1;
CREATE TABLE IF NOT EXISTS vault_project_worktrees (
  binding_id TEXT NOT NULL REFERENCES vault_project_bindings(id),
  canonical_root TEXT NOT NULL UNIQUE,
  created_at INTEGER NOT NULL,
  PRIMARY KEY(binding_id, canonical_root)
);
CREATE TABLE IF NOT EXISTS vault_legacy_ownership (
  node_id INTEGER PRIMARY KEY REFERENCES knowledge_nodes(id) ON DELETE CASCADE,
  vault_id TEXT NOT NULL REFERENCES vaults(id),
  claimed_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS vault_binding_events (
  id TEXT PRIMARY KEY, vault_id TEXT NOT NULL REFERENCES vaults(id), event_kind TEXT NOT NULL,
  previous_root TEXT NOT NULL, new_root TEXT NOT NULL, created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS vault_document_events (
  id TEXT PRIMARY KEY,
  document_id TEXT NOT NULL REFERENCES vault_documents(id),
  event_kind TEXT NOT NULL,
  previous_revision TEXT REFERENCES vault_revisions(id),
  revision_id TEXT REFERENCES vault_revisions(id),
  confirmed_hash TEXT,
  previous_hash TEXT,
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS vault_documents (
  id TEXT PRIMARY KEY,
  vault_id TEXT NOT NULL REFERENCES vaults(id),
  kind TEXT NOT NULL CHECK(kind IN ('source','note','url')),
  title TEXT NOT NULL,
  current_revision TEXT REFERENCES vault_revisions(id),
  state TEXT NOT NULL DEFAULT 'active' CHECK(state IN ('active','archived','missing','drifted')),
  created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS vault_revisions (
  id TEXT PRIMARY KEY,
  document_id TEXT NOT NULL REFERENCES vault_documents(id),
  relative_path TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  size INTEGER NOT NULL,
  predecessor TEXT REFERENCES vault_revisions(id),
  created_at INTEGER NOT NULL,
  UNIQUE(document_id, relative_path),
  UNIQUE(document_id, sha256)
);
CREATE TABLE IF NOT EXISTS vault_revision_sources (
  revision_id TEXT NOT NULL REFERENCES vault_revisions(id) ON DELETE CASCADE,
  source_revision_id TEXT NOT NULL REFERENCES vault_revisions(id),
  PRIMARY KEY(revision_id, source_revision_id),
  CHECK(revision_id <> source_revision_id)
);
CREATE TABLE IF NOT EXISTS vault_grants (
  id TEXT PRIMARY KEY,
  revision_id TEXT NOT NULL REFERENCES vault_revisions(id),
  scope TEXT NOT NULL CHECK(scope IN ('private-data','common','project')),
  project_key TEXT,
  binding_epoch TEXT,
  created_at INTEGER NOT NULL,
  revoked_at INTEGER,
  CHECK((scope = 'project' AND project_key IS NOT NULL AND binding_epoch IS NOT NULL)
        OR (scope <> 'project' AND project_key IS NULL))
);
CREATE UNIQUE INDEX IF NOT EXISTS vault_active_grant
  ON vault_grants(revision_id, scope, COALESCE(project_key, ''), COALESCE(binding_epoch, ''))
  WHERE revoked_at IS NULL;
CREATE TABLE IF NOT EXISTS vault_operations (
  id TEXT PRIMARY KEY,
  vault_id TEXT NOT NULL REFERENCES vaults(id),
  idempotency_key TEXT NOT NULL UNIQUE,
  proposal_id TEXT,
  proposal_candidate_hash TEXT,
  document_id TEXT NOT NULL REFERENCES vault_documents(id),
  expected_head TEXT,
  accepts_drift INTEGER NOT NULL DEFAULT 0,
  revision_id TEXT NOT NULL REFERENCES vault_revisions(id),
  relative_path TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('prepared','files_ready','committed','failed','conflict','cancelled')),
  created_at INTEGER NOT NULL,
  completed_at INTEGER
);
CREATE TABLE IF NOT EXISTS vault_operation_files (
  operation_id TEXT NOT NULL REFERENCES vault_operations(id) ON DELETE CASCADE,
  revision_id TEXT NOT NULL REFERENCES vault_revisions(id),
  relative_path TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  size INTEGER NOT NULL,
  role TEXT NOT NULL CHECK(role IN ('primary','source')),
  PRIMARY KEY(operation_id, revision_id),
  UNIQUE(operation_id, relative_path)
);
CREATE UNIQUE INDEX IF NOT EXISTS vault_one_active_operation
  ON vault_operations(vault_id) WHERE state IN ('prepared','files_ready');
CREATE VIRTUAL TABLE IF NOT EXISTS vault_fts USING fts5(revision_id UNINDEXED, title, content, tokenize='trigram');
CREATE TABLE IF NOT EXISTS vault_index_layout (
  id INTEGER PRIMARY KEY CHECK(id = 1),
  version INTEGER NOT NULL,
  rebuild_needed INTEGER NOT NULL DEFAULT 0
);
"#;

/// `vault_operations`를 **제안 표 참조 없이** 다시 만든다. SQLite가 권장하는
/// 표 재작성 절차 그대로다 — 새 표 → 복사 → 옛 표 삭제 → 개명 → 인덱스 재생성.
/// `vault_operation_files`는 이름으로 `vault_operations`를 참조하므로 개명 뒤
/// 그대로 붙는다. FK를 끄지 않으면 옛 표 삭제가 자식 행을 CASCADE로 지운다.
pub const REBUILD_OPERATIONS_WITHOUT_PROPOSALS: &str = r#"
PRAGMA foreign_keys=OFF;
BEGIN;
CREATE TABLE IF NOT EXISTS vault_operations_rebuilt (
  id TEXT PRIMARY KEY,
  vault_id TEXT NOT NULL REFERENCES vaults(id),
  idempotency_key TEXT NOT NULL UNIQUE,
  proposal_id TEXT,
  proposal_candidate_hash TEXT,
  document_id TEXT NOT NULL REFERENCES vault_documents(id),
  expected_head TEXT,
  accepts_drift INTEGER NOT NULL DEFAULT 0,
  revision_id TEXT NOT NULL REFERENCES vault_revisions(id),
  relative_path TEXT NOT NULL,
  sha256 TEXT NOT NULL,
  state TEXT NOT NULL CHECK(state IN ('prepared','files_ready','committed','failed','conflict','cancelled')),
  created_at INTEGER NOT NULL,
  completed_at INTEGER
);
INSERT INTO vault_operations_rebuilt
  (id, vault_id, idempotency_key, proposal_id, proposal_candidate_hash, document_id,
   expected_head, accepts_drift, revision_id, relative_path, sha256, state, created_at, completed_at)
SELECT id, vault_id, idempotency_key, proposal_id, proposal_candidate_hash, document_id,
       expected_head, accepts_drift, revision_id, relative_path, sha256, state, created_at, completed_at
  FROM vault_operations;
DROP TABLE vault_operations;
ALTER TABLE vault_operations_rebuilt RENAME TO vault_operations;
CREATE UNIQUE INDEX IF NOT EXISTS vault_one_active_operation
  ON vault_operations(vault_id) WHERE state IN ('prepared','files_ready');
COMMIT;
PRAGMA foreign_keys=ON;
"#;
