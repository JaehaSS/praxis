use super::super::{
    invalidated_view, retired_view, view, PreviewWorkbench, ReceiptView, MAX_PREPARE_CORRELATIONS,
    MAX_RECEIPTS,
};
use super::{expire, prepare_correlation_count, retire_until_room};

impl PreviewWorkbench {
    pub fn prepared_for(
        &self,
        task_id: i64,
        correlation_id: &str,
        context: &str,
        now: i64,
    ) -> Result<Option<ReceiptView>, String> {
        let mut tasks = self.tasks.lock().unwrap_or_else(|error| error.into_inner());
        let Some(task) = tasks.get_mut(&task_id) else {
            return Ok(None);
        };
        if task.invalidated {
            return Ok(Some(invalidated_view(String::new())));
        }
        expire(task, now);
        let context_hash = crate::preview_bridge::sha256_hex(context.as_bytes());
        if let Some(receipt) = task
            .entries
            .values()
            .find(|entry| entry.correlation_id.as_deref() == Some(correlation_id))
        {
            if receipt.context_hash != context_hash {
                return Err("prepare_context_mismatch".into());
            }
            return Ok(Some(view(
                receipt.request_id.clone(),
                receipt.status,
                receipt.result_id.clone(),
            )));
        }
        if let Some((previous_hash, request_id)) = task.retired_prepares.get(correlation_id) {
            if previous_hash != &context_hash {
                return Err("prepare_context_mismatch".into());
            }
            return Ok(Some(retired_view(request_id.clone())));
        }
        Ok(None)
    }

    pub fn prepare_for(
        &self,
        task_id: i64,
        correlation_id: &str,
        context: &str,
        now: i64,
    ) -> Result<ReceiptView, String> {
        let mut tasks = self.tasks.lock().unwrap_or_else(|error| error.into_inner());
        let task = tasks.entry(task_id).or_default();
        if task.invalidated {
            return Ok(invalidated_view(String::new()));
        }
        expire(task, now);
        let context_hash = crate::preview_bridge::sha256_hex(context.as_bytes());
        if let Some(receipt) = task
            .entries
            .values()
            .find(|entry| entry.correlation_id.as_deref() == Some(correlation_id))
        {
            if receipt.context_hash != context_hash {
                return Err("prepare_context_mismatch".into());
            }
            return Ok(view(
                receipt.request_id.clone(),
                receipt.status,
                receipt.result_id.clone(),
            ));
        }
        if let Some((previous_hash, request_id)) = task.retired_prepares.get(correlation_id) {
            if previous_hash != &context_hash {
                return Err("prepare_context_mismatch".into());
            }
            return Ok(retired_view(request_id.clone()));
        }
        if prepare_correlation_count(task) >= MAX_PREPARE_CORRELATIONS {
            return Err("receipt_capacity".into());
        }
        if task.entries.len() == MAX_RECEIPTS {
            retire_until_room(task);
        }
        if task.entries.len() == MAX_RECEIPTS {
            return Err("receipt_capacity".into());
        }
        Ok(self.issue_prepare(task, task_id, correlation_id, context_hash, now))
    }
}
