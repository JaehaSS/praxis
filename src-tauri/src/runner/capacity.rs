//! One admission budget shared by normal tasks, follow-up turns and workflows.
//!
//! A caller must retain the permit until its process tree is confirmed stopped.
//! A workflow quarantine must retain its permit in the supervisor, not drop it
//! merely because an RPC or heartbeat timed out.

use std::sync::Arc;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, TryAcquireError};

#[derive(Clone)]
pub struct RunnerCapacity(Arc<Semaphore>);

impl RunnerCapacity {
    pub fn new(max_concurrent_tasks: usize) -> Self {
        Self(Arc::new(Semaphore::new(max_concurrent_tasks)))
    }

    pub fn try_acquire(&self) -> Result<OwnedSemaphorePermit, TryAcquireError> {
        self.0.clone().try_acquire_owned()
    }

    pub fn available(&self) -> usize {
        self.0.available_permits()
    }
}
