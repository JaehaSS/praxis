use std::ffi::OsString;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use crate::managed_process::{ProcessLease, SharedProcessRegistrar};

use super::CheckResult;

pub fn run_check(cwd: &Path, command: &str, timeout_secs: u64) -> CheckResult {
    run_check_registered(cwd, command, timeout_secs, None)
}

pub fn run_check_registered(
    cwd: &Path,
    command: &str,
    timeout_secs: u64,
    registrar: Option<&SharedProcessRegistrar>,
) -> CheckResult {
    let (program, args) = invocation(command);
    let spawned = crate::managed_process::spawn_registered(program, &args, registrar, |process| {
        process
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    });
    let mut spawned = match spawned {
        Ok(spawned) => spawned,
        Err(error) => return failure(command, format!("spawn 실패: {error}")),
    };
    let lease = spawned.lease.take();
    let pid = spawned.pid;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(spawned.child.wait_with_output());
    });
    await_result(receiver, lease, pid, command, timeout_secs)
}

fn await_result(
    receiver: std::sync::mpsc::Receiver<std::io::Result<std::process::Output>>,
    lease: Option<Box<dyn ProcessLease>>,
    pid: u32,
    command: &str,
    timeout_secs: u64,
) -> CheckResult {
    match receiver.recv_timeout(Duration::from_secs(timeout_secs.max(1))) {
        Ok(Ok(output)) => output_result(command, output, lease),
        Ok(Err(error)) => {
            quarantine(lease, &format!("process wait failed: {error}"));
            failure(command, format!("실행 오류: {error}"))
        }
        Err(_) => timeout_result(receiver, lease, pid, command, timeout_secs),
    }
}

fn output_result(
    command: &str,
    output: std::process::Output,
    lease: Option<Box<dyn ProcessLease>>,
) -> CheckResult {
    if let Err(error) = complete(lease) {
        return failure(command, format!("process lease 완료 실패: {error}"));
    }
    let mut text = String::new();
    text.push_str(&String::from_utf8_lossy(&output.stdout));
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    CheckResult {
        command: command.into(),
        exit_code: output.status.code().unwrap_or(-1),
        tail: super::tail_str(&text, 16_384),
    }
}

fn timeout_result(
    receiver: std::sync::mpsc::Receiver<std::io::Result<std::process::Output>>,
    lease: Option<Box<dyn ProcessLease>>,
    pid: u32,
    command: &str,
    timeout_secs: u64,
) -> CheckResult {
    super::kill_group(pid);
    match receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(_) => {
            let _ = complete(lease);
        }
        Err(_) => quarantine(lease, "process waiter did not reap after timeout"),
    }
    failure(
        command,
        format!("타임아웃 ({timeout_secs}s) — 프로세스 종료됨"),
    )
}

fn invocation(command: &str) -> (&'static str, Vec<OsString>) {
    #[cfg(unix)]
    return ("sh", vec![OsString::from("-c"), OsString::from(command)]);
    #[cfg(windows)]
    return ("cmd", vec![OsString::from("/C"), OsString::from(command)]);
}

fn complete(lease: Option<Box<dyn ProcessLease>>) -> Result<(), String> {
    match lease {
        Some(lease) => lease.complete(),
        None => Ok(()),
    }
}

fn quarantine(lease: Option<Box<dyn ProcessLease>>, detail: &str) {
    if let Some(lease) = lease {
        let _ = lease.quarantine(detail);
    }
}

fn failure(command: &str, tail: String) -> CheckResult {
    CheckResult {
        command: command.into(),
        exit_code: -1,
        tail,
    }
}
