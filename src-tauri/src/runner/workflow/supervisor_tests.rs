//! Supervisor integration tests run the real store, scheduler and artifact
//! composition with a process-free Podman command runner.

use super::*;
use crate::runner::{
    capacity::RunnerCapacity,
    config::{ExecutionPolicy, RunnerConfig},
};
use crate::workflow::{query::RuntimeAdmission, WorkflowSpec};
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsString,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Default)]
struct FakePodman {
    state: Mutex<FakeState>,
    block_start: AtomicBool,
    reject_start: AtomicBool,
    unknown_inspect: AtomicBool,
    start_entered: tokio::sync::Notify,
    release_start: tokio::sync::Notify,
}

#[derive(Default)]
struct FakeState {
    next: usize,
    containers: HashMap<String, FakeContainer>,
    names: HashMap<String, String>,
    starts: usize,
    maximum_live: usize,
}

struct FakeContainer {
    name: String,
    labels: BTreeMap<String, String>,
    running: bool,
    log: String,
    workdir: PathBuf,
    check_logs: Vec<Vec<u8>>,
}

#[async_trait]
impl driver::CommandRunner for FakePodman {
    async fn run(
        &self,
        _: &Path,
        argv: &[OsString],
        _: std::time::Duration,
        _: usize,
    ) -> Result<driver::CommandResult> {
        let args: Vec<String> = argv
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        if args.first().map(String::as_str) == Some("start") {
            if self.reject_start.load(Ordering::SeqCst) {
                return Ok(exit(125));
            }
            let result = {
                let mut state = self.state.lock().unwrap();
                let id = args.last().unwrap();
                let container = state.containers.get_mut(id).unwrap();
                container.running = false; // successful command exits before next supervisor tick
                if !container.check_logs.is_empty() {
                    let directory = container.workdir.join(".praxis-check-logs");
                    std::fs::create_dir(&directory).unwrap();
                    for (index, bytes) in container.check_logs.iter().enumerate() {
                        std::fs::write(directory.join(format!("{index}.log")), bytes).unwrap();
                    }
                }
                state.starts += 1;
                state.maximum_live = state.maximum_live.max(state.containers.len());
                success(String::new())
            };
            if self.block_start.load(Ordering::SeqCst) {
                self.start_entered.notify_waiters();
                self.release_start.notified().await;
            }
            return Ok(result);
        }
        if self.unknown_inspect.load(Ordering::SeqCst)
            && matches!(
                args.first().map(String::as_str),
                Some("inspect") | Some("container")
            )
        {
            return Ok(driver::CommandResult {
                exit_code: None,
                stdout: vec![],
                stderr: vec![],
                timed_out: true,
                output_limited: false,
            });
        }
        let mut state = self.state.lock().unwrap();
        let result = match args.first().map(String::as_str) {
            Some("create") => {
                state.next += 1;
                let id = format!("{:064x}", state.next);
                let name = arg_after(&args, "--name").unwrap();
                let labels = labels(&args);
                let (log, check_logs) = verifier_log(&args);
                let workdir = workspace_mount(&args);
                state.names.insert(name.clone(), id.clone());
                state.containers.insert(
                    id.clone(),
                    FakeContainer {
                        name,
                        labels,
                        running: false,
                        log,
                        workdir,
                        check_logs,
                    },
                );
                success(id)
            }
            Some("container") if args.get(1).map(String::as_str) == Some("exists") => {
                if state.names.contains_key(args.last().unwrap()) {
                    success(String::new())
                } else {
                    exit(1)
                }
            }
            Some("inspect") => {
                let target = args.last().unwrap();
                let id = state
                    .names
                    .get(target)
                    .cloned()
                    .unwrap_or_else(|| target.clone());
                match state.containers.get(&id) {
                    Some(container) => success(json!([{"Id": id, "Config":{"Labels":container.labels}, "State":{"Running":container.running,"ExitCode":0}}]).to_string()),
                    None => exit(125),
                }
            }
            Some("stop") => success(String::new()),
            Some("logs") => {
                let text = state
                    .containers
                    .get(args.last().unwrap())
                    .unwrap()
                    .log
                    .clone();
                let mut result = success(text.clone());
                if text.starts_with('{') {
                    result.stderr = b"Podman diagnostic warning\n".to_vec();
                }
                result
            }
            Some("rm") => {
                let id = args.last().unwrap().clone();
                let container = state.containers.remove(&id).unwrap();
                state.names.remove(&container.name);
                success(String::new())
            }
            other => panic!("unexpected fake Podman argv: {other:?} {args:?}"),
        };
        Ok(result)
    }
}

fn success(stdout: String) -> driver::CommandResult {
    driver::CommandResult {
        exit_code: Some(0),
        stdout: stdout.into_bytes(),
        stderr: vec![],
        timed_out: false,
        output_limited: false,
    }
}
fn exit(code: i32) -> driver::CommandResult {
    driver::CommandResult {
        exit_code: Some(code),
        stdout: vec![],
        stderr: vec![],
        timed_out: false,
        output_limited: false,
    }
}
fn arg_after(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|arg| arg == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}
fn labels(args: &[String]) -> BTreeMap<String, String> {
    args.windows(2)
        .filter(|pair| pair[0] == "--label")
        .filter_map(|pair| {
            pair[1]
                .split_once('=')
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
        })
        .collect()
}
fn workspace_mount(args: &[String]) -> PathBuf {
    args.windows(2)
        .find(|pair| pair[0] == "--volume" && pair[1].contains(":/workspace:rw"))
        .and_then(|pair| pair[1].split_once(":/workspace:"))
        .map(|(source, _)| PathBuf::from(source))
        .expect("fake Podman create must mount workspace")
}
fn verifier_log(args: &[String]) -> (String, Vec<Vec<u8>>) {
    let checks = args
        .iter()
        .filter_map(|argument| serde_json::from_str::<Value>(argument).ok())
        .find_map(|value| value.get("checks").cloned());
    match checks {
        Some(Value::Array(checks)) => {
            let logs: Vec<Vec<u8>> = checks
                .iter()
                .map(|_| b"fake verifier output\n".to_vec())
                .collect();
            let receipt = checks
                .into_iter()
                .zip(&logs)
                .filter_map(|(check, bytes)| {
                    check.get("id").and_then(Value::as_str).map(
                        |id| json!({"id":id,"exit_code":0,"log_hash":super::config::hash(bytes)}),
                    )
                })
                .collect::<Vec<_>>();
            (
                json!({"schema_version":1,"checks":receipt}).to_string(),
                logs,
            )
        }
        _ => (String::new(), Vec::new()),
    }
}

fn root(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "praxis-supervisor-{name}-{}-{nonce}",
        std::process::id()
    ))
}

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn spec(commit: &str) -> WorkflowSpec {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/workflow-spec-v1.json"
    ))
    .unwrap();
    value["project_ref"] = json!("github.com/example/supervisor-test");
    value["base_commit"] = json!(commit);
    value["execution_profile_id"] = json!("test");
    for task in value["tasks"].as_array_mut().unwrap() {
        if task["kind"] == "agent" {
            task["kind"] = json!("command");
            task["command_profile_id"] = json!("mechanical");
        }
        if task["kind"] == "command" {
            task["command_profile_id"] = json!("mechanical");
        }
        task["resource_requests_by_step"] = json!({});
        task["manual_acceptance"] = json!([]);
        if task["command_profile_id"].is_null() {
            task["command_profile_id"] = json!("mechanical");
        }
    }
    WorkflowSpec::parse_json(&value.to_string()).unwrap()
}

fn one_step_spec(commit: &str) -> WorkflowSpec {
    let mut spec = spec(commit);
    spec.tasks
        .retain(|task| task.id == "api" || task.id == "final-integration");
    let final_task = spec
        .tasks
        .iter_mut()
        .find(|task| task.id == "final-integration")
        .unwrap();
    final_task
        .input_artifacts
        .retain(|input| input.task_id == "api");
    spec.edges
        .retain(|edge| edge.from == "api" && edge.to == "final-integration");
    spec.limits.max_tasks = 2;
    spec.limits.max_edges = 1;
    spec
}

async fn service(name: &str) -> (Arc<WorkflowService>, Arc<FakePodman>, PathBuf, String) {
    let root = root(name);
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "test@example.invalid"]);
    git(&repo, &["config", "user.name", "Test"]);
    std::fs::write(repo.join("README.md"), "base\n").unwrap();
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-qm", "base"]);
    let commit = git(&repo, &["rev-parse", "HEAD"]);
    let token = root.join("token");
    std::fs::write(&token, "token").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let runner = RunnerConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        repository_roots: vec![repo.canonicalize().unwrap()],
        max_concurrent_tasks: 2,
        execution_policy: ExecutionPolicy::AlwaysApprove,
        pairing_token_file: token,
    };
    let profile = json!({"schema_version":1,"id":"test","image":format!("example.invalid/worker@sha256:{}", "a".repeat(64)),"podman_executable":"/usr/bin/podman","cpu_limit":"1","memory_limit":"512m","pids_limit":32,"timeouts":{"probe_ms":100,"create_ms":100,"start_ms":100,"inspect_ms":100,"stop_ms":100,"remove_ms":100,"logs_ms":100}});
    let command = |id: &str| json!({"id":id,"runtime_profile_id":"test","vendor_id":null,"executable":"/bin/true","argv":[],"env":{}});
    let cfg = json!({"schema_version":1,"workspace_root":root.join("workspace"),"repositories":{"github.com/example/supervisor-test":repo},"profiles":{"test":profile},"commands":{"mechanical":command("mechanical"),"cargo-test-v1":command("cargo-test-v1"),"vitest-v1":command("vitest-v1")},"task_timeout_secs":30,"verifier_executable":"/usr/bin/python3"});
    let config_path = root.join("workflow.json");
    std::fs::write(&config_path, cfg.to_string()).unwrap();
    let database = root.join("workflow.sqlite");
    // WorkflowConfig canonicalizes its protected Runner DB before the store
    // opens it, matching production where Runner already owns this file.
    std::fs::File::create(&database).unwrap();
    let mut service =
        WorkflowService::open(&config_path, &runner, &database, RunnerCapacity::new(2))
            .await
            .unwrap();
    let fake = Arc::new(FakePodman::default());
    let profile = service.config.profiles["test"].clone();
    Arc::get_mut(&mut service).unwrap().drivers.insert(
        "test".into(),
        Arc::new(driver::PodmanDriver::with_runner(profile, fake.clone()).unwrap()),
    );
    (service, fake, root, commit)
}

async fn admit_and_start(service: &WorkflowService, id: &str, spec: &WorkflowSpec, commit: &str) {
    service
        .store
        .create_run(id, "create", spec, 1)
        .await
        .unwrap();
    let exported = service
        .artifacts
        .export_git_input(
            service.config.repositories.get(&spec.project_ref).unwrap(),
            commit,
            &service.config.input_policy,
        )
        .unwrap();
    let admission = RuntimeAdmission {
        config_hash: service.config_hash.clone(),
        base_input_hash: exported.input_tree_hash,
        scope_hash: "scope".into(),
    };
    service
        .store
        .record_admission(id, 1, "admit", &admission, 2)
        .await
        .unwrap();
    service
        .store
        .start_admitted(id, 1, "start", "scope", &service.config_hash, false, 3)
        .await
        .unwrap();
}

#[tokio::test]
async fn supervisor_runs_parallel_mechanical_steps_then_composes_and_accepts_final_artifact() {
    let (service, fake, root, commit) = service("complete").await;
    let spec = spec(&commit);
    admit_and_start(&service, "run", &spec, &commit).await;
    for _ in 0..20 {
        service.tick().await.unwrap();
        if service.store.run("run").await.unwrap().state != "running" {
            break;
        }
    }
    let snapshot = service.store.snapshot("run").await.unwrap();
    assert_eq!(snapshot["state"], "completed", "{snapshot}");
    assert_eq!(fake.state.lock().unwrap().maximum_live, 2);
    assert!(snapshot["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .all(|node| node["state"] == "verified"));
    let empty_log = super::config::hash(b"");
    assert!(service.store.owns_log("run", &empty_log).await.unwrap());
    assert_eq!(
        super::logs::read(&service.config.workspace_root, &empty_log).unwrap(),
        b""
    );
    service
        .store
        .create_run("other", "other-create", &spec, 10)
        .await
        .unwrap();
    assert!(!service.store.owns_log("other", &empty_log).await.unwrap());
    service.store.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn cancel_waits_for_start_gate_then_removes_the_owned_container_without_later_start() {
    let (service, fake, root, commit) = service("cancel-gate").await;
    let spec = one_step_spec(&commit);
    admit_and_start(&service, "run", &spec, &commit).await;
    fake.block_start.store(true, Ordering::SeqCst);
    let entered = fake.start_entered.notified();
    let tick = tokio::spawn({
        let service = service.clone();
        async move { service.tick().await }
    });
    entered.await;
    let cancel = tokio::spawn({
        let service = service.clone();
        async move { service.cancel("run", 1, "cancel").await }
    });
    tokio::task::yield_now().await;
    assert_eq!(service.store.run("run").await.unwrap().state, "running");
    assert!(
        !cancel.is_finished(),
        "cancel intent crossed the start/stop gate"
    );
    fake.release_start.notify_waiters();
    tick.await.unwrap().unwrap();
    cancel.await.unwrap().unwrap();
    assert_eq!(service.store.run("run").await.unwrap().state, "cancelled");
    {
        let state = fake.state.lock().unwrap();
        assert_eq!(state.starts, 1);
        assert!(state.containers.is_empty());
    }
    service.store.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn unknown_cleanup_quarantines_and_retains_the_scheduled_capacity_permit() {
    let (service, fake, root, commit) = service("unknown-cleanup").await;
    let spec = one_step_spec(&commit);
    admit_and_start(&service, "run", &spec, &commit).await;
    service.tick().await.unwrap();
    assert_eq!(service.scheduler.capacity().available(), 1);
    fake.unknown_inspect.store(true, Ordering::SeqCst);
    assert!(service.cancel("run", 1, "cancel").await.is_err());
    assert_eq!(service.store.run("run").await.unwrap().state, "quarantined");
    assert_eq!(service.scheduler.capacity().available(), 1);
    assert_eq!(fake.state.lock().unwrap().starts, 1);
    service.store.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn rejected_start_retains_a_readable_failure_log_and_releases_capacity() {
    let (service, fake, root, commit) = service("start-failure-log").await;
    let spec = one_step_spec(&commit);
    admit_and_start(&service, "run", &spec, &commit).await;
    fake.reject_start.store(true, Ordering::SeqCst);
    service.tick().await.unwrap();
    assert_eq!(
        service
            .store
            .nodes("run")
            .await
            .unwrap()
            .into_iter()
            .find(|node| node.node_id == "api")
            .unwrap()
            .state,
        "failed"
    );
    let steps = service.store.steps("run").await.unwrap();
    let digest = steps[0].log_hash.as_ref().unwrap();
    assert!(service.store.owns_log("run", digest).await.unwrap());
    assert_eq!(
        super::logs::read(&service.config.workspace_root, digest).unwrap(),
        b"workflow supervision failed"
    );
    assert_eq!(service.scheduler.capacity().available(), 2);
    assert!(fake.state.lock().unwrap().containers.is_empty());
    service.store.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
