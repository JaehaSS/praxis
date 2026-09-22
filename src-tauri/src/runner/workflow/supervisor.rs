//! One launch/stop gate serializes state transitions; containers themselves run concurrently.
//! The loop owns every permit until container removal and durable claim release are confirmed.
use super::{
    config::hash,
    driver::{AttemptSpec, EgressMode, PlannedIdentity, PodmanDriver},
    egress::{EgressPolicy, EgressProxy},
    profile::RegisteredCommandProfile,
    verifier, WorkflowService,
};
use crate::workflow::{
    artifacts::{AncestorArtifact, Artifact, CapturePolicy},
    lifecycle::{CleanupProof, ContainerRegistration, LaunchIntent, StepCleanup, StepExit},
    resources::{ClaimResult, StepLease},
    scheduler::ScheduledStep,
    verification::{ArtifactReceipt, CheckReceipt, ManualAcceptance},
    TaskKind, TaskSpec, WorkflowSpec,
};
use anyhow::{ensure, Context, Result};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio::sync::OwnedSemaphorePermit;

#[allow(dead_code)]
enum Permit {
    Scheduled(ScheduledStep),
    Verify(OwnedSemaphorePermit),
}
struct Active {
    lease: StepLease,
    _permit: Permit,
    driver: Arc<PodmanDriver>,
    identity: PlannedIdentity,
    spec: WorkflowSpec,
    task: TaskSpec,
    root: PathBuf,
    input_hash: String,
    artifact: Option<Artifact>,
    verify: bool,
    proxy: Option<EgressProxy>,
    launched: bool,
    deadline: i64,
    quarantined: bool,
    environment_hash: String,
}
#[derive(Default)]
pub struct Supervisor {
    active: BTreeMap<i64, Active>,
}
fn rid(step: i64, action: &str) -> String {
    format!("step-{step}-{action}")
}
pub(super) fn request_key(request: &str, action: &str) -> String {
    hash(format!("{request}:{action}").as_bytes())
}

impl WorkflowService {
    pub async fn run_loop(self: Arc<Self>) {
        while !self.stopping.load(Ordering::SeqCst) {
            if let Err(error) = self.tick().await {
                eprintln!("Workflow tick failed: {error}");
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }
    pub async fn tick(&self) -> Result<()> {
        let mut supervisor = self.supervisor.lock().await;
        let ids: Vec<_> = supervisor.active.keys().copied().collect();
        for id in ids {
            let mut active = supervisor.active.remove(&id).expect("active step");
            if active.quarantined {
                supervisor.active.insert(id, active);
                continue;
            }
            match active.poll(self).await {
                Ok(true) => {}
                Ok(false) => {
                    supervisor.active.insert(id, active);
                }
                Err(error) => {
                    eprintln!("Workflow step {} failed: {}", active.lease.step_id, error);
                    if !active.abort(self).await? {
                        supervisor.active.insert(id, active);
                    }
                }
            }
        }
        if self.stopping.load(Ordering::SeqCst) {
            return Ok(());
        }
        // Verification waits own neither an AI permit nor their preceding execute claims.
        for run_id in self.store.run_ids().await? {
            let run = self.store.run(&run_id).await?;
            if run.state != "running" {
                continue;
            }
            for attempt in self.store.attempts(&run_id).await? {
                if attempt.state != "active"
                    || supervisor
                        .active
                        .values()
                        .any(|a| a.lease.attempt_id == attempt.id)
                {
                    continue;
                }
                let nodes = self.store.nodes(&run_id).await?;
                if !nodes
                    .iter()
                    .any(|n| n.node_id == attempt.node_id && n.state == "verifying")
                {
                    continue;
                }
                let Ok(permit) = self.scheduler.capacity().try_acquire() else {
                    break;
                };
                if let ClaimResult::Claimed(lease) = self
                    .store
                    .claim_verify_step(
                        &run_id,
                        run.active_revision,
                        &attempt.node_id,
                        attempt.id,
                        crate::runner::now_secs(),
                        self.config.task_timeout_secs,
                    )
                    .await?
                {
                    self.launch_claim(&mut supervisor, lease, Permit::Verify(permit), true)
                        .await?;
                }
            }
        }
        while let Some(claim) = self
            .scheduler
            .claim_next(crate::runner::now_secs(), self.config.task_timeout_secs)
            .await?
        {
            let lease = claim.lease.clone();
            self.launch_claim(&mut supervisor, lease, Permit::Scheduled(claim), false)
                .await?;
        }
        Ok(())
    }

    async fn launch_claim(
        &self,
        supervisor: &mut Supervisor,
        lease: StepLease,
        permit: Permit,
        verify: bool,
    ) -> Result<()> {
        let spec = self.store.spec(&lease.run_id, lease.revision).await?;
        let task = spec
            .tasks
            .iter()
            .find(|t| t.id == lease.node_id)
            .context("workflow node not found")?
            .clone();
        let driver = self
            .drivers
            .get(&spec.execution_profile_id)
            .context("runtime profile not registered")?
            .clone();
        let identity = driver.prepare_identity(
            &lease.run_id,
            &lease.attempt_id.to_string(),
            &lease.step_id.to_string(),
        )?;
        let root = self
            .config
            .workspace_root
            .join("attempts")
            .join(&lease.run_id)
            .join(lease.step_id.to_string());
        let mut active = Active {
            lease,
            _permit: permit,
            driver,
            identity,
            spec,
            task,
            root,
            input_hash: String::new(),
            artifact: None,
            verify,
            proxy: None,
            launched: false,
            deadline: crate::runner::now_secs() + self.config.task_timeout_secs,
            quarantined: false,
            environment_hash: self.config_hash.clone(),
        };
        if let Err(error) = self
            .store
            .begin_launch(
                &active.lease,
                &rid(active.lease.step_id, "intent"),
                &LaunchIntent {
                    container_name: active.identity.name.clone(),
                    ownership_nonce: active.identity.nonce.clone(),
                    adapter_profile_hash: self.config_hash.clone(),
                },
                crate::runner::now_secs(),
            )
            .await
        {
            if !active.abort(self).await? {
                supervisor.active.insert(active.lease.step_id, active);
            }
            return Err(error);
        }
        let started = active.prepare_and_start(self).await;
        if let Err(error) = started {
            eprintln!("Workflow launch {} rejected: {error}", active.lease.step_id);
            if active.abort(self).await? {
                return Ok(());
            }
        }
        supervisor.active.insert(active.lease.step_id, active);
        Ok(())
    }

    pub async fn cancel(&self, id: &str, revision: i64, request_id: &str) -> Result<()> {
        // This gate also covers create/register/start. Once cancellation is durable no
        // in-flight launch can cross it and start a container afterward.
        let mut supervisor = self.supervisor.lock().await;
        self.store
            .cancel_intent(id, revision, request_id, crate::runner::now_secs())
            .await?;
        let mut proofs = Vec::new();
        for step in self.store.recovery_steps(id).await? {
            let proof = if let Some(active) = supervisor.active.get_mut(&step.lease.step_id) {
                active.cleanup().await
            } else {
                self.cleanup_recovered(&step).await
            };
            proofs.push(StepCleanup {
                step_id: step.lease.step_id,
                cleanup: proof,
            });
        }
        self.store
            .finish_cancel(
                id,
                revision,
                &request_key(request_id, "complete"),
                &proofs,
                crate::runner::now_secs(),
            )
            .await?;
        let safe = self.store.run(id).await?.state == "cancelled";
        if safe {
            supervisor
                .active
                .retain(|_, active| active.lease.run_id != id);
        }
        ensure!(
            safe,
            "workflow cleanup is quarantined; inspect and repair before retry"
        );
        Ok(())
    }
    pub async fn recover(&self) -> Result<()> {
        super::capability::recover(self).await?;
        for id in self.store.run_ids().await? {
            let run = self.store.run(&id).await?;
            if matches!(run.state.as_str(), "completed" | "cancelled" | "draft") {
                continue;
            }
            let steps = self.store.recovery_steps(&id).await?;
            self.store
                .fence_recovery(&id, crate::runner::now_secs())
                .await?;
            for step in steps {
                let proof = self.cleanup_recovered(&step).await;
                ensure!(
                    !matches!(proof, CleanupProof::Unknown { .. }),
                    "capability_unavailable: unresolved workflow container at recovery"
                );
                self.store
                    .repair_quarantined_step(
                        &id,
                        run.active_revision,
                        step.lease.step_id,
                        &format!("recover-{}-{}", run.epoch, step.lease.step_id),
                        &proof,
                        crate::runner::now_secs(),
                    )
                    .await?;
            }
        }
        Ok(())
    }
    async fn cleanup_recovered(
        &self,
        step: &crate::workflow::lifecycle::RecoveryStep,
    ) -> CleanupProof {
        if step.container_name.is_none()
            && step.ownership_nonce.is_none()
            && step.container_id.is_none()
        {
            return CleanupProof::Absent;
        }
        let result = async {
            let spec = self
                .store
                .spec(&step.lease.run_id, step.lease.revision)
                .await?;
            let driver = self
                .drivers
                .get(&spec.execution_profile_id)
                .context("recovery runtime profile unavailable")?
                .clone();
            let identity = PlannedIdentity {
                name: step
                    .container_name
                    .clone()
                    .context("missing container name")?,
                attempt_id: step.lease.attempt_id.to_string(),
                nonce: step
                    .ownership_nonce
                    .clone()
                    .context("missing ownership nonce")?,
            };
            let proof = cleanup_owned(&driver, &identity).await?;
            remove_secret(
                &self
                    .config
                    .workspace_root
                    .join("attempts")
                    .join(&step.lease.run_id)
                    .join(step.lease.step_id.to_string())
                    .join("credential"),
            )?;
            Ok::<_, anyhow::Error>(proof)
        }
        .await;
        result.unwrap_or_else(|_| CleanupProof::Unknown {
            reason: "container identity or exit could not be established".into(),
        })
    }
    pub async fn repair(&self, id: &str, revision: i64, request_id: &str) -> Result<()> {
        let mut supervisor = self.supervisor.lock().await;
        ensure!(
            self.store.run(id).await?.active_revision == revision,
            "stale workflow revision"
        );
        for step in self.store.recovery_steps(id).await? {
            let proof = if let Some(active) = supervisor.active.get_mut(&step.lease.step_id) {
                active.cleanup().await
            } else {
                self.cleanup_recovered(&step).await
            };
            self.store
                .repair_quarantined_step(
                    id,
                    revision,
                    step.lease.step_id,
                    &request_key(request_id, &step.lease.step_id.to_string()),
                    &proof,
                    crate::runner::now_secs(),
                )
                .await?;
            supervisor.active.remove(&step.lease.step_id);
        }
        Ok(())
    }
    pub async fn retry(&self, id: &str, revision: i64, node: &str, request_id: &str) -> Result<()> {
        let admission = self.store.admission(id, revision).await?;
        ensure!(
            admission.config_hash == self.config_hash,
            "runtime configuration changed; revalidate the workflow"
        );
        let spec = self.store.spec(id, revision).await?;
        self.require_capability(&spec).await?;
        self.store
            .retry_node(id, revision, node, request_id, crate::runner::now_secs())
            .await?;
        Ok(())
    }
    pub async fn accept(
        &self,
        id: &str,
        revision: i64,
        node: &str,
        attempt: i64,
        snapshot: &str,
        criterion: &str,
        request_id: &str,
    ) -> Result<()> {
        let artifact = self.store.artifact_for_attempt(id, attempt).await?;
        self.artifacts.load(&artifact.artifact_id)?;
        self.store
            .accept_manual(
                id,
                revision,
                node,
                attempt,
                request_id,
                &ManualAcceptance {
                    criterion: criterion.into(),
                    output_tree_hash: snapshot.into(),
                },
                crate::runner::now_secs(),
            )
            .await?;
        self.store
            .finalize_verification(
                id,
                revision,
                node,
                attempt,
                &request_key(request_id, "finalize"),
                crate::runner::now_secs(),
            )
            .await?;
        Ok(())
    }
    pub async fn artifact(&self, id: &str, artifact_id: &str) -> Result<serde_json::Value> {
        ensure!(
            self.store
                .artifacts(id)
                .await?
                .iter()
                .any(|a| a.artifact_id == artifact_id),
            "workflow artifact not found"
        );
        let (artifact, manifest, delta) = self.artifacts.load(artifact_id)?;
        Ok(serde_json::json!({"artifact":artifact,"manifest":manifest,"delta":delta}))
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.stopping.store(true, Ordering::SeqCst);
        {
            let mut jobs = self.capability_jobs.lock().await;
            while let Some(result) = jobs.join_next().await {
                result.context("capability worker join failed")?;
            }
        }
        let mut supervisor = self.supervisor.lock().await;
        for id in self.store.run_ids().await? {
            let run = self.store.run(&id).await?;
            if matches!(run.state.as_str(), "draft" | "completed" | "cancelled") {
                continue;
            }
            let steps = self.store.recovery_steps(&id).await?;
            self.store
                .fence_recovery(&id, crate::runner::now_secs())
                .await?;
            for step in steps {
                let proof = if let Some(active) = supervisor.active.get_mut(&step.lease.step_id) {
                    active.cleanup().await
                } else {
                    self.cleanup_recovered(&step).await
                };
                self.store
                    .repair_quarantined_step(
                        &id,
                        run.active_revision,
                        step.lease.step_id,
                        &format!("shutdown-{}-{}", run.epoch, step.lease.step_id),
                        &proof,
                        crate::runner::now_secs(),
                    )
                    .await?;
                supervisor.active.remove(&step.lease.step_id);
            }
        }
        Ok(())
    }
}

impl Active {
    async fn prepare_and_start(&mut self, service: &WorkflowService) -> Result<()> {
        let admission = service
            .store
            .admission(&self.lease.run_id, self.lease.revision)
            .await?;
        ensure!(
            admission.config_hash == service.config_hash,
            "runtime admission changed"
        );
        std::fs::create_dir_all(&self.root)?;
        let artifacts = service.artifacts.clone();
        let input = self.root.join("input");
        let work = self.root.join("work");
        if self.verify {
            let stored = service
                .store
                .artifact_for_attempt(&self.lease.run_id, self.lease.attempt_id)
                .await?;
            let (artifact, _, _) = artifacts.load(&stored.artifact_id)?;
            let copy = artifact.clone();
            tokio::task::spawn_blocking(move || artifacts.materialize(&copy, &work)).await??;
            self.input_hash = stored.input_tree_hash;
            self.artifact = Some(artifact);
        } else {
            let graph = self.spec.validate().map_err(anyhow::Error::msg)?;
            let ancestors = graph.ancestors_of(&self.task.id);
            let mut receipts = Vec::new();
            for (order, node) in graph.topological_order().iter().enumerate() {
                if ancestors.contains(node) {
                    let a = service
                        .store
                        .accepted_artifact(&self.lease.run_id, node)
                        .await?;
                    ensure!(
                        a.config_hash == service.config_hash,
                        "ancestor runtime configuration changed"
                    );
                    receipts.push(AncestorArtifact {
                        node_id: node.clone(),
                        topo_order: order as u32,
                        artifact_id: a.artifact_id,
                    });
                }
            }
            self.input_hash = tokio::task::spawn_blocking(move || {
                let composed = artifacts.compose_input(&admission.base_input_hash, receipts)?;
                artifacts.materialize_input(&composed.input_tree_hash, &input)?;
                artifacts.materialize_input(&composed.input_tree_hash, &work)?;
                Ok::<_, anyhow::Error>(composed.input_tree_hash)
            })
            .await??;
        }
        let prompt = self.root.join("prompt.txt");
        std::fs::write(&prompt, &self.task.objective)?;
        let command = if self.verify {
            verifier::command(&service.config, &self.spec, &self.task)?
        } else if self.task.kind == TaskKind::Agent {
            let vendor = self
                .driver
                .profile()
                .vendor
                .as_ref()
                .context("vendor profile unavailable")?;
            RegisteredCommandProfile {
                id: "workflow-agent".into(),
                runtime_profile_id: self.spec.execution_profile_id.clone(),
                vendor_id: Some(vendor.id.clone()),
                executable: vendor.executable.clone(),
                argv: vendor.argv.clone(),
                env: vendor.env.clone(),
            }
        } else if let Some(id) = &self.task.command_profile_id {
            service
                .config
                .command(id, &self.spec.execution_profile_id)?
                .clone()
        } else {
            RegisteredCommandProfile {
                id: "workflow-integrate".into(),
                runtime_profile_id: self.spec.execution_profile_id.clone(),
                vendor_id: None,
                executable: "/bin/true".into(),
                argv: vec![],
                env: Default::default(),
            }
        };
        let egress = if command.vendor_id.is_some() {
            let vendor = self
                .driver
                .profile()
                .vendor
                .as_ref()
                .context("vendor profile unavailable")?;
            let socket = self.root.join("egress.sock");
            self.proxy = Some(
                EgressProxy::start(
                    socket.clone(),
                    EgressPolicy::new(vendor.tls_domains.clone(), 8)?,
                )
                .await?,
            );
            let credential = if let Some(reference) = &vendor.credential_file_reference {
                let source = service
                    .config
                    .credentials
                    .get(reference)
                    .context("credential not registered")?;
                let destination = self.root.join("credential");
                copy_secret(source, &destination)?;
                Some(destination)
            } else {
                None
            };
            EgressMode::Vendor {
                socket_path: socket,
                credential_file: credential,
            }
        } else {
            EgressMode::Disabled
        };
        let mut resource_mounts = Vec::new();
        if self.verify {
            let claims = service.store.step_resource_claims(&self.lease).await?;
            let mut environments = Vec::new();
            for database in &service.config.test_databases {
                if claims.iter().any(|c| c.resource_id == database.resource_id) {
                    let mount = super::test_db::prepare(
                        &service.store,
                        &self.lease,
                        database,
                        &service.config.workspace_root,
                    )
                    .await?;
                    environments.push(mount.environment_hash);
                    resource_mounts.push(super::driver::ResourceMount {
                        host_path: mount.host_path,
                        container_path: mount.container_path,
                        read_only: mount.read_only,
                    });
                }
            }
            self.environment_hash =
                hash(&serde_json::to_vec(&(&service.config_hash, environments))?);
        }
        let attempt = AttemptSpec {
            run_id: self.lease.run_id.clone(),
            attempt_id: self.lease.attempt_id.to_string(),
            step_id: self.lease.step_id.to_string(),
            workdir: self.root.join("work"),
            prompt_file: prompt,
            egress,
            resource_mounts,
        };
        service.store.assert_lease_current(&self.lease).await?;
        ensure!(
            service.store.run(&self.lease.run_id).await?.state == "running",
            "workflow paused before launch"
        );
        let handle = self
            .driver
            .create_with_identity(&attempt, &command, &self.identity)
            .await?;
        service
            .store
            .register_container(
                &self.lease,
                &rid(self.lease.step_id, "register"),
                &ContainerRegistration {
                    container_id: handle.container_id.clone(),
                    ownership_nonce: self.identity.nonce.clone(),
                },
                crate::runner::now_secs(),
            )
            .await?;
        service
            .store
            .mark_step_running(
                &self.lease,
                &rid(self.lease.step_id, "running"),
                crate::runner::now_secs(),
            )
            .await?;
        self.driver.start(&handle).await?;
        self.launched = true;
        Ok(())
    }
    async fn poll(&mut self, service: &WorkflowService) -> Result<bool> {
        if !self.launched {
            return Ok(false);
        }
        let observed = self
            .driver
            .inspect_by_identity(&self.identity)
            .await?
            .context("container disappeared before output collection")?;
        if observed.inspection.running && crate::runner::now_secs() < self.deadline {
            return Ok(false);
        }
        let timed_out = observed.inspection.running;
        if timed_out {
            self.driver.stop(&observed.handle).await?;
        }
        let (logs, diagnostics) = self.driver.log_streams(&observed.handle).await?;
        let log_hash = super::logs::publish(
            &service.config.workspace_root,
            format!("{logs}{diagnostics}").as_bytes(),
        )?;
        let mut code = if timed_out {
            124
        } else {
            observed
                .inspection
                .exit_code
                .context("missing container exit status")?
        };
        // Remove the entire namespace before reading writable files or releasing resources.
        let cleanup = self.cleanup().await;
        ensure!(
            !matches!(cleanup, CleanupProof::Unknown { .. }),
            "container cleanup is unknown"
        );
        if code == 0 && !self.verify {
            let artifacts = service.artifacts.clone();
            let root = self.root.clone();
            let policy = CapturePolicy {
                include_paths: self.task.output_contract.include_paths.clone(),
                exclude_paths: self.task.output_contract.exclude_paths.clone(),
                write_paths: self.task.write_paths.clone(),
            };
            let artifact = tokio::task::spawn_blocking(move || {
                artifacts.capture(root.join("input"), root.join("work"), &policy)
            })
            .await??;
            ensure!(
                artifact.parent_input_hash == self.input_hash,
                "captured input changed"
            );
            let receipt = ArtifactReceipt {
                artifact_id: artifact.artifact_id.clone(),
                input_tree_hash: self.input_hash.clone(),
                parent_input_hash: artifact.parent_input_hash.clone(),
                output_tree_hash: artifact.output_tree_hash.clone(),
                delta_hash: artifact.delta_hash.clone(),
                manifest_hash: artifact.manifest_hash.clone(),
                task_spec_hash: self
                    .spec
                    .task_execution_hash(&self.task.id)
                    .map_err(anyhow::Error::msg)?,
                config_hash: service.config_hash.clone(),
            };
            service
                .store
                .record_artifact(
                    &self.lease,
                    &rid(self.lease.step_id, "artifact"),
                    &receipt,
                    crate::runner::now_secs(),
                )
                .await?;
        } else if self.verify {
            let artifact = self
                .artifact
                .as_ref()
                .context("missing verifier candidate")?;
            if let Ok(results) = verifier::parse(&logs, &self.task) {
                for (index, result) in results.into_iter().enumerate() {
                    let path = self
                        .root
                        .join("work/.praxis-check-logs")
                        .join(format!("{index}.log"));
                    ensure!(
                        path.canonicalize()?
                            .starts_with(self.root.join("work").canonicalize()?),
                        "check log escaped verification workspace"
                    );
                    let bytes = super::logs::read_bounded(&path)?;
                    ensure!(
                        hash(&bytes) == result.log_hash,
                        "verifier log digest mismatch"
                    );
                    super::logs::publish(&service.config.workspace_root, &bytes)?;
                    let declared = self
                        .task
                        .checks
                        .iter()
                        .find(|c| c.id == result.id)
                        .context("check not declared")?;
                    let profile = service
                        .config
                        .command(&declared.profile_id, &self.spec.execution_profile_id)?;
                    let receipt = CheckReceipt {
                        check_id: result.id.clone(),
                        profile_id: declared.profile_id.clone(),
                        snapshot_hash: artifact.output_tree_hash.clone(),
                        input_hash: self.lease.input_hash.clone(),
                        task_spec_hash: self
                            .spec
                            .task_execution_hash(&self.task.id)
                            .map_err(anyhow::Error::msg)?,
                        check_profile_hash: hash(&serde_json::to_vec(profile)?),
                        image_digest: self.driver.profile().image_digest().into(),
                        environment_hash: self.environment_hash.clone(),
                        exit_code: result.exit_code,
                        log_hash: result.log_hash,
                    };
                    service
                        .store
                        .record_check(
                            &self.lease,
                            &rid(self.lease.step_id, &format!("check-{}", result.id)),
                            &receipt,
                            crate::runner::now_secs(),
                        )
                        .await?;
                    if result.exit_code != 0 {
                        code = 1;
                    }
                }
            } else {
                code = 126;
            }
            service.artifacts.load(&artifact.artifact_id)?;
        }
        service
            .store
            .finish_step(
                &self.lease,
                &rid(self.lease.step_id, "finish"),
                &StepExit {
                    exit_code: code,
                    log_hash,
                    cleanup,
                },
                crate::runner::now_secs(),
            )
            .await?;
        if self.verify && code == 0 {
            service
                .store
                .finalize_verification(
                    &self.lease.run_id,
                    self.lease.revision,
                    &self.lease.node_id,
                    self.lease.attempt_id,
                    &rid(self.lease.step_id, "finalize"),
                    crate::runner::now_secs(),
                )
                .await?;
        }
        Ok(true)
    }
    async fn cleanup(&mut self) -> CleanupProof {
        let proof = cleanup_owned(&self.driver, &self.identity)
            .await
            .unwrap_or_else(|_| CleanupProof::Unknown {
                reason: "container identity/cleanup could not be established".into(),
            });
        if let Some(proxy) = self.proxy.take() {
            if proxy.shutdown().await.is_err() {
                return CleanupProof::Unknown {
                    reason: "egress proxy cleanup failed".into(),
                };
            }
        }
        if remove_secret(&self.root.join("credential")).is_err() {
            return CleanupProof::Unknown {
                reason: "temporary credential removal failed".into(),
            };
        }
        proof
    }
    async fn abort(&mut self, service: &WorkflowService) -> Result<bool> {
        let cleanup = self.cleanup().await;
        let safe = !matches!(cleanup, CleanupProof::Unknown { .. });
        let log_hash = match super::logs::publish(
            &service.config.workspace_root,
            b"workflow supervision failed",
        ) {
            Ok(digest) => digest,
            Err(_) => {
                self.quarantined = true;
                return Ok(false);
            }
        };
        let result = service
            .store
            .finish_step(
                &self.lease,
                &rid(self.lease.step_id, "abort"),
                &StepExit {
                    exit_code: 126,
                    log_hash,
                    cleanup,
                },
                crate::runner::now_secs(),
            )
            .await;
        self.quarantined = !safe;
        // A concurrent cancel owns the terminal transition; keep the permit until its gate runs.
        if result.is_err() {
            self.quarantined = true;
            return Ok(false);
        }
        Ok(safe)
    }
}

pub(super) async fn cleanup_owned(
    driver: &PodmanDriver,
    identity: &PlannedIdentity,
) -> Result<CleanupProof> {
    let Some(observed) = driver.inspect_by_identity(identity).await? else {
        return Ok(CleanupProof::Absent);
    };
    if observed.inspection.running {
        driver.stop(&observed.handle).await?;
    }
    ensure!(
        !driver.inspect(&observed.handle).await?.running,
        "container remains running"
    );
    driver.remove(&observed.handle).await?;
    ensure!(
        driver.inspect_by_identity(identity).await?.is_none(),
        "container removal not confirmed"
    );
    Ok(CleanupProof::Terminated {
        observed_identity: observed.handle.container_id,
    })
}
pub(super) fn copy_secret(source: &std::path::Path, destination: &std::path::Path) -> Result<()> {
    use std::io::{Read, Write};
    let source = std::fs::File::open(source)?;
    ensure!(
        source.metadata()?.is_file() && source.metadata()?.len() <= 1024 * 1024,
        "credential file is invalid or too large"
    );
    let mut bytes = Vec::new();
    source.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 1024 * 1024, "credential file is too large");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(destination)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}
pub(super) fn remove_secret(path: &std::path::Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}
