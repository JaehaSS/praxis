use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use sqlx::{Row, SqlitePool};

pub struct AdmissionGuard {
    #[cfg(unix)]
    file: File,
}

pub async fn shared(pool: &SqlitePool) -> anyhow::Result<AdmissionGuard> {
    #[cfg(unix)]
    {
        acquire(pool, nix::libc::LOCK_SH).await
    }
    #[cfg(not(unix))]
    {
        let _ = pool;
        Ok(AdmissionGuard {})
    }
}

pub async fn exclusive(pool: &SqlitePool) -> anyhow::Result<AdmissionGuard> {
    #[cfg(unix)]
    {
        acquire(pool, nix::libc::LOCK_EX).await
    }
    #[cfg(not(unix))]
    {
        let _ = pool;
        anyhow::bail!("unsupported_platform")
    }
}

pub async fn try_exclusive(pool: &SqlitePool) -> anyhow::Result<AdmissionGuard> {
    #[cfg(unix)]
    {
        acquire(pool, nix::libc::LOCK_EX | nix::libc::LOCK_NB).await
    }
    #[cfg(not(unix))]
    {
        let _ = pool;
        anyhow::bail!("unsupported_platform")
    }
}

async fn acquire(pool: &SqlitePool, mode: i32) -> anyhow::Result<AdmissionGuard> {
    let path = lock_path(pool).await?;
    tokio::task::spawn_blocking(move || acquire_file(&path, mode)).await?
}

async fn lock_path(pool: &SqlitePool) -> anyhow::Result<PathBuf> {
    let rows = sqlx::query("PRAGMA database_list").fetch_all(pool).await?;
    let file: String = rows
        .into_iter()
        .find(|row| row.try_get::<String, _>("name").ok().as_deref() == Some("main"))
        .ok_or_else(|| anyhow::anyhow!("SQLite main database is missing"))?
        .try_get("file")?;
    if file.is_empty() {
        return memory_guard();
    }
    let database = Path::new(&file).canonicalize()?;
    let parent = database
        .parent()
        .ok_or_else(|| anyhow::anyhow!("SQLite database has no parent"))?;
    let name = database
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("SQLite database has no name"))?
        .to_string_lossy();
    Ok(parent.join(format!(".{name}.vault-admission.lock")))
}

#[cfg(unix)]
fn acquire_file(path: &Path, mode: i32) -> anyhow::Result<AdmissionGuard> {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    if !file.metadata()?.is_file() {
        anyhow::bail!("vault admission lock is not a regular file")
    }
    if unsafe { nix::libc::flock(file.as_raw_fd(), mode) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(AdmissionGuard { file })
}

#[cfg(not(unix))]
fn acquire_file(_: &Path, _: i32) -> anyhow::Result<AdmissionGuard> {
    anyhow::bail!("vault admission lock is unsupported on this platform")
}

#[cfg(test)]
fn memory_guard() -> anyhow::Result<PathBuf> {
    Err(anyhow::anyhow!(
        "vault admission lock requires a file-backed SQLite database"
    ))
}

#[cfg(not(test))]
fn memory_guard() -> anyhow::Result<PathBuf> {
    Err(anyhow::anyhow!(
        "vault admission lock requires a file-backed SQLite database"
    ))
}

#[cfg(unix)]
impl Drop for AdmissionGuard {
    fn drop(&mut self) {
        use std::os::fd::AsRawFd;
        unsafe {
            nix::libc::flock(self.file.as_raw_fd(), nix::libc::LOCK_UN);
        }
    }
}
