use std::io::Read;
use std::process::{Child, ChildStderr, ChildStdout, ExitStatus};
use std::time::{Duration, Instant};

const POLL_INTERVAL: Duration = Duration::from_millis(20);
const STDERR_TAIL_LINES: usize = 12;
const STDERR_LINE_CHARS: usize = 400;

pub struct AgyProcessOutput {
    pub stdout: String,
    pub timed_out: bool,
    pub exit_desc: String,
    pub stderr_tail: String,
}

#[cfg(unix)]
pub fn capture(
    child: &mut Child,
    mut stdout: ChildStdout,
    mut stderr: Option<ChildStderr>,
    pid: u32,
    idle_timeout_secs: u64,
) -> Result<AgyProcessOutput, String> {
    set_nonblocking(&stdout)?;
    if let Some(pipe) = stderr.as_ref() {
        set_nonblocking(pipe)?;
    }
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    let mut last_output = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if read_available(&mut stdout, &mut stdout_bytes)? {
            last_output = Instant::now();
        }
        if let Some(pipe) = stderr.as_mut() {
            read_available(pipe, &mut stderr_bytes)?;
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            read_available(&mut stdout, &mut stdout_bytes)?;
            if let Some(pipe) = stderr.as_mut() {
                read_available(pipe, &mut stderr_bytes)?;
            }
            break status;
        }
        if !timed_out && last_output.elapsed() >= Duration::from_secs(idle_timeout_secs.max(1)) {
            timed_out = true;
            crate::verify::kill_group(pid);
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    Ok(output(stdout_bytes, stderr_bytes, timed_out, status))
}

#[cfg(unix)]
fn set_nonblocking(pipe: &impl std::os::fd::AsRawFd) -> Result<(), String> {
    let fd = pipe.as_raw_fd();
    let flags = unsafe { nix::libc::fcntl(fd, nix::libc::F_GETFL) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    let result = unsafe { nix::libc::fcntl(fd, nix::libc::F_SETFL, flags | nix::libc::O_NONBLOCK) };
    if result < 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(())
}

#[cfg(unix)]
fn read_available(reader: &mut impl Read, bytes: &mut Vec<u8>) -> Result<bool, String> {
    let mut read_any = false;
    let mut chunk = [0_u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => return Ok(read_any),
            Ok(count) => {
                bytes.extend_from_slice(&chunk[..count]);
                read_any = true;
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return Ok(read_any),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error.to_string()),
        }
    }
}

#[cfg(not(unix))]
pub fn capture(
    child: &mut Child,
    stdout: ChildStdout,
    stderr: Option<ChildStderr>,
    pid: u32,
    idle_timeout_secs: u64,
) -> Result<AgyProcessOutput, String> {
    let stdout_rx = read_in_background(stdout);
    let stderr_rx = stderr.map(read_in_background);
    let mut stdout_bytes = Vec::new();
    let mut stderr_bytes = Vec::new();
    let mut last_output = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if drain_channel(&stdout_rx, &mut stdout_bytes) {
            last_output = Instant::now();
        }
        if let Some(receiver) = stderr_rx.as_ref() {
            drain_channel(receiver, &mut stderr_bytes);
        }
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            std::thread::sleep(POLL_INTERVAL);
            drain_channel(&stdout_rx, &mut stdout_bytes);
            if let Some(receiver) = stderr_rx.as_ref() {
                drain_channel(receiver, &mut stderr_bytes);
            }
            break status;
        }
        if !timed_out && last_output.elapsed() >= Duration::from_secs(idle_timeout_secs.max(1)) {
            timed_out = true;
            crate::verify::kill_group(pid);
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    Ok(output(stdout_bytes, stderr_bytes, timed_out, status))
}

#[cfg(not(unix))]
fn read_in_background(
    mut reader: impl Read + Send + 'static,
) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut chunk = [0_u8; 8192];
        while let Ok(count) = reader.read(&mut chunk) {
            if count == 0 || sender.send(chunk[..count].to_vec()).is_err() {
                break;
            }
        }
    });
    receiver
}

#[cfg(not(unix))]
fn drain_channel(receiver: &std::sync::mpsc::Receiver<Vec<u8>>, bytes: &mut Vec<u8>) -> bool {
    let mut read_any = false;
    while let Ok(chunk) = receiver.try_recv() {
        bytes.extend_from_slice(&chunk);
        read_any = true;
    }
    read_any
}

fn output(
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
    status: ExitStatus,
) -> AgyProcessOutput {
    let stderr = String::from_utf8_lossy(&stderr);
    let stderr_tail = stderr
        .lines()
        .rev()
        .take(STDERR_TAIL_LINES)
        .map(|line| line.chars().take(STDERR_LINE_CHARS).collect::<String>())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n");
    AgyProcessOutput {
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        timed_out,
        exit_desc: exit_description(status),
        stderr_tail,
    }
}

fn exit_description(status: ExitStatus) -> String {
    if let Some(code) = status.code() {
        return format!("exit {code}");
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return format!("signal {signal}");
        }
    }
    "unknown".to_string()
}
