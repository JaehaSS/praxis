use sqlx::{Sqlite, Transaction};

use crate::evidence::EvidenceRecord;

pub(crate) async fn insert(
    tx: &mut Transaction<'_, Sqlite>,
    memory_id: i64,
    version: i64,
    observed_at: i64,
    expires_at: Option<i64>,
) -> Result<EvidenceRecord, sqlx::Error> {
    let locator_json = serde_json::json!({
        "actor_kind": "human",
        "confirmed_at": observed_at,
    })
    .to_string();
    let id = sqlx::query(
        "INSERT INTO memory_evidence \
         (memory_id, version, kind, locator_json, snapshot_hash, status, observed_at, checked_at, expires_at) \
         VALUES (?, ?, 'user_confirmation', ?, NULL, 'valid', ?, ?, ?)",
    )
    .bind(memory_id)
    .bind(version)
    .bind(&locator_json)
    .bind(observed_at)
    .bind(observed_at)
    .bind(expires_at)
    .execute(&mut **tx)
    .await?
    .last_insert_rowid();
    sqlx::query(
        "INSERT INTO memory_events \
         (memory_id, version, action, actor_kind, payload_json, created_at) \
         VALUES (?, ?, 'evidence_added', 'human', ?, ?)",
    )
    .bind(memory_id)
    .bind(version)
    .bind(&locator_json)
    .bind(observed_at)
    .execute(&mut **tx)
    .await?;
    Ok(EvidenceRecord {
        id,
        memory_id,
        version,
        kind: super::evidence_kind::USER_CONFIRMATION.to_string(),
        locator_json,
        snapshot_hash: None,
        status: super::evidence_status::VALID.to_string(),
        observed_at,
        checked_at: Some(observed_at),
        expires_at,
    })
}
