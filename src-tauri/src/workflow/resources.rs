//! Atomic, all-or-nothing step claims. Expiry is diagnostic, never permission to steal.

use super::{
    events, policy,
    store::{self, WorkflowStore},
    AccessMode, ResourceRequest, StepKind, TaskKind, WorkflowSpec,
};
use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, Transaction};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, sqlx::FromRow)]
pub struct ResourceDefinition {
    pub id: String,
    pub physical_identity: String,
    pub capacity: i64,
    pub repository: Option<String>,
    pub path_prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepLease {
    pub run_id: String,
    pub node_id: String,
    pub revision: i64,
    pub epoch: i64,
    pub attempt_id: i64,
    pub step_id: i64,
    pub generation: i64,
    pub input_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimResult {
    Claimed(StepLease),
    Waiting {
        reason: String,
        resource_id: Option<String>,
    },
}

#[derive(sqlx::FromRow)]
struct ExistingClaim {
    resource_id: String,
    mode: String,
    units: i64,
    repository: Option<String>,
    path_prefix: Option<String>,
}

impl WorkflowStore {
    pub async fn register_resource(&self, resource: &ResourceDefinition) -> anyhow::Result<()> {
        policy::validate_resource_id(&resource.id).map_err(anyhow::Error::msg)?;
        if resource.physical_identity.trim().is_empty()
            || resource.physical_identity.len() > 1024
            || !(1..=1024).contains(&resource.capacity)
        {
            bail!("invalid workflow resource definition");
        }
        match (&resource.repository, &resource.path_prefix) {
            (Some(repo), Some(path)) => {
                policy::validate_project_ref(repo).map_err(anyhow::Error::msg)?;
                policy::validate_safe_path(path).map_err(anyhow::Error::msg)?;
                if resource.physical_identity != format!("repo:{repo}:path:{path}") {
                    bail!("code resource identity must match normalized repository and prefix");
                }
            }
            (None, None) => {
                if resource.physical_identity.starts_with("repo:") {
                    bail!("code resource identity requires repository and path metadata");
                }
            }
            _ => bail!("code resources require both repository and path prefix"),
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let old:Option<ResourceDefinition>=sqlx::query_as("SELECT id,physical_identity,capacity,repository,path_prefix FROM workflow_resources WHERE id=?")
            .bind(&resource.id).fetch_optional(&mut *tx).await?;
        if let Some(old) = old {
            if old != *resource {
                bail!("resource definition is immutable");
            }
            return Ok(());
        }
        sqlx::query("INSERT INTO workflow_resources(id,physical_identity,capacity,repository,path_prefix) VALUES(?,?,?,?,?)")
            .bind(&resource.id).bind(&resource.physical_identity).bind(resource.capacity).bind(&resource.repository).bind(&resource.path_prefix).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Claims domain work only. The runtime scheduler must first hold RunnerCapacity and
    /// admit a verified adapter before using this receipt to start an external process.
    pub async fn claim_next_step(
        &self,
        run_id: &str,
        revision: i64,
        node_id: &str,
        now: i64,
        timeout_secs: i64,
    ) -> anyhow::Result<ClaimResult> {
        if !(1..=86400).contains(&timeout_secs) {
            bail!("invalid workflow step timeout");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let run = store::locked_run(&mut tx, run_id, revision).await?;
        if run.state != "running" {
            return Ok(waiting("run_not_running", None));
        }
        let json: String = sqlx::query_scalar(
            "SELECT spec_json FROM workflow_revisions WHERE run_id=? AND revision=?",
        )
        .bind(run_id)
        .bind(revision)
        .fetch_one(&mut *tx)
        .await?;
        let spec = WorkflowSpec::parse_json(&json).map_err(anyhow::Error::msg)?;
        if run.authorization_hash.as_deref() != Some(&spec.digest().map_err(anyhow::Error::msg)?) {
            bail!("current spec has no matching authorization");
        }
        let task = spec
            .tasks
            .iter()
            .find(|t| t.id == node_id)
            .context("workflow node not found")?;
        // An attempt waiting for its independent verifier owns neither an execution permit nor
        // resources. Count active process steps, not active evidence records.
        let active:i64=sqlx::query_scalar("SELECT COUNT(*) FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id WHERE a.run_id=? AND s.state IN ('claimed','running','quarantined')")
            .bind(run_id).fetch_one(&mut *tx).await?;
        if active >= i64::from(spec.limits.max_concurrent_tasks) {
            return Ok(waiting("capacity", None));
        }
        let (state, attempts): (String, i64) = sqlx::query_as(
            "SELECT state,attempt_count FROM workflow_nodes WHERE run_id=? AND node_id=?",
        )
        .bind(run_id)
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await?;
        if !matches!(state.as_str(), "pending" | "ready") {
            return Ok(waiting("node_not_ready", None));
        }
        if attempts >= i64::from(task.retry_policy.max_attempts) {
            return Ok(waiting("attempt_limit", None));
        }
        let failed: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflow_edges e JOIN workflow_nodes p ON p.run_id=e.run_id AND p.node_id=e.source WHERE e.run_id=? AND e.revision=? AND e.target=? AND p.state IN ('failed','cancelled','quarantined')")
            .bind(run_id).bind(revision).bind(node_id).fetch_one(&mut *tx).await?;
        if failed > 0 {
            return Ok(waiting("dependency_failed", None));
        }
        let pending:i64=sqlx::query_scalar("SELECT COUNT(*) FROM workflow_edges e JOIN workflow_nodes p ON p.run_id=e.run_id AND p.node_id=e.source WHERE e.run_id=? AND e.revision=? AND e.target=? AND (p.state<>'verified' OR p.accepted_attempt_id IS NULL)")
            .bind(run_id).bind(revision).bind(node_id).fetch_one(&mut *tx).await?;
        if pending > 0 {
            return Ok(waiting("dependency_pending", None));
        }
        // Bind input identity to the current accepted predecessor results, never a mutable HEAD.
        let predecessors:Vec<(String,String)>=sqlx::query_as("SELECT p.node_id,a.output_hash FROM workflow_edges e JOIN workflow_nodes p ON p.run_id=e.run_id AND p.node_id=e.source JOIN workflow_attempts a ON a.id=p.accepted_attempt_id AND a.run_id=p.run_id AND a.node_id=p.node_id WHERE e.run_id=? AND e.revision=? AND e.target=? AND a.state='succeeded' AND a.output_hash IS NOT NULL ORDER BY p.node_id")
            .bind(run_id).bind(revision).bind(node_id).fetch_all(&mut *tx).await?;
        let edge_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM workflow_edges WHERE run_id=? AND revision=? AND target=?",
        )
        .bind(run_id)
        .bind(revision)
        .bind(node_id)
        .fetch_one(&mut *tx)
        .await?;
        if predecessors.len() as i64 != edge_count {
            return Ok(waiting("input_unverified", None));
        }
        let input_hash = store::hash_request(
            "input",
            &serde_json::to_string(&(
                &spec.base_commit,
                &predecessors,
                &task.input_artifacts,
                &task.output_contract,
            ))?,
        );
        let step_kind = match task.kind {
            TaskKind::Agent => StepKind::Execute,
            // A command task produces its immutable candidate just like an agent task.  Its
            // declared checks still run in a separately-claimed verifier process afterwards.
            TaskKind::Command => StepKind::Execute,
            TaskKind::Integration => StepKind::Integrate,
        };
        let mut requests = task
            .resource_requests_by_step
            .get(&step_kind)
            .cloned()
            .unwrap_or_default();
        // File ownership is derived from the authorized write scope even if a plan omitted
        // redundant explicit resource entries. No two writes can bypass prefix arbitration.
        for path in &task.write_paths {
            let physical = format!("repo:{}:path:{path}", spec.project_ref);
            let existing: Option<ResourceDefinition> =
                sqlx::query_as("SELECT id,physical_identity,capacity,repository,path_prefix FROM workflow_resources WHERE physical_identity=?")
                    .bind(&physical)
                    .fetch_optional(&mut *tx)
                    .await?;
            let resource_id = if let Some(resource) = existing {
                if resource.repository.as_deref() != Some(spec.project_ref.as_str())
                    || resource.path_prefix.as_deref() != Some(path.as_str())
                {
                    bail!("stored code resource metadata does not match its identity");
                }
                resource.id
            } else {
                let id = format!("code-{}", store::hash_request(&spec.project_ref, path));
                sqlx::query("INSERT INTO workflow_resources(id,physical_identity,capacity,repository,path_prefix) VALUES(?,?,1,?,?)")
                    .bind(&id).bind(&physical).bind(&spec.project_ref).bind(path).execute(&mut *tx).await?;
                id
            };
            if let Some(req) = requests.iter_mut().find(|r| r.resource_id == resource_id) {
                req.mode = AccessMode::ExclusiveWrite;
                req.units = 1;
            } else {
                requests.push(ResourceRequest {
                    resource_id,
                    mode: AccessMode::ExclusiveWrite,
                    units: 1,
                });
            }
        }
        if let Some(reason) = conflict(&mut tx, &requests).await? {
            return Ok(reason);
        }
        let attempt_id:i64=sqlx::query_scalar("INSERT INTO workflow_attempts(run_id,node_id,revision,attempt_no,epoch,input_hash,state,created_at) VALUES(?,?,?,?,?,?,'active',?) RETURNING id")
            .bind(run_id).bind(node_id).bind(revision).bind(attempts+1).bind(run.epoch).bind(&input_hash).bind(now).fetch_one(&mut *tx).await?;
        let kind = serde_json::to_value(step_kind)?
            .as_str()
            .context("invalid step kind")?
            .to_string();
        let deadline = now
            .checked_add(timeout_secs)
            .context("step deadline overflow")?;
        let step_id:i64=sqlx::query_scalar("INSERT INTO workflow_steps(attempt_id,kind,generation,state,deadline) VALUES(?,?,1,'claimed',?) RETURNING id")
            .bind(attempt_id).bind(kind).bind(deadline).fetch_one(&mut *tx).await?;
        sqlx::query("UPDATE workflow_steps SET generation=? WHERE id=?")
            .bind(step_id)
            .bind(step_id)
            .execute(&mut *tx)
            .await?;
        for req in &requests {
            let mode = serde_json::to_value(req.mode)?
                .as_str()
                .context("invalid access mode")?
                .to_string();
            sqlx::query("INSERT INTO workflow_claims(step_id,resource_id,mode,units,generation) VALUES(?,?,?,?,?)")
                .bind(step_id).bind(&req.resource_id).bind(mode).bind(i64::from(req.units)).bind(step_id).execute(&mut *tx).await?;
        }
        sqlx::query("UPDATE workflow_nodes SET state='executing',attempt_count=attempt_count+1 WHERE run_id=? AND node_id=?")
            .bind(run_id).bind(node_id).execute(&mut *tx).await?;
        // A scheduler may have recorded a transient admission reason before this transaction
        // acquired the write lock. The successful claim is the authoritative transition.
        sqlx::query("DELETE FROM workflow_node_waits WHERE run_id=? AND node_id=?")
            .bind(run_id)
            .bind(node_id)
            .execute(&mut *tx)
            .await?;
        let lease = StepLease {
            run_id: run_id.into(),
            node_id: node_id.into(),
            revision,
            epoch: run.epoch,
            attempt_id,
            step_id,
            generation: step_id,
            input_hash,
        };
        events::append(
            &mut tx,
            run_id,
            "step_claimed",
            &serde_json::to_string(&lease)?,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(ClaimResult::Claimed(lease))
    }

    pub async fn assert_lease_current(&self, lease: &StepLease) -> anyhow::Result<()> {
        let valid:i64=sqlx::query_scalar("SELECT COUNT(*) FROM workflow_runs r JOIN workflow_attempts a ON a.run_id=r.id JOIN workflow_steps s ON s.attempt_id=a.id LEFT JOIN workflow_cancellations c ON c.run_id=r.id AND c.state='intent' WHERE r.id=? AND r.active_revision=? AND r.epoch=? AND r.state IN ('running','paused') AND c.run_id IS NULL AND a.id=? AND a.node_id=? AND a.state='active' AND a.input_hash=? AND s.id=? AND s.generation=? AND s.state IN ('claimed','running')")
            .bind(&lease.run_id).bind(lease.revision).bind(lease.epoch).bind(lease.attempt_id).bind(&lease.node_id).bind(&lease.input_hash).bind(lease.step_id).bind(lease.generation).fetch_one(&self.pool).await?;
        if valid != 1 {
            bail!("stale or quarantined workflow lease");
        }
        Ok(())
    }

    pub async fn claim_count(&self) -> anyhow::Result<i64> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM workflow_claims")
            .fetch_one(&self.pool)
            .await?)
    }

    /// Claims a verifier only after the producing execute/integrate step has released every
    /// resource. The caller must separately acquire RunnerCapacity immediately before this call.
    pub async fn claim_verify_step(
        &self,
        run_id: &str,
        revision: i64,
        node_id: &str,
        attempt_id: i64,
        now: i64,
        timeout_secs: i64,
    ) -> anyhow::Result<ClaimResult> {
        if !(1..=86400).contains(&timeout_secs) {
            bail!("invalid workflow step timeout");
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let run = store::locked_run(&mut tx, run_id, revision).await?;
        if run.state != "running" {
            return Ok(waiting("run_not_running", None));
        }
        let cancelled: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM workflow_cancellations WHERE run_id=? AND state='intent'",
        )
        .bind(run_id)
        .fetch_one(&mut *tx)
        .await?;
        if cancelled != 0 {
            return Ok(waiting("cancel_requested", None));
        }
        let json: String = sqlx::query_scalar(
            "SELECT spec_json FROM workflow_revisions WHERE run_id=? AND revision=?",
        )
        .bind(run_id)
        .bind(revision)
        .fetch_one(&mut *tx)
        .await?;
        let spec = WorkflowSpec::parse_json(&json).map_err(anyhow::Error::msg)?;
        let task = spec
            .tasks
            .iter()
            .find(|task| task.id == node_id)
            .context("workflow node not found")?;
        let active: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflow_steps s JOIN workflow_attempts a ON a.id=s.attempt_id WHERE a.run_id=? AND s.state IN ('claimed','running','quarantined')")
            .bind(run_id).fetch_one(&mut *tx).await?;
        if active >= i64::from(spec.limits.max_concurrent_tasks) {
            return Ok(waiting("capacity", None));
        }
        let attempt: (String, i64, String, i64) = sqlx::query_as("SELECT a.state,a.epoch,a.input_hash,n.attempt_count FROM workflow_attempts a JOIN workflow_nodes n ON n.run_id=a.run_id AND n.node_id=a.node_id WHERE a.id=? AND a.run_id=? AND a.node_id=? AND a.revision=?")
            .bind(attempt_id).bind(run_id).bind(node_id).bind(revision).fetch_optional(&mut *tx).await?.context("workflow attempt not found")?;
        if attempt.0 != "active" || attempt.1 != run.epoch || attempt.3 < 1 {
            return Ok(waiting("stale_attempt", None));
        }
        let node_state: String =
            sqlx::query_scalar("SELECT state FROM workflow_nodes WHERE run_id=? AND node_id=?")
                .bind(run_id)
                .bind(node_id)
                .fetch_one(&mut *tx)
                .await?;
        if node_state != "verifying" {
            return Ok(waiting("node_not_verifying", None));
        }
        let produced: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM workflow_artifact_receipts WHERE attempt_id=?",
        )
        .bind(attempt_id)
        .fetch_one(&mut *tx)
        .await?;
        if produced != 1 {
            return Ok(waiting("artifact_missing", None));
        }
        let execute_finished: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflow_steps WHERE attempt_id=? AND kind IN ('execute','integrate') AND state='finished'")
            .bind(attempt_id).fetch_one(&mut *tx).await?;
        if execute_finished != 1 {
            return Ok(waiting("execute_not_finished", None));
        }
        let previous_verify: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM workflow_steps WHERE attempt_id=? AND kind='verify'",
        )
        .bind(attempt_id)
        .fetch_one(&mut *tx)
        .await?;
        if previous_verify != 0 {
            return Ok(waiting("verify_already_claimed", None));
        }
        let requests = task
            .resource_requests_by_step
            .get(&StepKind::Verify)
            .cloned()
            .unwrap_or_default();
        if let Some(reason) = conflict(&mut tx, &requests).await? {
            return Ok(reason);
        }
        let deadline = now
            .checked_add(timeout_secs)
            .context("step deadline overflow")?;
        let step_id: i64 = sqlx::query_scalar("INSERT INTO workflow_steps(attempt_id,kind,generation,state,deadline) VALUES(?,'verify',1,'claimed',?) RETURNING id")
            .bind(attempt_id).bind(deadline).fetch_one(&mut *tx).await?;
        sqlx::query("UPDATE workflow_steps SET generation=? WHERE id=?")
            .bind(step_id)
            .bind(step_id)
            .execute(&mut *tx)
            .await?;
        for request in &requests {
            let mode = serde_json::to_value(request.mode)?
                .as_str()
                .context("invalid access mode")?
                .to_owned();
            sqlx::query("INSERT INTO workflow_claims(step_id,resource_id,mode,units,generation) VALUES(?,?,?,?,?)")
                .bind(step_id).bind(&request.resource_id).bind(mode).bind(i64::from(request.units)).bind(step_id).execute(&mut *tx).await?;
        }
        let lease = StepLease {
            run_id: run_id.into(),
            node_id: node_id.into(),
            revision,
            epoch: run.epoch,
            attempt_id,
            step_id,
            generation: step_id,
            input_hash: attempt.2,
        };
        events::append(
            &mut tx,
            run_id,
            "verify_claimed",
            &serde_json::to_string(&lease)?,
            now,
        )
        .await?;
        tx.commit().await?;
        Ok(ClaimResult::Claimed(lease))
    }
}

fn waiting(reason: &str, resource_id: Option<String>) -> ClaimResult {
    ClaimResult::Waiting {
        reason: reason.into(),
        resource_id,
    }
}

async fn conflict(
    tx: &mut Transaction<'_, Sqlite>,
    requests: &[ResourceRequest],
) -> anyhow::Result<Option<ClaimResult>> {
    let claims:Vec<ExistingClaim>=sqlx::query_as("SELECT c.resource_id,c.mode,c.units,r.repository,r.path_prefix FROM workflow_claims c JOIN workflow_resources r ON r.id=c.resource_id")
        .fetch_all(&mut **tx).await?;
    for req in requests {
        let resource:Option<ResourceDefinition>=sqlx::query_as("SELECT id,physical_identity,capacity,repository,path_prefix FROM workflow_resources WHERE id=?")
            .bind(&req.resource_id).fetch_optional(&mut **tx).await?;
        let Some(resource) = resource else {
            return Ok(Some(waiting(
                "resource_unavailable",
                Some(req.resource_id.clone()),
            )));
        };
        if req.units == 0
            || (req.mode != AccessMode::Capacity && req.units != 1)
            || i64::from(req.units) > resource.capacity
        {
            bail!("invalid resource demand");
        }
        let mut used = 0;
        for claim in &claims {
            let same = claim.resource_id == resource.id;
            let overlap = match (
                &resource.repository,
                &resource.path_prefix,
                &claim.repository,
                &claim.path_prefix,
            ) {
                (Some(a), Some(p), Some(b), Some(q)) => a == b && policy::paths_overlap(p, q),
                _ => false,
            };
            if !same && !overlap {
                continue;
            }
            match (&req.mode, claim.mode.as_str()) {
                (AccessMode::SharedRead, "shared_read") => {}
                (AccessMode::Capacity, "capacity") if same => used += claim.units,
                _ => {
                    // A derived requested prefix may have been inserted only in
                    // this losing transaction and will be rolled back. Return
                    // the durable blocking resource so the scheduler can resolve
                    // its owner after rollback (including parent/child overlap).
                    return Ok(Some(waiting(
                        "resource_busy",
                        Some(claim.resource_id.clone()),
                    )));
                }
            }
        }
        if used + i64::from(req.units) > resource.capacity {
            return Ok(Some(waiting(
                "resource_busy",
                Some(req.resource_id.clone()),
            )));
        }
    }
    Ok(None)
}
