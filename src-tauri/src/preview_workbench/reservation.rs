use super::{expire, PreviewWorkbench, ReceiptStatus};

pub(crate) struct ReceiptReservation {
    workbench: PreviewWorkbench,
    task_id: i64,
    request_id: String,
    committed: bool,
}

impl ReceiptReservation {
    pub(crate) fn commit(mut self) {
        let mut tasks = self
            .workbench
            .tasks
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let task = tasks
            .get_mut(&self.task_id)
            .expect("reserved receipt task exists");
        let receipt = task
            .entries
            .values_mut()
            .find(|entry| entry.request_id == self.request_id)
            .expect("reserved receipt exists");
        receipt.reserved = false;
        receipt.status = ReceiptStatus::Accepted;
        self.committed = true;
    }
}

impl Drop for ReceiptReservation {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let mut tasks = self
            .workbench
            .tasks
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let Some(task) = tasks.get_mut(&self.task_id) else {
            return;
        };
        if let Some(receipt) = task
            .entries
            .values_mut()
            .find(|entry| entry.request_id == self.request_id)
        {
            receipt.reserved = false;
        }
    }
}

impl PreviewWorkbench {
    pub(crate) fn reserve(
        &self,
        task_id: i64,
        request_id: &str,
        context: &str,
        now: i64,
    ) -> Result<ReceiptReservation, String> {
        let mut tasks = self.tasks.lock().unwrap_or_else(|error| error.into_inner());
        let Some(task) = tasks.get_mut(&task_id) else {
            return Err("receipt is no longer executable".into());
        };
        if task.invalidated {
            return Err("receipt is no longer executable".into());
        }
        expire(task, now);
        let Some(receipt) = task
            .entries
            .values_mut()
            .find(|entry| entry.request_id == request_id)
        else {
            return Err("receipt is no longer executable".into());
        };
        if receipt.context_hash != crate::preview_bridge::sha256_hex(context.as_bytes()) {
            return Err("receipt_context_mismatch".into());
        }
        if receipt.status != ReceiptStatus::Prepared || receipt.reserved {
            return Err("receipt is no longer executable".into());
        }
        receipt.reserved = true;
        Ok(ReceiptReservation {
            workbench: self.clone(),
            task_id,
            request_id: request_id.into(),
            committed: false,
        })
    }
}
