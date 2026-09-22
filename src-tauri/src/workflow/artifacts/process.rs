//! Bounded, shell-free local Git plumbing. No network or checkout commands.
use std::{
    fs::{self, File},
    path::Path,
    process::{Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};

pub(super) fn git(cwd: &Path, args: &[&str], output: &Path, max_bytes: u64) -> Result<ExitStatus> {
    let file = super::create_file(output)?;
    let mut command = Command::new("git");
    // Repository settings cannot change the selected object or introduce a
    // credential helper, replacement object, pager, or lazy network fetch.
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command
        .current_dir(cwd)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(["--no-pager", "-c", "core.hooksPath=/dev/null"])
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::from(file))
        .stderr(Stdio::null());
    let mut child = command.spawn().context("start local Git plumbing")?;
    let started = Instant::now();
    let result = (|| loop {
        if fs::metadata(output)?.len() > max_bytes {
            bail!("Git output exceeds workflow input limit");
        }
        if let Some(status) = child.try_wait()? {
            if fs::metadata(output)?.len() > max_bytes {
                bail!("Git output exceeds workflow input limit");
            }
            return Ok(status);
        }
        if started.elapsed() > Duration::from_secs(30) {
            bail!("local Git plumbing timed out");
        }
        std::thread::sleep(Duration::from_millis(10));
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

/// Private staging directories are never worker writable. Always clean them on
/// error, including a missing Git executable or a conflicting merge.
pub(super) struct Stage(pub(super) std::path::PathBuf);

impl Stage {
    pub(super) fn new(root: &Path) -> Result<Self> {
        let path = root.join(format!(
            ".input-stage-{}-{}",
            std::process::id(),
            super::STAGE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        }
        Ok(Self(path))
    }
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn read_bounded(path: &Path, maximum: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut file = File::open(path)?.take(maximum.saturating_add(1));
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    if bytes.len() as u64 > maximum {
        bail!("workflow input file exceeds maximum size");
    }
    Ok(bytes)
}
