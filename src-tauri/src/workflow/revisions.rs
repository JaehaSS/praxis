//! Immutable workflow revision application.
//!
//! A revision never turns a historical accepted attempt into a current verified result. The
//! artifact/evidence layer will later provide an explicit rebind operation. Until then a new
//! current binding is pending even for display-only changes; the prior binding remains intact.

use std::collections::BTreeSet;

use anyhow::{bail, Context};
use sqlx::{Sqlite, Transaction};

use super::{
    events,
    store::{self, MutationReceipt, WorkflowStore},
    WorkflowSpec,
};

impl WorkflowStore {
    /// Applies a new immutable plan revision while the run is quiescent.
    ///
    /// `expected_revision` is a compare-and-swap fence. Any active or quarantined execution,
    /// including a lingering resource claim, blocks the update instead of changing its inputs.
    pub async fn apply_revision(
        &self,
        run_id: &str,
        expected_revision: i64,
        request_id: &str,
        spec: &WorkflowSpec,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        spec.validate().map_err(anyhow::Error::msg)?;
        let spec_hash = spec.digest().map_err(anyhow::Error::msg)?;
        let request_hash = store::hash_request(
            "apply_revision",
            &format!("{expected_revision}:{spec_hash}"),
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, run_id, request_id, &request_hash).await? {
            return Ok(receipt);
        }
        let run = store::locked_run(&mut tx, run_id, expected_revision).await?;
        if !matches!(run.state.as_str(), "draft" | "paused") {
            bail!("workflow must be draft or paused to apply a revision");
        }
        ensure_quiescent(&mut tx, run_id).await?;
        reject_accepted_result_rebind(&mut tx, run_id).await?;
        let has_attempts = has_any_attempts(&mut tx, run_id).await?;
        let old_spec = load_spec(&mut tx, run_id, expected_revision).await?;
        reject_retired_id_reuse(&mut tx, run_id, spec).await?;
        let old_graph = old_spec.validate().map_err(anyhow::Error::msg)?;
        let impacted = old_graph
            .revision_impact(&old_spec, spec)
            .map_err(anyhow::Error::msg)?;
        let next_revision = expected_revision
            .checked_add(1)
            .context("workflow revision overflow")?;
        let old_hash = old_spec.digest().map_err(anyhow::Error::msg)?;

        store::insert_revision(&mut tx, run_id, next_revision, spec, &spec_hash).await?;
        // Waits describe the active revision's admission state. They are not historical
        // evidence and must never be presented after a successful compare-and-swap revision.
        sqlx::query("DELETE FROM workflow_node_waits WHERE run_id=?")
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM workflow_ready_queue WHERE run_id=?")
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        stale_impacted_bindings(&mut tx, run_id, expected_revision, next_revision, &impacted)
            .await?;
        retire_removed_nodes(&mut tx, run_id, &old_spec, spec).await?;
        insert_current_nodes_and_bindings(
            &mut tx,
            run_id,
            next_revision,
            spec,
            impacted.is_empty() && !has_attempts,
        )
        .await?;

        // Edges reference node identities; new tasks must exist before their edges.
        store::insert_edges(&mut tx, run_id, next_revision, spec).await?;

        // A previously exact approval remains valid only for a plan whose execution inputs are
        // unchanged. The current hash must still be updated because claim admission compares it
        // with the immutable active-revision digest.
        let next_authorization = if impacted.is_empty()
            && run.authorization_hash.as_deref() == Some(old_hash.as_str())
        {
            Some(spec_hash.as_str())
        } else {
            None
        };
        sqlx::query(
            "UPDATE workflow_runs \
             SET project_ref=?,base_commit=?,active_revision=?,authorization_hash=?,updated_at=? \
             WHERE id=? AND active_revision=? AND state IN ('draft','paused')",
        )
        .bind(&spec.project_ref)
        .bind(&spec.base_commit)
        .bind(next_revision)
        .bind(next_authorization)
        .bind(now)
        .bind(run_id)
        .bind(expected_revision)
        .execute(&mut *tx)
        .await?;
        events::append(
            &mut tx,
            run_id,
            "revision_applied",
            &serde_json::json!({
                "from": expected_revision,
                "to": next_revision,
                "impacted_nodes": impacted,
                "authorization_preserved": next_authorization.is_some(),
                "current_bindings": "pending_without_evidence_rebind"
            })
            .to_string(),
            now,
        )
        .await?;
        store::save_receipt(&mut tx, run_id, request_id, &request_hash, next_revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision: next_revision,
            replayed: false,
        })
    }

    /// Records explicit approval for a paused revision without starting it. `resume` performs
    /// the separate state transition after this exact digest has been authorized.
    pub async fn reauthorize_revision(
        &self,
        run_id: &str,
        revision: i64,
        spec_hash: &str,
        request_id: &str,
        now: i64,
    ) -> anyhow::Result<MutationReceipt> {
        store::check_identifier(request_id)?;
        let request_hash =
            store::hash_request("reauthorize_revision", &format!("{revision}:{spec_hash}"));
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        if let Some(receipt) = store::replay(&mut tx, run_id, request_id, &request_hash).await? {
            return Ok(receipt);
        }
        let run = store::locked_run(&mut tx, run_id, revision).await?;
        if run.state != "paused" {
            bail!("workflow revision must be paused to reauthorize");
        }
        ensure_quiescent(&mut tx, run_id).await?;
        let stored: String = sqlx::query_scalar(
            "SELECT spec_hash FROM workflow_revisions WHERE run_id=? AND revision=?",
        )
        .bind(run_id)
        .bind(revision)
        .fetch_one(&mut *tx)
        .await?;
        if stored != spec_hash {
            bail!("authorization does not match workflow spec");
        }
        sqlx::query("UPDATE workflow_runs SET authorization_hash=?,updated_at=? WHERE id=?")
            .bind(spec_hash)
            .bind(now)
            .bind(run_id)
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "DELETE FROM workflow_node_waits WHERE run_id=? AND reason='authorization_required'",
        )
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
        events::append(&mut tx, run_id, "revision_reauthorized", spec_hash, now).await?;
        store::save_receipt(&mut tx, run_id, request_id, &request_hash, revision).await?;
        tx.commit().await?;
        Ok(MutationReceipt {
            revision,
            replayed: false,
        })
    }
}

async fn ensure_quiescent(tx: &mut Transaction<'_, Sqlite>, run_id: &str) -> anyhow::Result<()> {
    let active_attempts: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workflow_attempts WHERE run_id=? AND state IN ('active','quarantined')",
    )
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await?;
    if active_attempts != 0 {
        bail!("workflow has active or quarantined attempts");
    }
    let active_claims: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workflow_claims c \
         JOIN workflow_steps s ON s.id=c.step_id \
         JOIN workflow_attempts a ON a.id=s.attempt_id \
         WHERE a.run_id=? AND (c.quarantined=1 OR s.state IN ('claimed','running','quarantined') \
                               OR a.state IN ('active','quarantined'))",
    )
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await?;
    if active_claims != 0 {
        bail!("workflow has active or quarantined resource claims");
    }
    Ok(())
}

async fn reject_accepted_result_rebind(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
) -> anyhow::Result<()> {
    let accepted: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM workflow_nodes WHERE run_id=? AND accepted_attempt_id IS NOT NULL",
    )
    .bind(run_id)
    .fetch_one(&mut **tx)
    .await?;
    if accepted != 0 {
        bail!("verified-result revision rebinding not yet supported");
    }
    Ok(())
}

async fn has_any_attempts(tx: &mut Transaction<'_, Sqlite>, run_id: &str) -> anyhow::Result<bool> {
    let attempts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM workflow_attempts WHERE run_id=?")
        .bind(run_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(attempts != 0)
}

async fn load_spec(
    tx: &mut Transaction<'_, Sqlite>,
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

async fn reject_retired_id_reuse(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    next: &WorkflowSpec,
) -> anyhow::Result<()> {
    let retired: Vec<String> =
        sqlx::query_scalar("SELECT node_id FROM workflow_nodes WHERE run_id=? AND state='retired'")
            .bind(run_id)
            .fetch_all(&mut **tx)
            .await?;
    let next_ids: BTreeSet<_> = next.tasks.iter().map(|task| task.id.as_str()).collect();
    if let Some(id) = retired.iter().find(|id| next_ids.contains(id.as_str())) {
        bail!("retired workflow task id cannot be reused: {id}");
    }
    Ok(())
}

async fn stale_impacted_bindings(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    old_revision: i64,
    next_revision: i64,
    impacted: &BTreeSet<String>,
) -> anyhow::Result<()> {
    for node_id in impacted {
        sqlx::query(
            "UPDATE workflow_node_bindings \
             SET validity='stale',invalidated_by_revision=? \
             WHERE run_id=? AND revision=? AND node_id=?",
        )
        .bind(next_revision)
        .bind(run_id)
        .bind(old_revision)
        .bind(node_id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn retire_removed_nodes(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    old: &WorkflowSpec,
    next: &WorkflowSpec,
) -> anyhow::Result<()> {
    let next_ids: BTreeSet<_> = next.tasks.iter().map(|task| task.id.as_str()).collect();
    for task in &old.tasks {
        if !next_ids.contains(task.id.as_str()) {
            sqlx::query(
                "UPDATE workflow_nodes \
                 SET state='retired',accepted_attempt_id=NULL \
                 WHERE run_id=? AND node_id=?",
            )
            .bind(run_id)
            .bind(&task.id)
            .execute(&mut **tx)
            .await?;
        }
    }
    Ok(())
}

async fn insert_current_nodes_and_bindings(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: &str,
    revision: i64,
    spec: &WorkflowSpec,
    preserve_unattempted_state: bool,
) -> anyhow::Result<()> {
    for task in &spec.tasks {
        let execution_hash = spec
            .task_execution_hash(&task.id)
            .map_err(anyhow::Error::msg)?;
        let existing: Option<String> =
            sqlx::query_scalar("SELECT state FROM workflow_nodes WHERE run_id=? AND node_id=?")
                .bind(run_id)
                .bind(&task.id)
                .fetch_optional(&mut **tx)
                .await?;
        match existing.as_deref() {
            None => {
                sqlx::query(
                    "INSERT INTO workflow_nodes(run_id,node_id,state,execution_hash) \
                     VALUES(?,?,'pending',?)",
                )
                .bind(run_id)
                .bind(&task.id)
                .bind(&execution_hash)
                .execute(&mut **tx)
                .await?;
            }
            Some("retired") => bail!("retired workflow task id cannot be reused: {}", task.id),
            Some(_) if preserve_unattempted_state => {}
            Some(_) => {
                // No accepted attempt is copied to this revision. A later evidence-aware layer
                // may rebind it after checking exact snapshots and input fingerprints.
                sqlx::query(
                    "UPDATE workflow_nodes \
                     SET state='pending',accepted_attempt_id=NULL,execution_hash=? \
                     WHERE run_id=? AND node_id=?",
                )
                .bind(&execution_hash)
                .bind(run_id)
                .bind(&task.id)
                .execute(&mut **tx)
                .await?;
            }
        }
        sqlx::query(
            "INSERT INTO workflow_node_bindings(run_id,revision,node_id,validity) \
             VALUES(?,?,?,'pending')",
        )
        .bind(run_id)
        .bind(revision)
        .bind(&task.id)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
