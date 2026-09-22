#![cfg(unix)]

#[path = "support/temp_root.rs"]
mod temp_root;

use std::ffi::OsString;
use std::sync::{Arc, Mutex};

use praxis_lib::managed_process::{self, ProcessLease, ProcessRegistrar, SharedProcessRegistrar};

#[test]
fn target_executes_only_after_registration_succeeds() {
    let marker = marker_path("success");
    let observed = Arc::new(Mutex::new(None));
    let registrar: SharedProcessRegistrar = Arc::new(FakeRegistrar {
        observed: observed.clone(),
        reject: false,
    });
    let args = vec![
        OsString::from("-c"),
        OsString::from(format!("printf ready > {}", marker.display())),
    ];

    let mut spawned =
        managed_process::spawn_registered("/bin/sh", &args, Some(&registrar), |_| {}).unwrap();
    let output = spawned.child.wait_with_output().unwrap();
    assert!(output.status.success());
    spawned.lease.take().unwrap().complete().unwrap();
    assert_eq!(
        observed.lock().unwrap().as_ref().copied(),
        Some(spawned.pid)
    );
    assert_eq!(std::fs::read_to_string(&marker).unwrap(), "ready");
    let _ = std::fs::remove_file(marker);
}

#[test]
fn registration_failure_never_executes_target_and_reaps_group() {
    let marker = marker_path("rejected");
    let observed = Arc::new(Mutex::new(None));
    let registrar: SharedProcessRegistrar = Arc::new(FakeRegistrar {
        observed: observed.clone(),
        reject: true,
    });
    let args = vec![
        OsString::from("-c"),
        OsString::from(format!("printf unsafe > {}", marker.display())),
    ];

    let error = match managed_process::spawn_registered("/bin/sh", &args, Some(&registrar), |_| {})
    {
        Ok(_) => panic!("registration unexpectedly succeeded"),
        Err(error) => error,
    };

    assert!(error.contains("registration rejected"));
    let pid = observed.lock().unwrap().unwrap();
    assert!(!marker.exists());
    assert!(!group_alive(pid));
}

#[test]
fn parent_crash_before_registration_commit_never_executes_target() {
    let marker = marker_path("crash-target");
    let ready = marker_path("crash-ready");
    let mut parent = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "managed_gate_crash_probe", "--nocapture"])
        .env("PRAXIS_GATE_TARGET", &marker)
        .env("PRAXIS_GATE_READY", &ready)
        .spawn()
        .unwrap();
    wait_for_path(&ready);
    let gate_pid: u32 = std::fs::read_to_string(&ready)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    parent.kill().unwrap();
    parent.wait().unwrap();
    for _ in 0..100 {
        if !group_alive(gate_pid) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!marker.exists());
    assert!(!group_alive(gate_pid));
    let _ = std::fs::remove_file(ready);
}

#[test]
fn managed_gate_crash_probe() {
    let Ok(marker) = std::env::var("PRAXIS_GATE_TARGET") else {
        return;
    };
    let ready = std::env::var("PRAXIS_GATE_READY").unwrap();
    let registrar: SharedProcessRegistrar = Arc::new(BlockingRegistrar {
        ready: ready.into(),
    });
    let args = vec![
        OsString::from("-c"),
        OsString::from(format!("printf unsafe > {marker}")),
    ];
    let result = managed_process::spawn_registered("/bin/sh", &args, Some(&registrar), |_| {});
    panic!(
        "parent crash probe unexpectedly returned: {}",
        result.is_ok()
    );
}

struct FakeRegistrar {
    observed: Arc<Mutex<Option<u32>>>,
    reject: bool,
}

struct BlockingRegistrar {
    ready: std::path::PathBuf,
}

impl ProcessRegistrar for BlockingRegistrar {
    fn register(&self, pid: u32) -> Result<Box<dyn ProcessLease>, String> {
        std::fs::write(&self.ready, pid.to_string()).unwrap();
        loop {
            std::thread::park_timeout(std::time::Duration::from_secs(60));
        }
    }
}

impl ProcessRegistrar for FakeRegistrar {
    fn register(&self, pid: u32) -> Result<Box<dyn ProcessLease>, String> {
        *self.observed.lock().unwrap() = Some(pid);
        if self.reject {
            return Err("registration rejected".into());
        }
        Ok(Box::new(FakeLease { pid }))
    }
}

struct FakeLease {
    pid: u32,
}

impl ProcessLease for FakeLease {
    fn complete(self: Box<Self>) -> Result<(), String> {
        if group_alive(self.pid) {
            return Err("process group still alive".into());
        }
        Ok(())
    }

    fn quarantine(self: Box<Self>, _detail: &str) -> Result<(), String> {
        Ok(())
    }
}

fn marker_path(label: &str) -> std::path::PathBuf {
    let path = temp_root::dir().join(format!(
        "praxis-managed-gate-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    path
}

fn wait_for_path(path: &std::path::Path) {
    for _ in 0..200 {
        if path.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("gate probe did not become ready");
}

fn group_alive(pid: u32) -> bool {
    nix::sys::signal::killpg(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
}
