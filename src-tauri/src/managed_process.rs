//! Process-group spawn with an exec'd registration gate for durable ownership.

use std::ffi::{OsStr, OsString};
use std::process::{Child, Command};
use std::sync::Arc;

pub trait ProcessRegistrar: Send + Sync {
    fn register(&self, pid: u32) -> Result<Box<dyn ProcessLease>, String>;
}

pub trait ProcessLease: Send {
    fn complete(self: Box<Self>) -> Result<(), String>;
    fn quarantine(self: Box<Self>, detail: &str) -> Result<(), String>;
}

pub type SharedProcessRegistrar = Arc<dyn ProcessRegistrar>;

pub struct SpawnedProcess {
    pub child: Child,
    pub lease: Option<Box<dyn ProcessLease>>,
    pub pid: u32,
}

pub fn spawn_registered<F>(
    program: impl AsRef<OsStr>,
    args: &[OsString],
    registrar: Option<&SharedProcessRegistrar>,
    configure: F,
) -> Result<SpawnedProcess, String>
where
    F: FnOnce(&mut Command),
{
    let program = program.as_ref();
    match registrar {
        Some(registrar) => spawn_gated(program, args, registrar, configure),
        None => spawn_direct(program, args, configure),
    }
}

fn spawn_direct<F>(
    program: &OsStr,
    args: &[OsString],
    configure: F,
) -> Result<SpawnedProcess, String>
where
    F: FnOnce(&mut Command),
{
    let mut command = Command::new(program);
    command.args(args);
    configure(&mut command);
    set_process_group(&mut command);
    let child = command.spawn().map_err(|error| error.to_string())?;
    let pid = child.id();
    Ok(SpawnedProcess {
        child,
        lease: None,
        pid,
    })
}

#[cfg(unix)]
fn spawn_gated<F>(
    program: &OsStr,
    args: &[OsString],
    registrar: &SharedProcessRegistrar,
    configure: F,
) -> Result<SpawnedProcess, String>
where
    F: FnOnce(&mut Command),
{
    use std::io::Write;
    use std::os::fd::AsRawFd;
    use std::os::unix::net::UnixStream;
    use std::os::unix::process::CommandExt;

    let (mut parent_gate, child_gate) = UnixStream::pair().map_err(|error| error.to_string())?;
    let gate_fd = child_gate.as_raw_fd();
    let mut command = gate_command(program, args);
    configure(&mut command);
    set_process_group(&mut command);
    unsafe {
        command.pre_exec(move || install_gate_fd(gate_fd));
    }
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    drop(child_gate);
    let pid = child.id();
    let lease = match registrar.register(pid) {
        Ok(lease) => lease,
        Err(error) => {
            drop(parent_gate);
            terminate_and_reap(&mut child);
            return Err(error);
        }
    };
    if let Err(error) = parent_gate.write_all(b"go\n") {
        let detail = format!("process gate release failed: {error}");
        let _ = lease.quarantine(&detail);
        terminate_and_reap(&mut child);
        return Err(detail);
    }
    drop(parent_gate);
    Ok(SpawnedProcess {
        child,
        lease: Some(lease),
        pid,
    })
}

#[cfg(not(unix))]
fn spawn_gated<F>(
    _program: &OsStr,
    _args: &[OsString],
    _registrar: &SharedProcessRegistrar,
    _configure: F,
) -> Result<SpawnedProcess, String>
where
    F: FnOnce(&mut Command),
{
    Err("durable managed process spawning is unsupported on this platform".into())
}

#[cfg(unix)]
fn gate_command(program: &OsStr, args: &[OsString]) -> Command {
    const SCRIPT: &str = "IFS= read -r gate <&3 || exit 125; \
        [ \"$gate\" = go ] || exit 125; exec 3<&-; exec \"$@\"";
    let mut command = Command::new("/bin/sh");
    command
        .arg("-c")
        .arg(SCRIPT)
        .arg("praxis-review-gate")
        .arg(program)
        .args(args);
    command
}

#[cfg(unix)]
fn install_gate_fd(source: std::os::fd::RawFd) -> std::io::Result<()> {
    const GATE_FD: std::os::fd::RawFd = 3;
    if source == GATE_FD {
        let result = unsafe { nix::libc::fcntl(GATE_FD, nix::libc::F_SETFD, 0) };
        return os_result(result);
    }
    let result = unsafe { nix::libc::dup2(source, GATE_FD) };
    if result < 0 {
        return Err(std::io::Error::last_os_error());
    }
    unsafe {
        nix::libc::close(source);
    }
    Ok(())
}

#[cfg(unix)]
fn os_result(result: i32) -> std::io::Result<()> {
    if result < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn set_process_group(command: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
}

fn terminate_and_reap(child: &mut Child) {
    crate::verify::kill_group(child.id());
    let _ = child.kill();
    let _ = child.wait();
}
