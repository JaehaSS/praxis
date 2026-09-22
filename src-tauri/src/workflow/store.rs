//! Workflow control transactions use a dedicated FULL-synchronous pool.
//! This library does not launch processes or authorize a runtime adapter.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Sqlite, SqlitePool, Transaction,
};
use std::path::Path;

use super::{
    events::{self, WorkflowEvent},
    schema, WorkflowSpec,
};

#[derive(Clone)]
pub struct WorkflowStore {
    pub(super) pool: SqlitePool,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct RunRecord {
    pub id: String,
    pub active_revision: i64,
    pub epoch: i64,
    pub state: String,
    pub authorization_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, sqlx::FromRow)]
pub struct NodeRecord {
    pub node_id: String,
    pub state: String,
    pub accepted_attempt_id: Option<i64>,
    pub attempt_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationReceipt {
    pub revision: i64,
    pub replayed: bool,
}

impl WorkflowStore {
    pub async fn open(path: &Path) -> anyhow::Result<Self> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Full)
            .foreign_keys(true)
            .busy_timeout(std::time::Duration::from_secs(5));
        let pool = SqlitePoolOptions::new()
            .max_connections(4)
            .after_connect(|conn, _| {
                Box::pin(async move {
                    let sync: i64 = sqlx::query_scalar("PRAGMA synchronous")
                        .fetch_one(&mut *conn)
                        .await?;
                    let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
                        .fetch_one(&mut *conn)
                        .await?;
                    if sync != 2 || fk != 1 {
                        return Err(sqlx::Error::Protocol(
                            "workflow requires FULL synchronous and foreign keys".into(),
                        ));
                    }
                    Ok(())
                })
            })
            .connect_with(options)
            .await?;
        let mut tx = pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::raw_sql(schema::SCHEMA).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(Self { pool })
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }

    /// Hold every pool connection simultaneously so the check cannot inspect one four times.
    pub async fn durability_settings(&self) -> anyhow::Result<Vec<(i64, i64)>> {
        let mut held = Vec::new();
        for _ in 0..4 {
            held.push(self.pool.acquire().await?);
        }
        let mut settings = Vec::new();
        for conn in &mut held {
            let sync = sqlx::query_scalar("PRAGMA synchronous")
                .fetch_one(&mut **conn)
                .await?;
            let fk = sqlx::query_scalar("PRAGMA foreign_keys")
                .fetch_one(&mut **conn)
                .await?;
            settings.push((sync, fk));
        }
        Ok(settings)
    }

    pub async fn create_run(
        &self,
        id: &str,
        request_id: &str,
        spec: &WorkflowSpec,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        check_identifier(id)?;
        check_identifier(request_id)?;
        spec.validate().map_err(anyhow::Error::msg)?;
        let spec_hash = spec.digest().map_err(anyhow::Error::msg)?;
        let request_hash = hash_request("create", &spec_hash);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = replay(&mut tx, id, request_id, &request_hash).await? {
            return Ok(receipt);
        }
        sqlx::query("INSERT INTO workflow_runs(id,project_ref,base_commit,active_revision,state,created_at,updated_at) VALUES(?,?,?,1,'draft',?,?)")
            .bind(id).bind(&spec.project_ref).bind(&spec.base_commit).bind(now).bind(now).execute(&mut *tx).await?;
        insert_revision(&mut tx, id, 1, spec, &spec_hash).await?;
        for task in &spec.tasks {
            sqlx::query("INSERT INTO workflow_nodes(run_id,node_id,state,execution_hash) VALUES(?,?,'pending',?)")
                .bind(id).bind(&task.id).bind(spec.task_execution_hash(&task.id).map_err(anyhow::Error::msg)?).execute(&mut *tx).await?;
            sqlx::query(
                "INSERT INTO workflow_node_bindings(run_id,revision,node_id) VALUES(?,1,?)",
            )
            .bind(id)
            .bind(&task.id)
            .execute(&mut *tx)
            .await?;
        }
        insert_edges(&mut tx, id, 1, spec).await?;
        save_receipt(&mut tx, id, request_id, &request_hash, 1).await?;
        events::append(&mut tx, id, "created", &spec_hash, now).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision: 1,
            replayed: false,
        })
    }

    pub async fn run(&self, id: &str) -> anyhow::Result<RunRecord> {
        sqlx::query_as("SELECT id,active_revision,epoch,state,authorization_hash FROM workflow_runs WHERE id=?")
            .bind(id).fetch_optional(&self.pool).await?.context("workflow not found")
    }

    pub async fn spec(&self, id: &str, revision: i64) -> anyhow::Result<WorkflowSpec> {
        let json: String = sqlx::query_scalar(
            "SELECT spec_json FROM workflow_revisions WHERE run_id=? AND revision=?",
        )
        .bind(id)
        .bind(revision)
        .fetch_optional(&self.pool)
        .await?
        .context("workflow revision not found")?;
        WorkflowSpec::parse_json(&json).map_err(anyhow::Error::msg)
    }

    pub async fn nodes(&self, id: &str) -> anyhow::Result<Vec<NodeRecord>> {
        Ok(sqlx::query_as("SELECT node_id,state,accepted_attempt_id,attempt_count FROM workflow_nodes WHERE run_id=? ORDER BY node_id")
            .bind(id).fetch_all(&self.pool).await?)
    }

    pub async fn events_after(&self, id: &str, after: i64) -> anyhow::Result<Vec<WorkflowEvent>> {
        Ok(sqlx::query_as("SELECT sequence,run_id,kind,detail,created_at FROM workflow_events WHERE run_id=? AND sequence>? ORDER BY sequence LIMIT 500")
            .bind(id).bind(after.max(0)).fetch_all(&self.pool).await?)
    }

    /// Record explicit authorization for the exact spec. Runtime admission is the caller's
    /// additional responsibility; no Runner route or process dispatcher is wired to this yet.
    pub async fn authorize_start(
        &self,
        id: &str,
        revision: i64,
        spec_hash: &str,
        request_id: &str,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        check_identifier(request_id)?;
        let hash = hash_request("start", &format!("{revision}:{spec_hash}"));
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = replay(&mut tx, id, request_id, &hash).await? {
            return Ok(receipt);
        }
        let run = locked_run(&mut tx, id, revision).await?;
        if run.state != "draft" {
            bail!("workflow must be draft to start");
        }
        let stored: String = sqlx::query_scalar(
            "SELECT spec_hash FROM workflow_revisions WHERE run_id=? AND revision=?",
        )
        .bind(id)
        .bind(revision)
        .fetch_one(&mut *tx)
        .await?;
        if stored != spec_hash {
            bail!("authorization does not match workflow spec");
        }
        sqlx::query(
            "UPDATE workflow_runs SET state='running',authorization_hash=?,updated_at=? WHERE id=?",
        )
        .bind(spec_hash)
        .bind(now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        refresh_ready(&mut tx, id, revision).await?;
        events::append(&mut tx, id, "started", spec_hash, now).await?;
        save_receipt(&mut tx, id, request_id, &hash, revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision,
            replayed: false,
        })
    }

    pub async fn pause(
        &self,
        id: &str,
        revision: i64,
        request_id: &str,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        self.set_paused(id, revision, request_id, true, now).await
    }

    pub async fn resume(
        &self,
        id: &str,
        revision: i64,
        request_id: &str,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        self.set_paused(id, revision, request_id, false, now).await
    }

    async fn set_paused(
        &self,
        id: &str,
        revision: i64,
        request_id: &str,
        paused: bool,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        check_identifier(request_id)?;
        let kind = if paused { "paused" } else { "resumed" };
        let hash = hash_request(kind, &revision.to_string());
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = replay(&mut tx, id, request_id, &hash).await? {
            return Ok(receipt);
        }
        let run = locked_run(&mut tx, id, revision).await?;
        if (paused && !matches!(run.state.as_str(), "running" | "failed"))
            || (!paused && run.state != "paused")
        {
            bail!("invalid workflow control transition");
        }
        if !paused && run.authorization_hash.is_none() {
            bail!("current revision needs authorization");
        }
        sqlx::query("UPDATE workflow_runs SET state=?,updated_at=? WHERE id=?")
            .bind(if paused { "paused" } else { "running" })
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        if !paused {
            refresh_ready(&mut tx, id, revision).await?;
            sqlx::query("DELETE FROM workflow_node_waits WHERE run_id=? AND reason='paused'")
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        events::append(&mut tx, id, kind, "{}", now).await?;
        save_receipt(&mut tx, id, request_id, &hash, revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision,
            replayed: false,
        })
    }

    /// A restart fences every old result. Existing claims are quarantined, never expired away.
    /// A process supervisor must later prove termination before any claim can be released.
    pub async fn fence_recovery(&self, id: &str, now: i64) -> anyhow::Result<i64> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let run: RunRecord = sqlx::query_as("SELECT id,active_revision,epoch,state,authorization_hash FROM workflow_runs WHERE id=?")
            .bind(id).fetch_optional(&mut *tx).await?.context("workflow not found")?;
        if matches!(run.state.as_str(), "completed" | "cancelled") {
            return Ok(run.epoch);
        }
        sqlx::query("UPDATE workflow_claims SET quarantined=1 WHERE step_id IN (SELECT s.id FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id WHERE a.run_id=? AND s.state IN ('claimed','running','quarantined'))")
            .bind(id).execute(&mut *tx).await?;
        sqlx::query("UPDATE workflow_steps SET state='quarantined' WHERE attempt_id IN (SELECT id FROM workflow_attempts WHERE run_id=? AND state IN ('active','quarantined')) AND state IN ('claimed','running','quarantined')")
            .bind(id).execute(&mut *tx).await?;
        // Only attempts with a live/quarantined process retain their claim and need recovery
        // inspection. An execute that already released its claim while waiting for verification
        // or manual acceptance has no process to investigate; fail it explicitly so it can be
        // retried instead of becoming an unrecoverable quarantined zombie.
        let active = sqlx::query("UPDATE workflow_attempts SET state='quarantined' WHERE run_id=? AND state IN ('active','quarantined') AND EXISTS (SELECT 1 FROM workflow_steps s WHERE s.attempt_id=workflow_attempts.id AND s.state='quarantined')")
            .bind(id).execute(&mut *tx).await?.rows_affected();
        let quiescent = sqlx::query("UPDATE workflow_attempts SET state='failed' WHERE run_id=? AND state IN ('active','quarantined') AND NOT EXISTS (SELECT 1 FROM workflow_steps s WHERE s.attempt_id=workflow_attempts.id AND s.state IN ('claimed','running','quarantined'))")
            .bind(id).execute(&mut *tx).await?.rows_affected();
        sqlx::query("UPDATE workflow_nodes SET state='quarantined' WHERE run_id=? AND node_id IN (SELECT node_id FROM workflow_attempts WHERE run_id=? AND state='quarantined')")
            .bind(id).bind(id).execute(&mut *tx).await?;
        sqlx::query("UPDATE workflow_nodes SET state='failed' WHERE run_id=? AND state IN ('executing','verifying','awaiting_acceptance') AND EXISTS (SELECT 1 FROM workflow_attempts a WHERE a.run_id=workflow_nodes.run_id AND a.node_id=workflow_nodes.node_id AND a.epoch=? AND a.state='failed')")
            .bind(id).bind(run.epoch).execute(&mut *tx).await?;
        sqlx::query("UPDATE workflow_runs SET epoch=epoch+1,state=?,updated_at=? WHERE id=?")
            .bind(if active > 0 {
                "quarantined"
            } else if quiescent > 0 {
                "failed"
            } else if run.state == "draft" {
                "draft"
            } else {
                "paused"
            })
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        events::append(
            &mut tx,
            id,
            "recovery_fenced",
            &(run.epoch + 1).to_string(),
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(run.epoch + 1)
    }
}

pub(super) fn check_identifier(value: &str) -> anyhow::Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"-_.".contains(&c))
    {
        bail!("invalid workflow/request identifier");
    }
    Ok(())
}

pub(super) fn hash_request(kind: &str, value: &str) -> String {
    format!("{:x}", Sha256::digest(format!("{kind}\0{value}")))
}

pub(super) async fn replay(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    request_id: &str,
    hash: &str,
) -> anyhow::Result<Option<MutationReceipt>> {
    let row: Option<(String,i64)> = sqlx::query_as("SELECT request_hash,result_revision FROM workflow_requests WHERE run_id=? AND request_id=?")
        .bind(id).bind(request_id).fetch_optional(&mut **tx).await?;
    if let Some((stored, revision)) = row {
        if stored != hash {
            bail!("request_id already used with different payload");
        }
        return Ok(Some(MutationReceipt {
            revision,
            replayed: true,
        }));
    }
    Ok(None)
}

pub(super) async fn save_receipt(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    request_id: &str,
    hash: &str,
    revision: i64,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO workflow_requests(run_id,request_id,request_hash,result_revision) VALUES(?,?,?,?)")
        .bind(id).bind(request_id).bind(hash).bind(revision).execute(&mut **tx).await?;
    Ok(())
}

pub(super) async fn locked_run(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    revision: i64,
) -> anyhow::Result<RunRecord> {
    let row: RunRecord = sqlx::query_as(
        "SELECT id,active_revision,epoch,state,authorization_hash FROM workflow_runs WHERE id=?",
    )
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .context("workflow not found")?;
    if row.active_revision != revision {
        bail!("stale workflow revision");
    }
    Ok(row)
}

pub(super) async fn insert_revision(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    revision: i64,
    spec: &WorkflowSpec,
    hash: &str,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO workflow_revisions(run_id,revision,spec_json,spec_hash) VALUES(?,?,?,?)",
    )
    .bind(id)
    .bind(revision)
    .bind(serde_json::to_string(spec)?)
    .bind(hash)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(super) async fn insert_edges(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    revision: i64,
    spec: &WorkflowSpec,
) -> anyhow::Result<()> {
    for edge in &spec.edges {
        sqlx::query("INSERT INTO workflow_edges(run_id,revision,source,target) VALUES(?,?,?,?)")
            .bind(id)
            .bind(revision)
            .bind(&edge.from)
            .bind(&edge.to)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

pub(super) async fn refresh_ready(
    tx: &mut Transaction<'_, Sqlite>,
    id: &str,
    revision: i64,
) -> anyhow::Result<()> {
    sqlx::query("UPDATE workflow_nodes SET state='ready' WHERE run_id=? AND state='pending' AND NOT EXISTS (SELECT 1 FROM workflow_edges e JOIN workflow_nodes p ON p.run_id=e.run_id AND p.node_id=e.source LEFT JOIN workflow_attempts a ON a.id=p.accepted_attempt_id AND a.run_id=p.run_id AND a.node_id=p.node_id WHERE e.run_id=workflow_nodes.run_id AND e.target=workflow_nodes.node_id AND e.revision=? AND (p.state<>'verified' OR p.accepted_attempt_id IS NULL OR a.state<>'succeeded' OR a.output_hash IS NULL))")
        .bind(id).bind(revision).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM workflow_node_waits WHERE run_id=? AND reason IN ('dependency_pending','dependency_failed','input_unverified') AND node_id IN (SELECT node_id FROM workflow_nodes WHERE run_id=? AND state='ready')")
        .bind(id).bind(id).execute(&mut **tx).await?;
    // Backfill ready nodes created by an earlier library version and enqueue newly-ready nodes
    // in the same FULL transaction. `rowid` is only a deterministic tie-breaker for a single
    // transition; subsequent scheduling uses this durable sequence.
    sqlx::query("INSERT OR IGNORE INTO workflow_ready_queue(run_id,node_id) SELECT run_id,node_id FROM workflow_nodes WHERE run_id=? AND state='ready' ORDER BY rowid")
        .bind(id).execute(&mut **tx).await?;
    Ok(())
}
