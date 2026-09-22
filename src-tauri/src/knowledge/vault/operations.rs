use sqlx::{Row, SqlitePool};

use super::catalog::{identifier, insert_grant};
use super::files::{hash, publish, publish_reader};
use super::scope::{scope_allows, scope_for_sources, Scope, ScopeRequest};

#[derive(Debug, Clone)]
pub struct OperationPlan {
    pub vault_id: String,
    pub document_id: String,
    pub document_title: Option<String>,
    pub expected_head: Option<String>,
    pub accepts_drift: bool,
    pub revision_id: String,
    pub relative_path: String,
    pub content: Vec<u8>,
    pub scope: ScopeRequest,
    pub proposal_id: Option<String>,
    pub proposal_candidate_hash: Option<String>,
    pub sources: Vec<String>,
    pub additional: Vec<PlannedRevision>,
}

#[derive(Debug, Clone)]
pub struct PlannedRevision {
    pub document_id: String,
    pub vault_id: String,
    pub document_title: String,
    pub revision_id: String,
    pub relative_path: String,
    pub content: Vec<u8>,
    pub scope: ScopeRequest,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Operation {
    pub id: String,
    pub vault_id: String,
    pub relative_path: String,
    pub sha256: String,
}

impl OperationPlan {
    pub fn new(
        vault_id: String,
        document_id: String,
        expected_head: Option<String>,
        revision_id: String,
        relative_path: String,
        content: Vec<u8>,
        scope: ScopeRequest,
    ) -> Self {
        Self {
            vault_id,
            document_id,
            document_title: None,
            expected_head,
            accepts_drift: false,
            revision_id,
            relative_path,
            content,
            scope,
            proposal_id: None,
            proposal_candidate_hash: None,
            sources: Vec::new(),
            additional: Vec::new(),
        }
    }
}

pub async fn prepare_operation(
    pool: &SqlitePool,
    plan: &OperationPlan,
    now: i64,
) -> anyhow::Result<Operation> {
    prepare_operation_with_metadata(
        pool,
        plan,
        &hash(&plan.content),
        plan.content.len() as u64,
        now,
    )
    .await
}

pub async fn prepare_operation_with_metadata(
    pool: &SqlitePool,
    plan: &OperationPlan,
    sha256: &str,
    size: u64,
    now: i64,
) -> anyhow::Result<Operation> {
    let size = i64::try_from(size)?;
    let idempotency_key = format!("{}:{}", plan.document_id, plan.revision_id);
    if let Some(operation) = existing_operation(pool, &idempotency_key).await? {
        return Ok(operation);
    }
    let mut tx = pool.begin().await?;
    insert_primary_document(&mut tx, plan, now).await?;
    ensure_expected_head(&mut tx, plan).await?;
    ensure_proposal_current(&mut tx, plan).await?;
    let operation = Operation {
        id: identifier("operation")?,
        vault_id: plan.vault_id.clone(),
        relative_path: plan.relative_path.clone(),
        sha256: sha256.into(),
    };
    for revision in &plan.additional {
        insert_additional_revision(&mut tx, revision, now).await?;
    }
    insert_revision(&mut tx, plan, sha256, size, now).await?;
    sqlx::query("INSERT INTO vault_operations (id, vault_id, idempotency_key, proposal_id, proposal_candidate_hash, document_id, expected_head, accepts_drift, revision_id, relative_path, sha256, state, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'prepared', ?)")
        .bind(&operation.id).bind(&plan.vault_id).bind(&idempotency_key).bind(&plan.proposal_id).bind(&plan.proposal_candidate_hash).bind(&plan.document_id).bind(&plan.expected_head).bind(plan.accepts_drift).bind(&plan.revision_id).bind(&plan.relative_path).bind(&operation.sha256).bind(now).execute(&mut *tx).await?;
    insert_operation_file(
        &mut tx,
        &operation.id,
        &plan.revision_id,
        &plan.relative_path,
        &operation.sha256,
        size,
        "primary",
    )
    .await?;
    for revision in &plan.additional {
        insert_operation_file(
            &mut tx,
            &operation.id,
            &revision.revision_id,
            &revision.relative_path,
            &hash(&revision.content),
            revision.content.len() as i64,
            "source",
        )
        .await?;
    }
    tx.commit().await?;
    Ok(operation)
}

pub async fn write_operation_file(
    pool: &SqlitePool,
    operation: &Operation,
    content: &[u8],
) -> anyhow::Result<()> {
    let root = vault_root(pool, &operation.vault_id).await?;
    publish(
        std::path::Path::new(&root.path),
        root.device,
        root.inode,
        &operation.relative_path,
        content,
    )?;
    mark_files_ready(pool, &operation.id).await
}

pub async fn write_operation_reader<R: std::io::Read>(
    pool: &SqlitePool,
    operation: &Operation,
    reader: &mut R,
    expected_size: u64,
) -> anyhow::Result<()> {
    let root = vault_root(pool, &operation.vault_id).await?;
    publish_reader(
        std::path::Path::new(&root.path),
        root.device,
        root.inode,
        &operation.relative_path,
        reader,
        &operation.sha256,
        expected_size,
    )?;
    mark_files_ready(pool, &operation.id).await
}

pub async fn write_operation_files(
    pool: &SqlitePool,
    operation: &Operation,
    plan: &OperationPlan,
) -> anyhow::Result<()> {
    let root = vault_root(pool, &operation.vault_id).await?;
    publish(
        std::path::Path::new(&root.path),
        root.device,
        root.inode,
        &operation.relative_path,
        &plan.content,
    )?;
    for revision in &plan.additional {
        publish(
            std::path::Path::new(&root.path),
            root.device,
            root.inode,
            &revision.relative_path,
            &revision.content,
        )?;
    }
    mark_files_ready(pool, &operation.id).await
}

pub async fn mark_files_ready(pool: &SqlitePool, operation_id: &str) -> anyhow::Result<()> {
    let updated = sqlx::query(
        "UPDATE vault_operations SET state = 'files_ready' WHERE id = ? AND state = 'prepared'",
    )
    .bind(operation_id)
    .execute(pool)
    .await?
    .rows_affected();
    if updated != 1 {
        anyhow::bail!("vault operation is not prepared")
    }
    Ok(())
}

pub async fn commit_operation(
    pool: &SqlitePool,
    operation_id: &str,
    now: i64,
) -> anyhow::Result<()> {
    verify_operation_files(pool, operation_id).await?;
    let mut tx = pool.begin().await?;
    let row = sqlx::query("SELECT document_id, expected_head, accepts_drift, revision_id, proposal_id, proposal_candidate_hash FROM vault_operations WHERE id = ? AND state = 'files_ready'")
        .bind(operation_id).fetch_optional(&mut *tx).await?.ok_or_else(|| anyhow::anyhow!("vault operation is not ready"))?;
    let document_id: String = row.try_get("document_id")?;
    let expected_head: Option<String> = row.try_get("expected_head")?;
    let revision_id: String = row.try_get("revision_id")?;
    validate_expected_head(
        pool,
        &mut tx,
        expected_head.as_deref(),
        row.try_get("accepts_drift")?,
        &revision_id,
    )
    .await?;
    validate_operation_sources(pool, &mut tx, operation_id, &revision_id).await?;
    if let Some(proposal_id) = row.try_get::<Option<String>, _>("proposal_id")? {
        validate_proposal_commit(
            pool,
            &mut tx,
            &proposal_id,
            row.try_get("proposal_candidate_hash")?,
            &revision_id,
        )
        .await?;
    }
    let updated = sqlx::query(
        "UPDATE vault_documents SET current_revision = ? WHERE id = ? AND current_revision IS ?",
    )
    .bind(&revision_id)
    .bind(&document_id)
    .bind(&expected_head)
    .execute(&mut *tx)
    .await?
    .rows_affected();
    if updated != 1 {
        anyhow::bail!("vault document head changed")
    }
    if row.try_get::<bool, _>("accepts_drift")? {
        sqlx::query("UPDATE vault_documents SET state = 'active' WHERE id = ?")
            .bind(&document_id)
            .execute(&mut *tx)
            .await?;
    }
    let extras = sqlx::query(
        "SELECT revision_id FROM vault_operation_files WHERE operation_id = ? AND role = 'source'",
    )
    .bind(operation_id)
    .fetch_all(&mut *tx)
    .await?;
    for extra in extras {
        let revision_id: String = extra.try_get("revision_id")?;
        let document_id: String =
            sqlx::query_scalar("SELECT document_id FROM vault_revisions WHERE id = ?")
                .bind(&revision_id)
                .fetch_one(&mut *tx)
                .await?;
        let changed = sqlx::query("UPDATE vault_documents SET current_revision = ? WHERE id = ? AND current_revision IS NULL").bind(&revision_id).bind(document_id).execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            anyhow::bail!("vault source document head changed")
        }
    }
    if let Some(proposal_id) = row.try_get::<Option<String>, _>("proposal_id")? {
        let changed = sqlx::query("UPDATE vault_proposals SET status = 'accepted', decided_at = ? WHERE id = ? AND status = 'pending'")
            .bind(now).bind(proposal_id).execute(&mut *tx).await?.rows_affected();
        if changed != 1 {
            anyhow::bail!("proposal is no longer pending")
        }
    }
    sqlx::query("UPDATE vault_operations SET state = 'committed', completed_at = ? WHERE id = ?")
        .bind(now)
        .bind(operation_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn validate_operation_sources(
    pool: &SqlitePool,
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    operation_id: &str,
    revision_id: &str,
) -> anyhow::Result<()> {
    let requested = operation_scope(tx, revision_id).await?;
    let sources = sqlx::query_scalar(
        "SELECT source_revision_id FROM vault_revision_sources WHERE revision_id = ?",
    )
    .bind(revision_id)
    .fetch_all(&mut **tx)
    .await?;
    let planned: Vec<String> = sqlx::query_scalar(
        "SELECT revision_id FROM vault_operation_files WHERE operation_id = ? AND role = 'source'",
    )
    .bind(operation_id)
    .fetch_all(&mut **tx)
    .await?;
    let published = sources
        .iter()
        .filter(|source| !planned.contains(source))
        .cloned()
        .collect::<Vec<_>>();
    validate_published_sources(pool, &published, &requested).await?;
    validate_planned_sources(pool, tx, &planned, &requested).await
}

async fn validate_published_sources(
    pool: &SqlitePool,
    sources: &[String],
    requested: &ScopeRequest,
) -> anyhow::Result<()> {
    if sources.is_empty() {
        return Ok(());
    }
    let available = scope_for_sources(pool, sources)
        .await?
        .ok_or_else(|| anyhow::anyhow!("operation sources are no longer compatible"))?;
    if !scope_allows(&available, requested) {
        anyhow::bail!("operation scope exceeds its source grants")
    }
    Ok(())
}

async fn validate_planned_sources(
    pool: &SqlitePool,
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    sources: &[String],
    requested: &ScopeRequest,
) -> anyhow::Result<()> {
    for source in sources {
        let scope = operation_scope(tx, source).await?;
        if !scope_allows(&scope.scope, requested) {
            anyhow::bail!("operation scope exceeds its planned source grant")
        }
        let ancestors: Vec<String> = sqlx::query_scalar(
            "SELECT source_revision_id FROM vault_revision_sources WHERE revision_id = ?",
        )
        .bind(source)
        .fetch_all(&mut **tx)
        .await?;
        if ancestors.is_empty() {
            continue;
        }
        let available = scope_for_sources(pool, &ancestors)
            .await?
            .ok_or_else(|| anyhow::anyhow!("planned source inputs are no longer compatible"))?;
        if !scope_allows(&available, requested) {
            anyhow::bail!("operation scope exceeds its planned source inputs")
        }
    }
    Ok(())
}

async fn validate_expected_head(
    pool: &SqlitePool,
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    expected_head: Option<&str>,
    accepts_drift: bool,
    revision_id: &str,
) -> anyhow::Result<()> {
    let Some(expected_head) = expected_head else {
        return Ok(());
    };
    let requested = operation_scope(tx, revision_id).await?;
    if accepts_drift {
        let state: String =
            sqlx::query_scalar("SELECT state FROM vault_documents WHERE current_revision = ?")
                .bind(expected_head)
                .fetch_one(&mut **tx)
                .await?;
        if state != "drifted" {
            anyhow::bail!("drift acceptance target changed")
        }
        let existing = operation_scope(tx, expected_head).await?;
        if !scope_allows(&existing.scope, &requested) {
            anyhow::bail!("operation scope exceeds its target scope")
        }
        return Ok(());
    }
    let available = scope_for_sources(pool, &[expected_head.into()])
        .await?
        .ok_or_else(|| anyhow::anyhow!("operation target is no longer current"))?;
    if !scope_allows(&available, &requested) {
        anyhow::bail!("operation scope exceeds its target scope")
    }
    Ok(())
}

async fn existing_operation(
    pool: &SqlitePool,
    idempotency_key: &str,
) -> anyhow::Result<Option<Operation>> {
    let row = sqlx::query("SELECT id, vault_id, relative_path, sha256 FROM vault_operations WHERE idempotency_key = ?").bind(idempotency_key).fetch_optional(pool).await?;
    row.as_ref().map(operation_from_row).transpose()
}

async fn ensure_expected_head(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    plan: &OperationPlan,
) -> anyhow::Result<()> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT current_revision FROM vault_documents WHERE id = ? AND vault_id = ?",
    )
    .bind(&plan.document_id)
    .bind(&plan.vault_id)
    .fetch_optional(&mut **tx)
    .await?;
    if row
        .ok_or_else(|| anyhow::anyhow!("vault document is missing"))?
        .0
        != plan.expected_head
    {
        anyhow::bail!("vault document head changed")
    }
    Ok(())
}

async fn insert_primary_document(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    plan: &OperationPlan,
    now: i64,
) -> anyhow::Result<()> {
    let Some(title) = &plan.document_title else {
        return Ok(());
    };
    sqlx::query("INSERT INTO vault_documents (id, vault_id, kind, title, created_at) VALUES (?, ?, 'note', ?, ?)")
        .bind(&plan.document_id)
        .bind(&plan.vault_id)
        .bind(title)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn ensure_proposal_current(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    plan: &OperationPlan,
) -> anyhow::Result<()> {
    let Some(id) = &plan.proposal_id else {
        return Ok(());
    };
    let hash = plan
        .proposal_candidate_hash
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("proposal operation is missing its candidate hash"))?;
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT candidate_hash, status FROM vault_proposals WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut **tx)
            .await?;
    if row != Some((hash.into(), "pending".into())) {
        anyhow::bail!("proposal changed; review it again")
    }
    Ok(())
}

async fn validate_proposal_commit(
    pool: &SqlitePool,
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    proposal_id: &str,
    expected_hash: Option<String>,
    revision_id: &str,
) -> anyhow::Result<()> {
    let expected_hash = expected_hash
        .ok_or_else(|| anyhow::anyhow!("proposal is missing its prepared candidate hash"))?;
    let row = sqlx::query("SELECT candidate_hash, status, requested_scope, requested_project_key, requested_binding_epoch FROM vault_proposals WHERE id = ?")
        .bind(proposal_id).fetch_one(&mut **tx).await?;
    if row.try_get::<String, _>("candidate_hash")? != expected_hash
        || row.try_get::<String, _>("status")? != "pending"
    {
        anyhow::bail!("proposal changed; review it again")
    }
    let requested = proposal_scope_from_row(&row)?;
    if operation_scope(tx, revision_id).await? != requested {
        anyhow::bail!("proposal scope changed; review it again")
    }
    let sources = sqlx::query(
        "SELECT source_revision_id, source_hash FROM vault_proposal_sources WHERE proposal_id = ?",
    )
    .bind(proposal_id)
    .fetch_all(&mut **tx)
    .await?;
    let mut revisions = Vec::with_capacity(sources.len());
    for source in sources {
        let revision: String = source.try_get("source_revision_id")?;
        let current: String = sqlx::query_scalar("SELECT sha256 FROM vault_revisions WHERE id = ?")
            .bind(&revision)
            .fetch_one(&mut **tx)
            .await?;
        if current != source.try_get::<String, _>("source_hash")? {
            anyhow::bail!("proposal source changed; review it again")
        }
        revisions.push(revision);
    }
    if !revisions.is_empty() {
        let available = scope_for_sources(pool, &revisions)
            .await?
            .ok_or_else(|| anyhow::anyhow!("proposal sources are no longer compatible"))?;
        if !scope_allows(&available, &requested) {
            anyhow::bail!("proposal scope exceeds its source grants")
        }
    }
    let snapshots: Vec<String> = sqlx::query_scalar(
        "SELECT snapshot_id FROM vault_proposal_snapshots WHERE proposal_id = ?",
    )
    .bind(proposal_id)
    .fetch_all(&mut **tx)
    .await?;
    for snapshot in snapshots {
        if !super::provenance::completion_snapshot_current(pool, &snapshot).await? {
            anyhow::bail!("completion snapshot is no longer eligible")
        }
    }
    Ok(())
}

async fn operation_scope(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    revision_id: &str,
) -> anyhow::Result<ScopeRequest> {
    let row = sqlx::query("SELECT scope, project_key, binding_epoch FROM vault_grants WHERE revision_id = ? AND revoked_at IS NULL")
        .bind(revision_id).fetch_one(&mut **tx).await?;
    scope_from_row(&row)
}

fn scope_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<ScopeRequest> {
    let scope = match row.try_get::<String, _>("scope")?.as_str() {
        "private-data" => Scope::PrivateData,
        "common" => Scope::Common,
        "project" => Scope::Project {
            key: row
                .try_get::<Option<String>, _>("project_key")?
                .ok_or_else(|| anyhow::anyhow!("project scope is missing a binding"))?,
            binding_epoch: row
                .try_get::<Option<String>, _>("binding_epoch")?
                .ok_or_else(|| anyhow::anyhow!("project scope is missing an epoch"))?,
        },
        _ => anyhow::bail!("unknown vault scope"),
    };
    Ok(ScopeRequest { scope })
}

fn proposal_scope_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<ScopeRequest> {
    let scope = match row.try_get::<String, _>("requested_scope")?.as_str() {
        "private-data" => Scope::PrivateData,
        "common" => Scope::Common,
        "project" => Scope::Project {
            key: row
                .try_get::<Option<String>, _>("requested_project_key")?
                .ok_or_else(|| anyhow::anyhow!("proposal project scope is missing a binding"))?,
            binding_epoch: row
                .try_get::<Option<String>, _>("requested_binding_epoch")?
                .ok_or_else(|| anyhow::anyhow!("proposal project scope is missing an epoch"))?,
        },
        _ => anyhow::bail!("unknown proposal scope"),
    };
    Ok(ScopeRequest { scope })
}

async fn insert_revision(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    plan: &OperationPlan,
    sha256: &str,
    size: i64,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO vault_revisions (id, document_id, relative_path, sha256, size, predecessor, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(&plan.revision_id).bind(&plan.document_id).bind(&plan.relative_path).bind(sha256).bind(size).bind(&plan.expected_head).bind(now).execute(&mut **tx).await?;
    for source in &plan.sources {
        sqlx::query(
            "INSERT INTO vault_revision_sources (revision_id, source_revision_id) VALUES (?, ?)",
        )
        .bind(&plan.revision_id)
        .bind(source)
        .execute(&mut **tx)
        .await?;
    }
    insert_grant(&mut **tx, &plan.revision_id, &plan.scope, now).await
}

async fn insert_additional_revision(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    revision: &PlannedRevision,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO vault_documents (id, vault_id, kind, title, created_at) VALUES (?, ?, 'source', ?, ?)")
        .bind(&revision.document_id).bind(&revision.vault_id).bind(&revision.document_title).bind(now).execute(&mut **tx).await?;
    sqlx::query("INSERT INTO vault_revisions (id, document_id, relative_path, sha256, size, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(&revision.revision_id).bind(&revision.document_id).bind(&revision.relative_path).bind(hash(&revision.content)).bind(revision.content.len() as i64).bind(now).execute(&mut **tx).await?;
    for source in &revision.sources {
        sqlx::query(
            "INSERT INTO vault_revision_sources (revision_id, source_revision_id) VALUES (?, ?)",
        )
        .bind(&revision.revision_id)
        .bind(source)
        .execute(&mut **tx)
        .await?;
    }
    insert_grant(&mut **tx, &revision.revision_id, &revision.scope, now).await?;
    Ok(())
}

async fn insert_operation_file(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    operation_id: &str,
    revision_id: &str,
    relative_path: &str,
    sha256: &str,
    size: i64,
    role: &str,
) -> anyhow::Result<()> {
    sqlx::query("INSERT INTO vault_operation_files (operation_id, revision_id, relative_path, sha256, size, role) VALUES (?, ?, ?, ?, ?, ?)").bind(operation_id).bind(revision_id).bind(relative_path).bind(sha256).bind(size).bind(role).execute(&mut **tx).await?;
    Ok(())
}

async fn verify_operation_files(pool: &SqlitePool, operation_id: &str) -> anyhow::Result<()> {
    let revisions = sqlx::query_scalar::<_, String>(
        "SELECT revision_id FROM vault_operation_files WHERE operation_id = ?",
    )
    .bind(operation_id)
    .fetch_all(pool)
    .await?;
    for revision_id in revisions {
        super::files::verify_revision(pool, &revision_id).await?;
    }
    Ok(())
}

struct VaultRoot {
    path: String,
    device: i64,
    inode: i64,
}
async fn vault_root(pool: &SqlitePool, vault_id: &str) -> anyhow::Result<VaultRoot> {
    let row: (String, i64, i64) = sqlx::query_as(
        "SELECT canonical_root, root_device, root_inode FROM vaults WHERE id = ? AND enabled = 1",
    )
    .bind(vault_id)
    .fetch_one(pool)
    .await?;
    let identity = super::platform::verified_root(std::path::Path::new(&row.0))?;
    if identity.canonical_root != row.0 || identity.device != row.1 || identity.inode != row.2 {
        anyhow::bail!("vault root identity changed")
    }
    Ok(VaultRoot {
        path: row.0,
        device: row.1,
        inode: row.2,
    })
}

fn operation_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<Operation> {
    Ok(Operation {
        id: row.try_get("id")?,
        vault_id: row.try_get("vault_id")?,
        relative_path: row.try_get("relative_path")?,
        sha256: row.try_get("sha256")?,
    })
}
