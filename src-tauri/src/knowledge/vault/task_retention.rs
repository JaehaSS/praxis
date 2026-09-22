use sqlx::{Sqlite, SqlitePool, Transaction};

use super::admission::{self, AdmissionGuard};

pub async fn try_admission_for_task(
    pool: &SqlitePool,
    task_id: i64,
) -> anyhow::Result<Option<AdmissionGuard>> {
    if !task_table_exists(pool).await? || !task_has_vault_data(pool, task_id).await? {
        return Ok(None);
    }
    let guard = match admission::try_exclusive(pool).await {
        Ok(guard) => guard,
        Err(error) if is_busy(&error) => anyhow::bail!("deletion_busy"),
        Err(error) => return Err(error),
    };
    Ok(Some(guard))
}

fn is_busy(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|error| error.kind() == std::io::ErrorKind::WouldBlock)
}

pub async fn delete_task_data(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: i64,
    has_admission: bool,
) -> anyhow::Result<()> {
    if !task_table_exists_tx(tx).await? {
        return Ok(());
    }
    if !task_has_vault_data_tx(tx, task_id).await? {
        return Ok(());
    }
    if !has_admission {
        anyhow::bail!("deletion_busy")
    }
    delete_consumed_previews(tx, task_id).await?;
    delete_consumed_policies(tx, task_id).await?;
    delete_usages(tx, task_id).await?;
    delete_snapshots(tx, task_id).await?;
    delete_attempt_data(tx, task_id).await?;
    Ok(())
}

async fn task_table_exists(pool: &SqlitePool) -> anyhow::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'vault_task_attempts')")
        .fetch_one(pool)
        .await
        .map_err(Into::into)
}

async fn task_table_exists_tx(tx: &mut Transaction<'_, Sqlite>) -> anyhow::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'vault_task_attempts')")
        .fetch_one(&mut **tx)
        .await
        .map_err(Into::into)
}

async fn task_has_vault_data(pool: &SqlitePool, task_id: i64) -> anyhow::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM vault_task_attempts WHERE task_id = ? UNION ALL SELECT 1 FROM vault_terminal_snapshots WHERE task_id = ? UNION ALL SELECT 1 FROM vault_usages WHERE task_id = ? UNION ALL SELECT 1 FROM vault_reference_previews WHERE consumed_task_id = ? UNION ALL SELECT 1 FROM vault_draft_policies WHERE consumed_task_id = ?)")
        .bind(task_id).bind(task_id).bind(task_id).bind(task_id).bind(task_id)
        .fetch_one(pool).await.map_err(Into::into)
}

async fn task_has_vault_data_tx(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: i64,
) -> anyhow::Result<bool> {
    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM vault_task_attempts WHERE task_id = ? UNION ALL SELECT 1 FROM vault_terminal_snapshots WHERE task_id = ? UNION ALL SELECT 1 FROM vault_usages WHERE task_id = ? UNION ALL SELECT 1 FROM vault_reference_previews WHERE consumed_task_id = ? UNION ALL SELECT 1 FROM vault_draft_policies WHERE consumed_task_id = ?)")
        .bind(task_id).bind(task_id).bind(task_id).bind(task_id).bind(task_id)
        .fetch_one(&mut **tx).await.map_err(Into::into)
}

async fn delete_consumed_previews(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: i64,
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM vault_reference_preview_items WHERE preview_id IN (SELECT id FROM vault_reference_previews WHERE consumed_task_id = ?)")
        .bind(task_id).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM vault_reference_previews WHERE consumed_task_id = ?")
        .bind(task_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn delete_consumed_policies(
    tx: &mut Transaction<'_, Sqlite>,
    task_id: i64,
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM vault_draft_policy_sources WHERE policy_id IN (SELECT id FROM vault_draft_policies WHERE consumed_task_id = ?)")
        .bind(task_id).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM vault_draft_policies WHERE consumed_task_id = ?")
        .bind(task_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn delete_usages(tx: &mut Transaction<'_, Sqlite>, task_id: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM vault_usages WHERE task_id = ? OR attempt_id IN (SELECT id FROM vault_task_attempts WHERE task_id = ?)")
        .bind(task_id).bind(task_id).execute(&mut **tx).await?;
    Ok(())
}

async fn delete_snapshots(tx: &mut Transaction<'_, Sqlite>, task_id: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM vault_terminal_snapshots WHERE task_id = ? OR attempt_id IN (SELECT id FROM vault_task_attempts WHERE task_id = ?)")
        .bind(task_id).bind(task_id).execute(&mut **tx).await?;
    Ok(())
}

async fn delete_attempt_data(tx: &mut Transaction<'_, Sqlite>, task_id: i64) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM vault_attempt_inputs WHERE attempt_id IN (SELECT id FROM vault_task_attempts WHERE task_id = ?)")
        .bind(task_id).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM vault_input_snapshots WHERE attempt_id IN (SELECT id FROM vault_task_attempts WHERE task_id = ?)")
        .bind(task_id).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM vault_task_attempts WHERE task_id = ?")
        .bind(task_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}
