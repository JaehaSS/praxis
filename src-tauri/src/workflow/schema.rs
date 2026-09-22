//! Additive workflow ledger. Existing session/task tables are deliberately untouched.

pub(super) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS workflow_runs (
    id TEXT PRIMARY KEY,
    project_ref TEXT NOT NULL,
    base_commit TEXT NOT NULL,
    active_revision INTEGER NOT NULL CHECK(active_revision > 0),
    epoch INTEGER NOT NULL DEFAULT 1 CHECK(epoch > 0),
    state TEXT NOT NULL CHECK(state IN ('draft','running','paused','failed','completed','cancelled','quarantined')),
    authorization_hash TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS workflow_revisions (
    run_id TEXT NOT NULL REFERENCES workflow_runs(id),
    revision INTEGER NOT NULL CHECK(revision > 0),
    spec_json TEXT NOT NULL,
    spec_hash TEXT NOT NULL,
    PRIMARY KEY(run_id, revision)
);
CREATE TABLE IF NOT EXISTS workflow_nodes (
    run_id TEXT NOT NULL REFERENCES workflow_runs(id),
    node_id TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('pending','ready','executing','verifying','verified','failed','cancelled','quarantined','awaiting_acceptance','retired')),
    execution_hash TEXT NOT NULL,
    accepted_attempt_id INTEGER,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count >= 0),
    PRIMARY KEY(run_id, node_id),
    FOREIGN KEY(run_id, node_id, accepted_attempt_id) REFERENCES workflow_attempts(run_id, node_id, id)
);
CREATE TABLE IF NOT EXISTS workflow_edges (
    run_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    source TEXT NOT NULL,
    target TEXT NOT NULL,
    PRIMARY KEY(run_id, revision, source, target),
    FOREIGN KEY(run_id, revision) REFERENCES workflow_revisions(run_id, revision),
    FOREIGN KEY(run_id, source) REFERENCES workflow_nodes(run_id, node_id),
    FOREIGN KEY(run_id, target) REFERENCES workflow_nodes(run_id, node_id),
    CHECK(source <> target)
);
CREATE TABLE IF NOT EXISTS workflow_node_bindings (
    run_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    node_id TEXT NOT NULL,
    accepted_attempt_id INTEGER,
    input_hash TEXT,
    validity TEXT NOT NULL DEFAULT 'pending' CHECK(validity IN ('pending','verified','stale')),
    invalidated_by_revision INTEGER,
    PRIMARY KEY(run_id, revision, node_id),
    FOREIGN KEY(run_id, revision) REFERENCES workflow_revisions(run_id, revision),
    FOREIGN KEY(run_id, node_id) REFERENCES workflow_nodes(run_id, node_id),
    FOREIGN KEY(run_id, node_id, accepted_attempt_id) REFERENCES workflow_attempts(run_id, node_id, id)
);
CREATE TABLE IF NOT EXISTS workflow_attempts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    revision INTEGER NOT NULL,
    attempt_no INTEGER NOT NULL CHECK(attempt_no > 0),
    epoch INTEGER NOT NULL,
    input_hash TEXT NOT NULL,
    state TEXT NOT NULL CHECK(state IN ('active','succeeded','failed','quarantined','cancelled')),
    output_hash TEXT,
    created_at INTEGER NOT NULL,
    FOREIGN KEY(run_id, node_id) REFERENCES workflow_nodes(run_id, node_id),
    FOREIGN KEY(run_id, revision) REFERENCES workflow_revisions(run_id, revision),
    UNIQUE(run_id, node_id, attempt_no),
    UNIQUE(run_id, node_id, id)
);
CREATE UNIQUE INDEX IF NOT EXISTS workflow_one_active_attempt
    ON workflow_attempts(run_id, node_id) WHERE state IN ('active','quarantined');
CREATE TABLE IF NOT EXISTS workflow_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    attempt_id INTEGER NOT NULL REFERENCES workflow_attempts(id),
    kind TEXT NOT NULL CHECK(kind IN ('execute','verify','integrate')),
    generation INTEGER NOT NULL CHECK(generation > 0),
    state TEXT NOT NULL CHECK(state IN ('claimed','running','finished','failed','quarantined')),
    process_identity TEXT,
    deadline INTEGER NOT NULL,
    UNIQUE(attempt_id, kind)
);
CREATE TABLE IF NOT EXISTS workflow_resources (
    id TEXT PRIMARY KEY,
    physical_identity TEXT NOT NULL UNIQUE,
    capacity INTEGER NOT NULL CHECK(capacity > 0),
    repository TEXT,
    path_prefix TEXT,
    CHECK((repository IS NULL) = (path_prefix IS NULL))
);
CREATE TABLE IF NOT EXISTS workflow_claims (
    step_id INTEGER NOT NULL REFERENCES workflow_steps(id),
    resource_id TEXT NOT NULL REFERENCES workflow_resources(id),
    mode TEXT NOT NULL CHECK(mode IN ('shared_read','exclusive_write','capacity')),
    units INTEGER NOT NULL CHECK(units > 0),
    generation INTEGER NOT NULL,
    quarantined INTEGER NOT NULL DEFAULT 0 CHECK(quarantined IN (0,1)),
    PRIMARY KEY(step_id, resource_id)
);
-- A wait is display and scheduling evidence, not a second lifecycle state.  Keeping it in a
-- separate table lets a node become ready again without losing why its most recent admission
-- attempt was deferred.
CREATE TABLE IF NOT EXISTS workflow_node_waits (
    run_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    reason TEXT NOT NULL,
    resource_id TEXT,
    owner_run_id TEXT,
    owner_node_id TEXT,
    first_wait_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    PRIMARY KEY(run_id, node_id),
    FOREIGN KEY(run_id, node_id) REFERENCES workflow_nodes(run_id, node_id)
);
CREATE INDEX IF NOT EXISTS workflow_node_waits_updated ON workflow_node_waits(updated_at);
-- Sequence is assigned once when a node first becomes ready in this revision. It is the
-- scheduler's deterministic FIFO key; node creation order is not a readiness order.
CREATE TABLE IF NOT EXISTS workflow_ready_queue (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    UNIQUE(run_id, node_id),
    FOREIGN KEY(run_id, node_id) REFERENCES workflow_nodes(run_id, node_id)
);
CREATE TABLE IF NOT EXISTS workflow_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id TEXT NOT NULL REFERENCES workflow_runs(id),
    kind TEXT NOT NULL,
    detail TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS workflow_events_run_sequence ON workflow_events(run_id, sequence);
CREATE TABLE IF NOT EXISTS workflow_requests (
    run_id TEXT NOT NULL REFERENCES workflow_runs(id),
    request_id TEXT NOT NULL,
    request_hash TEXT NOT NULL,
    result_revision INTEGER NOT NULL,
    PRIMARY KEY(run_id, request_id)
);
-- Lifecycle facts are append-only in spirit: the old scheduling tables retain their original
-- shape, while this ledger records every external side effect before it is attempted.
CREATE TABLE IF NOT EXISTS workflow_step_lifecycle (
    step_id INTEGER PRIMARY KEY REFERENCES workflow_steps(id),
    launch_state TEXT NOT NULL DEFAULT 'unlaunched' CHECK(launch_state IN ('unlaunched','intent','registered','running','finished','quarantined')),
    container_name TEXT,
    ownership_nonce TEXT,
    adapter_profile_hash TEXT,
    container_id TEXT,
    cancel_intent_at INTEGER,
    started_at INTEGER,
    exit_code INTEGER,
    log_hash TEXT,
    cleanup_state TEXT NOT NULL DEFAULT 'pending' CHECK(cleanup_state IN ('pending','absent','terminated','unknown')),
    cleanup_detail TEXT,
    finished_at INTEGER,
    UNIQUE(container_name),
    UNIQUE(container_id),
    CHECK((container_name IS NULL) = (ownership_nonce IS NULL))
);
CREATE TABLE IF NOT EXISTS workflow_cancellations (
    run_id TEXT PRIMARY KEY REFERENCES workflow_runs(id),
    revision INTEGER NOT NULL,
    epoch INTEGER NOT NULL,
    requested_at INTEGER NOT NULL,
    completed_at INTEGER,
    state TEXT NOT NULL CHECK(state IN ('intent','finished','quarantined'))
);
CREATE TABLE IF NOT EXISTS workflow_artifact_receipts (
    artifact_id TEXT NOT NULL,
    run_id TEXT NOT NULL,
    node_id TEXT NOT NULL,
    attempt_id INTEGER NOT NULL REFERENCES workflow_attempts(id),
    step_id INTEGER NOT NULL REFERENCES workflow_steps(id),
    claim_input_hash TEXT NOT NULL,
    input_tree_hash TEXT NOT NULL,
    parent_input_hash TEXT NOT NULL,
    output_tree_hash TEXT NOT NULL,
    delta_hash TEXT NOT NULL,
    manifest_hash TEXT NOT NULL,
    task_spec_hash TEXT NOT NULL,
    config_hash TEXT NOT NULL,
    recorded_at INTEGER NOT NULL,
    PRIMARY KEY(attempt_id)
);
CREATE TABLE IF NOT EXISTS workflow_check_receipts (
    attempt_id INTEGER NOT NULL REFERENCES workflow_attempts(id),
    check_id TEXT NOT NULL,
    profile_id TEXT NOT NULL,
    step_id INTEGER NOT NULL REFERENCES workflow_steps(id),
    snapshot_hash TEXT NOT NULL,
    input_hash TEXT NOT NULL,
    task_spec_hash TEXT NOT NULL,
    check_profile_hash TEXT NOT NULL,
    image_digest TEXT NOT NULL,
    environment_hash TEXT NOT NULL,
    exit_code INTEGER NOT NULL,
    log_hash TEXT NOT NULL,
    recorded_at INTEGER NOT NULL,
    PRIMARY KEY(attempt_id, check_id)
);
CREATE TABLE IF NOT EXISTS workflow_manual_acceptances (
    attempt_id INTEGER NOT NULL REFERENCES workflow_attempts(id),
    criterion TEXT NOT NULL,
    output_tree_hash TEXT NOT NULL,
    accepted_at INTEGER NOT NULL,
    PRIMARY KEY(attempt_id, criterion)
);
CREATE TABLE IF NOT EXISTS workflow_run_finalizations (
    run_id TEXT PRIMARY KEY REFERENCES workflow_runs(id),
    final_attempt_id INTEGER NOT NULL REFERENCES workflow_attempts(id),
    -- Content-addressed artifact IDs can legitimately recur across attempts, so this is an
    -- immutable audit reference rather than a SQLite foreign key to a non-unique hash.
    final_snapshot_id TEXT NOT NULL,
    completed_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS workflow_artifacts_attempt ON workflow_artifact_receipts(attempt_id);
CREATE INDEX IF NOT EXISTS workflow_checks_attempt ON workflow_check_receipts(attempt_id);
"#;
