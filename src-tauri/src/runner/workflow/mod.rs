//! Opt-in Linux workflow runtime and authenticated Runner API.
pub mod capability;
pub mod config;
pub mod driver;
#[cfg(unix)]
pub mod egress;
#[cfg(not(unix))]
#[path = "egress_unsupported.rs"]
pub mod egress;
pub mod http;
pub mod logs;
pub mod profile;
pub mod supervisor;
#[cfg(test)]
mod supervisor_tests;
pub mod test_db;
pub mod verifier;

use crate::runner::{capacity::RunnerCapacity, config::RunnerConfig};
use crate::workflow::{
    artifacts::ArtifactStore, query::RuntimeAdmission, scheduler::WorkflowScheduler,
    store::WorkflowStore, WorkflowSpec,
};
use anyhow::{ensure, Context, Result};
use config::{hash, WorkflowConfig};
use serde_json::Value;
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::Mutex;

pub struct WorkflowService {
    pub store: WorkflowStore,
    pub config: WorkflowConfig,
    pub config_hash: String,
    pub artifacts: ArtifactStore,
    pub scheduler: WorkflowScheduler,
    pub stopping: AtomicBool,
    pub(super) drivers: std::collections::BTreeMap<String, Arc<driver::PodmanDriver>>,
    pub(super) protected_paths: Vec<std::path::PathBuf>,
    pub(super) capabilities: Mutex<std::collections::BTreeMap<String, driver::CapabilityReceipt>>,
    pub(super) capability_jobs: Mutex<tokio::task::JoinSet<()>>,
    pub(super) control: Mutex<()>,
    pub(super) supervisor: Mutex<supervisor::Supervisor>,
}

impl WorkflowService {
    pub async fn open(
        path: &Path,
        runner: &RunnerConfig,
        db: &Path,
        capacity: RunnerCapacity,
    ) -> Result<Arc<Self>> {
        let config = WorkflowConfig::load(path, runner, db)?;
        let config_hash = config.digest()?;
        let store = WorkflowStore::open(db).await?;
        store.ensure_admission_schema().await?;
        for resource in &config.resources {
            store.register_resource(resource).await?;
        }
        let artifacts = ArtifactStore::open(config.workspace_root.join("artifacts"))?;
        let drivers = config
            .profiles
            .iter()
            .map(|(id, profile)| {
                Ok((
                    id.clone(),
                    Arc::new(driver::PodmanDriver::new(profile.clone())?),
                ))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>>>()?;
        let protected_paths = vec![
            std::path::absolute(db)?,
            std::path::absolute(&runner.pairing_token_file)?,
        ];
        let service = Arc::new(Self {
            scheduler: WorkflowScheduler::new(store.clone(), capacity),
            store,
            config,
            config_hash,
            artifacts,
            stopping: AtomicBool::new(false),
            drivers,
            protected_paths,
            capabilities: Mutex::new(Default::default()),
            capability_jobs: Mutex::new(tokio::task::JoinSet::new()),
            control: Mutex::new(()),
            supervisor: Mutex::new(supervisor::Supervisor::default()),
        });
        service.recover().await?;
        Ok(service)
    }
    pub async fn list(&self) -> Result<Vec<Value>> {
        let mut snapshots = Vec::new();
        for id in self.store.run_ids().await? {
            snapshots.push(self.store.snapshot(&id).await?);
        }
        Ok(snapshots)
    }
    pub async fn create(&self, spec: &WorkflowSpec, request_id: &str) -> Result<(String, i64)> {
        self.config.validate_spec(spec)?;
        let id = format!("wf-{}", &hash(request_id.as_bytes())[..32]);
        let receipt = self
            .store
            .create_run(&id, request_id, spec, crate::runner::now_secs())
            .await?;
        Ok((id, receipt.revision))
    }
    pub async fn validate(
        &self,
        id: &str,
        revision: i64,
        request_id: &str,
    ) -> Result<RuntimeAdmission> {
        let _guard = self.control.lock().await;
        let run = self.store.run(id).await?;
        ensure!(run.active_revision == revision, "stale workflow revision");
        let spec = self.store.spec(id, revision).await?;
        self.config.validate_spec(&spec)?;
        self.require_capability(&spec).await?;
        let artifacts = self.artifacts.clone();
        let repo = self
            .config
            .repositories
            .get(&spec.project_ref)
            .context("repository is not registered")?
            .clone();
        let commit = spec.base_commit.clone();
        let policy = self.config.input_policy.clone();
        let exported =
            tokio::task::spawn_blocking(move || artifacts.export_git_input(repo, &commit, &policy))
                .await??;
        let scope_hash = hash(&serde_json::to_vec(&(
            id,
            revision,
            spec.digest().map_err(anyhow::Error::msg)?,
            &self.config_hash,
            &exported.input_tree_hash,
            &exported.filter_hash,
        ))?);
        let admission = RuntimeAdmission {
            config_hash: self.config_hash.clone(),
            base_input_hash: exported.input_tree_hash,
            scope_hash,
        };
        self.store
            .record_admission(
                id,
                revision,
                request_id,
                &admission,
                crate::runner::now_secs(),
            )
            .await?;
        Ok(admission)
    }
    pub async fn start(
        &self,
        id: &str,
        revision: i64,
        request_id: &str,
        approved: Option<&str>,
        resume: bool,
    ) -> Result<()> {
        let _guard = self.control.lock().await;
        ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "workflow Runner is stopping"
        );
        let spec = self.store.spec(id, revision).await?;
        self.config.validate_spec(&spec)?;
        self.require_capability(&spec).await?;
        let admission = self.store.admission(id, revision).await?;
        // Resume without fresh scope is allowed only while the already-authorized spec
        // and server configuration remain exact. Revisions require renewed approval.
        let scope = if let Some(hash) = approved {
            hash.to_string()
        } else {
            ensure!(
                resume
                    && self.store.run(id).await?.authorization_hash.as_deref()
                        == Some(&spec.digest().map_err(anyhow::Error::msg)?)
                    && self
                        .store
                        .authorized_runtime_scope(id, revision)
                        .await?
                        .as_deref()
                        == Some(admission.scope_hash.as_str()),
                "explicit workflow scope approval is required"
            );
            admission.scope_hash.clone()
        };
        self.artifacts.load_input_tree(&admission.base_input_hash)?;
        ensure!(
            !self.stopping.load(Ordering::SeqCst),
            "workflow Runner is stopping"
        );
        self.store
            .start_admitted(
                id,
                revision,
                request_id,
                &scope,
                &self.config_hash,
                resume,
                crate::runner::now_secs(),
            )
            .await?;
        Ok(())
    }
}
