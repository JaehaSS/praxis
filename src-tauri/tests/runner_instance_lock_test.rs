#![cfg(unix)]

#[path = "support/temp_root.rs"]
mod temp_root;

use std::process::Command;

use praxis_lib::runner::instance_lock::RunnerInstanceLock;
use praxis_lib::runner::{self, config::RunnerConfig};

const PROBE_DB_ENV: &str = "PRAXIS_RUNNER_LOCK_PROBE_DB";

#[test]
fn canonical_db_lock_rejects_a_second_process_using_a_symlink_alias() {
    let root = temporary_dir("owner");
    let alias = root.with_extension("alias");
    std::os::unix::fs::symlink(&root, &alias).unwrap();
    let db_path = root.join("runner.sqlite");
    let _owner = RunnerInstanceLock::acquire(&db_path).unwrap();

    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "runner_instance_lock_probe", "--nocapture"])
        .env(PROBE_DB_ENV, alias.join("runner.sqlite"))
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_file(alias);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn canonical_db_lock_rejects_a_symlinked_database_file() {
    let root = temporary_dir("file-alias");
    let db_path = root.join("runner.sqlite");
    let alias = root.join("runner-alias.sqlite");
    std::fs::write(&db_path, []).unwrap();
    std::os::unix::fs::symlink(&db_path, &alias).unwrap();
    let _owner = RunnerInstanceLock::acquire(&db_path).unwrap();

    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "runner_instance_lock_probe", "--nocapture"])
        .env(PROBE_DB_ENV, alias)
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "probe failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn runner_instance_lock_probe() {
    let Some(db_path) = std::env::var_os(PROBE_DB_ENV) else {
        return;
    };
    let error = RunnerInstanceLock::acquire(std::path::Path::new(&db_path)).unwrap_err();
    assert!(error.to_string().contains("already owns"));
}

#[tokio::test]
async fn runner_runtime_holds_the_database_lock_until_drop() {
    let root = temporary_dir("runtime");
    let db_path = root.join("runner.sqlite");
    let first = runner::initialize(config_for(&root), db_path.to_str().unwrap(), 1)
        .await
        .unwrap();

    let error = match runner::initialize(config_for(&root), db_path.to_str().unwrap(), 2).await {
        Ok(_) => panic!("second Runner unexpectedly acquired the same database"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("already owns"));

    drop(first);
    runner::initialize(config_for(&root), db_path.to_str().unwrap(), 3)
        .await
        .unwrap();
    let _ = std::fs::remove_dir_all(root);
}

fn temporary_dir(label: &str) -> std::path::PathBuf {
    let root =
        temp_root::dir().join(format!("praxis-runner-lock-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

fn config_for(root: &std::path::Path) -> RunnerConfig {
    RunnerConfig {
        bind: "127.0.0.1:47831".parse().unwrap(),
        repository_roots: vec![root.to_path_buf()],
        max_concurrent_tasks: 1,
        execution_policy: praxis_lib::runner::config::ExecutionPolicy::AlwaysApprove,
        pairing_token_file: root.join("token"),
    }
}
