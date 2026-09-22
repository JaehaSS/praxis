use sqlx::SqlitePool;

use super::{ReviewOperation, ReviewPhase};
use crate::managed_process::{ProcessLease, ProcessRegistrar, SharedProcessRegistrar};

pub fn registrar(
    pool: SqlitePool,
    task_id: i64,
    operation: ReviewOperation,
    phase: ReviewPhase,
) -> Result<SharedProcessRegistrar, String> {
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = (pool, task_id, operation, phase);
        return Err("durable Runner review is unsupported on this platform".into());
    }
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| "Runner review requires an active Tokio runtime".to_string())?;
        Ok(std::sync::Arc::new(DurableRegistrar {
            pool,
            runtime,
            task_id,
            operation,
            phase,
        }))
    }
}

struct DurableRegistrar {
    pool: SqlitePool,
    runtime: tokio::runtime::Handle,
    task_id: i64,
    operation: ReviewOperation,
    phase: ReviewPhase,
}

impl ProcessRegistrar for DurableRegistrar {
    fn register(&self, pid: u32) -> Result<Box<dyn ProcessLease>, String> {
        let receipt = self
            .runtime
            .block_on(super::register_observed(
                &self.pool,
                self.task_id,
                self.operation,
                self.phase,
                pid,
                now(),
            ))
            .map_err(|error| error.to_string())?;
        Ok(Box::new(DurableLease {
            pool: self.pool.clone(),
            runtime: self.runtime.clone(),
            task_id: self.task_id,
            operation: self.operation,
            receipt_id: receipt.id,
            pid,
        }))
    }
}

struct DurableLease {
    pool: SqlitePool,
    runtime: tokio::runtime::Handle,
    task_id: i64,
    operation: ReviewOperation,
    receipt_id: i64,
    pid: u32,
}

impl ProcessLease for DurableLease {
    fn complete(self: Box<Self>) -> Result<(), String> {
        match crate::verify::process_group_alive_checked(self.pid) {
            Ok(false) => {}
            Ok(true) => {
                let detail = format!("process group {} remains alive after wait", self.pid);
                self.persist_quarantine(&detail)?;
                return Err(detail);
            }
            Err(error) => {
                let detail = format!(
                    "process group {} absence could not be confirmed: {error}",
                    self.pid
                );
                self.persist_quarantine(&detail)?;
                return Err(detail);
            }
        }
        let resolved = self
            .runtime
            .block_on(super::resolve(
                &self.pool,
                self.task_id,
                self.operation,
                self.receipt_id,
                now(),
                "review_process_completed",
            ))
            .map_err(|error| error.to_string())?;
        if !resolved {
            return Err("stale review process lease completion".into());
        }
        Ok(())
    }

    fn quarantine(self: Box<Self>, detail: &str) -> Result<(), String> {
        self.persist_quarantine(detail)
    }
}

impl DurableLease {
    fn persist_quarantine(&self, detail: &str) -> Result<(), String> {
        let quarantined = self
            .runtime
            .block_on(super::quarantine(
                &self.pool,
                self.task_id,
                self.operation,
                self.receipt_id,
                detail,
                now(),
            ))
            .map_err(|error| error.to_string())?;
        if !quarantined {
            return Err("stale review process lease quarantine".into());
        }
        Ok(())
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
