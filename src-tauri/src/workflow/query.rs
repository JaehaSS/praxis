//! Consistent API snapshots and admission bound to immutable server configuration.
use super::{
    events,
    store::{self, MutationReceipt, WorkflowStore},
    WorkflowSpec,
};
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, Serialize, Deserialize, sqlx::FromRow)]
pub struct RuntimeAdmission {
    pub config_hash: String,
    pub base_input_hash: String,
    pub scope_hash: String,
}

type WaitRow = (String, Option<String>, Option<String>, Option<String>);

impl WorkflowStore {
    pub async fn owns_log(&self, run_id: &str, digest: &str) -> anyhow::Result<bool> {
        self.run(run_id).await?;
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM (
            SELECT l.log_hash FROM workflow_step_lifecycle l JOIN workflow_steps s ON s.id=l.step_id
            JOIN workflow_attempts a ON a.id=s.attempt_id WHERE a.run_id=? AND l.log_hash=?
            UNION ALL SELECT c.log_hash FROM workflow_check_receipts c JOIN workflow_attempts a ON a.id=c.attempt_id
            WHERE a.run_id=? AND c.log_hash=?)")
            .bind(run_id).bind(digest).bind(run_id).bind(digest).fetch_one(&self.pool).await?;
        Ok(count > 0)
    }
    pub async fn ensure_admission_schema(&self) -> anyhow::Result<()> {
        sqlx::raw_sql("CREATE TABLE IF NOT EXISTS workflow_admissions (
          run_id TEXT NOT NULL, revision INTEGER NOT NULL, config_hash TEXT NOT NULL,
          base_input_hash TEXT NOT NULL, scope_hash TEXT NOT NULL,
          PRIMARY KEY(run_id,revision), FOREIGN KEY(run_id,revision) REFERENCES workflow_revisions(run_id,revision));
          CREATE TABLE IF NOT EXISTS workflow_runtime_authorizations (
          run_id TEXT NOT NULL, revision INTEGER NOT NULL, scope_hash TEXT NOT NULL,
          PRIMARY KEY(run_id,revision), FOREIGN KEY(run_id,revision) REFERENCES workflow_revisions(run_id,revision));")
            .execute(&self.pool).await?;
        Ok(())
    }

    pub async fn run_ids(&self) -> anyhow::Result<Vec<String>> {
        Ok(
            sqlx::query_scalar("SELECT id FROM workflow_runs ORDER BY created_at,id")
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn snapshot(&self, id: &str) -> anyhow::Result<Value> {
        // Every value and the event cursor come from the same SQLite read snapshot.
        let mut tx = self.pool.begin().await?;
        let (revision, state): (i64, String) =
            sqlx::query_as("SELECT active_revision,state FROM workflow_runs WHERE id=?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .context("workflow not found")?;
        let spec_json: String = sqlx::query_scalar(
            "SELECT spec_json FROM workflow_revisions WHERE run_id=? AND revision=?",
        )
        .bind(id)
        .bind(revision)
        .fetch_one(&mut *tx)
        .await?;
        let spec = WorkflowSpec::parse_json(&spec_json).map_err(anyhow::Error::msg)?;
        let rows: Vec<(String,String)> = sqlx::query_as("SELECT node_id,state FROM workflow_nodes WHERE run_id=? AND state<>'retired' ORDER BY node_id")
            .bind(id).fetch_all(&mut *tx).await?;
        let mut nodes = Vec::new();
        for (node_id, node_state) in rows {
            let task = spec
                .tasks
                .iter()
                .find(|t| t.id == node_id)
                .context("node missing from revision")?;
            let wait: Option<WaitRow> = sqlx::query_as("SELECT reason,resource_id,owner_run_id,owner_node_id FROM workflow_node_waits WHERE run_id=? AND node_id=?")
                .bind(id).bind(&node_id).fetch_optional(&mut *tx).await?;
            let attempts: Vec<(i64,i64,String)> = sqlx::query_as("SELECT id,attempt_no,state FROM workflow_attempts WHERE run_id=? AND node_id=? ORDER BY attempt_no")
                .bind(id).bind(&node_id).fetch_all(&mut *tx).await?;
            let check_rows:Vec<(String,i64,String)>=sqlx::query_as("SELECT check_id,exit_code,snapshot_hash FROM workflow_check_receipts WHERE attempt_id=(SELECT MAX(id) FROM workflow_attempts WHERE run_id=? AND node_id=?) ORDER BY check_id")
                .bind(id).bind(&node_id).fetch_all(&mut *tx).await?;
            nodes.push(json!({"id":node_id,"phase_id":task.phase_id,"state":node_state,
                "wait_reason":if node_state=="awaiting_acceptance" {Some("manual_acceptance")}else{wait.as_ref().map(|w|w.0.as_str())},
                "checks":check_rows.into_iter().map(|(id,exit,hash)|json!({"id":id,"state":if exit==0{"passed"}else{"failed"},"detail":hash})).collect::<Vec<_>>(),
                "wait_detail":wait.as_ref().map(|w|format!("resource={} owner={}/{}",w.1.as_deref().unwrap_or("-"),w.2.as_deref().unwrap_or("-"),w.3.as_deref().unwrap_or("-"))),
                "inputs":task.input_artifacts.iter().map(|a|format!("{}:{}",a.task_id,a.artifact)).collect::<Vec<_>>(),
                "write_paths":task.write_paths,
                "resources":task.resource_requests_by_step.values().flatten().map(|r|r.resource_id.clone()).collect::<std::collections::BTreeSet<_>>(),
                "attempts":attempts.into_iter().map(|(id,number,state)|json!({"id":id.to_string(),"number":number,"state":state})).collect::<Vec<_>>()
            }));
        }
        let sequence: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(sequence),0) FROM workflow_events WHERE run_id=?",
        )
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(
            json!({"id":id,"revision":revision,"state":state,"last_sequence":sequence,"spec":spec,"nodes":nodes}),
        )
    }

    pub async fn admission(&self, id: &str, revision: i64) -> anyhow::Result<RuntimeAdmission> {
        sqlx::query_as("SELECT config_hash,base_input_hash,scope_hash FROM workflow_admissions WHERE run_id=? AND revision=?")
            .bind(id).bind(revision).fetch_optional(&self.pool).await?.context("workflow must be validated for this revision")
    }

    pub async fn authorized_runtime_scope(
        &self,
        id: &str,
        revision: i64,
    ) -> anyhow::Result<Option<String>> {
        Ok(sqlx::query_scalar(
            "SELECT scope_hash FROM workflow_runtime_authorizations WHERE run_id=? AND revision=?",
        )
        .bind(id)
        .bind(revision)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn record_admission(
        &self,
        id: &str,
        revision: i64,
        request_id: &str,
        admission: &RuntimeAdmission,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        let hash = store::hash_request("validate", &serde_json::to_string(&(revision, admission))?);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, id, request_id, &hash).await? {
            return Ok(receipt);
        }
        let run = store::locked_run(&mut tx, id, revision).await?;
        if !matches!(run.state.as_str(), "draft" | "paused" | "failed") {
            bail!("workflow must be draft or paused for validation")
        }
        sqlx::query("INSERT INTO workflow_admissions(run_id,revision,config_hash,base_input_hash,scope_hash) VALUES(?,?,?,?,?) ON CONFLICT(run_id,revision) DO UPDATE SET config_hash=excluded.config_hash,base_input_hash=excluded.base_input_hash,scope_hash=excluded.scope_hash")
            .bind(id).bind(revision).bind(&admission.config_hash).bind(&admission.base_input_hash).bind(&admission.scope_hash).execute(&mut *tx).await?;
        events::append(&mut tx, id, "runtime_validated", &admission.scope_hash, now).await?;
        store::save_receipt(&mut tx, id, request_id, &hash, revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision,
            replayed: false,
        })
    }

    /// Approval covers server-side profiles and exported input as well as the user plan.
    pub async fn start_admitted(
        &self,
        id: &str,
        revision: i64,
        request_id: &str,
        scope_hash: &str,
        config_hash: &str,
        resume: bool,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        let hash = store::hash_request(
            if resume {
                "runtime_resume"
            } else {
                "runtime_start"
            },
            &serde_json::to_string(&(revision, scope_hash, config_hash))?,
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, id, request_id, &hash).await? {
            return Ok(receipt);
        }
        let run = store::locked_run(&mut tx, id, revision).await?;
        if run.state != if resume { "paused" } else { "draft" } {
            bail!("workflow state does not permit this action")
        }
        let cancelling:i64=sqlx::query_scalar("SELECT COUNT(*) FROM workflow_cancellations WHERE run_id=? AND state IN ('intent','quarantined')")
            .bind(id).fetch_one(&mut *tx).await?;
        if cancelling != 0 {
            bail!("workflow cancellation is in progress")
        }
        let admission:RuntimeAdmission=sqlx::query_as("SELECT config_hash,base_input_hash,scope_hash FROM workflow_admissions WHERE run_id=? AND revision=?")
            .bind(id).bind(revision).fetch_optional(&mut *tx).await?.context("workflow requires runtime validation")?;
        if admission.scope_hash != scope_hash || admission.config_hash != config_hash {
            bail!("approval scope is stale")
        }
        let spec_hash: String = sqlx::query_scalar(
            "SELECT spec_hash FROM workflow_revisions WHERE run_id=? AND revision=?",
        )
        .bind(id)
        .bind(revision)
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE workflow_runs SET state='running',authorization_hash=?,updated_at=? WHERE id=?",
        )
        .bind(spec_hash)
        .bind(now)
        .bind(id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("INSERT INTO workflow_runtime_authorizations(run_id,revision,scope_hash) VALUES(?,?,?) ON CONFLICT(run_id,revision) DO UPDATE SET scope_hash=excluded.scope_hash")
            .bind(id).bind(revision).bind(scope_hash).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM workflow_node_waits WHERE run_id=? AND reason IN ('paused','authorization_required')").bind(id).execute(&mut *tx).await?;
        store::refresh_ready(&mut tx, id, revision).await?;
        events::append(
            &mut tx,
            id,
            if resume { "resumed" } else { "started" },
            scope_hash,
            now,
        )
        .await?;
        store::save_receipt(&mut tx, id, request_id, &hash, revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision,
            replayed: false,
        })
    }
}
