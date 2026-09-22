use super::{invalidated_view, retired_view, view, PreviewWorkbench};
use super::{ReceiptStatus, ReceiptView, TaskReceipts};

pub(super) fn expire(task: &mut TaskReceipts, now: i64) {
    for receipt in task.entries.values_mut() {
        if receipt.status == ReceiptStatus::Prepared
            && !receipt.reserved
            && now >= receipt.created_at
            && now - receipt.created_at >= super::PREPARED_TTL_SECS
        {
            receipt.status = ReceiptStatus::Rejected;
        }
    }
}

pub(super) fn retire_until_room(task: &mut TaskReceipts) {
    while task.entries.len() >= super::MAX_RECEIPTS {
        let seq = task.retired_through + 1;
        let Some(receipt) = task.entries.get(&seq) else {
            return;
        };
        if !matches!(
            receipt.status,
            ReceiptStatus::Finished | ReceiptStatus::Rejected
        ) {
            return;
        }
        let receipt = task.entries.remove(&seq).expect("checked receipt exists");
        if let Some(correlation_id) = receipt.correlation_id {
            task.retired_prepares
                .insert(correlation_id, (receipt.context_hash, receipt.request_id));
        }
        task.retired_through = seq;
    }
}

fn prepare_correlation_count(task: &TaskReceipts) -> usize {
    task.retired_prepares.len()
        + task
            .entries
            .values()
            .filter(|entry| entry.correlation_id.is_some())
            .count()
}

impl PreviewWorkbench {
    fn issue_prepare(
        &self,
        task: &mut TaskReceipts,
        task_id: i64,
        correlation_id: &str,
        context_hash: String,
        now: i64,
    ) -> ReceiptView {
        task.next_seq += 1;
        let seq = task.next_seq;
        let request_id = format!("{}:{task_id}:{seq}:{context_hash}", self.app_epoch);
        task.entries.insert(
            seq,
            super::Receipt {
                request_id: request_id.clone(),
                context_hash,
                correlation_id: Some(correlation_id.into()),
                created_at: now,
                reserved: false,
                status: ReceiptStatus::Prepared,
                result_id: None,
            },
        );
        view(request_id, ReceiptStatus::Prepared, None)
    }

    pub fn reject_prepared(
        &self,
        task_id: i64,
        request_id: &str,
        context: &str,
        now: i64,
    ) -> Result<ReceiptView, String> {
        let mut tasks = self.tasks.lock().unwrap_or_else(|error| error.into_inner());
        let Some(task) = tasks.get_mut(&task_id) else {
            return Ok(invalidated_view(request_id.into()));
        };
        expire(task, now);
        let Some(receipt) = task
            .entries
            .values_mut()
            .find(|entry| entry.request_id == request_id)
        else {
            return Ok(invalidated_view(request_id.into()));
        };
        if receipt.context_hash != crate::preview_bridge::sha256_hex(context.as_bytes()) {
            return Err("receipt_context_mismatch".into());
        }
        if receipt.status == ReceiptStatus::Prepared && !receipt.reserved {
            receipt.status = ReceiptStatus::Rejected;
        }
        Ok(view(
            receipt.request_id.clone(),
            receipt.status,
            receipt.result_id.clone(),
        ))
    }
}

pub(super) fn missing_view(
    epoch: &str,
    task_id: i64,
    task: &TaskReceipts,
    request_id: &str,
) -> ReceiptView {
    let parts = request_id.split(':').collect::<Vec<_>>();
    if parts.len() != 4 || parts[0] != epoch || parts[1].parse::<i64>().ok() != Some(task_id) {
        return invalidated_view(request_id.into());
    }
    let Some(seq) = parts[2].parse::<u64>().ok() else {
        return invalidated_view(request_id.into());
    };
    if parts[3].is_empty() || seq > task.retired_through {
        return invalidated_view(request_id.into());
    }
    retired_view(request_id.into())
}

#[cfg(test)]
mod lifecycle_tests;
mod prepare;
