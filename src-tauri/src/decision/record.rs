use sqlx::{Sqlite, Transaction};

use super::provenance::{ApprovalProvenance, ArtifactLink};

const SUMMARY: &str = "Isolated worktree changes approved and merged.";

#[derive(sqlx::FromRow)]
struct ExistingRecord {
    id: i64,
    kind: Option<String>,
    outcome: Option<String>,
    actor_kind: Option<String>,
    task_id: Option<i64>,
    summary: Option<String>,
    status: String,
}

pub async fn record_approval(
    tx: &mut Transaction<'_, Sqlite>,
    provenance: &ApprovalProvenance,
    now: i64,
) -> anyhow::Result<i64> {
    let key = provenance.decision_key_hash()?;
    let links = provenance.links()?;
    let inserted = sqlx::query(
        "INSERT OR IGNORE INTO decision_records \
         (decision_key_hash, kind, outcome, actor_kind, task_id, summary, status, created_at) \
         VALUES (?, 'task_approval', 'approved', 'local_human', ?, ?, 'active', ?)",
    )
    .bind(&key)
    .bind(provenance.task_id)
    .bind(SUMMARY)
    .bind(now)
    .execute(&mut **tx)
    .await?;
    if inserted.rows_affected() == 1 {
        let id = inserted.last_insert_rowid();
        insert_links(tx, id, &links, now).await?;
        return Ok(id);
    }
    validate_existing(tx, &key, provenance, &links).await
}

async fn insert_links(
    tx: &mut Transaction<'_, Sqlite>,
    decision_id: i64,
    links: &[ArtifactLink],
    now: i64,
) -> anyhow::Result<()> {
    for link in links {
        sqlx::query(
            "INSERT INTO decision_artifact_links \
             (decision_id, relation, artifact_kind, artifact_ref, created_at) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(decision_id)
        .bind(link.relation.as_str())
        .bind(link.kind.as_str())
        .bind(&link.artifact_ref)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

async fn validate_existing(
    tx: &mut Transaction<'_, Sqlite>,
    key: &str,
    provenance: &ApprovalProvenance,
    expected_links: &[ArtifactLink],
) -> anyhow::Result<i64> {
    let row: ExistingRecord = sqlx::query_as(
        "SELECT id, kind, outcome, actor_kind, task_id, summary, status \
         FROM decision_records WHERE decision_key_hash = ?",
    )
    .bind(key)
    .fetch_one(&mut **tx)
    .await?;
    let record_matches = row.kind.as_deref() == Some("task_approval")
        && row.outcome.as_deref() == Some("approved")
        && row.actor_kind.as_deref() == Some("local_human")
        && row.task_id == Some(provenance.task_id)
        && row.summary.as_deref() == Some(SUMMARY)
        && row.status == "active";
    if !record_matches {
        anyhow::bail!("decision payload conflict for idempotency key");
    }
    let actual: Vec<(String, String, String)> = sqlx::query_as(
        "SELECT relation, artifact_kind, artifact_ref FROM decision_artifact_links \
         WHERE decision_id = ? ORDER BY artifact_kind, artifact_ref, relation",
    )
    .bind(row.id)
    .fetch_all(&mut **tx)
    .await?;
    if actual != link_tuples(expected_links) {
        anyhow::bail!("decision payload conflict for idempotency key");
    }
    Ok(row.id)
}

fn link_tuples(links: &[ArtifactLink]) -> Vec<(String, String, String)> {
    links
        .iter()
        .map(|link| {
            (
                link.relation.as_str().to_string(),
                link.kind.as_str().to_string(),
                link.artifact_ref.clone(),
            )
        })
        .collect()
}
