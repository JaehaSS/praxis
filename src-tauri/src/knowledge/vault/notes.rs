use sqlx::SqlitePool;
use std::path::{Component, Path};

use super::operations::{commit_operation, prepare_operation, write_operation_file, OperationPlan};
use super::scope::{scope_allows, scope_for_sources, ScopeRequest};

const MAX_NOTE_TITLE_BYTES: usize = 512;
const MAX_NOTE_BODY_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct NoteDraft {
    pub vault_id: String,
    pub title: String,
    pub body: String,
    pub target_document_id: Option<String>,
    pub expected_base: Option<String>,
    pub source_revisions: Vec<String>,
    pub scope: ScopeRequest,
}

#[derive(Debug, Clone)]
pub struct SavedNote {
    pub document_id: String,
    pub revision_id: String,
}

pub async fn create_note(
    pool: &SqlitePool,
    draft: &NoteDraft,
    now: i64,
) -> anyhow::Result<SavedNote> {
    save_note(pool, draft, None, now).await
}

pub async fn editable_note_body(pool: &SqlitePool, revision_id: &str) -> anyhow::Result<String> {
    let bytes = super::files::read_revision(pool, revision_id).await?;
    let body = String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("note is not UTF-8"))?;
    let sources = sqlx::query_scalar(
        "SELECT source_revision_id FROM vault_revision_sources WHERE revision_id = ? ORDER BY source_revision_id",
    )
    .bind(revision_id)
    .fetch_all(pool)
    .await?;
    if sources.is_empty() {
        return Ok(body);
    }
    let relative: String =
        sqlx::query_scalar("SELECT relative_path FROM vault_revisions WHERE id = ?")
            .bind(revision_id)
            .fetch_one(pool)
            .await?;
    let footer = note_content(pool, "", &sources, &[], &relative).await?;
    Ok(body.strip_suffix(&footer).unwrap_or(&body).to_owned())
}

pub(crate) async fn save_note(
    pool: &SqlitePool,
    draft: &NoteDraft,
    proposal_id: Option<String>,
    now: i64,
) -> anyhow::Result<SavedNote> {
    save_note_with_additional(pool, draft, proposal_id, Vec::new(), now).await
}

pub(crate) async fn save_note_with_additional(
    pool: &SqlitePool,
    draft: &NoteDraft,
    proposal_id: Option<String>,
    additional: Vec<super::operations::PlannedRevision>,
    now: i64,
) -> anyhow::Result<SavedNote> {
    let _admission = super::shared_admission(pool).await?;
    validate_draft(draft)?;
    let target = match &draft.target_document_id {
        Some(id) => Some(target_note(pool, draft, id).await?),
        None => None,
    };
    let document_id = target
        .as_ref()
        .map(|target| target.document_id.clone())
        .unwrap_or(super::catalog::identifier("document")?);
    let head = target.as_ref().map(|target| target.head.clone());
    let revision_id = super::catalog::identifier("revision")?;
    let relative = format!("notes/{document_id}/{revision_id}.md");
    let mut persisted_sources = target
        .as_ref()
        .map(|target| target.sources.clone())
        .unwrap_or_default();
    persisted_sources.extend(draft.source_revisions.iter().cloned());
    persisted_sources.sort_unstable();
    persisted_sources.dedup();
    validate_sources(pool, &persisted_sources, &draft.scope).await?;
    validate_additional_scopes(&additional, &draft.scope)?;
    if let Some(target) = &target {
        if !scope_allows(&target.scope, &draft.scope) {
            anyhow::bail!("note scope exceeds its current scope")
        }
    }
    let content = note_content(
        pool,
        &draft.body,
        &persisted_sources,
        &additional,
        &relative,
    )
    .await?;
    let mut sources = persisted_sources;
    sources.extend(
        additional
            .iter()
            .map(|revision| revision.revision_id.clone()),
    );
    sources.sort_unstable();
    sources.dedup();
    let plan = OperationPlan {
        vault_id: draft.vault_id.clone(),
        document_id: document_id.clone(),
        document_title: target.is_none().then(|| draft.title.clone()),
        expected_head: head,
        accepts_drift: false,
        revision_id: revision_id.clone(),
        relative_path: relative,
        content: content.into_bytes(),
        scope: draft.scope.clone(),
        proposal_id: proposal_id.clone(),
        proposal_candidate_hash: proposal_id
            .as_ref()
            .map(|_| super::files::hash(draft.body.as_bytes())),
        sources,
        additional,
    };
    let operation = prepare_operation(pool, &plan, now).await?;
    if plan.additional.is_empty() {
        write_operation_file(pool, &operation, &plan.content).await?;
    } else {
        super::operations::write_operation_files(pool, &operation, &plan).await?;
    }
    commit_operation(pool, &operation.id, now).await?;
    let _ = super::index::index_revision(pool, &revision_id).await;
    for source in &plan.additional {
        let _ = super::index::index_revision(pool, &source.revision_id).await;
    }
    Ok(SavedNote {
        document_id,
        revision_id,
    })
}

fn validate_draft(draft: &NoteDraft) -> anyhow::Result<()> {
    if draft.title.len() > MAX_NOTE_TITLE_BYTES || draft.body.len() > MAX_NOTE_BODY_BYTES {
        anyhow::bail!("note title or body exceeds its size limit")
    }
    if draft.target_document_id.is_some() != draft.expected_base.is_some() {
        anyhow::bail!("note update requires an expected base")
    }
    Ok(())
}

async fn validate_sources(
    pool: &SqlitePool,
    sources: &[String],
    request: &ScopeRequest,
) -> anyhow::Result<()> {
    if sources.is_empty() {
        return Ok(());
    }
    let scope = scope_for_sources(pool, sources)
        .await?
        .ok_or_else(|| anyhow::anyhow!("vault sources are no longer compatible"))?;
    if !scope_allows(&scope, request) {
        anyhow::bail!("note scope exceeds its source grants")
    }
    Ok(())
}

fn validate_additional_scopes(
    additional: &[super::operations::PlannedRevision],
    request: &ScopeRequest,
) -> anyhow::Result<()> {
    for revision in additional {
        if !scope_allows(&revision.scope.scope, request) {
            anyhow::bail!("note scope exceeds its planned source scope")
        }
    }
    Ok(())
}

struct TargetNote {
    document_id: String,
    head: String,
    scope: super::Scope,
    sources: Vec<String>,
}

async fn target_note(
    pool: &SqlitePool,
    draft: &NoteDraft,
    document_id: &str,
) -> anyhow::Result<TargetNote> {
    let row: (String, String, String, Option<String>) = sqlx::query_as(
        "SELECT vault_id, kind, state, current_revision FROM vault_documents WHERE id = ?",
    )
    .bind(document_id)
    .fetch_one(pool)
    .await?;
    let expected = draft
        .expected_base
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("note update requires an expected base"))?;
    if row.0 != draft.vault_id
        || row.1 != "note"
        || row.2 != "active"
        || row.3.as_deref() != Some(expected)
    {
        anyhow::bail!("note target changed; reload it before saving")
    }
    super::files::verify_revision(pool, expected).await?;
    if draft
        .source_revisions
        .iter()
        .any(|source| source == expected)
    {
        anyhow::bail!("a note cannot link its previous head as a source")
    }
    let scope = scope_for_sources(pool, &[expected.into()])
        .await?
        .ok_or_else(|| anyhow::anyhow!("note target is no longer current"))?;
    let sources = sqlx::query_scalar("SELECT source_revision_id FROM vault_revision_sources WHERE revision_id = ? ORDER BY source_revision_id")
        .bind(expected).fetch_all(pool).await?;
    Ok(TargetNote {
        document_id: document_id.into(),
        head: expected.into(),
        scope,
        sources,
    })
}

async fn note_content(
    pool: &SqlitePool,
    body: &str,
    sources: &[String],
    additional: &[super::operations::PlannedRevision],
    relative: &str,
) -> anyhow::Result<String> {
    let mut links = Vec::new();
    for revision_id in sources {
        let row: (String, String) = sqlx::query_as("SELECT d.title, r.relative_path FROM vault_revisions r JOIN vault_documents d ON d.id = r.document_id WHERE r.id = ?")
            .bind(revision_id).fetch_one(pool).await?;
        links.push(row);
    }
    links.extend(additional.iter().map(|revision| {
        (
            revision.document_title.clone(),
            revision.relative_path.clone(),
        )
    }));
    links.sort_unstable_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)));
    if links.is_empty() {
        return Ok(body.into());
    }
    let links = links
        .into_iter()
        .map(|(title, target)| {
            format!(
                "- [{}](<{}>)",
                escape_title(&title),
                escape_path(&relative_link(relative, &target))
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!("{body}\n\n## Sources\n{links}"))
}

fn relative_link(note_relative: &str, target: &str) -> String {
    let note = Path::new(note_relative)
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .filter_map(normal)
        .collect::<Vec<_>>();
    let target = Path::new(target)
        .components()
        .filter_map(normal)
        .collect::<Vec<_>>();
    let shared = note
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    std::iter::repeat_n("..", note.len() - shared)
        .chain(target[shared..].iter().map(String::as_str))
        .collect::<Vec<_>>()
        .join("/")
}

fn normal(component: Component<'_>) -> Option<String> {
    match component {
        Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
        _ => None,
    }
}

fn escape_title(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]")
}

fn escape_path(value: &str) -> String {
    value
        .replace('\\', "%5C")
        .replace('<', "%3C")
        .replace('>', "%3E")
}
