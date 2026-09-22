use sqlx::{Row, SqlitePool};

use super::operations::commit_operation;

#[derive(Debug, Clone, Default)]
pub struct RecoveryReport {
    pub recovered: usize,
    pub conflicts: usize,
    pub reindex_needed: usize,
}

pub async fn recover_operations(pool: &SqlitePool, now: i64) -> anyhow::Result<RecoveryReport> {
    let _admission = super::exclusive_admission(pool).await?;
    let rows = sqlx::query("SELECT id, state FROM vault_operations WHERE state IN ('prepared','files_ready') ORDER BY created_at")
        .fetch_all(pool).await?;
    let mut report = RecoveryReport::default();
    for row in rows {
        let id: String = row.try_get("id")?;
        match operation_files_match(pool, &id).await {
            Ok(true) => {
                sqlx::query("UPDATE vault_operations SET state = 'files_ready' WHERE id = ? AND state = 'prepared'").bind(&id).execute(pool).await?;
                if commit_operation(pool, &id, now).await.is_ok() {
                    report.recovered += 1;
                    if index_operation(pool, &id).await.is_err() {
                        report.reindex_needed += 1;
                    }
                } else {
                    conflict(pool, &id, now).await?;
                    report.conflicts += 1;
                }
            }
            Ok(false) | Err(_) => {
                conflict(pool, &id, now).await?;
                report.conflicts += 1;
            }
        }
    }
    Ok(report)
}

async fn index_operation(pool: &SqlitePool, operation_id: &str) -> anyhow::Result<()> {
    let revisions = sqlx::query_scalar::<_, String>(
        "SELECT revision_id FROM vault_operation_files WHERE operation_id = ?",
    )
    .bind(operation_id)
    .fetch_all(pool)
    .await?;
    for revision in revisions {
        super::index::index_revision(pool, &revision).await?;
    }
    Ok(())
}

async fn operation_files_match(pool: &SqlitePool, operation_id: &str) -> anyhow::Result<bool> {
    let revisions = sqlx::query_scalar::<_, String>(
        "SELECT revision_id FROM vault_operation_files WHERE operation_id = ?",
    )
    .bind(operation_id)
    .fetch_all(pool)
    .await?;
    if revisions.is_empty() {
        return Ok(false);
    }
    for revision_id in revisions {
        if super::files::verify_revision(pool, &revision_id)
            .await
            .is_err()
        {
            return Ok(false);
        }
    }
    Ok(true)
}

async fn conflict(pool: &SqlitePool, operation_id: &str, now: i64) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    let proposal_id: Option<String> =
        sqlx::query_scalar("SELECT proposal_id FROM vault_operations WHERE id = ?")
            .bind(operation_id)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query("UPDATE vault_operations SET state = 'conflict' WHERE id = ?")
        .bind(operation_id)
        .execute(&mut *tx)
        .await?;
    if let Some(proposal_id) = proposal_id {
        sqlx::query("UPDATE vault_proposals SET status = 'conflict', decided_at = ? WHERE id = ? AND status = 'pending'")
            .bind(now)
            .bind(proposal_id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::time::Duration;

    use sqlx::sqlite::SqlitePoolOptions;

    use super::super::{
        create_document, create_text_source, register_vault, DocumentDraft, OperationPlan, Scope,
        ScopeRequest, TextSourceDraft,
    };
    use super::*;

    async fn pool() -> SqlitePool {
        let path =
            crate::testtmp::dir().join(format!("vault-recovery-{}.sqlite", std::process::id()));
        let url = format!("sqlite://{}?mode=rwc", path.display());
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect(&url)
            .await
            .unwrap();
        crate::knowledge::migrate(&pool).await.unwrap();
        crate::knowledge::vault::migrate(&pool).await.unwrap();
        pool
    }

    fn root(name: &str) -> std::path::PathBuf {
        let path =
            crate::testtmp::dir().join(format!("vault-recovery-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    async fn prepare_missing_operation(pool: &SqlitePool) -> (String, String) {
        let vault = register_vault(pool, &root("root"), 1).await.unwrap();
        let document = create_document(
            pool,
            &DocumentDraft {
                vault_id: vault.id.clone(),
                kind: "source".into(),
                title: "missing".into(),
            },
            2,
        )
        .await
        .unwrap();
        let plan = OperationPlan::new(
            vault.id.clone(),
            document.id,
            None,
            "revision-recovery".into(),
            "sources/missing.txt".into(),
            b"missing".to_vec(),
            ScopeRequest {
                scope: Scope::PrivateData,
            },
        );
        let operation = super::super::operations::prepare_operation(pool, &plan, 3)
            .await
            .unwrap();
        (operation.id, vault.id)
    }

    #[tokio::test]
    async fn recovery_waits_for_a_live_writer_and_conflicts_missing_files() {
        let pool = pool().await;
        let (operation, vault_id) = prepare_missing_operation(&pool).await;
        let guard = super::super::shared_admission(&pool).await.unwrap();
        let blocked_pool = pool.clone();
        let mut blocked = tokio::spawn(async move { recover_operations(&blocked_pool, 4).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(50), &mut blocked)
                .await
                .is_err()
        );
        drop(guard);
        let report = tokio::time::timeout(Duration::from_secs(1), &mut blocked)
            .await
            .unwrap()
            .unwrap();
        let report = report.unwrap();
        assert_eq!(report.conflicts, 1);
        let state: String = sqlx::query_scalar("SELECT state FROM vault_operations WHERE id = ?")
            .bind(operation)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(state, "conflict");
        create_text_source(
            &pool,
            &TextSourceDraft {
                vault_id,
                title: "new write".into(),
                body: "body".into(),
                scope: ScopeRequest {
                    scope: Scope::PrivateData,
                },
            },
            5,
        )
        .await
        .unwrap();
    }
}
