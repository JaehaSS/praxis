//! In-process exclusion for Runner worktree mutation and finalization.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

#[derive(Clone, Default)]
pub struct WorktreeLocks {
    entries: Arc<Mutex<HashMap<PathBuf, Arc<AsyncMutex<()>>>>>,
}

impl WorktreeLocks {
    pub async fn acquire(&self, path: &Path) -> OwnedMutexGuard<()> {
        let key = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        let lock = self
            .entries
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .entry(key)
            .or_insert_with(|| Arc::new(AsyncMutex::new(())))
            .clone();
        lock.lock_owned().await
    }
}
