//! Domain-only workflow admission scheduling.
//!
//! This module creates durable workflow claims but deliberately does not launch a process.
//! The returned [`ScheduledStep`] owns the shared Runner permit until a future supervisor has
//! proved that its process tree stopped and released the associated resource claim.

use std::sync::Arc;

use sqlx::FromRow;
use tokio::sync::{Mutex, OwnedSemaphorePermit};

use crate::runner::capacity::RunnerCapacity;

use super::{
    policy,
    resources::{ClaimResult, StepLease},
    store::{self, WorkflowStore},
};

const DEFAULT_SCAN_LIMIT: usize = 32;

/// A persisted reason why the current revision could not be admitted.
#[derive(Debug, Clone, PartialEq, Eq, FromRow)]
pub struct WaitRecord {
    pub run_id: String,
    pub node_id: String,
    pub reason: String,
    pub resource_id: Option<String>,
    pub owner_run_id: Option<String>,
    pub owner_node_id: Option<String>,
    pub first_wait_at: i64,
    pub updated_at: i64,
}

/// A claimed step plus the runner-global permit that authorizes its future process tree.
///
/// Dropping this value returns the permit. A runtime supervisor must therefore retain it until
/// it has stopped the process and completed its durable claim-release transaction.
pub struct ScheduledStep {
    pub lease: StepLease,
    _capacity: OwnedSemaphorePermit,
}

/// Bounded, fair selector for the workflow domain library.
#[derive(Clone)]
pub struct WorkflowScheduler {
    store: WorkflowStore,
    capacity: RunnerCapacity,
    scan_limit: usize,
    cursor: Arc<Mutex<SchedulerCursor>>,
}

#[derive(Default)]
struct SchedulerCursor {
    last_claimed_run: Option<String>,
    scan_after: Option<(String, String)>,
}

#[derive(Debug, FromRow)]
struct Candidate {
    run_id: String,
    revision: i64,
    node_id: String,
}

#[derive(Debug, FromRow)]
struct ClaimOwner {
    run_id: String,
    node_id: String,
    resource_id: String,
    repository: Option<String>,
    path_prefix: Option<String>,
}

impl WorkflowScheduler {
    pub fn new(store: WorkflowStore, capacity: RunnerCapacity) -> Self {
        Self::with_scan_limit(store, capacity, DEFAULT_SCAN_LIMIT)
    }

    pub fn with_scan_limit(
        store: WorkflowStore,
        capacity: RunnerCapacity,
        scan_limit: usize,
    ) -> Self {
        Self {
            store,
            capacity,
            scan_limit: scan_limit.max(1),
            cursor: Arc::new(Mutex::new(SchedulerCursor::default())),
        }
    }

    pub fn capacity(&self) -> RunnerCapacity {
        self.capacity.clone()
    }

    /// Scans a bounded set of ready nodes and atomically claims one, if possible.
    ///
    /// A resource-blocked node releases its tentative permit before the next candidate is
    /// examined, so it cannot create head-of-line blocking. `claim_next_step` performs the
    /// authoritative BEGIN IMMEDIATE rechecks after the permit has been acquired.
    pub async fn claim_next(
        &self,
        now: i64,
        timeout_secs: i64,
    ) -> anyhow::Result<Option<ScheduledStep>> {
        self.refresh_running_ready().await?;
        self.record_passive_waits(now).await?;
        let candidates = self.fair_candidates().await?;

        for candidate in candidates.into_iter().take(self.scan_limit) {
            let permit = match self.capacity.try_acquire() {
                Ok(permit) => permit,
                Err(_) => {
                    self.record_wait(&candidate, "capacity", None, None, now)
                        .await?;
                    return Ok(None);
                }
            };

            match self
                .store
                .claim_next_step(
                    &candidate.run_id,
                    candidate.revision,
                    &candidate.node_id,
                    now,
                    timeout_secs,
                )
                .await
            {
                Ok(ClaimResult::Claimed(lease)) => {
                    let mut cursor = self.cursor.lock().await;
                    cursor.last_claimed_run = Some(candidate.run_id);
                    cursor.scan_after = None;
                    return Ok(Some(ScheduledStep {
                        lease,
                        _capacity: permit,
                    }));
                }
                Ok(ClaimResult::Waiting {
                    reason,
                    resource_id,
                }) => {
                    let owner = match resource_id.as_deref() {
                        Some(resource_id) if reason == "resource_busy" => {
                            self.resource_owner(resource_id).await?
                        }
                        _ => None,
                    };
                    self.record_wait(&candidate, &reason, resource_id, owner, now)
                        .await?;
                    self.cursor.lock().await.scan_after =
                        Some((candidate.run_id.clone(), candidate.node_id.clone()));
                    // The DB transaction did not create a step, so this permit must be returned
                    // before scanning another candidate.
                    drop(permit);
                }
                Err(error) if error.to_string().contains("stale workflow revision") => {
                    // A revision CAS won between the read-only scan and the claim transaction.
                    // The conditional write below is a no-op for the new revision.
                    self.record_wait(&candidate, "stale_revision", None, None, now)
                        .await?;
                    self.cursor.lock().await.scan_after =
                        Some((candidate.run_id.clone(), candidate.node_id.clone()));
                    drop(permit);
                }
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }

    async fn fair_candidates(&self) -> anyhow::Result<Vec<Candidate>> {
        let candidates: Vec<Candidate> = sqlx::query_as(
            "SELECT r.id AS run_id,r.active_revision AS revision,n.node_id \
             FROM workflow_runs r \
             JOIN workflow_revisions v ON v.run_id=r.id AND v.revision=r.active_revision \
             JOIN workflow_nodes n ON n.run_id=r.id \
             JOIN workflow_ready_queue q ON q.run_id=n.run_id AND q.node_id=n.node_id \
             WHERE r.state='running' AND r.authorization_hash=v.spec_hash AND n.state='ready' \
             ORDER BY r.created_at,r.id,q.sequence",
        )
        .fetch_all(&self.store.pool)
        .await?;
        let cursor = self.cursor.lock().await;
        let mut candidates = round_robin(candidates, cursor.last_claimed_run.as_deref());
        if let Some((run_id, node_id)) = cursor.scan_after.as_ref() {
            if let Some(index) = candidates
                .iter()
                .position(|candidate| candidate.run_id == *run_id && candidate.node_id == *node_id)
            {
                let offset = (index + 1) % candidates.len();
                candidates.rotate_left(offset);
            }
        }
        Ok(candidates)
    }

    async fn refresh_running_ready(&self) -> anyhow::Result<()> {
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let runs: Vec<(String, i64)> =
            sqlx::query_as("SELECT id,active_revision FROM workflow_runs WHERE state='running'")
                .fetch_all(&mut *tx)
                .await?;
        for (run_id, revision) in runs {
            store::refresh_ready(&mut tx, &run_id, revision).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn record_passive_waits(&self, now: i64) -> anyhow::Result<()> {
        // Observation and its persisted reason share one writer transaction.
        // A concurrent resume/reauthorization cannot interleave between them.
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        let paused: Vec<Candidate> = sqlx::query_as(
            "SELECT r.id AS run_id,r.active_revision AS revision,n.node_id \
             FROM workflow_runs r JOIN workflow_nodes n ON n.run_id=r.id \
             WHERE r.state='paused' AND n.state IN ('pending','ready')",
        )
        .fetch_all(&mut *tx)
        .await?;
        for candidate in paused {
            Self::write_wait(&mut tx, &candidate, "paused", None, None, now).await?;
        }

        let unauthorized: Vec<Candidate> = sqlx::query_as(
            "SELECT r.id AS run_id,r.active_revision AS revision,n.node_id \
             FROM workflow_runs r \
             JOIN workflow_revisions v ON v.run_id=r.id AND v.revision=r.active_revision \
             JOIN workflow_nodes n ON n.run_id=r.id \
             WHERE r.state='running' AND n.state IN ('pending','ready') \
               AND (r.authorization_hash IS NULL OR r.authorization_hash<>v.spec_hash)",
        )
        .fetch_all(&mut *tx)
        .await?;
        for candidate in unauthorized {
            Self::write_wait(
                &mut tx,
                &candidate,
                "authorization_required",
                None,
                None,
                now,
            )
            .await?;
        }

        let failed_dependencies: Vec<Candidate> = sqlx::query_as(
            "WITH RECURSIVE failed_descendants(run_id,revision,node_id) AS ( \
                 SELECT r.id,r.active_revision,n.node_id \
                 FROM workflow_runs r JOIN workflow_nodes n ON n.run_id=r.id \
                 WHERE r.state='running' AND n.state IN ('failed','cancelled','quarantined') \
                 UNION \
                 SELECT e.run_id,e.revision,e.target \
                 FROM workflow_edges e JOIN failed_descendants d \
                   ON d.run_id=e.run_id AND d.revision=e.revision AND d.node_id=e.source \
             ) \
             SELECT DISTINCT r.id AS run_id,r.active_revision AS revision,n.node_id \
             FROM workflow_runs r \
             JOIN workflow_nodes n ON n.run_id=r.id \
             JOIN failed_descendants d ON d.run_id=r.id AND d.revision=r.active_revision AND d.node_id=n.node_id \
             WHERE r.state='running' AND n.state IN ('pending','ready') \
               AND EXISTS (SELECT 1 FROM workflow_revisions v WHERE v.run_id=r.id AND v.revision=r.active_revision AND r.authorization_hash=v.spec_hash) \
             ",
        )
        .fetch_all(&mut *tx)
        .await?;
        for candidate in failed_dependencies {
            Self::write_wait(&mut tx, &candidate, "dependency_failed", None, None, now).await?;
        }

        let pending_dependencies: Vec<Candidate> = sqlx::query_as(
            "WITH RECURSIVE failed_descendants(run_id,revision,node_id) AS ( \
                 SELECT r.id,r.active_revision,n.node_id \
                 FROM workflow_runs r JOIN workflow_nodes n ON n.run_id=r.id \
                 WHERE r.state='running' AND n.state IN ('failed','cancelled','quarantined') \
                 UNION \
                 SELECT e.run_id,e.revision,e.target \
                 FROM workflow_edges e JOIN failed_descendants d \
                   ON d.run_id=e.run_id AND d.revision=e.revision AND d.node_id=e.source \
             ) \
             SELECT DISTINCT r.id AS run_id,r.active_revision AS revision,n.node_id \
             FROM workflow_runs r \
             JOIN workflow_nodes n ON n.run_id=r.id \
             WHERE r.state='running' AND n.state='pending' \
               AND EXISTS (SELECT 1 FROM workflow_revisions v WHERE v.run_id=r.id AND v.revision=r.active_revision AND r.authorization_hash=v.spec_hash) \
               AND EXISTS (SELECT 1 FROM workflow_edges e JOIN workflow_nodes p \
                           ON p.run_id=e.run_id AND p.node_id=e.source \
                           WHERE e.run_id=r.id AND e.revision=r.active_revision \
                             AND e.target=n.node_id AND p.state<>'verified') \
               AND NOT EXISTS (SELECT 1 FROM workflow_edges e JOIN workflow_nodes p \
                               ON p.run_id=e.run_id AND p.node_id=e.source \
                               WHERE e.run_id=r.id AND e.revision=r.active_revision \
                                 AND e.target=n.node_id \
                                 AND p.state IN ('failed','cancelled','quarantined')) \
               AND NOT EXISTS (SELECT 1 FROM failed_descendants d \
                               WHERE d.run_id=r.id AND d.revision=r.active_revision AND d.node_id=n.node_id)",
        )
        .fetch_all(&mut *tx)
        .await?;
        for candidate in pending_dependencies {
            Self::write_wait(&mut tx, &candidate, "dependency_pending", None, None, now).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    async fn record_wait(
        &self,
        candidate: &Candidate,
        reason: &str,
        resource_id: Option<String>,
        owner: Option<(String, String)>,
        now: i64,
    ) -> anyhow::Result<()> {
        // This is a raced admission observation, not a durable wait reason.
        // The run may already have resumed; the atomic passive scan owns paused
        // state and must be the only path that records it.
        if reason == "run_not_running" {
            return Ok(());
        }
        let mut tx = self.store.pool.begin_with("BEGIN IMMEDIATE").await?;
        Self::write_wait(&mut tx, candidate, reason, resource_id, owner, now).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn write_wait(
        tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        candidate: &Candidate,
        reason: &str,
        resource_id: Option<String>,
        owner: Option<(String, String)>,
        now: i64,
    ) -> anyhow::Result<()> {
        let (owner_run_id, owner_node_id) = owner.unzip();
        // The active-revision and ready/pending predicates make delayed observations harmless:
        // a concurrent claim or revision cannot resurrect an obsolete wait reason.
        sqlx::query(
            "INSERT INTO workflow_node_waits(\
                 run_id,node_id,reason,resource_id,owner_run_id,owner_node_id,first_wait_at,updated_at\
             ) SELECT ?,?,?,?,?,?,?,? \
               WHERE EXISTS (SELECT 1 FROM workflow_runs r JOIN workflow_nodes n ON n.run_id=r.id \
                             WHERE r.id=? AND r.active_revision=? AND n.node_id=? \
                               AND n.state IN ('pending','ready') \
                               AND ((?='paused' AND r.state='paused') OR (?<>'paused' AND r.state='running'))) \
             ON CONFLICT(run_id,node_id) DO UPDATE SET \
                 reason=excluded.reason,resource_id=excluded.resource_id, \
                 owner_run_id=excluded.owner_run_id,owner_node_id=excluded.owner_node_id, \
                 updated_at=excluded.updated_at",
        )
        .bind(&candidate.run_id)
        .bind(&candidate.node_id)
        .bind(reason)
        .bind(resource_id)
        .bind(owner_run_id)
        .bind(owner_node_id)
        .bind(now)
        .bind(now)
        .bind(&candidate.run_id)
        .bind(candidate.revision)
        .bind(&candidate.node_id)
        .bind(reason)
        .bind(reason)
        .execute(&mut **tx)
        .await?;
        Ok(())
    }

    async fn resource_owner(
        &self,
        requested_resource_id: &str,
    ) -> anyhow::Result<Option<(String, String)>> {
        let requested: Option<(Option<String>, Option<String>)> =
            sqlx::query_as("SELECT repository,path_prefix FROM workflow_resources WHERE id=?")
                .bind(requested_resource_id)
                .fetch_optional(&self.store.pool)
                .await?;
        let Some((repository, path_prefix)) = requested else {
            return Ok(None);
        };
        let owners: Vec<ClaimOwner> = sqlx::query_as(
            "SELECT a.run_id,a.node_id,c.resource_id,r.repository,r.path_prefix \
             FROM workflow_claims c \
             JOIN workflow_resources r ON r.id=c.resource_id \
             JOIN workflow_steps s ON s.id=c.step_id \
             JOIN workflow_attempts a ON a.id=s.attempt_id \
             WHERE s.state IN ('claimed','running','quarantined') \
             ORDER BY c.quarantined DESC,s.id",
        )
        .fetch_all(&self.store.pool)
        .await?;
        let owner = owners.into_iter().find(|owner| {
            owner.resource_id == requested_resource_id
                || matches!(
                    (&repository, &path_prefix, &owner.repository, &owner.path_prefix),
                    (Some(left_repo), Some(left_path), Some(right_repo), Some(right_path))
                        if left_repo == right_repo && policy::paths_overlap(left_path, right_path)
                )
        });
        Ok(owner.map(|owner| (owner.run_id, owner.node_id)))
    }
}

impl WorkflowStore {
    pub async fn waits(&self, run_id: &str) -> anyhow::Result<Vec<WaitRecord>> {
        Ok(sqlx::query_as(
            "SELECT run_id,node_id,reason,resource_id,owner_run_id,owner_node_id,first_wait_at,updated_at \
             FROM workflow_node_waits WHERE run_id=? ORDER BY node_id",
        )
        .bind(run_id)
        .fetch_all(&self.pool)
        .await?)
    }
}

fn round_robin(candidates: Vec<Candidate>, last_run: Option<&str>) -> Vec<Candidate> {
    let mut groups: Vec<Vec<Candidate>> = Vec::new();
    for candidate in candidates {
        if groups
            .last()
            .is_some_and(|group| group[0].run_id == candidate.run_id)
        {
            groups.last_mut().expect("checked group").push(candidate);
        } else {
            groups.push(vec![candidate]);
        }
    }
    if groups.is_empty() {
        return Vec::new();
    }
    let start = last_run
        .and_then(|last| groups.iter().position(|group| group[0].run_id == last))
        .map(|index| (index + 1) % groups.len())
        .unwrap_or(0);
    groups.rotate_left(start);
    let mut ordered = Vec::new();
    loop {
        let mut added = false;
        for group in &mut groups {
            if !group.is_empty() {
                ordered.push(group.remove(0));
                added = true;
            }
        }
        if !added {
            return ordered;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workflow::WorkflowSpec;

    #[tokio::test]
    async fn delayed_paused_claim_result_cannot_write_a_wait_after_resume() {
        let root = crate::testtmp::dir().join("workflow-raced-pause-claim");
        std::fs::create_dir_all(&root).unwrap();
        let store = WorkflowStore::open(&root.join("db.sqlite")).await.unwrap();
        let spec =
            WorkflowSpec::parse_json(include_str!("../../tests/fixtures/workflow-spec-v1.json"))
                .unwrap();
        store.create_run("run", "create", &spec, 1).await.unwrap();
        store
            .authorize_start("run", 1, &spec.digest().unwrap(), "start", 2)
            .await
            .unwrap();
        let candidate = Candidate {
            run_id: "run".into(),
            revision: 1,
            node_id: "api".into(),
        };
        store.pause("run", 1, "pause", 3).await.unwrap();
        let ClaimResult::Waiting {
            reason,
            resource_id,
        } = store.claim_next_step("run", 1, "api", 4, 30).await.unwrap()
        else {
            panic!("paused run was claimed");
        };
        assert_eq!(reason, "run_not_running");
        store.resume("run", 1, "resume", 5).await.unwrap();
        let scheduler = WorkflowScheduler::new(store.clone(), RunnerCapacity::new(0));
        scheduler
            .record_wait(&candidate, &reason, resource_id, None, 6)
            .await
            .unwrap();
        assert!(store.waits("run").await.unwrap().is_empty());
        store.close().await;
        std::fs::remove_dir_all(root).unwrap();
    }
}
