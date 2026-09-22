//! Rootless Podman adapter for workflow attempts.
//!
//! This module owns only container lifecycle mechanics. The supervisor records
//! the create/register/start intent and persists quarantine decisions; callers
//! must not treat a timed-out Podman operation as permission to retry it.

use anyhow::{anyhow, bail, ensure, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::timeout;

use super::profile::{path_is_safe_mount_source, RegisteredCommandProfile, RuntimeProfile};

const OUTPUT_LIMIT: usize = 1024 * 1024;
/// Logs are evidence and follow the workflow plan's 10 MiB step cap. Podman
/// control responses retain the smaller cap above.
const LOG_OUTPUT_LIMIT: usize = 10 * 1024 * 1024;
const WORKSPACE: &str = "/workspace";
const PROMPT_PATH: &str = "/praxis/prompt.txt";
const EGRESS_SOCKET_PATH: &str = "/praxis/egress.sock";
const CREDENTIAL_PATH: &str = "/praxis/credential";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CapabilityReceipt {
    pub schema_version: u32,
    pub status: CapabilityStatus,
    pub image_digest: String,
    pub checks: Vec<CapabilityCheck>,
    pub runtime_verified: bool,
    pub remaining_evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityStatus {
    CapabilityUnavailable,
    PrerequisiteReady,
    Verified,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CapabilityCheck {
    pub id: String,
    pub passed: bool,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub timed_out: bool,
    pub output_limited: bool,
}

impl CommandResult {
    pub fn succeeded(&self) -> bool {
        self.exit_code == Some(0) && !self.timed_out && !self.output_limited
    }
}

#[async_trait]
pub trait CommandRunner: Send + Sync {
    async fn run(
        &self,
        executable: &Path,
        argv: &[OsString],
        timeout: Duration,
        output_limit: usize,
    ) -> Result<CommandResult>;
}

#[derive(Debug, Default)]
pub struct TokioCommandRunner;

#[async_trait]
impl CommandRunner for TokioCommandRunner {
    async fn run(
        &self,
        executable: &Path,
        argv: &[OsString],
        duration: Duration,
        output_limit: usize,
    ) -> Result<CommandResult> {
        let mut command = Command::new(executable);
        command
            .args(argv)
            .env_clear()
            .env("LANG", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        // These are for the *host Podman process* (rootless storage and its
        // runtime socket), never passed through to the container.
        for key in ["HOME", "XDG_RUNTIME_DIR", "PATH"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let mut child = command
            .spawn()
            .with_context(|| format!("start {}", executable.display()))?;
        let mut stdout = child
            .stdout
            .take()
            .context("Podman stdout was not captured")?;
        let mut stderr = child
            .stderr
            .take()
            .context("Podman stderr was not captured")?;
        let stdout_task =
            tokio::spawn(async move { read_bounded(&mut stdout, output_limit).await });
        let stderr_task =
            tokio::spawn(async move { read_bounded(&mut stderr, output_limit).await });
        let status = match timeout(duration, child.wait()).await {
            Ok(status) => status.context("wait for Podman")?,
            Err(_) => {
                // Kill then reap the exact child. The attempted operation remains
                // ambiguous to the caller even though its CLI process is gone.
                let _ = child.start_kill();
                let _ = child.wait().await;
                let (stdout, stderr) = join_output(stdout_task, stderr_task).await?;
                return Ok(CommandResult {
                    exit_code: None,
                    stdout: stdout.0,
                    stderr: stderr.0,
                    timed_out: true,
                    output_limited: stdout.1 || stderr.1,
                });
            }
        };
        let (stdout, stderr) = join_output(stdout_task, stderr_task).await?;
        Ok(CommandResult {
            exit_code: status.code(),
            stdout: stdout.0,
            stderr: stderr.0,
            timed_out: false,
            output_limited: stdout.1 || stderr.1,
        })
    }
}

async fn join_output(
    stdout: tokio::task::JoinHandle<Result<(Vec<u8>, bool)>>,
    stderr: tokio::task::JoinHandle<Result<(Vec<u8>, bool)>>,
) -> Result<((Vec<u8>, bool), (Vec<u8>, bool))> {
    Ok((
        stdout.await.context("join Podman stdout reader")??,
        stderr.await.context("join Podman stderr reader")??,
    ))
}

async fn read_bounded(
    reader: &mut (impl tokio::io::AsyncRead + Unpin),
    limit: usize,
) -> Result<(Vec<u8>, bool)> {
    let mut result = Vec::with_capacity(limit.min(8192));
    let mut buffer = [0u8; 8192];
    let mut limited = false;
    loop {
        let count = reader.read(&mut buffer).await?;
        if count == 0 {
            return Ok((result, limited));
        }
        let space = limit.saturating_sub(result.len());
        result.extend_from_slice(&buffer[..count.min(space)]);
        limited |= count > space;
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressMode {
    Disabled,
    Vendor {
        socket_path: PathBuf,
        credential_file: Option<PathBuf>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceMount {
    pub host_path: PathBuf,
    pub container_path: PathBuf,
    pub read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptSpec {
    pub run_id: String,
    pub attempt_id: String,
    pub step_id: String,
    pub workdir: PathBuf,
    /// A freshly written, read-only prompt file. Agent objectives are passed
    /// through this file, never interpolated into an argv or a shell command.
    pub prompt_file: PathBuf,
    pub egress: EgressMode,
    pub resource_mounts: Vec<ResourceMount>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttemptHandle {
    pub container_id: String,
    pub name: String,
    pub attempt_id: String,
    pub nonce: String,
}

/// Identity generated before the supervisor writes its create intent. The
/// nonce must be persisted before `create_with_identity` is called.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannedIdentity {
    pub name: String,
    pub attempt_id: String,
    pub nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedContainer {
    pub handle: AttemptHandle,
    pub inspection: ContainerInspection,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerInspection {
    pub id: String,
    pub running: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuarantinedOperation {
    pub attempt_id: String,
    pub container_id: Option<String>,
    pub operation: String,
}

pub struct PodmanDriver {
    profile: RuntimeProfile,
    runner: Arc<dyn CommandRunner>,
    quarantined: Mutex<Vec<QuarantinedOperation>>,
}

impl PodmanDriver {
    pub fn new(profile: RuntimeProfile) -> Result<Self> {
        Self::with_runner(profile, Arc::new(TokioCommandRunner))
    }

    pub fn with_runner(profile: RuntimeProfile, runner: Arc<dyn CommandRunner>) -> Result<Self> {
        profile.validate()?;
        Ok(Self {
            profile,
            runner,
            quarantined: Mutex::new(Vec::new()),
        })
    }

    pub fn profile(&self) -> &RuntimeProfile {
        &self.profile
    }
    pub fn quarantined_operations(&self) -> Vec<QuarantinedOperation> {
        self.quarantined
            .lock()
            .expect("quarantine mutex poisoned")
            .clone()
    }

    /// Local prerequisite checks only. It never creates, pulls, starts or logs
    /// into a container registry, and it never returns `runtime_verified=true`.
    pub async fn probe(&self) -> CapabilityReceipt {
        let mut checks = Vec::new();
        if !cfg!(target_os = "linux") {
            checks.push(check("host_linux", false, "requires a Linux runner"));
            return unavailable(&self.profile, checks);
        }
        checks.push(check("host_linux", true, "Linux runner detected"));
        let info = match self
            .call(
                vec!["info".into(), "--format".into(), "json".into()],
                self.profile.timeouts.probe_ms,
            )
            .await
        {
            Ok(result) if result.succeeded() => result,
            Ok(result) => {
                checks.push(check("podman_callable", false, command_detail(&result)));
                return unavailable(&self.profile, checks);
            }
            Err(error) => {
                checks.push(check("podman_callable", false, error.to_string()));
                return unavailable(&self.profile, checks);
            }
        };
        checks.push(check("podman_callable", true, "podman info completed"));
        let parsed: serde_json::Value = match serde_json::from_slice(&info.stdout) {
            Ok(value) => value,
            Err(_) => {
                checks.push(check(
                    "podman_rootless",
                    false,
                    "podman info returned malformed JSON",
                ));
                return unavailable(&self.profile, checks);
            }
        };
        let rootless = parsed
            .pointer("/host/security/rootless")
            .or_else(|| parsed.pointer("/host/rootless"))
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        checks.push(check(
            "podman_rootless",
            rootless,
            if rootless {
                "Podman reports rootless execution"
            } else {
                "Podman does not report rootless execution"
            },
        ));
        let controllers = parsed
            .pointer("/host/cgroupControllers")
            .or_else(|| parsed.pointer("/host/cgroup_controllers"))
            .and_then(serde_json::Value::as_array);
        let controller_present = |name: &str| {
            controllers.is_some_and(|items| items.iter().any(|item| item.as_str() == Some(name)))
        };
        let cgroup_v2 = parsed
            .pointer("/host/cgroupVersion")
            .or_else(|| parsed.pointer("/host/cgroup_version"))
            .and_then(serde_json::Value::as_str)
            == Some("v2");
        let resources = cgroup_v2
            && ["cpu", "memory", "pids"]
                .into_iter()
                .all(controller_present);
        checks.push(check(
            "cgroup_v2_resource_limits",
            resources,
            if resources {
                "cgroup v2 exposes cpu, memory, and pids"
            } else {
                "cgroup v2 resource controllers are incomplete"
            },
        ));
        let image = match self
            .call(
                vec![
                    "image".into(),
                    "inspect".into(),
                    "--format".into(),
                    "json".into(),
                    self.profile.image.clone().into(),
                ],
                self.profile.timeouts.probe_ms,
            )
            .await
        {
            Ok(result) if result.succeeded() => result,
            Ok(result) => {
                checks.push(check(
                    "pinned_image_present",
                    false,
                    command_detail(&result),
                ));
                return unavailable(&self.profile, checks);
            }
            Err(error) => {
                checks.push(check("pinned_image_present", false, error.to_string()));
                return unavailable(&self.profile, checks);
            }
        };
        let matching = match serde_json::from_slice::<serde_json::Value>(&image.stdout) {
            Ok(value) => {
                let item = value
                    .as_array()
                    .and_then(|items| items.first())
                    .unwrap_or(&value);
                item.get("Digest")
                    .or_else(|| item.get("digest"))
                    .and_then(serde_json::Value::as_str)
                    == Some(self.profile.image_digest())
            }
            Err(_) => false,
        };
        checks.push(check(
            "pinned_image_present",
            matching,
            if matching {
                "specified image digest is present locally"
            } else {
                "local image digest does not match profile"
            },
        ));
        if checks.iter().all(|check| check.passed) {
            prerequisite_ready(&self.profile, checks)
        } else {
            unavailable(&self.profile, checks)
        }
    }

    /// A full Linux smoke must be orchestrated as a normal persisted attempt:
    /// it needs the same proxy, credentials, mounts and cleanup receipt as a
    /// worker. This method exists only to fail closed if a caller mistakes the
    /// prerequisite probe for that stronger evidence.
    pub async fn verify_capability(&self) -> CapabilityReceipt {
        let mut receipt = self.probe().await;
        receipt.status = CapabilityStatus::CapabilityUnavailable;
        receipt.runtime_verified = false;
        receipt.checks.push(check(
            "supervised_linux_smoke",
            false,
            "requires a persisted hardened attempt and independent evidence receipt",
        ));
        receipt
    }

    pub fn prepare_identity(
        &self,
        run_id: &str,
        attempt_id: &str,
        step_id: &str,
    ) -> Result<PlannedIdentity> {
        validate_identity_part("run", run_id)?;
        validate_identity_part("attempt", attempt_id)?;
        validate_identity_part("step", step_id)?;
        let nonce = random_nonce()?;
        Ok(PlannedIdentity {
            name: format!("workflow-{run_id}-{attempt_id}-{step_id}"),
            attempt_id: attempt_id.to_owned(),
            nonce,
        })
    }

    /// Convenience for callers that do not persist lifecycle state. Workflow
    /// supervision must use `prepare_identity` then `create_with_identity`.
    pub async fn create(
        &self,
        attempt: &AttemptSpec,
        command: &RegisteredCommandProfile,
    ) -> Result<AttemptHandle> {
        let identity =
            self.prepare_identity(&attempt.run_id, &attempt.attempt_id, &attempt.step_id)?;
        self.create_with_identity(attempt, command, &identity).await
    }

    pub async fn create_with_identity(
        &self,
        attempt: &AttemptSpec,
        command: &RegisteredCommandProfile,
        identity: &PlannedIdentity,
    ) -> Result<AttemptHandle> {
        command.validate_for(&self.profile)?;
        validate_attempt(attempt, command, &self.profile)?;
        ensure!(
            identity.attempt_id == attempt.attempt_id,
            "planned identity belongs to another attempt"
        );
        ensure!(
            identity.name
                == format!(
                    "workflow-{}-{}-{}",
                    attempt.run_id, attempt.attempt_id, attempt.step_id
                ),
            "planned container name does not match workflow identity"
        );
        ensure!(
            identity.nonce.len() == 32
                && identity.nonce.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "planned ownership nonce is invalid"
        );
        let args = self.create_args(attempt, command, &identity.name, &identity.nonce)?;
        let result = self.call(args, self.profile.timeouts.create_ms).await?;
        if result.timed_out || result.output_limited {
            return Err(self.quarantine(
                &attempt.attempt_id,
                None,
                "create",
                "container create outcome is ambiguous",
            ));
        }
        ensure!(
            result.exit_code == Some(0),
            "podman create failed: {}",
            safe_stderr(&result)
        );
        let container_id = String::from_utf8(result.stdout)
            .context("podman create returned non-UTF8 container ID")?
            .trim()
            .to_owned();
        ensure!(
            is_container_id(&container_id),
            "podman create returned an invalid container ID"
        );
        Ok(AttemptHandle {
            container_id,
            name: identity.name.clone(),
            attempt_id: attempt.attempt_id.clone(),
            nonce: identity.nonce.clone(),
        })
    }

    pub async fn register(&self, handle: &AttemptHandle) -> Result<ContainerInspection> {
        self.inspect(handle).await
    }

    pub async fn start(&self, handle: &AttemptHandle) -> Result<()> {
        self.assert_identity(handle).await?;
        let result = self
            .call(
                vec!["start".into(), handle.container_id.clone().into()],
                self.profile.timeouts.start_ms,
            )
            .await?;
        if result.timed_out || result.output_limited {
            return Err(self.quarantine(
                &handle.attempt_id,
                Some(&handle.container_id),
                "start",
                "container start outcome is ambiguous",
            ));
        }
        ensure!(
            result.exit_code == Some(0),
            "podman start failed: {}",
            safe_stderr(&result)
        );
        Ok(())
    }

    pub async fn inspect(&self, handle: &AttemptHandle) -> Result<ContainerInspection> {
        let result = self
            .call(
                vec![
                    "inspect".into(),
                    "--format".into(),
                    "json".into(),
                    handle.container_id.clone().into(),
                ],
                self.profile.timeouts.inspect_ms,
            )
            .await?;
        if result.timed_out || result.output_limited {
            return Err(self.quarantine(
                &handle.attempt_id,
                Some(&handle.container_id),
                "inspect",
                "container identity outcome is ambiguous",
            ));
        }
        ensure!(
            result.exit_code == Some(0),
            "podman inspect failed: {}",
            safe_stderr(&result)
        );
        let (id, inspection) =
            match decode_inspection(&result.stdout, &handle.attempt_id, &handle.nonce) {
                Ok(value) => value,
                Err(_) => {
                    return Err(self.quarantine(
                        &handle.attempt_id,
                        Some(&handle.container_id),
                        "inspect",
                        "container identity or ownership labels do not match",
                    ))
                }
            };
        if id != handle.container_id {
            return Err(self.quarantine(
                &handle.attempt_id,
                Some(&handle.container_id),
                "inspect",
                "container identity or ownership labels do not match",
            ));
        }
        Ok(inspection)
    }

    /// Resolve a persisted create intent after a lost create response. The
    /// `container exists` probe makes absence explicit; it never guesses from a
    /// failed `inspect` invocation.
    pub async fn inspect_by_identity(
        &self,
        identity: &PlannedIdentity,
    ) -> Result<Option<ObservedContainer>> {
        let exists = self
            .call(
                vec![
                    "container".into(),
                    "exists".into(),
                    identity.name.clone().into(),
                ],
                self.profile.timeouts.inspect_ms,
            )
            .await?;
        if exists.timed_out || exists.output_limited {
            return Err(self.quarantine(
                &identity.attempt_id,
                None,
                "container_exists",
                "container existence outcome is ambiguous",
            ));
        }
        if exists.exit_code == Some(1) {
            return Ok(None);
        }
        ensure!(
            exists.exit_code == Some(0),
            "podman container exists failed: {}",
            safe_stderr(&exists)
        );
        let result = self
            .call(
                vec![
                    "inspect".into(),
                    "--format".into(),
                    "json".into(),
                    identity.name.clone().into(),
                ],
                self.profile.timeouts.inspect_ms,
            )
            .await?;
        if result.timed_out || result.output_limited {
            return Err(self.quarantine(
                &identity.attempt_id,
                None,
                "inspect",
                "container identity outcome is ambiguous",
            ));
        }
        ensure!(
            result.exit_code == Some(0),
            "podman inspect failed after container exists: {}",
            safe_stderr(&result)
        );
        let (container_id, inspection) =
            match decode_inspection(&result.stdout, &identity.attempt_id, &identity.nonce) {
                Ok(value) => value,
                Err(_) => {
                    return Err(self.quarantine(
                        &identity.attempt_id,
                        None,
                        "inspect",
                        "container identity or ownership labels do not match",
                    ))
                }
            };
        let handle = AttemptHandle {
            container_id,
            name: identity.name.clone(),
            attempt_id: identity.attempt_id.clone(),
            nonce: identity.nonce.clone(),
        };
        Ok(Some(ObservedContainer { handle, inspection }))
    }

    pub async fn stop(&self, handle: &AttemptHandle) -> Result<()> {
        self.assert_identity(handle).await?;
        let result = self
            .call(
                vec![
                    "stop".into(),
                    "--time".into(),
                    "10".into(),
                    handle.container_id.clone().into(),
                ],
                self.profile.timeouts.stop_ms,
            )
            .await?;
        if result.timed_out || result.output_limited {
            return Err(self.quarantine(
                &handle.attempt_id,
                Some(&handle.container_id),
                "stop",
                "container stop outcome is ambiguous",
            ));
        }
        ensure!(
            result.exit_code == Some(0),
            "podman stop failed: {}",
            safe_stderr(&result)
        );
        Ok(())
    }

    pub async fn remove(&self, handle: &AttemptHandle) -> Result<()> {
        self.assert_identity(handle).await?;
        let result = self
            .call(
                vec!["rm".into(), handle.container_id.clone().into()],
                self.profile.timeouts.remove_ms,
            )
            .await?;
        if result.timed_out || result.output_limited {
            return Err(self.quarantine(
                &handle.attempt_id,
                Some(&handle.container_id),
                "remove",
                "container removal outcome is ambiguous",
            ));
        }
        ensure!(
            result.exit_code == Some(0),
            "podman rm failed: {}",
            safe_stderr(&result)
        );
        Ok(())
    }

    pub async fn logs(&self, handle: &AttemptHandle) -> Result<String> {
        let (stdout, stderr) = self.log_streams(handle).await?;
        Ok(format!("{stdout}{stderr}"))
    }

    pub async fn log_streams(&self, handle: &AttemptHandle) -> Result<(String, String)> {
        self.assert_identity(handle).await?;
        let result = self
            .call_with_limit(
                vec!["logs".into(), handle.container_id.clone().into()],
                self.profile.timeouts.logs_ms,
                LOG_OUTPUT_LIMIT,
            )
            .await?;
        ensure!(
            !result.timed_out && !result.output_limited,
            "podman logs did not complete within bounds"
        );
        ensure!(
            result.exit_code == Some(0),
            "podman logs failed: {}",
            safe_stderr(&result)
        );
        ensure!(
            result.stdout.len() + result.stderr.len() <= LOG_OUTPUT_LIMIT,
            "combined container log exceeds 10 MiB"
        );
        Ok((
            String::from_utf8(result.stdout).context("podman logs returned non-UTF8 stdout")?,
            String::from_utf8(result.stderr).context("podman logs returned non-UTF8 stderr")?,
        ))
    }

    fn create_args(
        &self,
        attempt: &AttemptSpec,
        command: &RegisteredCommandProfile,
        name: &str,
        nonce: &str,
    ) -> Result<Vec<OsString>> {
        let mut args: Vec<OsString> = vec![
            "create".into(),
            "--pull=never".into(),
            "--name".into(),
            name.into(),
            "--label".into(),
            format!("praxis.workflow.attempt={}", attempt.attempt_id).into(),
            "--label".into(),
            format!("praxis.workflow.nonce={nonce}").into(),
            "--network".into(),
            "none".into(),
            "--read-only".into(),
            "--userns".into(),
            "keep-id".into(),
            "--cap-drop".into(),
            "ALL".into(),
            "--security-opt".into(),
            "no-new-privileges".into(),
            "--pids-limit".into(),
            self.profile.pids_limit.to_string().into(),
            "--memory".into(),
            self.profile.memory_limit.clone().into(),
            "--cpus".into(),
            self.profile.cpu_limit.clone().into(),
            "--workdir".into(),
            WORKSPACE.into(),
            "--tmpfs".into(),
            "/tmp:rw,noexec,nosuid,nodev,size=64m".into(),
            "--tmpfs".into(),
            "/home/runner:rw,noexec,nosuid,nodev,size=64m".into(),
            "--env".into(),
            "HOME=/home/runner".into(),
            "--env".into(),
            "TMPDIR=/tmp".into(),
            "--volume".into(),
            mount(&attempt.workdir, WORKSPACE, false)?,
            "--volume".into(),
            mount(&attempt.prompt_file, PROMPT_PATH, true)?,
        ];
        // Image ENV is untrusted configuration for this adapter.  Make every
        // conventional proxy variable deterministic so a mechanical command
        // cannot inherit an image-defined path around network=none.
        for key in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
            "NO_PROXY",
            "no_proxy",
        ] {
            args.extend(["--env".into(), format!("{key}=").into()]);
        }
        for resource in &attempt.resource_mounts {
            ensure!(
                matches!(attempt.egress, EgressMode::Disabled),
                "vendor cannot mount shared databases"
            );
            ensure!(
                resource.container_path.starts_with("/praxis/test-db")
                    && !resource
                        .container_path
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir)),
                "invalid shared DB mount target"
            );
            ensure!(
                resource.host_path.is_dir(),
                "shared DB must mount its disposable directory"
            );
            args.extend([
                "--volume".into(),
                mount(
                    &resource.host_path,
                    resource
                        .container_path
                        .to_str()
                        .context("invalid shared DB target")?,
                    resource.read_only,
                )?,
            ]);
        }
        let (executable, argv, env) = match &attempt.egress {
            EgressMode::Disabled => (&command.executable, &command.argv, &command.env),
            EgressMode::Vendor {
                socket_path,
                credential_file,
            } => {
                let vendor = self
                    .profile
                    .vendor
                    .as_ref()
                    .context("vendor egress requested without configured vendor")?;
                args.extend([
                    "--volume".into(),
                    mount(socket_path, EGRESS_SOCKET_PATH, false)?,
                ]);
                match (&vendor.credential_file_reference, credential_file) {
                    (Some(_), Some(path)) => {
                        args.extend(["--volume".into(), mount(path, CREDENTIAL_PATH, true)?])
                    }
                    (Some(_), None) => bail!("vendor requires a temporary credential file"),
                    (None, None) => {}
                    (None, Some(_)) => bail!("vendor has no credential file contract"),
                }
                args.extend([
                    "--env".into(),
                    "HTTPS_PROXY=http://127.0.0.1:18080".into(),
                    "--env".into(),
                    "HTTP_PROXY=http://127.0.0.1:18080".into(),
                    "--env".into(),
                    "https_proxy=http://127.0.0.1:18080".into(),
                    "--env".into(),
                    "http_proxy=http://127.0.0.1:18080".into(),
                ]);
                // The forwarder receives its fixed contract and then launches
                // the image-registered vendor executable; no worker text joins
                // this argv and no shell is involved.
                for (key, value) in &vendor.env {
                    args.extend(["--env".into(), format!("{key}={value}").into()]);
                }
                if vendor.credential_file_reference.is_some() {
                    args.extend([
                        "--env".into(),
                        format!("PRAXIS_WORKFLOW_CREDENTIAL_FILE={CREDENTIAL_PATH}").into(),
                    ]);
                }
                args.extend([
                    "--env".into(),
                    format!("PRAXIS_WORKFLOW_PROMPT_FILE={PROMPT_PATH}").into(),
                    "--entrypoint".into(),
                    vendor.forwarder_path.as_os_str().to_os_string(),
                ]);
                args.push(self.profile.image.clone().into());
                args.extend([
                    "--contract".into(),
                    vendor.forwarder_contract.clone().into(),
                    "--socket".into(),
                    EGRESS_SOCKET_PATH.into(),
                    "--listen".into(),
                    "127.0.0.1:18080".into(),
                    "--prompt".into(),
                    PROMPT_PATH.into(),
                    "--".into(),
                    vendor.executable.as_os_str().to_os_string(),
                ]);
                args.extend(vendor.argv.iter().cloned().map(OsString::from));
                return Ok(args);
            }
        };
        for (key, value) in env {
            args.extend(["--env".into(), format!("{key}={value}").into()]);
        }
        args.extend(["--entrypoint".into(), executable.as_os_str().to_os_string()]);
        args.push(self.profile.image.clone().into());
        args.extend(argv.iter().cloned().map(OsString::from));
        Ok(args)
    }

    async fn assert_identity(&self, handle: &AttemptHandle) -> Result<()> {
        self.inspect(handle).await.map(|_| ())
    }
    async fn call(&self, args: Vec<OsString>, timeout_ms: u64) -> Result<CommandResult> {
        self.call_with_limit(args, timeout_ms, OUTPUT_LIMIT).await
    }
    async fn call_with_limit(
        &self,
        args: Vec<OsString>,
        timeout_ms: u64,
        output_limit: usize,
    ) -> Result<CommandResult> {
        self.runner
            .run(
                &self.profile.podman_executable,
                &args,
                Duration::from_millis(timeout_ms),
                output_limit,
            )
            .await
    }
    fn quarantine(
        &self,
        attempt_id: &str,
        container_id: Option<&str>,
        operation: &str,
        detail: &str,
    ) -> anyhow::Error {
        self.quarantined
            .lock()
            .expect("quarantine mutex poisoned")
            .push(QuarantinedOperation {
                attempt_id: attempt_id.to_owned(),
                container_id: container_id.map(str::to_owned),
                operation: operation.to_owned(),
            });
        anyhow!("workflow container quarantined after {operation}: {detail}")
    }
}

fn validate_attempt(
    attempt: &AttemptSpec,
    command: &RegisteredCommandProfile,
    profile: &RuntimeProfile,
) -> Result<()> {
    validate_identity_part("run", &attempt.run_id)?;
    validate_identity_part("attempt", &attempt.attempt_id)?;
    validate_identity_part("step", &attempt.step_id)?;
    ensure!(
        path_is_safe_mount_source(&attempt.workdir)
            && path_is_safe_mount_source(&attempt.prompt_file),
        "attempt mount source is unsafe"
    );
    match (&attempt.egress, &command.vendor_id) {
        (EgressMode::Disabled, None) => Ok(()),
        (
            EgressMode::Vendor {
                socket_path,
                credential_file,
            },
            Some(vendor_id),
        ) => {
            ensure!(
                profile
                    .vendor
                    .as_ref()
                    .is_some_and(|vendor| vendor.id == *vendor_id),
                "attempt egress vendor does not match command profile"
            );
            ensure!(
                path_is_safe_mount_source(socket_path),
                "egress socket path is unsafe"
            );
            if let Some(path) = credential_file {
                ensure!(path_is_safe_mount_source(path), "credential path is unsafe");
            }
            Ok(())
        }
        _ => bail!("attempt egress mode does not match its registered command profile"),
    }
}

fn mount(host: &Path, container: &str, readonly: bool) -> Result<OsString> {
    ensure!(path_is_safe_mount_source(host), "unsafe mount source");
    let rendered = host.as_os_str().to_string_lossy();
    ensure!(
        !rendered.contains(':') && !rendered.contains(','),
        "mount source contains Podman volume syntax characters"
    );
    Ok(format!(
        "{}:{container}:{}{},rbind",
        rendered,
        if readonly { "ro" } else { "rw" },
        if readonly { ",nosuid,nodev,noexec" } else { "" }
    )
    .into())
}

fn validate_identity_part(kind: &str, value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|value| value.is_ascii_alphanumeric() || matches!(value, b'-' | b'_')),
        "workflow {kind} ID is invalid"
    );
    Ok(())
}

fn decode_inspection(
    stdout: &[u8],
    attempt_id: &str,
    nonce: &str,
) -> Result<(String, ContainerInspection)> {
    let raw: serde_json::Value =
        serde_json::from_slice(stdout).context("podman inspect returned malformed JSON")?;
    let item = raw
        .as_array()
        .and_then(|items| items.first())
        .unwrap_or(&raw);
    let id = item
        .get("Id")
        .or_else(|| item.get("id"))
        .and_then(serde_json::Value::as_str)
        .context("inspect response lacks container ID")?
        .to_owned();
    let labels = item
        .pointer("/Config/Labels")
        .or_else(|| item.pointer("/config/labels"))
        .and_then(serde_json::Value::as_object)
        .context("inspect response lacks labels")?;
    ensure!(
        labels
            .get("praxis.workflow.attempt")
            .and_then(serde_json::Value::as_str)
            == Some(attempt_id),
        "container attempt ownership label does not match"
    );
    ensure!(
        labels
            .get("praxis.workflow.nonce")
            .and_then(serde_json::Value::as_str)
            == Some(nonce),
        "container ownership nonce does not match"
    );
    let state = item.get("State").or_else(|| item.get("state"));
    Ok((
        id.clone(),
        ContainerInspection {
            id,
            running: state
                .and_then(|value| value.get("Running").or_else(|| value.get("running")))
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false),
            exit_code: state
                .and_then(|value| value.get("ExitCode").or_else(|| value.get("exitCode")))
                .and_then(serde_json::Value::as_i64)
                .and_then(|code| i32::try_from(code).ok()),
        },
    ))
}

fn random_nonce() -> Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| anyhow!("obtain container ownership nonce: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn is_container_id(value: &str) -> bool {
    (12..=128).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
fn safe_stderr(result: &CommandResult) -> String {
    String::from_utf8_lossy(&result.stderr)
        .chars()
        .take(512)
        .collect()
}
fn command_detail(result: &CommandResult) -> String {
    if result.timed_out {
        "command timed out".into()
    } else if result.output_limited {
        "command exceeded output limit".into()
    } else {
        format!(
            "command exited with {}",
            result
                .exit_code
                .map_or("unknown".into(), |code| code.to_string())
        )
    }
}
fn check(id: &str, passed: bool, detail: impl Into<String>) -> CapabilityCheck {
    CapabilityCheck {
        id: id.into(),
        passed,
        detail: detail.into(),
    }
}
fn remaining_evidence() -> Vec<String> {
    vec![
        "vendor_auth_and_attempt_proxy".into(),
        "network_egress_enforcement".into(),
        "sandbox_mount_isolation".into(),
        "resource_limit_enforcement".into(),
        "container_exit_and_child_cleanup".into(),
        "host_database_access_denial".into(),
    ]
}
fn unavailable(profile: &RuntimeProfile, checks: Vec<CapabilityCheck>) -> CapabilityReceipt {
    CapabilityReceipt {
        schema_version: 1,
        status: CapabilityStatus::CapabilityUnavailable,
        image_digest: profile.image_digest().to_owned(),
        checks,
        runtime_verified: false,
        remaining_evidence: remaining_evidence(),
    }
}
fn prerequisite_ready(profile: &RuntimeProfile, checks: Vec<CapabilityCheck>) -> CapabilityReceipt {
    CapabilityReceipt {
        schema_version: 1,
        status: CapabilityStatus::PrerequisiteReady,
        image_digest: profile.image_digest().to_owned(),
        checks,
        runtime_verified: false,
        remaining_evidence: remaining_evidence(),
    }
}

#[cfg(test)]
mod tests {
    use super::super::profile::{RuntimeTimeouts, VendorProfile, FORWARDER_CONTRACT_V1};
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FakeRunner {
        calls: Mutex<Vec<Vec<String>>>,
    }
    #[async_trait]
    impl CommandRunner for FakeRunner {
        async fn run(
            &self,
            _: &Path,
            args: &[OsString],
            _: Duration,
            _: usize,
        ) -> Result<CommandResult> {
            self.calls.lock().unwrap().push(
                args.iter()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect(),
            );
            Ok(CommandResult {
                exit_code: Some(0),
                stdout: b"abcdef123456".to_vec(),
                stderr: vec![],
                timed_out: false,
                output_limited: false,
            })
        }
    }
    fn profile() -> RuntimeProfile {
        RuntimeProfile {
            schema_version: 1,
            id: "linux".into(),
            image: format!("registry.example/worker@sha256:{}", "a".repeat(64)),
            podman_executable: "/usr/bin/podman".into(),
            cpu_limit: "1".into(),
            memory_limit: "512m".into(),
            pids_limit: 64,
            timeouts: RuntimeTimeouts {
                probe_ms: 100,
                create_ms: 100,
                start_ms: 100,
                inspect_ms: 100,
                stop_ms: 100,
                remove_ms: 100,
                logs_ms: 100,
            },
            vendor: Some(VendorProfile {
                id: "vendor".into(),
                executable: "/usr/bin/vendor".into(),
                argv: vec!["--json".into()],
                env: BTreeMap::new(),
                tls_domains: vec!["api.vendor.example".into()],
                credential_file_reference: Some("vendor-token".into()),
                forwarder_path: "/usr/bin/praxis-forwarder".into(),
                forwarder_contract: FORWARDER_CONTRACT_V1.into(),
            }),
        }
    }
    #[tokio::test]
    async fn create_uses_literal_hardened_argv_and_no_host_home() {
        let runner = Arc::new(FakeRunner::default());
        let driver = PodmanDriver::with_runner(profile(), runner.clone()).unwrap();
        let command = RegisteredCommandProfile {
            id: "agent".into(),
            runtime_profile_id: "linux".into(),
            vendor_id: Some("vendor".into()),
            executable: "/ignored".into(),
            argv: vec![],
            env: BTreeMap::new(),
        };
        let attempt = AttemptSpec {
            run_id: "run1".into(),
            attempt_id: "a1".into(),
            step_id: "step1".into(),
            workdir: "/var/tmp/attempt-work".into(),
            prompt_file: "/var/tmp/attempt-prompt".into(),
            egress: EgressMode::Vendor {
                socket_path: "/var/tmp/attempt-egress".into(),
                credential_file: Some("/var/tmp/attempt-credential".into()),
            },
            resource_mounts: vec![],
        };
        driver.create(&attempt, &command).await.unwrap();
        let call = runner.calls.lock().unwrap()[0].join(" ");
        for required in [
            "--network none",
            "--read-only",
            "--userns keep-id",
            "--cap-drop ALL",
            "no-new-privileges",
            "--workdir /workspace",
            "/praxis/prompt.txt:ro",
            "/praxis/egress.sock:rw",
        ] {
            assert!(call.contains(required), "{required}: {call}");
        }
        assert!(!call.contains("/Users/") && !call.contains("/root"));
    }

    #[derive(Default)]
    struct LogRunner {
        limits: Mutex<Vec<usize>>,
    }

    #[async_trait]
    impl CommandRunner for LogRunner {
        async fn run(
            &self,
            _: &Path,
            args: &[OsString],
            _: Duration,
            output_limit: usize,
        ) -> Result<CommandResult> {
            self.limits.lock().unwrap().push(output_limit);
            let operation = args
                .first()
                .and_then(|value| value.to_str())
                .unwrap_or_default();
            let stdout = if operation == "inspect" {
                br#"[{"Id":"abcdef123456","Config":{"Labels":{"praxis.workflow.attempt":"a1","praxis.workflow.nonce":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}},"State":{"Running":false,"ExitCode":0}}]"#.to_vec()
            } else {
                vec![b'x'; 2 * 1024 * 1024]
            };
            Ok(CommandResult {
                exit_code: Some(0),
                stdout,
                stderr: if operation == "logs" {
                    b"Podman warning\n".to_vec()
                } else {
                    vec![]
                },
                timed_out: false,
                output_limited: false,
            })
        }
    }

    #[tokio::test]
    async fn logs_accept_evidence_above_control_cap_but_below_ten_mib_cap() {
        let runner = Arc::new(LogRunner::default());
        let driver = PodmanDriver::with_runner(profile(), runner.clone()).unwrap();
        let handle = AttemptHandle {
            container_id: "abcdef123456".into(),
            name: "workflow-run-a1-step".into(),
            attempt_id: "a1".into(),
            nonce: "a".repeat(32),
        };
        let (stdout, stderr) = driver.log_streams(&handle).await.unwrap();
        assert_eq!(stdout.len(), 2 * 1024 * 1024);
        assert_eq!(stderr, "Podman warning\n");
        assert_eq!(
            *runner.limits.lock().unwrap(),
            vec![OUTPUT_LIMIT, LOG_OUTPUT_LIMIT]
        );
    }
}
