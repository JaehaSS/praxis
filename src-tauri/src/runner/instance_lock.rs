//! Cross-process ownership lock for one Runner database.

use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct RunnerInstanceLock {
    #[cfg(unix)]
    file: std::fs::File,
    path: PathBuf,
}

impl RunnerInstanceLock {
    pub fn acquire(db_path: &Path) -> anyhow::Result<Self> {
        let path = canonical_lock_path(db_path)?;
        #[cfg(unix)]
        {
            use std::os::fd::AsRawFd;
            use std::os::unix::fs::OpenOptionsExt;

            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(&path)?;
            set_close_on_exec(file.as_raw_fd())?;
            let result = unsafe {
                nix::libc::flock(file.as_raw_fd(), nix::libc::LOCK_EX | nix::libc::LOCK_NB)
            };
            if result != 0 {
                let error = std::io::Error::last_os_error();
                anyhow::bail!(
                    "another Runner already owns database {}: {error}",
                    db_path.display()
                );
            }
            Ok(Self { file, path })
        }
        #[cfg(not(unix))]
        anyhow::bail!("durable Runner ownership is unsupported on this platform")
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(unix)]
impl Drop for RunnerInstanceLock {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe {
            nix::libc::flock(self.file.as_raw_fd(), nix::libc::LOCK_UN);
        }
    }
}

#[cfg(unix)]
fn set_close_on_exec(fd: std::os::fd::RawFd) -> std::io::Result<()> {
    let flags = unsafe { nix::libc::fcntl(fd, nix::libc::F_GETFD) };
    if flags < 0 {
        return Err(std::io::Error::last_os_error());
    }
    let result = unsafe { nix::libc::fcntl(fd, nix::libc::F_SETFD, flags | nix::libc::FD_CLOEXEC) };
    if result < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn canonical_lock_path(db_path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = if db_path.is_absolute() {
        db_path.to_path_buf()
    } else {
        std::env::current_dir()?.join(db_path)
    };
    let resolved = if std::fs::symlink_metadata(&absolute).is_ok() {
        absolute.canonicalize()?
    } else {
        let parent = absolute
            .parent()
            .ok_or_else(|| anyhow::anyhow!("Runner database path has no parent"))?
            .canonicalize()?;
        let file_name = absolute
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("Runner database path has no file name"))?;
        parent.join(file_name)
    };
    let parent = resolved
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Runner database path has no parent"))?;
    let file_name = resolved
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("Runner database path has no file name"))?
        .to_string_lossy();
    Ok(parent.join(format!(".{file_name}.runner.lock")))
}
