//! Live capability checks use the same owned sandbox and cleanup path as real attempts.
//! A prerequisite receipt or a supplied JSON flag can never activate execution.
use super::{
    config::hash,
    driver::{
        AttemptSpec, CapabilityCheck, CapabilityReceipt, CapabilityStatus, EgressMode,
        PlannedIdentity, PodmanDriver,
    },
    egress::{EgressPolicy, EgressProxy},
    profile::{RegisteredCommandProfile, RuntimeProfile},
    supervisor::{cleanup_owned, copy_secret},
    WorkflowService,
};
use crate::workflow::{lifecycle::CleanupProof, TaskKind, WorkflowSpec};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::Duration;

const ISOLATION_CHECK: &str = r#"
import json,os,socket,sys
expect=json.loads(sys.argv[1])
status=dict(line.split(':',1) for line in open('/proc/self/status') if ':' in line)
assert int(status['NoNewPrivs'].strip())==1
assert int(status['CapEff'].strip(),16)==0
assert set(os.listdir('/sys/class/net')) <= {'lo'}
root=[line.split(' - ')[0].split() for line in open('/proc/self/mountinfo') if line.split()[4]=='/'][0]
assert 'ro' in root[5].split(',')
assert int(open('/sys/fs/cgroup/pids.max').read()) <= expect['pids']
assert int(open('/sys/fs/cgroup/memory.max').read()) <= expect['memory']
quota,period=open('/sys/fs/cgroup/cpu.max').read().split()
assert quota!='max' and int(quota)/int(period) <= expect['cpu']+0.001
for path in expect['protected']: assert not os.path.exists(path), 'host path exposed'
assert not os.path.exists('/praxis/credential')
assert not os.path.exists('/praxis/egress.sock')
with open('/workspace/write-probe','w') as f: f.write('isolated')
try:
    with open('/praxis/prompt.txt','w') as f: f.write('forbidden')
    raise AssertionError('control input writable')
except OSError: pass
print('PRAXIS_ISOLATION_OK')
"#;
#[derive(Clone)]
struct SmokeContext {
    config: super::config::WorkflowConfig,
    protected_paths: Vec<std::path::PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct Intent {
    profile: RuntimeProfile,
    identity: PlannedIdentity,
}

impl WorkflowService {
    pub async fn require_capability(&self, spec: &WorkflowSpec) -> Result<()> {
        self.check_profile(
            &spec.execution_profile_id,
            spec.tasks.iter().any(|task| task.kind == TaskKind::Agent),
        )
        .await
    }
    pub async fn check_configured_profiles(&self) -> Result<Vec<CapabilityReceipt>> {
        for (id, profile) in &self.config.profiles {
            self.check_profile(id, profile.vendor.is_some()).await?;
        }
        Ok(self.capabilities.lock().await.values().cloned().collect())
    }
    async fn check_profile(&self, profile_id: &str, needs_vendor: bool) -> Result<()> {
        let key = format!("{profile_id}:{needs_vendor}");
        let mut cache = self.capabilities.lock().await;
        let profile = self
            .config
            .profiles
            .get(profile_id)
            .context("runtime profile not registered")?
            .clone();
        let driver = self
            .drivers
            .get(&profile.id)
            .context("runtime driver unavailable")?
            .clone();
        // Image existence/rootless capability are rechecked even for cached live smoke evidence.
        let prerequisite = driver.probe().await;
        if prerequisite.status != CapabilityStatus::PrerequisiteReady {
            let failed = prerequisite
                .checks
                .iter()
                .filter(|c| !c.passed)
                .map(|c| c.id.clone())
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!("capability_unavailable: {failed}");
        }
        if cache.get(&key).is_some_and(|r| r.runtime_verified) {
            return Ok(());
        }
        ensure!(
            !self.stopping.load(std::sync::atomic::Ordering::SeqCst),
            "workflow Runner is stopping"
        );
        let permit = self
            .scheduler
            .capacity()
            .try_acquire()
            .map_err(|_| anyhow::anyhow!("capability_unavailable: Runner capacity is busy"))?;
        let context = SmokeContext {
            config: self.config.clone(),
            protected_paths: self.protected_paths.clone(),
        };
        let (send, receive) = tokio::sync::oneshot::channel();
        // Disconnecting the HTTP client cannot drop a live container's cleanup future.
        // The Runner owns and joins this bounded job even when the response is abandoned.
        let mut jobs = self.capability_jobs.lock().await;
        while jobs.try_join_next().is_some() {}
        jobs.spawn(async move {
            let result = verify(&context, &driver, needs_vendor).await;
            let unresolved = context.config.workspace_root.join("capability-attempts");
            let unresolved =
                std::fs::read_dir(unresolved).is_ok_and(|mut entries| entries.next().is_some());
            if unresolved {
                permit.forget();
            } else {
                drop(permit);
            }
            let _ = send.send(result);
        });
        drop(jobs);
        let receipt = receive.await.context("capability worker failed")??;
        ensure!(
            receipt.runtime_verified,
            "capability_unavailable: supervised Linux smoke failed"
        );
        let evidence = self
            .config
            .workspace_root
            .join(format!("capability-{}.json", hash(key.as_bytes())));
        durable_write(&evidence, &serde_json::to_vec_pretty(&receipt)?)?;
        cache.insert(key, receipt);
        Ok(())
    }
}

async fn verify(
    service: &SmokeContext,
    driver: &PodmanDriver,
    vendor: bool,
) -> Result<CapabilityReceipt> {
    let profile = driver.profile();
    let memory = memory_bytes(&profile.memory_limit)?;
    let cpu: f64 = profile.cpu_limit.parse().context("invalid CPU limit")?;
    let protected = &service.protected_paths;
    let command = RegisteredCommandProfile {
        id: "capability-isolation".into(),
        runtime_profile_id: profile.id.clone(),
        vendor_id: None,
        executable: service.config.verifier_executable.clone(),
        argv: vec![
            "-c".into(),
            ISOLATION_CHECK.into(),
            json!({"memory":memory,"cpu":cpu,"pids":profile.pids_limit,"protected":protected})
                .to_string(),
        ],
        env: Default::default(),
    };
    let mechanical = smoke_attempt(service, driver, &command, false).await?;
    ensure!(
        mechanical.0.contains("PRAXIS_ISOLATION_OK"),
        "capability_unavailable: isolation smoke receipt missing"
    );
    let mut checks=vec![CapabilityCheck{id:"container_isolation_limits_cleanup".into(),passed:true,detail:"live cgroup CPU/memory/pids, network-none, read-only root/control, no host protected paths, owned container removal".into()}];
    if vendor {
        let profile_vendor = profile
            .vendor
            .as_ref()
            .context("capability_unavailable: vendor profile absent")?;
        let command = RegisteredCommandProfile {
            id: "capability-vendor".into(),
            runtime_profile_id: profile.id.clone(),
            vendor_id: Some(profile_vendor.id.clone()),
            executable: profile_vendor.executable.clone(),
            argv: profile_vendor.argv.clone(),
            env: profile_vendor.env.clone(),
        };
        let result = smoke_attempt(service, driver, &command, true).await?;
        ensure!(
            result.1 > 0 && result.0.contains("PRAXIS_WORKFLOW_CAPABILITY_OK"),
            "capability_unavailable: vendor did not confirm prompt via attempt proxy"
        );
        checks.push(CapabilityCheck{id:"vendor_auth_attempt_proxy".into(),passed:true,detail:"registered vendor exited successfully, used a validated TLS CONNECT tunnel and returned the smoke marker; temporary credentials and proxy removed".into()});
    }
    Ok(CapabilityReceipt {
        schema_version: 1,
        status: CapabilityStatus::Verified,
        image_digest: profile.image_digest().into(),
        checks,
        runtime_verified: true,
        remaining_evidence: vec![],
    })
}

async fn smoke_attempt(
    service: &SmokeContext,
    driver: &PodmanDriver,
    command: &RegisteredCommandProfile,
    vendor: bool,
) -> Result<(String, usize)> {
    let mut random = [0u8; 16];
    getrandom::getrandom(&mut random).map_err(|_| anyhow::anyhow!("random source unavailable"))?;
    let id = hash(&random)[..24].to_string();
    let identity = driver.prepare_identity("capability", &id, "1")?;
    let root = service
        .config
        .workspace_root
        .join("capability-attempts")
        .join(&id);
    std::fs::create_dir_all(root.join("work"))?;
    let intent_path = root.join("intent.json");
    durable_write(
        &intent_path,
        &serde_json::to_vec(&Intent {
            profile: driver.profile().clone(),
            identity: identity.clone(),
        })?,
    )?;
    let prompt = root.join("prompt.txt");
    std::fs::write(
        &prompt,
        "Respond with exactly PRAXIS_WORKFLOW_CAPABILITY_OK. Do not modify files or execute tools.",
    )?;
    let mut proxy = None;
    let result = async {
        let egress = if vendor {
            let vendor = driver.profile().vendor.as_ref().context("vendor absent")?;
            let socket = root.join("egress.sock");
            proxy = Some(
                EgressProxy::start(
                    socket.clone(),
                    EgressPolicy::new(vendor.tls_domains.clone(), 4)?,
                )
                .await?,
            );
            let credential = if let Some(reference) = &vendor.credential_file_reference {
                let source = service
                    .config
                    .credentials
                    .get(reference)
                    .context("vendor credential reference absent")?;
                let destination = root.join("credential");
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
        let attempt = AttemptSpec {
            run_id: "capability".into(),
            attempt_id: id.clone(),
            step_id: "1".into(),
            workdir: root.join("work"),
            prompt_file: prompt,
            egress,
            resource_mounts: vec![],
        };
        let handle = driver
            .create_with_identity(&attempt, command, &identity)
            .await?;
        driver.register(&handle).await?;
        driver.start(&handle).await?;
        let end = tokio::time::Instant::now()
            + Duration::from_secs(service.config.task_timeout_secs.min(120) as u64);
        loop {
            let inspection = driver.inspect(&handle).await?;
            if !inspection.running {
                ensure!(
                    inspection.exit_code == Some(0),
                    "capability_unavailable: sandbox smoke command failed"
                );
                let logs = driver.logs(&handle).await?;
                let connections = proxy.as_ref().map_or(0, |p| p.successful_connects());
                return Ok::<_, anyhow::Error>((logs, connections as usize));
            }
            ensure!(
                tokio::time::Instant::now() < end,
                "capability_unavailable: sandbox smoke timed out"
            );
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    .await;
    let cleanup = cleanup_owned(driver, &identity).await;
    if let Some(proxy) = proxy {
        proxy.shutdown().await?;
    }
    super::supervisor::remove_secret(&root.join("credential"))?;
    let proof = cleanup?;
    ensure!(
        !matches!(proof, CleanupProof::Unknown { .. }),
        "capability_unavailable: smoke cleanup unknown"
    );
    std::fs::remove_dir_all(&root)?;
    result
}

pub async fn recover(service: &WorkflowService) -> Result<()> {
    let directory = service.config.workspace_root.join("capability-attempts");
    if !directory.exists() {
        return Ok(());
    }
    for entry in std::fs::read_dir(directory)? {
        let root = entry?.path();
        let intent_path = root.join("intent.json");
        if !intent_path.exists() {
            continue;
        }
        let bytes = std::fs::read(&intent_path)?;
        ensure!(bytes.len() < 128 * 1024, "invalid smoke recovery intent");
        let intent: Intent = serde_json::from_slice(&bytes)?;
        let driver = PodmanDriver::new(intent.profile)?;
        cleanup_owned(&driver, &intent.identity).await?;
        std::fs::remove_dir_all(root)?;
    }
    Ok(())
}
fn durable_write(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::File::open(path.parent().context("missing receipt directory")?)?.sync_all()?;
    Ok(())
}
fn memory_bytes(value: &str) -> Result<u64> {
    let lower = value.to_ascii_lowercase();
    let split = lower
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(lower.len());
    let number = &lower[..split];
    let factor = match &lower[split..] {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1024,
        "m" | "mb" | "mib" => 1024 * 1024,
        "g" | "gb" | "gib" => 1024 * 1024 * 1024,
        "t" | "tb" | "tib" => 1024u64.pow(4),
        _ => anyhow::bail!("invalid memory limit"),
    };
    number
        .parse::<u64>()
        .ok()
        .and_then(|n| n.checked_mul(factor))
        .filter(|n| *n > 0)
        .context("invalid memory limit")
}
