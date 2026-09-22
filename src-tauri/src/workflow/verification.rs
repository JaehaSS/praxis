//! Immutable artifact and check receipts.
//!
//! The worker cannot turn a node verified.  It may only supply a receipt tied to a fenced step;
//! `finalize_verification` validates the complete evidence set and performs the accepted binding.

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
pub struct ArtifactReceipt {
    pub artifact_id: String,
    /// Scheduler's predecessor fingerprint, retained separately from the real tree identity.
    pub input_tree_hash: String,
    pub parent_input_hash: String,
    pub output_tree_hash: String,
    pub delta_hash: String,
    pub manifest_hash: String,
    pub task_spec_hash: String,
    pub config_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckReceipt {
    pub check_id: String,
    pub profile_id: String,
    pub snapshot_hash: String,
    pub input_hash: String,
    pub task_spec_hash: String,
    pub check_profile_hash: String,
    pub image_digest: String,
    pub environment_hash: String,
    pub exit_code: i32,
    pub log_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManualAcceptance {
    pub criterion: String,
    pub output_tree_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, FromRow)]
pub struct ArtifactRecord {
    pub artifact_id: String,
    pub run_id: String,
    pub node_id: String,
    pub attempt_id: i64,
    pub step_id: i64,
    pub claim_input_hash: String,
    pub input_tree_hash: String,
    pub parent_input_hash: String,
    pub output_tree_hash: String,
    pub delta_hash: String,
    pub manifest_hash: String,
    pub task_spec_hash: String,
    pub config_hash: String,
    pub recorded_at: i64,
}

impl WorkflowStore {
    pub async fn artifact_for_attempt(
        &self,
        run_id: &str,
        attempt_id: i64,
    ) -> anyhow::Result<ArtifactRecord> {
        sqlx::query_as("SELECT artifact_id,run_id,node_id,attempt_id,step_id,claim_input_hash,input_tree_hash,parent_input_hash,output_tree_hash,delta_hash,manifest_hash,task_spec_hash,config_hash,recorded_at FROM workflow_artifact_receipts WHERE run_id=? AND attempt_id=?")
            .bind(run_id).bind(attempt_id).fetch_optional(&self.pool).await?.context("workflow artifact not found")
    }

    pub async fn accepted_artifact(
        &self,
        run_id: &str,
        node_id: &str,
    ) -> anyhow::Result<ArtifactRecord> {
        sqlx::query_as("SELECT r.artifact_id,r.run_id,r.node_id,r.attempt_id,r.step_id,r.claim_input_hash,r.input_tree_hash,r.parent_input_hash,r.output_tree_hash,r.delta_hash,r.manifest_hash,r.task_spec_hash,r.config_hash,r.recorded_at FROM workflow_nodes n JOIN workflow_artifact_receipts r ON r.attempt_id=n.accepted_attempt_id WHERE n.run_id=? AND n.node_id=? AND n.state='verified'")
            .bind(run_id).bind(node_id).fetch_optional(&self.pool).await?.context("workflow node has no accepted artifact")
    }

    pub async fn artifacts(&self, run_id: &str) -> anyhow::Result<Vec<ArtifactRecord>> {
        Ok(sqlx::query_as("SELECT artifact_id,run_id,node_id,attempt_id,step_id,claim_input_hash,input_tree_hash,parent_input_hash,output_tree_hash,delta_hash,manifest_hash,task_spec_hash,config_hash,recorded_at FROM workflow_artifact_receipts WHERE run_id=? ORDER BY attempt_id")
            .bind(run_id).fetch_all(&self.pool).await?)
    }

    /// Records a fully-published content-addressed artifact while the producing step is fenced.
    pub async fn record_artifact(
        &self,
        lease: &StepLease,
        request_id: &str,
        artifact: &ArtifactReceipt,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        validate_artifact(artifact)?;
        let hash = store::hash_request(
            "artifact_receipt",
            &serde_json::to_string(&(lease, artifact))?,
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, &lease.run_id, request_id, &hash).await? {
            return Ok(receipt);
        }
        let expected_hash =
            assert_receipt_lease(&mut tx, lease, "s.kind IN ('execute','integrate')").await?;
        assert_started(&mut tx, lease.step_id).await?;
        if artifact.task_spec_hash != expected_hash {
            bail!("artifact task spec hash does not match the claimed node");
        }
        if artifact.parent_input_hash != artifact.input_tree_hash {
            bail!("artifact parent input hash must be the exact input tree hash");
        }
        assert_runtime_config(
            &mut tx,
            &lease.run_id,
            lease.revision,
            &artifact.config_hash,
        )
        .await?;
        sqlx::query(
            "INSERT INTO workflow_artifact_receipts(artifact_id,run_id,node_id,attempt_id,step_id,claim_input_hash,input_tree_hash,parent_input_hash,output_tree_hash,delta_hash,manifest_hash,task_spec_hash,config_hash,recorded_at) \
             VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(&artifact.artifact_id).bind(&lease.run_id).bind(&lease.node_id).bind(lease.attempt_id).bind(lease.step_id)
        .bind(&lease.input_hash).bind(&artifact.input_tree_hash).bind(&artifact.parent_input_hash).bind(&artifact.output_tree_hash)
        .bind(&artifact.delta_hash).bind(&artifact.manifest_hash).bind(&artifact.task_spec_hash).bind(&artifact.config_hash).bind(now)
        .execute(&mut *tx).await?;
        events::append(
            &mut tx,
            &lease.run_id,
            "artifact_recorded",
            &artifact.artifact_id,
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

    /// Records one verifier process result. Its profile and exact immutable input remain part of
    /// the receipt even when the check has a zero exit status.
    pub async fn record_check(
        &self,
        lease: &StepLease,
        request_id: &str,
        check: &CheckReceipt,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        validate_check(check)?;
        let hash = store::hash_request("check_receipt", &serde_json::to_string(&(lease, check))?);
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, &lease.run_id, request_id, &hash).await? {
            return Ok(receipt);
        }
        let expected_hash = assert_receipt_lease(&mut tx, lease, "s.kind='verify'").await?;
        assert_started(&mut tx, lease.step_id).await?;
        let spec = spec_in_tx(&mut tx, &lease.run_id, lease.revision).await?;
        let task = spec
            .tasks
            .iter()
            .find(|task| task.id == lease.node_id)
            .context("workflow node not found")?;
        let expected = task
            .checks
            .iter()
            .find(|candidate| candidate.id == check.check_id)
            .context("unknown workflow check")?;
        if expected.profile_id != check.profile_id {
            bail!("check receipt profile does not match task contract");
        }
        if check.task_spec_hash != expected_hash || check.input_hash != lease.input_hash {
            bail!("check receipt is not bound to this attempt input and task");
        }
        let artifact: Option<String> = sqlx::query_scalar(
            "SELECT output_tree_hash FROM workflow_artifact_receipts WHERE attempt_id=?",
        )
        .bind(lease.attempt_id)
        .fetch_optional(&mut *tx)
        .await?;
        if artifact.as_deref() != Some(check.snapshot_hash.as_str()) {
            bail!("check snapshot hash is not this attempt's artifact");
        }
        sqlx::query(
            "INSERT INTO workflow_check_receipts(attempt_id,check_id,profile_id,step_id,snapshot_hash,input_hash,task_spec_hash,check_profile_hash,image_digest,environment_hash,exit_code,log_hash,recorded_at) \
             VALUES(?,?,?,?,?,?,?,?,?,?,?,?,?)",
        )
        .bind(lease.attempt_id).bind(&check.check_id).bind(&check.profile_id).bind(lease.step_id).bind(&check.snapshot_hash)
        .bind(&check.input_hash).bind(&check.task_spec_hash).bind(&check.check_profile_hash).bind(&check.image_digest)
        .bind(&check.environment_hash).bind(check.exit_code).bind(&check.log_hash).bind(now).execute(&mut *tx).await?;
        events::append(
            &mut tx,
            &lease.run_id,
            "check_recorded",
            &check.check_id,
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

    /// Human approval is scoped to one declared criterion and precisely the artifact hash shown.
    pub async fn accept_manual(
        &self,
        run_id: &str,
        revision: i64,
        node_id: &str,
        attempt_id: i64,
        request_id: &str,
        acceptance: &ManualAcceptance,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        validate_nonempty("manual criterion", &acceptance.criterion)?;
        validate_hash("manual output hash", &acceptance.output_tree_hash)?;
        let hash = store::hash_request(
            "manual_acceptance",
            &serde_json::to_string(&(revision, node_id, attempt_id, acceptance))?,
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, run_id, request_id, &hash).await? {
            return Ok(receipt);
        }
        store::locked_run(&mut tx, run_id, revision).await?;
        let spec = spec_in_tx(&mut tx, run_id, revision).await?;
        let task = spec
            .tasks
            .iter()
            .find(|task| task.id == node_id)
            .context("workflow node not found")?;
        if !task
            .manual_acceptance
            .iter()
            .any(|criterion| criterion == &acceptance.criterion)
        {
            bail!("manual criterion is not declared by task");
        }
        let output: Option<String> = sqlx::query_scalar("SELECT output_tree_hash FROM workflow_artifact_receipts WHERE run_id=? AND node_id=? AND attempt_id=?")
            .bind(run_id).bind(node_id).bind(attempt_id).fetch_optional(&mut *tx).await?;
        if output.as_deref() != Some(acceptance.output_tree_hash.as_str()) {
            bail!("manual acceptance output hash does not match artifact");
        }
        sqlx::query("INSERT INTO workflow_manual_acceptances(attempt_id,criterion,output_tree_hash,accepted_at) VALUES(?,?,?,?)")
            .bind(attempt_id).bind(&acceptance.criterion).bind(&acceptance.output_tree_hash).bind(now).execute(&mut *tx).await?;
        events::append(
            &mut tx,
            run_id,
            "manual_accepted",
            &acceptance.criterion,
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

    /// The only operation which makes an attempt and node verified.
    pub async fn finalize_verification(
        &self,
        run_id: &str,
        revision: i64,
        node_id: &str,
        attempt_id: i64,
        request_id: &str,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        let hash = store::hash_request(
            "finalize_verification",
            &format!("{revision}:{node_id}:{attempt_id}"),
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, run_id, request_id, &hash).await? {
            return Ok(receipt);
        }
        let run = store::locked_run(&mut tx, run_id, revision).await?;
        if !matches!(run.state.as_str(), "running" | "paused") {
            bail!("workflow is not accepting verification results");
        }
        let spec = spec_in_tx(&mut tx, run_id, revision).await?;
        let task = spec
            .tasks
            .iter()
            .find(|task| task.id == node_id)
            .context("workflow node not found")?;
        let expected_hash: String = sqlx::query_scalar(
            "SELECT execution_hash FROM workflow_nodes WHERE run_id=? AND node_id=?",
        )
        .bind(run_id)
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await?;
        let attempt: (String, String) = sqlx::query_as("SELECT state,input_hash FROM workflow_attempts WHERE id=? AND run_id=? AND node_id=? AND revision=? AND epoch=?")
            .bind(attempt_id).bind(run_id).bind(node_id).bind(revision).bind(run.epoch).fetch_optional(&mut *tx).await?.context("stale workflow attempt")?;
        if attempt.0 != "active" {
            bail!("attempt is not awaiting verification");
        }
        let artifact: (String, String) = sqlx::query_as("SELECT artifact_id,output_tree_hash FROM workflow_artifact_receipts WHERE attempt_id=? AND run_id=? AND node_id=? AND claim_input_hash=? AND task_spec_hash=?")
            .bind(attempt_id).bind(run_id).bind(node_id).bind(&attempt.1).bind(&expected_hash).fetch_optional(&mut *tx).await?.context("attempt has no exact artifact receipt")?;
        let finished_verify: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflow_steps s JOIN workflow_step_lifecycle l ON l.step_id=s.id WHERE s.attempt_id=? AND s.kind='verify' AND s.state='finished' AND l.exit_code=0 AND l.cleanup_state IN ('absent','terminated')")
            .bind(attempt_id).fetch_one(&mut *tx).await?;
        if finished_verify == 0 {
            bail!("verifier has not completed with cleanup proof");
        }
        for check in &task.checks {
            let valid: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflow_check_receipts WHERE attempt_id=? AND check_id=? AND profile_id=? AND snapshot_hash=? AND input_hash=? AND task_spec_hash=? AND exit_code=0")
                .bind(attempt_id).bind(&check.id).bind(&check.profile_id).bind(&artifact.1).bind(&attempt.1).bind(&expected_hash).fetch_one(&mut *tx).await?;
            if valid != 1 {
                bail!(
                    "required check has no accepted successful receipt: {}",
                    check.id
                );
            }
        }
        let accepted: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflow_manual_acceptances WHERE attempt_id=? AND output_tree_hash=?")
            .bind(attempt_id).bind(&artifact.1).fetch_one(&mut *tx).await?;
        if accepted < task.manual_acceptance.len() as i64 {
            sqlx::query("UPDATE workflow_nodes SET state='awaiting_acceptance' WHERE run_id=? AND node_id=?").bind(run_id).bind(node_id).execute(&mut *tx).await?;
            events::append(&mut tx, run_id, "manual_acceptance_required", node_id, now).await?;
            store::save_receipt(&mut tx, run_id, request_id, &hash, revision).await?;
            tx.commit().await?;
            return Ok(MutationReceipt {
                revision,
                replayed: false,
            });
        }
        let manual_exact: i64 = sqlx::query_scalar("SELECT COUNT(DISTINCT criterion) FROM workflow_manual_acceptances WHERE attempt_id=? AND output_tree_hash=? AND criterion IN (SELECT value FROM json_each(?))")
            .bind(attempt_id).bind(&artifact.1).bind(serde_json::to_string(&task.manual_acceptance)?).fetch_one(&mut *tx).await?;
        if manual_exact != task.manual_acceptance.len() as i64 {
            bail!("manual acceptances do not exactly cover declared criteria");
        }
        sqlx::query("UPDATE workflow_attempts SET state='succeeded',output_hash=? WHERE id=? AND state='active'").bind(&artifact.1).bind(attempt_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE workflow_nodes SET state='verified',accepted_attempt_id=? WHERE run_id=? AND node_id=?").bind(attempt_id).bind(run_id).bind(node_id).execute(&mut *tx).await?;
        sqlx::query("UPDATE workflow_node_bindings SET accepted_attempt_id=?,input_hash=?,validity='verified' WHERE run_id=? AND revision=? AND node_id=?")
            .bind(attempt_id).bind(&attempt.1).bind(run_id).bind(revision).bind(node_id).execute(&mut *tx).await?;
        events::append(&mut tx, run_id, "node_verified", node_id, now).await?;
        finalize_run_if_complete(
            &mut tx,
            run_id,
            revision,
            &spec,
            attempt_id,
            &artifact.0,
            node_id,
            now,
        )
        .await?;
        store::refresh_ready(&mut tx, run_id, revision).await?;
        store::save_receipt(&mut tx, run_id, request_id, &hash, revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision,
            replayed: false,
        })
    }
}

async fn assert_receipt_lease(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    lease: &StepLease,
    kind: &str,
) -> anyhow::Result<String> {
    let sql = format!("SELECT n.execution_hash FROM workflow_runs r JOIN workflow_attempts a ON a.run_id=r.id JOIN workflow_nodes n ON n.run_id=a.run_id AND n.node_id=a.node_id JOIN workflow_steps s ON s.attempt_id=a.id LEFT JOIN workflow_cancellations c ON c.run_id=r.id AND c.state='intent' WHERE r.id=? AND r.active_revision=? AND r.epoch=? AND r.state IN ('running','paused') AND c.run_id IS NULL AND a.id=? AND a.node_id=? AND a.state='active' AND a.input_hash=? AND s.id=? AND s.generation=? AND s.state IN ('claimed','running') AND {kind}");
    sqlx::query_scalar(&sql)
        .bind(&lease.run_id)
        .bind(lease.revision)
        .bind(lease.epoch)
        .bind(lease.attempt_id)
        .bind(&lease.node_id)
        .bind(&lease.input_hash)
        .bind(lease.step_id)
        .bind(lease.generation)
        .fetch_optional(&mut **tx)
        .await?
        .context("stale or cancelled workflow lease")
}

async fn assert_started(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    step_id: i64,
) -> anyhow::Result<()> {
    let state: Option<String> =
        sqlx::query_scalar("SELECT launch_state FROM workflow_step_lifecycle WHERE step_id=?")
            .bind(step_id)
            .fetch_optional(&mut **tx)
            .await?;
    if state.as_deref() != Some("running") {
        bail!("workflow step has no durable start receipt");
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

/// The domain store also works without the optional runtime-admission table for library tests.
/// Once the runtime has installed that table, an artifact cannot claim a different configuration.
async fn assert_runtime_config(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    run_id: &str,
    revision: i64,
    config_hash: &str,
) -> anyhow::Result<()> {
    let installed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='workflow_admissions'",
    )
    .fetch_one(&mut **tx)
    .await?;
    if installed == 0 {
        return Ok(());
    }
    let admitted: Option<String> = sqlx::query_scalar(
        "SELECT config_hash FROM workflow_admissions WHERE run_id=? AND revision=?",
    )
    .bind(run_id)
    .bind(revision)
    .fetch_optional(&mut **tx)
    .await?;
    if admitted.as_deref() != Some(config_hash) {
        bail!("artifact config hash does not match runtime admission");
    }
    Ok(())
}

async fn finalize_run_if_complete(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    run_id: &str,
    revision: i64,
    spec: &WorkflowSpec,
    attempt_id: i64,
    artifact_id: &str,
    node_id: &str,
    now: i64,
) -> anyhow::Result<()> {
    if node_id != spec.final_task_id {
        return Ok(());
    }
    let remaining: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workflow_nodes WHERE run_id=? AND state NOT IN ('verified','retired')",
    )
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await?;
    if remaining != 0 {
        bail!("final node cannot complete while required nodes are unverified");
    }
    sqlx::query("INSERT INTO workflow_run_finalizations(run_id,final_attempt_id,final_snapshot_id,completed_at) VALUES(?,?,?,?)")
        .bind(run_id).bind(attempt_id).bind(artifact_id).bind(now).execute(&mut **tx).await?;
    sqlx::query("UPDATE workflow_runs SET state='completed',updated_at=? WHERE id=? AND active_revision=? AND state IN ('running','paused')")
        .bind(now).bind(run_id).bind(revision).execute(&mut **tx).await?;
    events::append(tx, run_id, "completed", artifact_id, now).await
}

fn validate_artifact(artifact: &ArtifactReceipt) -> anyhow::Result<()> {
    validate_nonempty("artifact id", &artifact.artifact_id)?;
    for (label, value) in [
        ("input tree hash", &artifact.input_tree_hash),
        ("parent input hash", &artifact.parent_input_hash),
        ("output tree hash", &artifact.output_tree_hash),
        ("delta hash", &artifact.delta_hash),
        ("manifest hash", &artifact.manifest_hash),
        ("task spec hash", &artifact.task_spec_hash),
        ("config hash", &artifact.config_hash),
    ] {
        validate_hash(label, value)?;
    }
    Ok(())
}
fn validate_check(check: &CheckReceipt) -> anyhow::Result<()> {
    validate_nonempty("check id", &check.check_id)?;
    validate_nonempty("profile id", &check.profile_id)?;
    for (label, value) in [
        ("snapshot hash", &check.snapshot_hash),
        ("input hash", &check.input_hash),
        ("task spec hash", &check.task_spec_hash),
        ("check profile hash", &check.check_profile_hash),
        ("environment hash", &check.environment_hash),
        ("log hash", &check.log_hash),
    ] {
        validate_hash(label, value)?;
    }
    validate_image_digest(&check.image_digest)?;
    Ok(())
}
fn validate_hash(label: &str, value: &str) -> anyhow::Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("invalid {label}");
    }
    Ok(())
}
fn validate_image_digest(value: &str) -> anyhow::Result<()> {
    let Some(hash) = value.strip_prefix("sha256:") else {
        bail!("image digest must use canonical sha256:<64hex> form");
    };
    validate_hash("image digest", hash)
}
fn validate_nonempty(label: &str, value: &str) -> anyhow::Result<()> {
    if value.trim().is_empty() || value.len() > 1024 {
        bail!("invalid {label}");
    }
    Ok(())
}
