//! Direct-mode checkout ownership across async requests and Praxis processes.

use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fs2::FileExt;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

#[derive(Clone, Default)]
pub struct DirectCheckoutLocks {
    entries: Arc<Mutex<HashMap<PathBuf, Arc<AsyncMutex<()>>>>>,
}

pub struct DirectCheckoutGuard {
    _process: OwnedMutexGuard<()>,
    _file: Option<DirectCheckoutFileLock>,
}

impl DirectCheckoutLocks {
    pub async fn acquire(&self, repo: &Path) -> anyhow::Result<DirectCheckoutGuard> {
        let key = repo.canonicalize().unwrap_or_else(|_| repo.to_path_buf());
        let lock = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(key.clone())
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone();
        let process = lock.lock_owned().await;
        let file = tokio::task::spawn_blocking(move || DirectCheckoutFileLock::acquire(&key))
            .await
            .map_err(|error| anyhow::anyhow!(error.to_string()))??;
        Ok(DirectCheckoutGuard {
            _process: process,
            _file: file,
        })
    }
}

struct DirectCheckoutFileLock {
    _file: File,
}

impl DirectCheckoutFileLock {
    fn acquire(repo: &Path) -> anyhow::Result<Option<Self>> {
        let Some(path) = direct_checkout_lock_path(repo)? else {
            return Ok(None);
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;

            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(path)?;
            file.lock_exclusive()?;
            Ok(Some(Self { _file: file }))
        }
        #[cfg(not(unix))]
        {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(path)?;
            file.lock_exclusive()?;
            Ok(Some(Self { _file: file }))
        }
    }
}

fn direct_checkout_lock_path(repo: &Path) -> anyhow::Result<Option<PathBuf>> {
    if !super::is_git_repository(repo) {
        return Ok(None);
    }
    let common_dir = super::run_git(repo, &["rev-parse", "--git-common-dir"])?;
    let common_dir = Path::new(common_dir.trim());
    let common_dir = if common_dir.is_absolute() {
        common_dir.to_path_buf()
    } else {
        repo.join(common_dir)
    };
    Ok(Some(common_dir.join("praxis-direct-branch.lock")))
}
