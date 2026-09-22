//! Durable, fenced lifecycle records for external workflow steps.
//!
//! This module deliberately records intent before the runtime calls an adapter.  It never starts,
//! stops, or inspects a container itself; the supervisor supplies the observed identity and cleanup
//! proof and retains the RunnerCapacity permit until `finish_step` or `finish_cancel` succeeds.

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

use super::{
    events,
    resources::StepLease,
    store::{self, MutationReceipt, WorkflowStore},
    WorkflowSpec,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchIntent {
    pub container_name: String,
    pub ownership_nonce: String,
    pub adapter_profile_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerRegistration {
    pub container_id: String,
    pub ownership_nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CleanupProof {
    Absent,
    Terminated { observed_identity: String },
    Unknown { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepExit {
    pub exit_code: i32,
    pub log_hash: String,
    pub cleanup: CleanupProof,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepCleanup {
    pub step_id: i64,
    pub cleanup: CleanupProof,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FinishStep {
    Verifying,
    Failed,
    Quarantined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct AttemptRecord {
    pub id: i64,
    pub run_id: String,
    pub node_id: String,
    pub revision: i64,
    pub attempt_no: i64,
    pub epoch: i64,
    pub input_hash: String,
    pub state: String,
    pub output_hash: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct StepRecord {
    pub id: i64,
    pub attempt_id: i64,
    pub kind: String,
    pub generation: i64,
    pub state: String,
    pub deadline: i64,
    pub launch_state: Option<String>,
    pub container_name: Option<String>,
    pub ownership_nonce: Option<String>,
    pub container_id: Option<String>,
    pub cancel_intent_at: Option<i64>,
    pub exit_code: Option<i64>,
    pub log_hash: Option<String>,
    pub cleanup_state: Option<String>,
}

/// The immutable resource identity attached to a currently fenced step.  Runtime adapters use
/// this view to choose a mount; they must never infer a host path from a task string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct ResourceClaimView {
    pub resource_id: String,
    pub physical_identity: String,
    pub mode: String,
    pub units: i64,
    pub generation: i64,
    pub capacity: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryStep {
    pub lease: StepLease,
    pub state: String,
    pub container_name: Option<String>,
    pub ownership_nonce: Option<String>,
    pub container_id: Option<String>,
    pub cleanup_state: Option<String>,
}

#[derive(FromRow)]
struct RecoveryRow {
    run_id: String,
    node_id: String,
    revision: i64,
    epoch: i64,
    attempt_id: i64,
    step_id: i64,
    generation: i64,
    input_hash: String,
    state: String,
    container_name: Option<String>,
    ownership_nonce: Option<String>,
    container_id: Option<String>,
    cleanup_state: Option<String>,
}

pub fn deterministic_container_name(lease: &StepLease) -> String {
    format!(
        "workflow-{}-{}-{}",
        lease.run_id, lease.attempt_id, lease.step_id
    )
}

impl WorkflowStore {
    pub async fn attempts(&self, run_id: &str) -> anyhow::Result<Vec<AttemptRecord>> {
        Ok(sqlx::query_as(
            "SELECT id,run_id,node_id,revision,attempt_no,epoch,input_hash,state,output_hash,created_at \
             FROM workflow_attempts WHERE run_id=? ORDER BY id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn steps(&self, run_id: &str) -> anyhow::Result<Vec<StepRecord>> {
        Ok(sqlx::query_as(
            "SELECT s.id,s.attempt_id,s.kind,s.generation,s.state,s.deadline, \
                    l.launch_state,l.container_name,l.ownership_nonce,l.container_id, \
                    l.cancel_intent_at,l.exit_code,l.log_hash,l.cleanup_state \
             FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id \
             LEFT JOIN workflow_step_lifecycle l ON l.step_id=s.id \
             WHERE a.run_id=? ORDER BY s.id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?)
    }

    pub async fn step_resource_claims(
        &self,
        lease: &StepLease,
    ) -> anyhow::Result<Vec<ResourceClaimView>> {
        self.assert_lease_current(lease).await?;
        Ok(sqlx::query_as(
            "SELECT c.resource_id,r.physical_identity,c.mode,c.units,c.generation,r.capacity \
             FROM workflow_claims c JOIN workflow_resources r ON r.id=c.resource_id \
             JOIN workflow_steps s ON s.id=c.step_id \
             JOIN workflow_attempts a ON a.id=s.attempt_id \
             JOIN workflow_runs w ON w.id=a.run_id \
             LEFT JOIN workflow_cancellations x ON x.run_id=w.id AND x.state='intent' \
             WHERE c.step_id=? AND c.generation=? AND c.quarantined=0 \
               AND s.generation=? AND a.id=? AND a.node_id=? AND a.input_hash=? AND a.state='active' \
               AND w.id=? AND w.active_revision=? AND w.epoch=? AND w.state IN ('running','paused') \
               AND x.run_id IS NULL ORDER BY c.resource_id",
        )
        .bind(lease.step_id)
        .bind(lease.generation)
        .bind(lease.generation)
        .bind(lease.attempt_id)
        .bind(&lease.node_id)
        .bind(&lease.input_hash)
        .bind(&lease.run_id)
        .bind(lease.revision)
        .bind(lease.epoch)
        .fetch_all(&self.pool)
        .await?)
    }

    /// Returns exactly the identities owned by the fenced run. Recovery must not discover or
    /// clean up containers by wildcard/name prefix alone.
    pub async fn recovery_steps(&self, run_id: &str) -> anyhow::Result<Vec<RecoveryStep>> {
        let rows: Vec<RecoveryRow> = sqlx::query_as(
            "SELECT a.run_id,a.node_id,a.revision,a.epoch,a.id AS attempt_id,s.id AS step_id,s.generation,a.input_hash,s.state, \
                    l.container_name,l.ownership_nonce,l.container_id,l.cleanup_state \
             FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id \
             LEFT JOIN workflow_step_lifecycle l ON l.step_id=s.id \
             WHERE a.run_id=? AND s.state IN ('claimed','running','quarantined') ORDER BY s.id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| RecoveryStep {
                lease: StepLease {
                    run_id: row.run_id,
                    node_id: row.node_id,
                    revision: row.revision,
                    epoch: row.epoch,
                    attempt_id: row.attempt_id,
                    step_id: row.step_id,
                    generation: row.generation,
                    input_hash: row.input_hash,
                },
                state: row.state,
                container_name: row.container_name,
                ownership_nonce: row.ownership_nonce,
                container_id: row.container_id,
                cleanup_state: row.cleanup_state,
            })
            .collect())
    }

    /// Repair has no force-unlock path: it requires the same exact cleanup proof as cancellation.
    pub async fn repair_quarantined_step(
        &self,
        run_id: &str,
        revision: i64,
        step_id: i64,
        request_id: &str,
        cleanup: &CleanupProof,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        validate_cleanup(cleanup)?;
        let hash = store::hash_request(
            "repair_quarantine",
            &serde_json::to_string(&(revision, step_id, cleanup))?,
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, run_id, request_id, &hash).await? {
            return Ok(receipt);
        }
        store::locked_run(&mut tx, run_id, revision).await?;
        let owner: Option<(i64, String)> = sqlx::query_as("SELECT a.id,s.state FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id WHERE s.id=? AND a.run_id=?")
            .bind(step_id).bind(run_id).fetch_optional(&mut *tx).await?;
        let Some((attempt_id, state)) = owner else {
            bail!("workflow step not found");
        };
        if state != "quarantined" {
            bail!("workflow step is not quarantined");
        }
        if matches!(cleanup, CleanupProof::Unknown { .. }) {
            bail!("repair requires confirmed absence or termination");
        }
        ensure_registered_identity(&mut tx, step_id, cleanup).await?;
        let fields = cleanup_fields(cleanup)?;
        sqlx::query("DELETE FROM workflow_claims WHERE step_id=? AND quarantined=1")
            .bind(step_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO workflow_step_lifecycle(step_id,launch_state,cleanup_state,cleanup_detail,finished_at) VALUES(?,'finished',?,?,?) ON CONFLICT(step_id) DO UPDATE SET launch_state='finished',cleanup_state=excluded.cleanup_state,cleanup_detail=excluded.cleanup_detail,finished_at=excluded.finished_at")
            .bind(step_id).bind(fields.0).bind(fields.1).bind(now).execute(&mut *tx).await?;
        sqlx::query("UPDATE workflow_steps SET state='failed' WHERE id=?")
            .bind(step_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE workflow_attempts SET state='failed' WHERE id=? AND state='quarantined'",
        )
        .bind(attempt_id)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE workflow_nodes SET state='failed' WHERE run_id=? AND node_id=(SELECT node_id FROM workflow_attempts WHERE id=?)").bind(run_id).bind(attempt_id).execute(&mut *tx).await?;
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id WHERE a.run_id=? AND s.state='quarantined'").bind(run_id).fetch_one(&mut *tx).await?;
        if remaining == 0 {
            sqlx::query("UPDATE workflow_runs SET state='paused',updated_at=? WHERE id=? AND state='quarantined'").bind(now).bind(run_id).execute(&mut *tx).await?;
        }
        events::append(
            &mut tx,
            run_id,
            "quarantine_repaired",
            &step_id.to_string(),
            now,
        )
        .await?;
        store::save_receipt(&mut tx, run_id, request_id, &hash, revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision,
            replayed: false,
        })
    }

    /// Writes the launch receipt before the adapter is permitted to create anything.
    pub async fn begin_launch(
        &self,
        lease: &StepLease,
        request_id: &str,
        intent: &LaunchIntent,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        validate_intent(lease, intent)?;
        let hash = store::hash_request("launch_intent", &serde_json::to_string(&(lease, intent))?);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, &lease.run_id, request_id, &hash).await? {
            return Ok(receipt);
        }
        assert_lease_in_tx(&mut tx, lease, true, false).await?;
        sqlx::query(
            "INSERT INTO workflow_step_lifecycle(step_id,launch_state,container_name,ownership_nonce,adapter_profile_hash) \
             VALUES(?,'intent',?,?,?) \
             ON CONFLICT(step_id) DO UPDATE SET launch_state=excluded.launch_state,container_name=excluded.container_name, \
                 ownership_nonce=excluded.ownership_nonce,adapter_profile_hash=excluded.adapter_profile_hash \
             WHERE workflow_step_lifecycle.launch_state='unlaunched'",
        )
        .bind(lease.step_id)
        .bind(&intent.container_name)
        .bind(&intent.ownership_nonce)
        .bind(&intent.adapter_profile_hash)
        .execute(&mut *tx)
        .await?;
        ensure_lifecycle_state(&mut tx, lease.step_id, "intent").await?;
        events::append(
            &mut tx,
            &lease.run_id,
            "launch_intent",
            &serde_json::to_string(intent)?,
            now,
        )
        .await?;
        store::save_receipt(&mut tx, &lease.run_id, request_id, &hash, lease.revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision: lease.revision,
            replayed: false,
        })
    }

    /// Records the adapter's immutable container identity after create and before start.
    pub async fn register_container(
        &self,
        lease: &StepLease,
        request_id: &str,
        registration: &ContainerRegistration,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        validate_nonempty("container_id", &registration.container_id)?;
        validate_nonempty("ownership_nonce", &registration.ownership_nonce)?;
        let hash = store::hash_request(
            "container_registration",
            &serde_json::to_string(&(lease, registration))?,
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, &lease.run_id, request_id, &hash).await? {
            return Ok(receipt);
        }
        assert_lease_in_tx(&mut tx, lease, true, false).await?;
        let nonce: Option<String> = sqlx::query_scalar("SELECT ownership_nonce FROM workflow_step_lifecycle WHERE step_id=? AND launch_state='intent'")
            .bind(lease.step_id).fetch_optional(&mut *tx).await?;
        if nonce.as_deref() != Some(registration.ownership_nonce.as_str()) {
            bail!("container registration does not match durable launch intent");
        }
        sqlx::query("UPDATE workflow_step_lifecycle SET launch_state='registered',container_id=? WHERE step_id=? AND launch_state='intent'")
            .bind(&registration.container_id).bind(lease.step_id).execute(&mut *tx).await?;
        ensure_lifecycle_state(&mut tx, lease.step_id, "registered").await?;
        events::append(
            &mut tx,
            &lease.run_id,
            "container_registered",
            &serde_json::to_string(registration)?,
            now,
        )
        .await?;
        store::save_receipt(&mut tx, &lease.run_id, request_id, &hash, lease.revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision: lease.revision,
            replayed: false,
        })
    }

    /// Fence immediately before the irreversible adapter start call.
    pub async fn mark_step_running(
        &self,
        lease: &StepLease,
        request_id: &str,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        let hash = store::hash_request("step_started", &serde_json::to_string(lease)?);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, &lease.run_id, request_id, &hash).await? {
            return Ok(receipt);
        }
        assert_lease_in_tx(&mut tx, lease, true, false).await?;
        sqlx::query("UPDATE workflow_steps SET state='running' WHERE id=? AND state='claimed'")
            .bind(lease.step_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE workflow_step_lifecycle SET launch_state='running',started_at=? WHERE step_id=? AND launch_state='registered'")
            .bind(now).bind(lease.step_id).execute(&mut *tx).await?;
        ensure_lifecycle_state(&mut tx, lease.step_id, "running").await?;
        events::append(
            &mut tx,
            &lease.run_id,
            "step_running",
            &lease.step_id.to_string(),
            now,
        )
        .await?;
        store::save_receipt(&mut tx, &lease.run_id, request_id, &hash, lease.revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision: lease.revision,
            replayed: false,
        })
    }

    /// Accepts an exit only while the lease remains current and cleanup is positively known.
    pub async fn finish_step(
        &self,
        lease: &StepLease,
        request_id: &str,
        exit: &StepExit,
        now: i64,
    ) -> anyhow::Result<FinishStep> {
        store::check_identifier(request_id)?;
        validate_hash("log_hash", &exit.log_hash)?;
        validate_cleanup(&exit.cleanup)?;
        let hash = store::hash_request("step_finished", &serde_json::to_string(&(lease, exit))?);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if store::replay(&mut tx, &lease.run_id, request_id, &hash)
            .await?
            .is_some()
        {
            return finish_outcome(&mut tx, lease.step_id).await;
        }
        assert_lease_in_tx(&mut tx, lease, false, true).await?;
        if exit.exit_code == 0 {
            ensure_lifecycle_state(&mut tx, lease.step_id, "running").await?;
        }
        ensure_registered_identity(&mut tx, lease.step_id, &exit.cleanup).await?;
        let cleanup = cleanup_fields(&exit.cleanup)?;
        if cleanup.0 == "unknown" {
            quarantine_step(&mut tx, lease, &cleanup.1, now).await?;
            store::save_receipt(&mut tx, &lease.run_id, request_id, &hash, lease.revision).await?;
            tx.commit().await?;
            return Ok(FinishStep::Quarantined);
        }
        sqlx::query(
            "DELETE FROM workflow_claims WHERE step_id=? AND generation=? AND quarantined=0",
        )
        .bind(lease.step_id)
        .bind(lease.generation)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE workflow_step_lifecycle SET launch_state='finished',exit_code=?,log_hash=?,cleanup_state=?,cleanup_detail=?,finished_at=? WHERE step_id=?")
            .bind(exit.exit_code).bind(&exit.log_hash).bind(cleanup.0).bind(cleanup.1).bind(now).bind(lease.step_id).execute(&mut *tx).await?;
        if exit.exit_code == 0 {
            sqlx::query("UPDATE workflow_steps SET state='finished' WHERE id=?")
                .bind(lease.step_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE workflow_nodes SET state='verifying' WHERE run_id=? AND node_id=?")
                .bind(&lease.run_id)
                .bind(&lease.node_id)
                .execute(&mut *tx)
                .await?;
            events::append(
                &mut tx,
                &lease.run_id,
                "step_finished",
                &lease.step_id.to_string(),
                now,
            )
            .await?;
            store::save_receipt(&mut tx, &lease.run_id, request_id, &hash, lease.revision).await?;
            tx.commit().await?;
            Ok(FinishStep::Verifying)
        } else {
            sqlx::query("UPDATE workflow_steps SET state='failed' WHERE id=?")
                .bind(lease.step_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query(
                "UPDATE workflow_attempts SET state='failed' WHERE id=? AND state='active'",
            )
            .bind(lease.attempt_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE workflow_nodes SET state='failed' WHERE run_id=? AND node_id=?")
                .bind(&lease.run_id)
                .bind(&lease.node_id)
                .execute(&mut *tx)
                .await?;
            let spec = spec_in_tx(&mut tx, &lease.run_id, lease.revision).await?;
            if spec.final_task_id == lease.node_id {
                sqlx::query("UPDATE workflow_runs SET state='failed',updated_at=? WHERE id=?")
                    .bind(now)
                    .bind(&lease.run_id)
                    .execute(&mut *tx)
                    .await?;
            }
            events::append(
                &mut tx,
                &lease.run_id,
                "step_failed",
                &lease.step_id.to_string(),
                now,
            )
            .await?;
            store::save_receipt(&mut tx, &lease.run_id, request_id, &hash, lease.revision).await?;
            tx.commit().await?;
            Ok(FinishStep::Failed)
        }
    }

    /// Blocks all new launches before a supervisor acquires its per-step stop gates.
    pub async fn cancel_intent(
        &self,
        run_id: &str,
        revision: i64,
        request_id: &str,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        let hash = store::hash_request("cancel_intent", &revision.to_string());
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, run_id, request_id, &hash).await? {
            return Ok(receipt);
        }
        let run = store::locked_run(&mut tx, run_id, revision).await?;
        if !matches!(
            run.state.as_str(),
            "draft" | "running" | "paused" | "failed"
        ) {
            bail!("workflow cannot be cancelled from its current state");
        }
        sqlx::query("INSERT INTO workflow_cancellations(run_id,revision,epoch,requested_at,state) VALUES(?,?,?,?, 'intent') ON CONFLICT(run_id) DO UPDATE SET revision=excluded.revision,epoch=excluded.epoch,requested_at=excluded.requested_at,state='intent',completed_at=NULL")
            .bind(run_id).bind(revision).bind(run.epoch).bind(now).execute(&mut *tx).await?;
        sqlx::query("UPDATE workflow_step_lifecycle SET cancel_intent_at=? WHERE step_id IN (SELECT s.id FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id WHERE a.run_id=? AND s.state IN ('claimed','running'))")
            .bind(now).bind(run_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE workflow_runs SET state='paused',updated_at=? WHERE id=?")
            .bind(now)
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        events::append(&mut tx, run_id, "cancel_intent", "{}", now).await?;
        store::save_receipt(&mut tx, run_id, request_id, &hash, revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision,
            replayed: false,
        })
    }

    /// Completes cancellation only after every active step has a specific absence/termination proof.
    pub async fn finish_cancel(
        &self,
        run_id: &str,
        revision: i64,
        request_id: &str,
        cleanup: &[StepCleanup],
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        for proof in cleanup {
            validate_cleanup(&proof.cleanup)?;
        }
        let hash = store::hash_request(
            "finish_cancel",
            &serde_json::to_string(&(revision, cleanup))?,
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, run_id, request_id, &hash).await? {
            return Ok(receipt);
        }
        let run = store::locked_run(&mut tx, run_id, revision).await?;
        let state: Option<String> = sqlx::query_scalar(
            "SELECT state FROM workflow_cancellations WHERE run_id=? AND revision=? AND epoch=?",
        )
        .bind(run_id)
        .bind(revision)
        .bind(run.epoch)
        .fetch_optional(&mut *tx)
        .await?;
        if state.as_deref() != Some("intent") {
            bail!("workflow has no current cancellation intent");
        }
        let active: Vec<i64> = sqlx::query_scalar("SELECT s.id FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id WHERE a.run_id=? AND s.state IN ('claimed','running') ORDER BY s.id")
            .bind(run_id).fetch_all(&mut *tx).await?;
        let mut proofs = std::collections::BTreeMap::new();
        for proof in cleanup {
            if proofs.insert(proof.step_id, &proof.cleanup).is_some() {
                bail!("duplicate cleanup proof");
            }
        }
        if active.iter().any(|id| !proofs.contains_key(id))
            || proofs.keys().any(|id| !active.contains(id))
        {
            bail!("cleanup proofs do not exactly cover active steps");
        }
        for (step_id, proof) in &proofs {
            ensure_registered_identity(&mut tx, *step_id, proof).await?;
        }
        if proofs
            .values()
            .any(|proof| matches!(proof, CleanupProof::Unknown { .. }))
        {
            for id in &active {
                let detail = cleanup_fields(proofs[id])?.1;
                sqlx::query("UPDATE workflow_step_lifecycle SET launch_state='quarantined',cleanup_state='unknown',cleanup_detail=?,finished_at=? WHERE step_id=?").bind(detail).bind(now).bind(id).execute(&mut *tx).await?;
            }
            sqlx::query("UPDATE workflow_claims SET quarantined=1 WHERE step_id IN (SELECT s.id FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id WHERE a.run_id=?)").bind(run_id).execute(&mut *tx).await?;
            sqlx::query("UPDATE workflow_steps SET state='quarantined' WHERE id IN (SELECT s.id FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id WHERE a.run_id=? AND s.state IN ('claimed','running'))").bind(run_id).execute(&mut *tx).await?;
            sqlx::query("UPDATE workflow_attempts SET state='quarantined' WHERE run_id=? AND state='active'").bind(run_id).execute(&mut *tx).await?;
            sqlx::query("UPDATE workflow_nodes SET state='quarantined' WHERE run_id=? AND state IN ('executing','verifying')").bind(run_id).execute(&mut *tx).await?;
            sqlx::query("UPDATE workflow_cancellations SET state='quarantined' WHERE run_id=?")
                .bind(run_id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE workflow_runs SET state='quarantined',updated_at=? WHERE id=?")
                .bind(now)
                .bind(run_id)
                .execute(&mut *tx)
                .await?;
            events::append(&mut tx, run_id, "cancel_quarantined", "{}", now).await?;
        } else {
            for id in &active {
                let fields = cleanup_fields(proofs[id])?;
                sqlx::query("INSERT INTO workflow_step_lifecycle(step_id,launch_state,cleanup_state,cleanup_detail,finished_at) VALUES(?,'finished',?,?,?) ON CONFLICT(step_id) DO UPDATE SET launch_state='finished',cleanup_state=excluded.cleanup_state,cleanup_detail=excluded.cleanup_detail,finished_at=excluded.finished_at").bind(id).bind(fields.0).bind(fields.1).bind(now).execute(&mut *tx).await?;
            }
            sqlx::query("DELETE FROM workflow_claims WHERE step_id IN (SELECT s.id FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id WHERE a.run_id=? AND s.state IN ('claimed','running'))").bind(run_id).execute(&mut *tx).await?;
            sqlx::query("UPDATE workflow_steps SET state='finished' WHERE id IN (SELECT s.id FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id WHERE a.run_id=? AND s.state IN ('claimed','running'))").bind(run_id).execute(&mut *tx).await?;
            sqlx::query(
                "UPDATE workflow_attempts SET state='cancelled' WHERE run_id=? AND state='active'",
            )
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE workflow_nodes SET state='cancelled' WHERE run_id=? AND state IN ('pending','ready','executing','verifying','awaiting_acceptance')").bind(run_id).execute(&mut *tx).await?;
            sqlx::query(
                "UPDATE workflow_cancellations SET state='finished',completed_at=? WHERE run_id=?",
            )
            .bind(now)
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE workflow_runs SET state='cancelled',updated_at=? WHERE id=?")
                .bind(now)
                .bind(run_id)
                .execute(&mut *tx)
                .await?;
            events::append(&mut tx, run_id, "cancelled", "{}", now).await?;
        }
        store::save_receipt(&mut tx, run_id, request_id, &hash, revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision,
            replayed: false,
        })
    }

    pub async fn retry_node(
        &self,
        run_id: &str,
        revision: i64,
        node_id: &str,
        request_id: &str,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        let hash = store::hash_request("retry_node", &format!("{revision}:{node_id}"));
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, run_id, request_id, &hash).await? {
            return Ok(receipt);
        }
        let run = store::locked_run(&mut tx, run_id, revision).await?;
        if sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM workflow_cancellations WHERE run_id=? AND state='intent'",
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?
            != 0
        {
            bail!("workflow cancellation is in progress");
        }
        let spec = spec_in_tx(&mut tx, run_id, revision).await?;
        let task = spec
            .tasks
            .iter()
            .find(|task| task.id == node_id)
            .context("workflow node not found")?;
        let node: (String, i64) = sqlx::query_as(
            "SELECT state,attempt_count FROM workflow_nodes WHERE run_id=? AND node_id=?",
        )
        .bind(run_id)
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await?;
        if node.0 != "failed" || node.1 >= i64::from(task.retry_policy.max_attempts) {
            bail!("node cannot be retried under its retry policy");
        }
        sqlx::query("UPDATE workflow_nodes SET state='pending',accepted_attempt_id=NULL WHERE run_id=? AND node_id=?").bind(run_id).bind(node_id).execute(&mut *tx).await?;
        if run.state == "failed" {
            sqlx::query("UPDATE workflow_runs SET state='running',updated_at=? WHERE id=?")
                .bind(now)
                .bind(run_id)
                .execute(&mut *tx)
                .await?;
        }
        store::refresh_ready(&mut tx, run_id, revision).await?;
        events::append(&mut tx, run_id, "retry_requested", node_id, now).await?;
        store::save_receipt(&mut tx, run_id, request_id, &hash, revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision,
            replayed: false,
        })
    }
}

async fn assert_lease_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    lease: &StepLease,
    before_start: bool,
    allow_finished: bool,
) -> anyhow::Result<()> {
    let run_state = if before_start {
        "r.state='running'"
    } else {
        "r.state IN ('running','paused')"
    };
    let step_state = if allow_finished {
        "s.state IN ('claimed','running','finished')"
    } else {
        "s.state IN ('claimed','running')"
    };
    let sql = format!("SELECT COUNT(*) FROM workflow_runs r JOIN workflow_attempts a ON a.run_id=r.id JOIN workflow_steps s ON s.attempt_id=a.id LEFT JOIN workflow_cancellations c ON c.run_id=r.id AND c.state='intent' WHERE r.id=? AND r.active_revision=? AND r.epoch=? AND {run_state} AND c.run_id IS NULL AND a.id=? AND a.node_id=? AND a.state='active' AND a.input_hash=? AND s.id=? AND s.generation=? AND {step_state}");
    let valid: i64 = sqlx::query_scalar(&sql)
        .bind(&lease.run_id)
        .bind(lease.revision)
        .bind(lease.epoch)
        .bind(lease.attempt_id)
        .bind(&lease.node_id)
        .bind(&lease.input_hash)
        .bind(lease.step_id)
        .bind(lease.generation)
        .fetch_one(&mut **tx)
        .await?;
    if valid != 1 {
        bail!("stale, cancelled, or quarantined workflow lease");
    }
    Ok(())
}

async fn ensure_lifecycle_state(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    step_id: i64,
    expected: &str,
) -> anyhow::Result<()> {
    let actual: Option<String> =
        sqlx::query_scalar("SELECT launch_state FROM workflow_step_lifecycle WHERE step_id=?")
            .bind(step_id)
            .fetch_optional(&mut **tx)
            .await?;
    if actual.as_deref() != Some(expected) {
        bail!("invalid workflow lifecycle transition");
    }
    Ok(())
}

async fn finish_outcome(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    step_id: i64,
) -> anyhow::Result<FinishStep> {
    let state: Option<String> = sqlx::query_scalar("SELECT state FROM workflow_steps WHERE id=?")
        .bind(step_id)
        .fetch_optional(&mut **tx)
        .await?;
    match state.as_deref() {
        Some("quarantined") => Ok(FinishStep::Quarantined),
        Some("failed") => Ok(FinishStep::Failed),
        Some("finished") => Ok(FinishStep::Verifying),
        _ => bail!("replayed step receipt has no terminal outcome"),
    }
}

async fn quarantine_step(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    lease: &StepLease,
    detail: &str,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO workflow_step_lifecycle(step_id,launch_state,cleanup_state,cleanup_detail,finished_at) VALUES(?,'quarantined','unknown',?,?) ON CONFLICT(step_id) DO UPDATE SET launch_state='quarantined',cleanup_state='unknown',cleanup_detail=excluded.cleanup_detail,finished_at=excluded.finished_at")
        .bind(lease.step_id).bind(detail).bind(now).execute(&mut **tx).await?;
    sqlx::query("UPDATE workflow_claims SET quarantined=1 WHERE step_id=?")
        .bind(lease.step_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE workflow_steps SET state='quarantined' WHERE id=?")
        .bind(lease.step_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE workflow_attempts SET state='quarantined' WHERE id=?")
        .bind(lease.attempt_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE workflow_nodes SET state='quarantined' WHERE run_id=? AND node_id=?")
        .bind(&lease.run_id)
        .bind(&lease.node_id)
        .execute(&mut **tx)
        .await?;
    sqlx::query("UPDATE workflow_runs SET state='quarantined',updated_at=? WHERE id=?")
        .bind(now)
        .bind(&lease.run_id)
        .execute(&mut **tx)
        .await?;
    events::append(tx, &lease.run_id, "step_quarantined", detail, now).await
}

async fn ensure_registered_identity(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    step_id: i64,
    cleanup: &CleanupProof,
) -> anyhow::Result<()> {
    let registered: Option<String> = sqlx::query_scalar::<_, Option<String>>(
        "SELECT container_id FROM workflow_step_lifecycle WHERE step_id=?",
    )
    .bind(step_id)
    .fetch_optional(&mut **tx)
    .await?
    .flatten();
    if let CleanupProof::Terminated { observed_identity } = cleanup {
        if registered.as_deref() != Some(observed_identity.as_str()) {
            bail!("cleanup identity does not match the registered container");
        }
    }
    Ok(())
}

async fn spec_in_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    run_id: &str,
    revision: i64,
) -> anyhow::Result<WorkflowSpec> {
    let json: String = sqlx::query_scalar(
        "SELECT spec_json FROM workflow_revisions WHERE run_id=? AND revision=?",
    )
    .bind(run_id)
    .bind(revision)
    .fetch_one(&mut **tx)
    .await?;
    WorkflowSpec::parse_json(&json).map_err(anyhow::Error::msg)
}

fn validate_intent(lease: &StepLease, intent: &LaunchIntent) -> anyhow::Result<()> {
    if intent.container_name != deterministic_container_name(lease) {
        bail!("container name does not match the deterministic step name");
    }
    if intent.ownership_nonce.len() != 32
        || !intent
            .ownership_nonce
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("ownership nonce must be a 32-character hex value");
    }
    validate_hash("adapter_profile_hash", &intent.adapter_profile_hash)
}

fn validate_nonempty(label: &str, value: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() || value.len() > 1024 {
        bail!("invalid {label}");
    }
    Ok(())
}
fn validate_hash(label: &str, value: &str) -> anyhow::Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid {label}");
    }
    Ok(())
}
fn validate_cleanup(proof: &CleanupProof) -> anyhow::Result<()> {
    match proof {
        CleanupProof::Absent => Ok(()),
        CleanupProof::Terminated { observed_identity } => {
            validate_nonempty("observed_identity", observed_identity)
        }
        CleanupProof::Unknown { reason } => validate_nonempty("cleanup reason", reason),
    }
}
fn cleanup_fields(proof: &CleanupProof) -> anyhow::Result<(&'static str, String)> {
    match proof {
        CleanupProof::Absent => Ok(("absent", String::new())),
        CleanupProof::Terminated { observed_identity } => {
            Ok(("terminated", observed_identity.clone()))
        }
        CleanupProof::Unknown { reason } => Ok(("unknown", reason.clone())),
    }
}
