#![cfg(target_os = "macos")]

use crate::knowledge::vault::{
    create_note, create_text_source, current_revision, read_revision, recover_operations,
    register_vault, NoteDraft, Scope, ScopeRequest, TextSourceDraft,
};

use super::super::catalog::identifier;
use super::super::operations::{prepare_operation, write_operation_file, OperationPlan};

#[tokio::test]
async fn note_update_requires_current_base_and_carries_source_ancestry() {
    let pool = crate::knowledge::tests::test_pool().await;
    crate::knowledge::migrate(&pool).await.unwrap();
    let root = crate::testtmp::dir().join(format!("vault-note-update-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let scope = ScopeRequest {
        scope: Scope::PrivateData,
    };
    let first_source = source(&pool, &vault.id, "first", 2).await;
    let note = create_note(
        &pool,
        &NoteDraft {
            vault_id: vault.id.clone(),
            title: "Note".into(),
            body: "first note".into(),
            target_document_id: None,
            expected_base: None,
            source_revisions: vec![first_source.revision_id.clone()],
            scope: scope.clone(),
        },
        3,
    )
    .await
    .unwrap();
    let old = note.revision_id;
    let old_bytes = read_revision(&pool, &old).await.unwrap();
    let second_source = source(&pool, &vault.id, "second", 4).await;
    let updated = create_note(
        &pool,
        &NoteDraft {
            vault_id: vault.id.clone(),
            title: "ignored for target".into(),
            body: "updated note".into(),
            target_document_id: Some(note.document_id.clone()),
            expected_base: Some(old.clone()),
            source_revisions: vec![second_source.revision_id.clone()],
            scope: scope.clone(),
        },
        5,
    )
    .await
    .unwrap();
    assert_ne!(updated.revision_id, old);
    assert_eq!(read_revision(&pool, &old).await.unwrap(), old_bytes);
    assert_eq!(
        String::from_utf8(read_revision(&pool, &updated.revision_id).await.unwrap())
            .unwrap()
            .matches("## Sources")
            .count(),
        1
    );
    let sources: Vec<String> = sqlx::query_scalar("SELECT source_revision_id FROM vault_revision_sources WHERE revision_id = ? ORDER BY source_revision_id")
        .bind(&updated.revision_id)
        .fetch_all(&pool)
        .await
        .unwrap();
    let mut expected_sources = vec![first_source.revision_id, second_source.revision_id];
    expected_sources.sort_unstable();
    assert_eq!(sources, expected_sources);
    let files_before = file_count(&root);
    assert!(create_note(
        &pool,
        &NoteDraft {
            vault_id: vault.id,
            title: "Note".into(),
            body: "stale".into(),
            target_document_id: Some(note.document_id.clone()),
            expected_base: Some(old),
            source_revisions: vec![],
            scope,
        },
        6,
    )
    .await
    .is_err());
    assert_eq!(file_count(&root), files_before);
    assert_eq!(
        current_revision(&pool, &note.document_id)
            .await
            .unwrap()
            .unwrap()
            .id,
        updated.revision_id
    );
}

#[tokio::test]
async fn rejected_new_note_does_not_create_a_document() {
    let pool = crate::knowledge::tests::test_pool().await;
    crate::knowledge::migrate(&pool).await.unwrap();
    let root = crate::testtmp::dir().join(format!("vault-note-reject-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let source = source(&pool, &vault.id, "private", 2).await;
    let result = create_note(
        &pool,
        &NoteDraft {
            vault_id: vault.id,
            title: "rejected".into(),
            body: "body".into(),
            target_document_id: None,
            expected_base: None,
            source_revisions: vec![source.revision_id],
            scope: ScopeRequest {
                scope: Scope::Common,
            },
        },
        3,
    )
    .await;
    assert!(result.is_err());
    assert_eq!(document_count(&pool).await, 1);
    assert_eq!(file_count(&root), 1);
}

#[tokio::test]
async fn recovery_conflicts_when_a_note_source_is_revoked() {
    let pool = crate::knowledge::tests::test_pool().await;
    crate::knowledge::migrate(&pool).await.unwrap();
    let root = crate::testtmp::dir().join(format!("vault-note-recovery-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let scope = ScopeRequest {
        scope: Scope::PrivateData,
    };
    let source = source(&pool, &vault.id, "source", 2).await;
    let note = create_note(
        &pool,
        &NoteDraft {
            vault_id: vault.id.clone(),
            title: "Note".into(),
            body: "before".into(),
            target_document_id: None,
            expected_base: None,
            source_revisions: vec![source.revision_id.clone()],
            scope: scope.clone(),
        },
        3,
    )
    .await
    .unwrap();
    let plan = OperationPlan::new(
        vault.id,
        note.document_id.clone(),
        Some(note.revision_id.clone()),
        identifier("revision").unwrap(),
        format!("notes/{}/recovery.md", note.document_id),
        b"after".to_vec(),
        scope,
    );
    let mut plan = plan;
    plan.sources = vec![source.revision_id.clone()];
    let operation = prepare_operation(&pool, &plan, 4).await.unwrap();
    write_operation_file(&pool, &operation, &plan.content)
        .await
        .unwrap();
    sqlx::query("UPDATE vault_grants SET revoked_at = 5 WHERE revision_id = ?")
        .bind(&source.revision_id)
        .execute(&pool)
        .await
        .unwrap();
    let report = recover_operations(&pool, 6).await.unwrap();
    assert_eq!(report.recovered, 0);
    assert_eq!(report.conflicts, 1);
    assert_eq!(
        current_revision(&pool, &note.document_id)
            .await
            .unwrap()
            .unwrap()
            .id,
        note.revision_id
    );
}

async fn source(
    pool: &sqlx::SqlitePool,
    vault_id: &str,
    title: &str,
    now: i64,
) -> crate::knowledge::vault::ImportedFile {
    create_text_source(
        pool,
        &TextSourceDraft {
            vault_id: vault_id.into(),
            title: title.into(),
            body: title.into(),
            scope: ScopeRequest {
                scope: Scope::PrivateData,
            },
        },
        now,
    )
    .await
    .unwrap()
}

fn file_count(root: &std::path::Path) -> usize {
    std::fs::read_dir(root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .map(|path| if path.is_dir() { file_count(&path) } else { 1 })
        .sum()
}

async fn document_count(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM vault_documents")
        .fetch_one(pool)
        .await
        .unwrap()
}
