use std::future::Future;

use sqlx::SqlitePool;

use super::{
    repair_ledger, repair_query, ReviewProcessQuarantine, ReviewProcessRepairError,
    ReviewProcessRepairResult, ReviewProcessRepairStatus, TaskReconciliationGuard,
};
use crate::runner::process_identity::ProcessTerminationOutcome;

pub(crate) async fn list_quarantined(
    pool: &SqlitePool,
) -> anyhow::Result<Vec<ReviewProcessQuarantine>> {
    repair_query::quarantined(pool).await
}

pub(crate) async fn receipt_task_id(
    pool: &SqlitePool,
    receipt_id: i64,
) -> anyhow::Result<Option<i64>> {
    repair_query::receipt_task_id(pool, receipt_id).await
}

pub(crate) async fn repair_quarantined(
    pool: &SqlitePool,
    receipt_id: i64,
    now: i64,
    _guard: &TaskReconciliationGuard,
) -> Result<ReviewProcessRepairResult, ReviewProcessRepairError> {
    reconcile_with(pool, receipt_id, now, |pgid, identity| async move {
        crate::runner::process_identity::terminate_if_matches(pgid, &identity).await
    })
    .await
}

pub(super) async fn reconcile_with<F, Fut>(
    pool: &SqlitePool,
    receipt_id: i64,
    now: i64,
    terminate: F,
) -> Result<ReviewProcessRepairResult, ReviewProcessRepairError>
where
    F: FnOnce(i64, String) -> Fut,
    Fut: Future<Output = anyhow::Result<ProcessTerminationOutcome>>,
{
    let target = current_target(pool, receipt_id).await?;
    let Some(target) = target else {
        return Ok(result(
            receipt_id,
            ReviewProcessRepairStatus::AlreadyResolved,
        ));
    };
    if crate::runner::process_identity::checked_process_group_id(target.pgid).is_err() {
        return defer(pool, &target, "invalid_process_group_id", now).await;
    }
    let outcome = terminate(target.pgid, target.identity_hash.clone()).await;
    match outcome {
        Ok(ProcessTerminationOutcome::Absent) => {
            resolve(pool, &target, "resolved_absent", now).await
        }
        Ok(ProcessTerminationOutcome::Terminated) => {
            resolve(pool, &target, "resolved_terminated", now).await
        }
        Ok(ProcessTerminationOutcome::IdentityMismatch) => {
            defer(pool, &target, "identity_mismatch", now).await
        }
        Err(error) => {
            eprintln!(
                "Runner review process repair deferred for receipt {}: {error}",
                target.receipt_id
            );
            defer(pool, &target, "verification_uncertain", now).await
        }
    }
}

async fn current_target(
    pool: &SqlitePool,
    receipt_id: i64,
) -> Result<Option<repair_query::RepairTarget>, ReviewProcessRepairError> {
    let target = repair_query::target(pool, receipt_id)
        .await
        .map_err(ReviewProcessRepairError::Internal)?
        .ok_or(ReviewProcessRepairError::ReceiptNotFound)?;
    let Some(current_receipt_id) = target.current_receipt_id else {
        return Ok(None);
    };
    if current_receipt_id != receipt_id {
        return Err(ReviewProcessRepairError::StaleReceipt);
    }
    if target.current_state.as_deref() != Some("quarantined") {
        return Err(ReviewProcessRepairError::NotQuarantined);
    }
    Ok(Some(target))
}

async fn resolve(
    pool: &SqlitePool,
    target: &repair_query::RepairTarget,
    outcome: &str,
    now: i64,
) -> Result<ReviewProcessRepairResult, ReviewProcessRepairError> {
    let resolved = repair_ledger::resolve(pool, target.task_id, target.receipt_id, outcome, now)
        .await
        .map_err(ReviewProcessRepairError::Internal)?;
    if !resolved {
        return Err(ReviewProcessRepairError::StaleReceipt);
    }
    let status = match outcome {
        "resolved_absent" => ReviewProcessRepairStatus::ResolvedAbsent,
        _ => ReviewProcessRepairStatus::ResolvedTerminated,
    };
    Ok(result(target.receipt_id, status))
}

async fn defer(
    pool: &SqlitePool,
    target: &repair_query::RepairTarget,
    reason: &str,
    now: i64,
) -> Result<ReviewProcessRepairResult, ReviewProcessRepairError> {
    let detail = repair_query::public_detail(reason);
    let deferred = repair_ledger::defer(pool, target.task_id, target.receipt_id, reason, now)
        .await
        .map_err(ReviewProcessRepairError::Internal)?;
    if !deferred {
        return Err(ReviewProcessRepairError::StaleReceipt);
    }
    Ok(ReviewProcessRepairResult {
        receipt_id: target.receipt_id,
        status: ReviewProcessRepairStatus::StillQuarantined,
        reason: Some(reason.to_string()),
        detail: Some(detail.to_string()),
    })
}

fn result(receipt_id: i64, status: ReviewProcessRepairStatus) -> ReviewProcessRepairResult {
    ReviewProcessRepairResult {
        receipt_id,
        status,
        reason: None,
        detail: None,
    }
}
