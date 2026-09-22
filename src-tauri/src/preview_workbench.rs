use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use serde::Serialize;

mod view;
use view::{invalidated_view, retired_view, view};
mod lifecycle;
use lifecycle::{expire, missing_view, retire_until_room};
pub mod plugin;
mod reservation;
pub(crate) use reservation::ReceiptReservation;

const MAX_RECEIPTS: usize = 128;
/// Retired prepare correlations are retained for this app run so a lost reply can never mint a
/// second receipt. New correlations fail closed once this per-task bound is reached.
const MAX_PREPARE_CORRELATIONS: usize = 4096;
const PREPARED_TTL_SECS: i64 = 86_400;

#[derive(Clone)]
pub struct PreviewWorkbench {
    app_epoch: String,
    tasks: Arc<Mutex<HashMap<i64, TaskReceipts>>>,
}

#[derive(Default)]
struct TaskReceipts {
    next_seq: u64,
    retired_through: u64,
    invalidated: bool,
    entries: BTreeMap<u64, Receipt>,
    retired_prepares: BTreeMap<String, (String, String)>,
}

struct Receipt {
    request_id: String,
    context_hash: String,
    correlation_id: Option<String>,
    created_at: i64,
    reserved: bool,
    status: ReceiptStatus,
    result_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    Prepared,
    Accepted,
    Finished,
    Rejected,
    Retired,
    Invalidated,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReceiptView {
    pub request_id: String,
    pub status: ReceiptStatus,
    pub accepted: bool,
    pub running: bool,
    pub retryable: bool,
    pub result_id: Option<String>,
}

impl Default for PreviewWorkbench {
    fn default() -> Self {
        Self::new()
    }
}

impl PreviewWorkbench {
    pub fn new() -> Self {
        Self::with_epoch(crate::preview_bridge::random_hex_id().unwrap_or_default())
    }

    pub fn with_epoch(app_epoch: String) -> Self {
        Self {
            app_epoch,
            tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn app_epoch(&self) -> &str {
        &self.app_epoch
    }

    pub fn prepare(&self, task_id: i64, context: &str, now: i64) -> Result<ReceiptView, String> {
        let mut tasks = self.tasks.lock().unwrap_or_else(|error| error.into_inner());
        let task = tasks.entry(task_id).or_default();
        if task.invalidated {
            return Ok(invalidated_view(String::new()));
        }
        expire(task, now);
        if task.entries.len() == MAX_RECEIPTS {
            retire_until_room(task);
        }
        if task.entries.len() == MAX_RECEIPTS {
            return Err("receipt_capacity".into());
        }
        task.next_seq += 1;
        let seq = task.next_seq;
        let context_hash = crate::preview_bridge::sha256_hex(context.as_bytes());
        let request_id = format!("{}:{task_id}:{seq}:{context_hash}", self.app_epoch);
        task.entries.insert(
            seq,
            Receipt {
                request_id: request_id.clone(),
                context_hash,
                correlation_id: None,
                created_at: now,
                reserved: false,
                status: ReceiptStatus::Prepared,
                result_id: None,
            },
        );
        Ok(view(request_id, ReceiptStatus::Prepared, None))
    }

    pub fn accept(
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
        if task.invalidated {
            return Ok(invalidated_view(request_id.into()));
        }
        expire(task, now);
        let Some(receipt) = task
            .entries
            .values_mut()
            .find(|entry| entry.request_id == request_id)
        else {
            return Ok(missing_view(&self.app_epoch, task_id, task, request_id));
        };
        if receipt.context_hash != crate::preview_bridge::sha256_hex(context.as_bytes()) {
            return Err("receipt_context_mismatch".into());
        }
        if receipt.status == ReceiptStatus::Prepared {
            receipt.status = ReceiptStatus::Accepted;
        }
        Ok(view(
            receipt.request_id.clone(),
            receipt.status,
            receipt.result_id.clone(),
        ))
    }

    pub fn receipt(&self, task_id: i64, request_id: &str) -> ReceiptView {
        self.receipt_at(task_id, request_id, 0)
    }

    pub fn receipt_at(&self, task_id: i64, request_id: &str, now: i64) -> ReceiptView {
        let mut tasks = self.tasks.lock().unwrap_or_else(|error| error.into_inner());
        let Some(task) = tasks.get_mut(&task_id) else {
            return invalidated_view(request_id.into());
        };
        if task.invalidated {
            return invalidated_view(request_id.into());
        }
        expire(task, now);
        task.entries
            .values()
            .find(|entry| entry.request_id == request_id)
            .map(|entry| {
                view(
                    entry.request_id.clone(),
                    entry.status,
                    entry.result_id.clone(),
                )
            })
            .unwrap_or_else(|| missing_view(&self.app_epoch, task_id, task, request_id))
    }

    pub fn finish(&self, task_id: i64, request_id: &str, result_id: String) {
        let mut tasks = self.tasks.lock().unwrap_or_else(|error| error.into_inner());
        let Some(task) = tasks.get_mut(&task_id) else {
            return;
        };
        if let Some(receipt) = task
            .entries
            .values_mut()
            .find(|entry| entry.request_id == request_id && entry.status == ReceiptStatus::Accepted)
        {
            receipt.status = ReceiptStatus::Finished;
            receipt.result_id = Some(result_id);
        }
    }

    pub fn invalidate(&self, task_id: i64) {
        let mut tasks = self.tasks.lock().unwrap_or_else(|error| error.into_inner());
        tasks.entry(task_id).or_default().invalidated = true;
    }
}
