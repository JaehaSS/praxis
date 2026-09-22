#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;

use crate::knowledge::vault::provenance::{
    grant_consent, record_revision_input, record_terminal_snapshot, record_user_input,
    start_attempt,
};
use crate::knowledge::vault::retrieval::{consume_preview, create_preview};
use crate::knowledge::vault::usage::{mark_delivery, record_pending, DeliveryState};
use crate::knowledge::vault::{
    create_note, current_revision, index_revision, migrate, read_revision, rebind_vault,
    recover_operations, register_project, register_vault, PlannedRevision, Scope, ScopeRequest,
    TextSourceDraft,
};

use super::super::operations::{prepare_operation, write_operation_file, OperationPlan};
use super::super::{create_text_source, import_file, ImportRequest, NoteDraft};

#[tokio::test]
async fn durable_vault_lifecycle_preserves_sources_notes_and_completion() {
    let (pool, database) = pool().await;
    let root = directory("integration-vault");
    let project = directory("integration-project");
    let other_project = directory("integration-other-project");
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let binding = register_project(&pool, &project, 1).await.unwrap();
    let other_binding = register_project(&pool, &other_project, 1).await.unwrap();
    seed_memory_history(&pool).await;
    let scope = project_scope(&binding);
    let source = imported(&pool, &vault.id, &project, "project", scope.clone(), 2).await;
    let private = create_text_source(
        &pool,
        &TextSourceDraft {
            vault_id: vault.id.clone(),
            title: "private".into(),
            body: "private facts".into(),
            scope: ScopeRequest {
                scope: Scope::PrivateData,
            },
        },
        3,
    )
    .await
    .unwrap();
    let note = create_note(
        &pool,
        &NoteDraft {
            vault_id: vault.id.clone(),
            title: "linked note".into(),
            body: "project summary".into(),
            target_document_id: None,
            expected_base: None,
            source_revisions: vec![source.revision_id.clone()],
            scope: scope.clone(),
        },
        4,
    )
    .await
    .unwrap();
    let (snapshot, attempt) = done_snapshot(&pool, &vault.id, &binding, &project).await;
    // 제안 파이프라인이 사라진 뒤에도 완료 스냅샷은 남는다 — 출처 기록의 원천이다.
    assert!(!snapshot.is_empty());
    let preview = create_preview(&pool, &binding, "project", "integration", 10)
        .await
        .unwrap();
    assert!(!preview.references.is_empty());
    assert!(
        create_preview(&pool, &other_binding, "project", "other", 10)
            .await
            .unwrap()
            .references
            .is_empty()
    );
    let consumed = consume_preview(&pool, &binding, "project", "integration", 1)
        .await
        .unwrap()
        .unwrap();
    let reference = &consumed.references[0];
    record_revision_input(
        &pool,
        &attempt,
        &source.document.id,
        &reference.revision_id,
        &reference.revision_hash,
        10,
    )
    .await
    .unwrap();
    record_pending(&pool, 1, &attempt, &consumed, 10)
        .await
        .unwrap();
    mark_delivery(&pool, 1, &attempt, DeliveryState::Delivered, 10)
        .await
        .unwrap();

    let old_note = current_revision(&pool, &note.document_id)
        .await
        .unwrap()
        .unwrap();
    let old_bytes = read_revision(&pool, &old_note.id).await.unwrap();
    let hashes = current_hashes(&pool).await;
    let durable = durable_rows(&pool).await;
    let revisions = current_ids(&pool).await;
    pool.close().await;
    let reopened = crate::db::init_pool(&database).await.unwrap();
    crate::knowledge::migrate(&reopened).await.unwrap();
    migrate(&reopened).await.unwrap();
    sqlx::query("DELETE FROM vault_fts")
        .execute(&reopened)
        .await
        .unwrap();
    for revision in &revisions {
        index_revision(&reopened, revision).await.unwrap();
    }
    assert_eq!(current_hashes(&reopened).await, hashes);
    assert_eq!(durable_rows(&reopened).await, durable);
    assert_eq!(
        read_revision(&reopened, &old_note.id).await.unwrap(),
        old_bytes
    );
    let second = crate::db::init_pool(&database).await.unwrap();
    let updated = update_note(&second, &note.document_id, &old_note.id, scope.clone(), 10)
        .await
        .unwrap();
    assert!(update_note(
        &reopened,
        &note.document_id,
        &old_note.id,
        scope.clone(),
        10
    )
    .await
    .is_err());
    assert_ne!(updated.revision_id, old_note.id);
    assert_eq!(
        read_revision(&reopened, &old_note.id).await.unwrap(),
        old_bytes
    );
    let rebound = directory("integration-rebound");
    copy_current_files(&reopened, &root, &rebound).await;
    let active = active_ids(&reopened).await;
    rebind_vault(&reopened, &vault.id, &rebound, &active, 11)
        .await
        .unwrap();
    let (operation, note_id, source_id) = partial_operation(&reopened, &vault.id).await;
    assert_eq!(
        recover_operations(&reopened, 12).await.unwrap().conflicts,
        1
    );
    assert_eq!(operation_state(&reopened, &operation).await, "conflict");
    assert_eq!(current_head(&reopened, &note_id).await, None);
    assert_eq!(current_head(&reopened, &source_id).await, None);
    assert_eq!(private.document.vault_id, vault.id);
}

async fn pool() -> (SqlitePool, String) {
    let pool = crate::knowledge::tests::raw_pool().await;
    let database = sqlx::query_scalar("SELECT file FROM pragma_database_list WHERE name = 'main'")
        .fetch_one(&pool)
        .await
        .unwrap();
    crate::knowledge::migrate(&pool).await.unwrap();
    migrate(&pool).await.unwrap();
    (pool, database)
}

async fn imported(
    pool: &SqlitePool,
    vault_id: &str,
    dir: &Path,
    title: &str,
    scope: ScopeRequest,
    now: i64,
) -> crate::knowledge::vault::ImportedFile {
    let path = dir.join("source.txt");
    std::fs::write(&path, "project reference").unwrap();
    import_file(
        pool,
        &ImportRequest {
            vault_id: vault_id.into(),
            source: path,
            title: title.into(),
            scope,
        },
        now,
    )
    .await
    .unwrap()
}

async fn done_snapshot(
    pool: &SqlitePool,
    vault_id: &str,
    binding: &crate::knowledge::vault::ProjectBinding,
    root: &Path,
) -> (String, String) {
    let root = root.to_string_lossy().to_string();
    sqlx::query("INSERT INTO tasks (id, repo, branch, base, worktree_path, instruction, state, created_at, updated_at) VALUES (1, ?, 'main', 'base', ?, 'project', 'done', 1, 1)")
        .bind(&root).bind(&root).execute(pool).await.unwrap();
    let provider =
        crate::capture::invoke::provider_identity(&crate::capture::invoke::profile(pool).await);
    grant_consent(pool, binding, &provider, 5).await.unwrap();
    let attempt = start_attempt(pool, 1, vault_id, binding, &provider, None, 6)
        .await
        .unwrap();
    record_user_input(pool, 1, "project", 7).await.unwrap();
    let snapshot = record_terminal_snapshot(pool, 1, "done", "completion source", 7)
        .await
        .unwrap()
        .unwrap();
    (snapshot, attempt)
}

fn directory(name: &str) -> PathBuf {
    let path = crate::testtmp::dir().join(format!("vault-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn project_scope(binding: &crate::knowledge::vault::ProjectBinding) -> ScopeRequest {
    ScopeRequest {
        scope: Scope::Project {
            key: binding.id.clone(),
            binding_epoch: binding.epoch.clone(),
        },
    }
}

async fn current_ids(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar(
        "SELECT current_revision FROM vault_documents WHERE current_revision IS NOT NULL",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn active_ids(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar("SELECT current_revision FROM vault_documents WHERE state = 'active' AND current_revision IS NOT NULL")
        .fetch_all(pool).await.unwrap()
}

async fn current_hashes(pool: &SqlitePool) -> Vec<String> {
    sqlx::query_scalar("SELECT r.sha256 FROM vault_documents d JOIN vault_revisions r ON r.id = d.current_revision ORDER BY r.id")
        .fetch_all(pool).await.unwrap()
}

async fn seed_memory_history(pool: &SqlitePool) {
    let node: i64 = sqlx::query_scalar("INSERT INTO knowledge_nodes (source, external_id, kind, title, content_hash, updated_at, synced_at) VALUES ('test', 'integration-memory', 'note', 'memory', 'old-hash', 1, 1) RETURNING id")
        .fetch_one(pool).await.unwrap();
    sqlx::query("INSERT INTO knowledge_chunks (node_id, ord, doc_title, heading, content) VALUES (?, 0, 'memory', 'history', 'old memory revision')")
        .bind(node).execute(pool).await.unwrap();
}

async fn durable_rows(pool: &SqlitePool) -> Vec<(String, Vec<String>)> {
    let tables = [
        "knowledge_nodes",
        "knowledge_chunks",
        "vaults",
        "vault_documents",
        "vault_revisions",
        "vault_grants",
        "vault_revision_sources",
        "vault_document_events",
        "vault_binding_events",
        "vault_operations",
        "vault_operation_files",
        "vault_task_attempts",
        "vault_attempt_inputs",
        "vault_input_snapshots",
        "vault_terminal_snapshots",
        "vault_reference_previews",
        "vault_reference_preview_items",
        "vault_usages",
    ];
    let mut snapshots = Vec::with_capacity(tables.len());
    for table in tables {
        let columns: Vec<String> = sqlx::query_scalar(&format!(
            "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
        ))
        .fetch_all(pool)
        .await
        .unwrap();
        let values = columns
            .iter()
            .map(|column| format!("quote(\"{column}\")"))
            .collect::<Vec<_>>()
            .join(" || '|' || ");
        let rows = sqlx::query_scalar(&format!("SELECT {values} FROM {table} ORDER BY rowid"))
            .fetch_all(pool)
            .await
            .unwrap();
        snapshots.push((table.into(), rows));
    }
    snapshots
}

async fn copy_current_files(pool: &SqlitePool, from: &Path, to: &Path) {
    for path in sqlx::query_scalar::<_, String>("SELECT r.relative_path FROM vault_documents d JOIN vault_revisions r ON r.id = d.current_revision WHERE d.state = 'active'")
        .fetch_all(pool).await.unwrap() {
        let target = to.join(&path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(from.join(path), target).unwrap();
    }
}

async fn update_note(
    pool: &SqlitePool,
    document_id: &str,
    expected_base: &str,
    scope: ScopeRequest,
    now: i64,
) -> anyhow::Result<crate::knowledge::vault::SavedNote> {
    create_note(
        pool,
        &NoteDraft {
            vault_id: sqlx::query_scalar("SELECT vault_id FROM vault_documents WHERE id = ?")
                .bind(document_id)
                .fetch_one(pool)
                .await
                .unwrap(),
            title: "linked note".into(),
            body: "updated project summary".into(),
            target_document_id: Some(document_id.into()),
            expected_base: Some(expected_base.into()),
            source_revisions: vec![],
            scope,
        },
        now,
    )
    .await
}

async fn partial_operation(pool: &SqlitePool, vault_id: &str) -> (String, String, String) {
    let note_id = "integration-partial-note".to_string();
    let source_id = "integration-partial-source".to_string();
    let mut plan = OperationPlan::new(
        vault_id.into(),
        note_id.clone(),
        None,
        "integration-partial-note-revision".into(),
        "notes/integration-partial.md".into(),
        b"partial note".to_vec(),
        ScopeRequest {
            scope: Scope::PrivateData,
        },
    );
    plan.document_title = Some("partial note".into());
    plan.additional.push(PlannedRevision {
        document_id: source_id.clone(),
        vault_id: vault_id.into(),
        document_title: "completion source".into(),
        revision_id: "integration-partial-source-revision".into(),
        relative_path: "sources/integration-partial.txt".into(),
        content: b"completion".to_vec(),
        scope: ScopeRequest {
            scope: Scope::PrivateData,
        },
        sources: vec![],
    });
    let operation = prepare_operation(pool, &plan, 12).await.unwrap();
    write_operation_file(pool, &operation, &plan.content)
        .await
        .unwrap();
    (operation.id, note_id, source_id)
}

async fn operation_state(pool: &SqlitePool, id: &str) -> String {
    sqlx::query_scalar("SELECT state FROM vault_operations WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn current_head(pool: &SqlitePool, id: &str) -> Option<String> {
    sqlx::query_scalar("SELECT current_revision FROM vault_documents WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}
