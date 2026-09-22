use std::ffi::OsString;
use std::io::Write;
use std::process::Stdio;
use std::time::Duration;

use crate::managed_process::{ProcessLease, SharedProcessRegistrar, SpawnedProcess};

pub(super) fn run(
    bin: &str,
    args: &[&str],
    prompt: &str,
    timeout_secs: u64,
    registrar: Option<&SharedProcessRegistrar>,
) -> Result<String, String> {
    let tmp = temporary_directory()?;
    let result = run_in_directory(bin, args, prompt, timeout_secs, registrar, &tmp);
    let _ = std::fs::remove_dir_all(&tmp);
    result
}

pub(super) fn run_in_directory(
    bin: &str, args: &[&str], prompt: &str, timeout_secs: u64,
    registrar: Option<&SharedProcessRegistrar>, cwd: &std::path::Path,
) -> Result<String, String> {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    let spawned = crate::managed_process::spawn_registered(bin, &args, registrar, |command| {
        command
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    })
    .map_err(|error| format!("{bin} 실행 실패: {error} (설치/PATH 확인)"));
    spawned.and_then(|mut spawned| {
        write_prompt(&mut spawned, prompt)?;
        await_output(spawned, timeout_secs)
    })
}

fn temporary_directory() -> Result<std::path::PathBuf, String> {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!("praxis-reviewer-{}-{nanos}", std::process::id()));
    std::fs::create_dir(&tmp).map_err(|error| format!("temp 생성 실패: {error}"))?;
    Ok(tmp)
}

fn write_prompt(spawned: &mut SpawnedProcess, prompt: &str) -> Result<(), String> {
    let Some(mut stdin) = spawned.child.stdin.take() else {
        return abort_spawned(spawned, "리뷰어 stdin 핸들 없음".into());
    };
    if let Err(error) = stdin
        .write_all(prompt.as_bytes())
        .and_then(|_| stdin.flush())
    {
        return abort_spawned(spawned, format!("리뷰어 stdin 쓰기 실패: {error}"));
    }
    Ok(())
}

fn abort_spawned(spawned: &mut SpawnedProcess, detail: String) -> Result<(), String> {
    if let Some(lease) = spawned.lease.take() {
        let _ = lease.quarantine(&detail);
    }
    crate::verify::kill_group(spawned.pid);
    let _ = spawned.child.kill();
    let _ = spawned.child.wait();
    Err(detail)
}

fn await_output(mut spawned: SpawnedProcess, timeout_secs: u64) -> Result<String, String> {
    let lease = spawned.lease.take();
    let pid = spawned.pid;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(spawned.child.wait_with_output());
    });
    match receiver.recv_timeout(Duration::from_secs(timeout_secs.max(1))) {
        Ok(Ok(output)) => finish_output(output, lease),
        Ok(Err(error)) => {
            quarantine(lease, &format!("reviewer wait failed: {error}"));
            Err(format!("리뷰어 오류: {error}"))
        }
        Err(_) => timeout(receiver, lease, pid, timeout_secs),
    }
}

fn finish_output(
    output: std::process::Output,
    lease: Option<Box<dyn ProcessLease>>,
) -> Result<String, String> {
    complete(lease)?;
    if !output.status.success() {
        return Err(format!(
            "리뷰어 비정상 종료(code {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn timeout(
    receiver: std::sync::mpsc::Receiver<std::io::Result<std::process::Output>>,
    lease: Option<Box<dyn ProcessLease>>,
    pid: u32,
    timeout_secs: u64,
) -> Result<String, String> {
    crate::verify::kill_group(pid);
    match receiver.recv_timeout(Duration::from_secs(2)) {
        Ok(_) => {
            let _ = complete(lease);
        }
        Err(_) => quarantine(lease, "reviewer waiter did not reap after timeout"),
    }
    Err(format!("리뷰어 타임아웃 ({timeout_secs}s)"))
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
