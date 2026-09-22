use std::path::Path;

use sqlx::{Row, SqlitePool};

use super::platform;
use super::scope::{Scope, ScopeRequest};

#[derive(Debug, Clone)]
pub struct Vault {
    pub id: String,
    pub canonical_root: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct DocumentDraft {
    pub vault_id: String,
    pub kind: String,
    pub title: String,
}

#[derive(Debug, Clone)]
pub struct VaultDocument {
    pub id: String,
    pub vault_id: String,
    pub kind: String,
    pub title: String,
    pub state: String,
    pub current_revision: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Revision {
    pub id: String,
    pub document_id: String,
    pub relative_path: String,
    pub sha256: String,
    pub size: i64,
    pub predecessor: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DocumentDetail {
    pub document: VaultDocument,
    pub current_revision: Option<Revision>,
    pub history: Vec<Revision>,
    pub source_revisions: Vec<String>,
    pub indexed: bool,
}

pub async fn register_vault(pool: &SqlitePool, root: &Path, now: i64) -> anyhow::Result<Vault> {
    let _admission = super::exclusive_admission(pool).await?;
    let identity = platform::verified_root(root)?;
    let active: Option<String> =
        sqlx::query_scalar("SELECT id FROM vaults WHERE enabled = 1 AND writable = 1")
            .fetch_optional(pool)
            .await?;
    if active.is_some() {
        anyhow::bail!("disconnect the active writable vault before registering another")
    }
    let existing: Option<String> = sqlx::query_scalar(
        "SELECT id FROM vaults WHERE canonical_root = ? AND root_device = ? AND root_inode = ?",
    )
    .bind(&identity.canonical_root)
    .bind(identity.device)
    .bind(identity.inode)
    .fetch_optional(pool)
    .await?;
    if let Some(id) = existing {
        sqlx::query("UPDATE vaults SET enabled = 1, writable = 1 WHERE id = ?")
            .bind(&id)
            .execute(pool)
            .await?;
        super::ownership::claim_legacy_owned_nodes_unlocked(pool, now).await?;
        return Ok(Vault {
            id,
            canonical_root: identity.canonical_root,
            enabled: true,
        });
    }
    reject_overlapping_vault(pool, Path::new(&identity.canonical_root)).await?;
    reject_wiki_overlap(pool, Path::new(&identity.canonical_root)).await?;
    let id = identifier("vault")?;
    sqlx::query("INSERT INTO vaults (id, canonical_root, root_device, root_inode, registered_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&id).bind(&identity.canonical_root).bind(identity.device).bind(identity.inode).bind(now)
        .execute(pool).await?;
    super::ownership::claim_legacy_owned_nodes_unlocked(pool, now).await?;
    Ok(Vault {
        id,
        canonical_root: identity.canonical_root,
        enabled: true,
    })
}

pub async fn rebind_vault(
    pool: &SqlitePool,
    vault_id: &str,
    root: &Path,
    confirmed_revisions: &[String],
    now: i64,
) -> anyhow::Result<Vault> {
    let _admission = super::exclusive_admission(pool).await?;
    let identity = platform::verified_root(root)?;
    let previous: String = sqlx::query_scalar("SELECT canonical_root FROM vaults WHERE id = ?")
        .bind(vault_id)
        .fetch_one(pool)
        .await?;
    super::ownership::claim_legacy_owned_nodes_unlocked(pool, now).await?;
    let revisions: Vec<(String, String)> = sqlx::query_as("SELECT r.id, r.sha256 FROM vault_revisions r JOIN vault_documents d ON d.id = r.document_id WHERE d.vault_id = ? AND d.current_revision = r.id AND d.state = 'active'").bind(vault_id).fetch_all(pool).await?;
    // 확인한 revision만 새 위치에서 읽어 해시를 대조한다. 미확인 revision은 막지 않는다 —
    // 설계(0066 §4)대로 grant만 회수해 inactive로 남기고, 다음 스캔·수동 범위 지정이 되살린다.
    // 확인했는데 해시가 다른 것은 사용자가 본 것과 파일이 다르다는 뜻이라 여기서 멈춘다.
    let mut confirmed: Vec<&str> = Vec::new();
    for (id, digest) in &revisions {
        if !confirmed_revisions.contains(id) {
            continue;
        }
        let bytes = super::files::read_revision_at(&identity.canonical_root, id, pool).await?;
        if super::files::hash(&bytes) != *digest {
            anyhow::bail!("vault rebind revision hash changed: {id}")
        }
        confirmed.push(id);
    }
    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE vaults SET canonical_root = ?, root_device = ?, root_inode = ?, enabled = 1, writable = 1 WHERE id = ?").bind(&identity.canonical_root).bind(identity.device).bind(identity.inode).bind(vault_id).execute(&mut *tx).await?;
    for (revision_id, _) in &revisions {
        sqlx::query(
            "UPDATE vault_grants SET revoked_at = ? WHERE revision_id = ? AND revoked_at IS NULL",
        )
        .bind(now)
        .bind(revision_id)
        .execute(&mut *tx)
        .await?;
    }
    for revision_id in &confirmed {
        insert_grant(
            &mut *tx,
            revision_id,
            &ScopeRequest {
                scope: Scope::PrivateData,
            },
            now,
        )
        .await?;
    }
    sqlx::query("INSERT INTO vault_binding_events (id, vault_id, event_kind, previous_root, new_root, created_at) VALUES (?, ?, 'rebound', ?, ?, ?)").bind(identifier("binding")?).bind(vault_id).bind(previous).bind(&identity.canonical_root).bind(now).execute(&mut *tx).await?;
    tx.commit().await?;
    super::ownership::claim_legacy_owned_nodes_unlocked(pool, now).await?;
    Ok(Vault {
        id: vault_id.into(),
        canonical_root: identity.canonical_root,
        enabled: true,
    })
}

async fn reject_overlapping_vault(pool: &SqlitePool, root: &Path) -> anyhow::Result<()> {
    let roots = sqlx::query_scalar::<_, String>("SELECT canonical_root FROM vaults")
        .fetch_all(pool)
        .await?;
    if roots.iter().any(|existing| {
        root == Path::new(existing)
            || root.starts_with(existing)
            || Path::new(existing).starts_with(root)
    }) {
        anyhow::bail!("personal knowledge vault duplicates or nests a registered vault")
    }
    Ok(())
}

async fn reject_wiki_overlap(pool: &SqlitePool, root: &Path) -> anyhow::Result<()> {
    let exists: Option<(i64,)> = sqlx::query_as(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'knowledge_sources'",
    )
    .fetch_optional(pool)
    .await?;
    if exists.is_none() {
        return Ok(());
    }
    for space in crate::knowledge::wiki::spaces(pool).await? {
        let existing = Path::new(&space.root);
        if existing == root || existing.starts_with(root) || root.starts_with(existing) {
            anyhow::bail!("personal knowledge vault overlaps a Wiki root")
        }
    }
    Ok(())
}

pub async fn create_document(
    pool: &SqlitePool,
    draft: &DocumentDraft,
    now: i64,
) -> anyhow::Result<VaultDocument> {
    let id = identifier("document")?;
    sqlx::query("INSERT INTO vault_documents (id, vault_id, kind, title, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&id).bind(&draft.vault_id).bind(&draft.kind).bind(&draft.title).bind(now).execute(pool).await?;
    get_document(pool, &id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("created vault document is missing"))
}

pub async fn list_documents(
    pool: &SqlitePool,
    vault_id: &str,
) -> anyhow::Result<Vec<VaultDocument>> {
    let rows = sqlx::query("SELECT id, vault_id, kind, title, state, current_revision FROM vault_documents WHERE vault_id = ? ORDER BY created_at DESC")
        .bind(vault_id).fetch_all(pool).await?;
    rows.iter().map(document_from_row).collect()
}

pub async fn get_document(pool: &SqlitePool, id: &str) -> anyhow::Result<Option<VaultDocument>> {
    let row = sqlx::query("SELECT id, vault_id, kind, title, state, current_revision FROM vault_documents WHERE id = ?")
        .bind(id).fetch_optional(pool).await?;
    row.as_ref().map(document_from_row).transpose()
}

pub async fn current_revision(
    pool: &SqlitePool,
    document_id: &str,
) -> anyhow::Result<Option<Revision>> {
    let row = sqlx::query("SELECT r.id, r.document_id, r.relative_path, r.sha256, r.size, r.predecessor FROM vault_revisions r JOIN vault_documents d ON d.current_revision = r.id WHERE d.id = ?")
        .bind(document_id).fetch_optional(pool).await?;
    row.as_ref().map(revision_from_row).transpose()
}

pub async fn document_detail(
    pool: &SqlitePool,
    document_id: &str,
) -> anyhow::Result<Option<DocumentDetail>> {
    let Some(document) = get_document(pool, document_id).await? else {
        return Ok(None);
    };
    let rows = sqlx::query("SELECT id, document_id, relative_path, sha256, size, predecessor FROM vault_revisions WHERE document_id = ? ORDER BY created_at DESC")
        .bind(document_id).fetch_all(pool).await?;
    let history = rows
        .iter()
        .map(revision_from_row)
        .collect::<anyhow::Result<Vec<_>>>()?;
    let current_revision = history
        .iter()
        .find(|revision| Some(&revision.id) == document.current_revision.as_ref())
        .cloned();
    let source_revisions = match &current_revision { Some(revision) => sqlx::query_scalar("SELECT source_revision_id FROM vault_revision_sources WHERE revision_id = ? ORDER BY source_revision_id").bind(&revision.id).fetch_all(pool).await?, None => Vec::new() };
    let indexed = match &current_revision {
        Some(revision) => {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vault_fts WHERE revision_id = ?")
                .bind(&revision.id)
                .fetch_one(pool)
                .await?
                > 0
        }
        None => false,
    };
    Ok(Some(DocumentDetail {
        document,
        current_revision,
        history,
        source_revisions,
        indexed,
    }))
}

pub async fn archive_document(pool: &SqlitePool, id: &str, archived: bool) -> anyhow::Result<()> {
    let state = if archived { "archived" } else { "active" };
    sqlx::query("UPDATE vault_documents SET state = ? WHERE id = ?")
        .bind(state)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn change_scope(
    pool: &SqlitePool,
    revision_id: &str,
    scope: &ScopeRequest,
    now: i64,
) -> anyhow::Result<()> {
    let sources: Vec<String> = sqlx::query_scalar(
        "SELECT source_revision_id FROM vault_revision_sources WHERE revision_id = ?",
    )
    .bind(revision_id)
    .fetch_all(pool)
    .await?;
    if !sources.is_empty() {
        let available = super::scope_for_sources(pool, &sources)
            .await?
            .ok_or_else(|| anyhow::anyhow!("vault sources are no longer compatible"))?;
        if !super::scope::scope_allows(&available, scope) {
            anyhow::bail!("note scope exceeds its source grants")
        }
    }
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE vault_grants SET revoked_at = ? WHERE revision_id = ? AND revoked_at IS NULL",
    )
    .bind(now)
    .bind(revision_id)
    .execute(&mut *tx)
    .await?;
    insert_grant(&mut *tx, revision_id, scope, now).await?;
    tx.commit().await?;
    Ok(())
}

pub(crate) async fn insert_grant<'e, E>(
    executor: E,
    revision_id: &str,
    request: &ScopeRequest,
    now: i64,
) -> anyhow::Result<()>
where
    E: sqlx::Executor<'e, Database = sqlx::Sqlite>,
{
    let (scope, project_key, binding_epoch) = match &request.scope {
        Scope::PrivateData => ("private-data", None, None),
        Scope::Common => ("common", None, None),
        Scope::Project { key, binding_epoch } => ("project", Some(key), Some(binding_epoch)),
    };
    sqlx::query("INSERT INTO vault_grants (id, revision_id, scope, project_key, binding_epoch, created_at) VALUES (?, ?, ?, ?, ?, ?)")
        .bind(identifier("grant")?).bind(revision_id).bind(scope).bind(project_key).bind(binding_epoch).bind(now).execute(executor).await?;
    Ok(())
}

fn document_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<VaultDocument> {
    Ok(VaultDocument {
        id: row.try_get("id")?,
        vault_id: row.try_get("vault_id")?,
        kind: row.try_get("kind")?,
        title: row.try_get("title")?,
        state: row.try_get("state")?,
        current_revision: row.try_get("current_revision")?,
    })
}
fn revision_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<Revision> {
    Ok(Revision {
        id: row.try_get("id")?,
        document_id: row.try_get("document_id")?,
        relative_path: row.try_get("relative_path")?,
        sha256: row.try_get("sha256")?,
        size: row.try_get("size")?,
        predecessor: row.try_get("predecessor")?,
    })
}

pub(crate) fn identifier(prefix: &str) -> anyhow::Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes)?;
    Ok(format!(
        "{prefix}-{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    ))
}
