use sqlx::SqlitePool;

use super::ReviewProcessQuarantine;

#[derive(Debug, sqlx::FromRow)]
pub(super) struct RepairTarget {
    pub receipt_id: i64,
    pub task_id: i64,
    pub pgid: i64,
    pub identity_hash: String,
    pub current_receipt_id: Option<i64>,
    pub current_state: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct QuarantineRow {
    receipt_id: i64,
    task_id: i64,
    operation: String,
    phase: String,
    pgid: i64,
    detail: Option<String>,
    reason: Option<String>,
    created_at: i64,
    updated_at: i64,
}

pub(super) async fn target(
    pool: &SqlitePool,
    receipt_id: i64,
) -> anyhow::Result<Option<RepairTarget>> {
    sqlx::query_as(
        "SELECT r.id AS receipt_id, r.task_id, r.pgid, r.identity_hash, \
         l.receipt_id AS current_receipt_id, l.state AS current_state \
         FROM review_process_receipts r \
         LEFT JOIN review_process_leases l \
           ON l.task_id = r.task_id AND l.operation = r.operation \
         WHERE r.id = ?",
    )
    .bind(receipt_id)
    .fetch_optional(pool)
    .await
    .map_err(Into::into)
}

pub(super) async fn receipt_task_id(
    pool: &SqlitePool,
    receipt_id: i64,
) -> anyhow::Result<Option<i64>> {
    sqlx::query_scalar("SELECT task_id FROM review_process_receipts WHERE id = ?")
        .bind(receipt_id)
        .fetch_optional(pool)
        .await
        .map_err(Into::into)
}

pub(super) async fn quarantined(pool: &SqlitePool) -> anyhow::Result<Vec<ReviewProcessQuarantine>> {
    let rows = sqlx::query_as::<_, QuarantineRow>(
        "SELECT r.id AS receipt_id, r.task_id, r.operation, r.phase, r.pgid, \
         l.detail, l.reason, r.created_at, l.updated_at \
         FROM review_process_receipts r \
         JOIN review_process_leases l ON l.receipt_id = r.id \
         WHERE l.state = 'quarantined' ORDER BY l.updated_at DESC, r.id DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(map_quarantine).collect())
}

fn map_quarantine(row: QuarantineRow) -> ReviewProcessQuarantine {
    let reason = row
        .reason
        .as_deref()
        .unwrap_or_else(|| public_reason(row.detail.as_deref()));
    ReviewProcessQuarantine {
        receipt_id: row.receipt_id,
        task_id: row.task_id,
        operation: row.operation,
        phase: row.phase,
        pgid: row.pgid,
        state: "quarantined",
        reason: reason.to_string(),
        detail: public_detail(reason).to_string(),
        created_at: row.created_at,
        updated_at: row.updated_at,
    }
}

fn public_reason(detail: Option<&str>) -> &'static str {
    let detail = detail.unwrap_or_default();
    if detail.contains("birth identity") || detail.contains("identity mismatch") {
        return "identity_mismatch";
    }
    if detail.contains("leader exited") || detail.contains("leaderless") {
        return "leaderless_group";
    }
    if detail.contains("signal-safe range") {
        return "invalid_process_group_id";
    }
    "verification_uncertain"
}

pub(super) fn public_detail(reason: &str) -> &'static str {
    match reason {
        "identity_mismatch" => "저장된 프로세스와 현재 프로세스의 신원이 다릅니다.",
        "leaderless_group" => "리더가 없는 프로세스 그룹의 소유권을 확인할 수 없습니다.",
        "invalid_process_group_id" => "저장된 프로세스 그룹 ID가 안전 범위를 벗어났습니다.",
        _ => "프로세스 소유권 또는 종료 상태를 안전하게 확인할 수 없습니다.",
    }
}
