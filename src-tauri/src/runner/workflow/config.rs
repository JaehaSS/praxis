//! Explicit opt-in configuration. No image installation, mutable tags or implicit repository access.
use super::profile::{RegisteredCommandProfile, RuntimeProfile};
use crate::{
    runner::config::RunnerConfig,
    workflow::{artifacts::GitInputPolicy, resources::ResourceDefinition, TaskKind, WorkflowSpec},
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkflowConfig {
    pub schema_version: u32,
    pub workspace_root: PathBuf,
    pub repositories: BTreeMap<String, PathBuf>,
    pub profiles: BTreeMap<String, RuntimeProfile>,
    pub commands: BTreeMap<String, RegisteredCommandProfile>,
    #[serde(default)]
    pub credentials: BTreeMap<String, PathBuf>,
    #[serde(default)]
    pub resources: Vec<ResourceDefinition>,
    #[serde(default)]
    pub test_databases: Vec<super::test_db::DbProfile>,
    #[serde(default)]
    pub input_policy: GitInputPolicy,
    pub task_timeout_secs: i64,
    /// Python 3 in the pinned image runs the fixed verifier dispatcher without a shell.
    pub verifier_executable: PathBuf,
}

impl WorkflowConfig {
    pub fn load(path: &Path, runner: &RunnerConfig, db: &Path) -> Result<Self> {
        let meta = std::fs::metadata(path)?;
        ensure!(
            meta.len() <= 1024 * 1024,
            "workflow configuration exceeds 1 MiB"
        );
        let mut config: Self = serde_json::from_slice(&std::fs::read(path)?)?;
        ensure!(
            config.schema_version == 1,
            "unsupported workflow configuration version"
        );
        ensure!(
            (1..=86400).contains(&config.task_timeout_secs),
            "invalid workflow task timeout"
        );
        ensure!(
            config.verifier_executable.is_absolute(),
            "verifier must be an absolute in-image path"
        );
        ensure!(
            !config.profiles.is_empty() && config.profiles.len() <= 32,
            "invalid workflow profile count"
        );
        ensure!(
            !config.repositories.is_empty(),
            "workflow repository registry is empty"
        );
        ensure!(
            config.workspace_root.is_absolute(),
            "workflow workspace_root must be absolute"
        );
        std::fs::create_dir_all(&config.workspace_root)?;
        config.workspace_root = config.workspace_root.canonicalize()?;
        ensure!(
            config.workspace_root != Path::new("/"),
            "invalid workspace root"
        );
        let db = db.canonicalize()?;
        let token = runner.pairing_token_file.canonicalize()?;
        ensure!(
            !db.starts_with(&config.workspace_root) && !token.starts_with(&config.workspace_root),
            "workflow workspace must be separate from Runner database and token"
        );
        for (key, repository) in &mut config.repositories {
            crate::workflow::policy::validate_project_ref(key).map_err(anyhow::Error::msg)?;
            *repository = repository.canonicalize()?;
            ensure!(
                repository.is_dir()
                    && runner
                        .repository_roots
                        .iter()
                        .any(|r| repository.starts_with(r)),
                "workflow repository is outside allowed Runner roots"
            );
            ensure!(
                !config.workspace_root.starts_with(&*repository)
                    && !repository.starts_with(&config.workspace_root),
                "workflow workspace and repositories must be disjoint"
            );
        }
        for (id, profile) in &config.profiles {
            ensure!(*id == profile.id, "runtime profile key mismatch");
            profile.validate()?;
        }
        for (id, command) in &config.commands {
            ensure!(*id == command.id, "command profile key mismatch");
            command.validate_for(
                config
                    .profiles
                    .get(&command.runtime_profile_id)
                    .context("unknown command runtime profile")?,
            )?;
            ensure!(
                command.vendor_id.is_none(),
                "registered checks and commands must not receive vendor egress"
            );
        }
        for path in config.credentials.values_mut() {
            *path = path.canonicalize()?;
            ensure!(
                path.is_file() && !path.starts_with(&config.workspace_root),
                "credential must be a separate regular file"
            );
            ensure!(
                !config
                    .repositories
                    .values()
                    .any(|repo| path.starts_with(repo)),
                "credential must be outside exported repositories"
            );
        }
        let mut database_ids = std::collections::BTreeSet::new();
        for database in &config.test_databases {
            ensure!(
                database_ids.insert(&database.resource_id),
                "duplicate SQLite fixture resource"
            );
            ensure!(
                config
                    .resources
                    .iter()
                    .any(|r| r.id == database.resource_id && r.repository.is_none()),
                "SQLite fixture resource is not registered"
            );
            ensure!(
                database.fixture_path.is_absolute() && database.fixture_path.is_file(),
                "SQLite fixture must be an existing absolute path"
            );
            let fixture = database.fixture_path.canonicalize()?;
            ensure!(
                fixture != db && fixture != token && !fixture.starts_with(&config.workspace_root),
                "test fixture must be separate from Runner state"
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                &config.workspace_root,
                std::fs::Permissions::from_mode(0o700),
            )?;
        }
        Ok(config)
    }

    pub fn digest(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }

    pub fn validate_spec(&self, spec: &WorkflowSpec) -> Result<()> {
        spec.validate().map_err(anyhow::Error::msg)?;
        ensure!(
            self.repositories.contains_key(&spec.project_ref),
            "project_ref is not a registered repository"
        );
        let runtime = self
            .profiles
            .get(&spec.execution_profile_id)
            .context("execution profile is not registered")?;
        for task in &spec.tasks {
            if task.kind == TaskKind::Agent {
                let vendor = runtime
                    .vendor
                    .as_ref()
                    .context("agent task requires a configured vendor profile")?;
                if let Some(reference) = &vendor.credential_file_reference {
                    ensure!(
                        self.credentials.contains_key(reference),
                        "vendor credential reference is not registered"
                    );
                }
            }
            if let Some(id) = &task.command_profile_id {
                self.command(id, &runtime.id)?;
            }
            for check in &task.checks {
                self.command(&check.profile_id, &runtime.id)?;
            }
            for (step, requests) in &task.resource_requests_by_step {
                for request in requests {
                    if self
                        .test_databases
                        .iter()
                        .any(|db| db.resource_id == request.resource_id)
                    {
                        ensure!(*step == crate::workflow::StepKind::Verify
                            && request.mode != crate::workflow::AccessMode::Capacity,
                            "SQLite fixtures are available only to shared_read/exclusive_write verification steps");
                    }
                }
            }
            for request in task.resource_requests_by_step.values().flatten() {
                ensure!(
                    self.resources.iter().any(|r| r.id == request.resource_id),
                    "resource is not registered"
                );
            }
        }
        Ok(())
    }
    pub fn command(&self, id: &str, runtime_id: &str) -> Result<&RegisteredCommandProfile> {
        let command = self
            .commands
            .get(id)
            .context("command/check profile is not registered")?;
        ensure!(
            command.runtime_profile_id == runtime_id && command.vendor_id.is_none(),
            "command/check profile has incompatible runtime or egress"
        );
        Ok(command)
    }
}
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
