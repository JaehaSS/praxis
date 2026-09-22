use sqlx::SqlitePool;

use super::{OwnedReviewProcess, ReviewOperation, ReviewProcessReceipt, ReviewProcessState};
use crate::runner::process_identity::ProcessTerminationOutcome;

pub(crate) async fn reconcile(pool: &SqlitePool, now: i64) -> anyhow::Result<u64> {
    let mut recovered = 0;
    for owned in unresolved(pool).await? {
        if owned.lease.state == ReviewProcessState::Quarantined {
            continue;
        }
        let operation = parse_operation(&owned.receipt.operation)?;
        let outcome = crate::runner::process_identity::terminate_if_matches(
            owned.receipt.pgid,
            &owned.receipt.identity_hash,
        )
        .await;
        match outcome {
            Ok(ProcessTerminationOutcome::Terminated) => {
                super::resolve(
                    pool,
                    owned.receipt.task_id,
                    operation,
                    owned.receipt.id,
                    now,
                    "review_process_recovered",
                )
                .await?;
            }
            Ok(ProcessTerminationOutcome::Absent) => {
                super::resolve(
                    pool,
                    owned.receipt.task_id,
                    operation,
                    owned.receipt.id,
                    now,
                    "review_process_absent",
                )
                .await?;
            }
            Ok(ProcessTerminationOutcome::IdentityMismatch) => {
                super::quarantine(
                    pool,
                    owned.receipt.task_id,
                    operation,
                    owned.receipt.id,
                    "process birth identity does not match the durable review receipt",
                    now,
                )
                .await?;
            }
            Err(error) => {
                super::quarantine(
                    pool,
                    owned.receipt.task_id,
                    operation,
                    owned.receipt.id,
                    &error.to_string(),
                    now,
                )
                .await?;
            }
        }
        recovered += 1;
    }
    Ok(recovered)
}

async fn unresolved(pool: &SqlitePool) -> anyhow::Result<Vec<OwnedReviewProcess>> {
    let receipts = sqlx::query_as::<_, ReviewProcessReceipt>(
        "SELECT r.* FROM review_process_receipts r \
         JOIN review_process_leases l ON l.receipt_id = r.id ORDER BY r.id",
    )
    .fetch_all(pool)
    .await?;
    let mut owned = Vec::with_capacity(receipts.len());
    for receipt in receipts {
        let operation = parse_operation(&receipt.operation)?;
        let lease = super::lease(pool, receipt.task_id, operation)
            .await?
            .ok_or_else(|| anyhow::anyhow!("review process lease disappeared"))?;
        owned.push(OwnedReviewProcess { lease, receipt });
    }
    Ok(owned)
}

fn parse_operation(value: &str) -> anyhow::Result<ReviewOperation> {
    match value {
        "verify" => Ok(ReviewOperation::Verify),
        "challenge" => Ok(ReviewOperation::Challenge),
        "repair" => Ok(ReviewOperation::Repair),
        _ => anyhow::bail!("invalid review process operation"),
    }
}
