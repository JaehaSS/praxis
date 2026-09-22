use std::path::PathBuf;

use crate::knowledge::vault::{migrate, register_vault, Scope, ScopeRequest};

#[tokio::test]
async fn migration_is_idempotent() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    migrate(&pool).await.unwrap();
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM sqlite_master WHERE name = 'vault_documents'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count.0, 1);
}

#[tokio::test]
async fn private_scope_is_never_available_for_automatic_search() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let vault = register_vault(&pool, &root("private-search"), 1)
        .await
        .unwrap();
    let request = ScopeRequest {
        scope: Scope::PrivateData,
    };
    let source = crate::knowledge::vault::create_text_source(
        &pool,
        &crate::knowledge::vault::TextSourceDraft {
            vault_id: vault.id,
            title: "secret research".into(),
            body: "confidential finding".into(),
            scope: request.clone(),
        },
        2,
    )
    .await
    .unwrap();
    assert!(crate::knowledge::vault::search(
        &pool,
        "confidential",
        &ScopeRequest {
            scope: Scope::Common
        },
        10
    )
    .await
    .unwrap()
    .is_empty());
    let manual = crate::knowledge::vault::search_browse(&pool, "confidential", 0)
        .await
        .unwrap();
    assert_eq!(manual.hits.len(), 1);
    assert_eq!(manual.hits[0].revision_id, source.revision_id);
}

#[tokio::test]
async fn moving_a_project_requires_explicit_rebinding() {
    use crate::knowledge::vault::{register_project, resolve_project};
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let original = root("project-before-move");
    let moved = original.with_extension("moved");
    let _ = std::fs::remove_dir_all(&moved);
    let binding = register_project(&pool, &original, 1).await.unwrap();
    assert_eq!(
        resolve_project(&pool, &original).await.unwrap().unwrap(),
        binding
    );
    std::fs::rename(&original, &moved).unwrap();
    assert!(resolve_project(&pool, &moved).await.unwrap().is_none());
}

#[tokio::test]
async fn derived_note_cannot_widen_private_source_scope() {
    use crate::knowledge::vault::{
        change_scope, create_note, create_text_source, scope_for_sources, NoteDraft,
        TextSourceDraft,
    };
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let vault = register_vault(&pool, &root("derived-scope"), 1)
        .await
        .unwrap();
    let scope = ScopeRequest {
        scope: Scope::PrivateData,
    };
    let source = create_text_source(
        &pool,
        &TextSourceDraft {
            vault_id: vault.id.clone(),
            title: "research".into(),
            body: "private evidence".into(),
            scope: scope.clone(),
        },
        2,
    )
    .await
    .unwrap();
    let note = create_note(
        &pool,
        &NoteDraft {
            vault_id: vault.id,
            title: "conclusion".into(),
            body: "derived knowledge".into(),
            target_document_id: None,
            expected_base: None,
            source_revisions: vec![source.revision_id],
            scope,
        },
        3,
    )
    .await
    .unwrap();
    assert!(change_scope(
        &pool,
        &note.revision_id,
        &ScopeRequest {
            scope: Scope::Common
        },
        4
    )
    .await
    .is_err());
    assert_eq!(
        scope_for_sources(&pool, &[note.revision_id]).await.unwrap(),
        Some(Scope::PrivateData)
    );
}

#[tokio::test]
async fn second_writable_vault_requires_explicit_disconnect() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let first = root("first");
    let second = root("second");
    let vault = register_vault(&pool, &first, 1).await.unwrap();
    assert!(register_vault(&pool, &second, 2).await.is_err());
    let active: String =
        sqlx::query_scalar("SELECT id FROM vaults WHERE enabled = 1 AND writable = 1")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(active, vault.id);
}

#[tokio::test]
async fn reconnecting_a_disconnected_root_preserves_its_identity() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let path = root("reconnect");
    let vault = register_vault(&pool, &path, 1).await.unwrap();
    sqlx::query("UPDATE vaults SET enabled = 0 WHERE id = ?")
        .bind(&vault.id)
        .execute(&pool)
        .await
        .unwrap();
    let reconnected = register_vault(&pool, &path, 2).await.unwrap();
    assert_eq!(reconnected.id, vault.id);
    assert!(reconnected.enabled);
}

#[tokio::test]
async fn vault_rebind_revokes_existing_grants_to_private_data() {
    use crate::knowledge::vault::{create_text_source, rebind_vault, TextSourceDraft};
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let original = root("rebind-original");
    let vault = register_vault(&pool, &original, 1).await.unwrap();
    let source = create_text_source(
        &pool,
        &TextSourceDraft {
            vault_id: vault.id.clone(),
            title: "source".into(),
            body: "body".into(),
            scope: ScopeRequest {
                scope: Scope::Common,
            },
        },
        2,
    )
    .await
    .unwrap();
    let rebound = original.with_extension("rebound");
    let _ = std::fs::remove_dir_all(&rebound);
    std::fs::rename(&original, &rebound).unwrap();
    rebind_vault(
        &pool,
        &vault.id,
        &rebound,
        std::slice::from_ref(&source.revision_id),
        3,
    )
        .await
        .unwrap();
    let active: (String,) = sqlx::query_as(
        "SELECT scope FROM vault_grants WHERE revision_id = ? AND revoked_at IS NULL",
    )
    .bind(&source.revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(active.0, "private-data");
    let revoked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM vault_grants WHERE revision_id = ? AND revoked_at IS NOT NULL",
    )
    .bind(&source.revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(revoked, 1);
}

#[tokio::test]
async fn vault_rebind_leaves_unconfirmed_revisions_without_grants_instead_of_failing() {
    // 설계(0066 §4): 미선택 자료는 inactive로 남는다. 전부 확인해야만 통과하는 것이 아니다.
    use crate::knowledge::vault::{create_text_source, rebind_vault, TextSourceDraft};
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let original = root("rebind-partial-original");
    let vault = register_vault(&pool, &original, 1).await.unwrap();
    let draft = |title: &str| TextSourceDraft {
        vault_id: vault.id.clone(),
        title: title.into(),
        body: format!("body of {title}"),
        scope: ScopeRequest {
            scope: Scope::Common,
        },
    };
    let confirmed = create_text_source(&pool, &draft("confirmed"), 2)
        .await
        .unwrap();
    let skipped = create_text_source(&pool, &draft("skipped"), 3)
        .await
        .unwrap();
    let rebound = original.with_extension("rebound");
    let _ = std::fs::remove_dir_all(&rebound);
    std::fs::rename(&original, &rebound).unwrap();
    let vault = rebind_vault(
        &pool,
        &vault.id,
        &rebound,
        std::slice::from_ref(&confirmed.revision_id),
        4,
    )
    .await
    .unwrap();
    assert!(vault.enabled);
    let active_scope = |revision: String| {
        let pool = pool.clone();
        async move {
            sqlx::query_scalar::<_, String>(
                "SELECT scope FROM vault_grants WHERE revision_id = ? AND revoked_at IS NULL",
            )
            .bind(revision)
            .fetch_optional(&pool)
            .await
            .unwrap()
        }
    };
    assert_eq!(
        active_scope(confirmed.revision_id.clone()).await.as_deref(),
        Some("private-data")
    );
    assert_eq!(active_scope(skipped.revision_id.clone()).await, None);
    let revoked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM vault_grants WHERE revision_id = ? AND revoked_at IS NOT NULL",
    )
    .bind(&skipped.revision_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(revoked, 1);
}

#[tokio::test]
async fn vault_rebind_rejects_a_confirmed_revision_whose_file_changed() {
    use crate::knowledge::vault::{create_text_source, rebind_vault, TextSourceDraft};
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let original = root("rebind-drift-original");
    let vault = register_vault(&pool, &original, 1).await.unwrap();
    let source = create_text_source(
        &pool,
        &TextSourceDraft {
            vault_id: vault.id.clone(),
            title: "source".into(),
            body: "body".into(),
            scope: ScopeRequest {
                scope: Scope::Common,
            },
        },
        2,
    )
    .await
    .unwrap();
    let relative: String =
        sqlx::query_scalar("SELECT relative_path FROM vault_revisions WHERE id = ?")
            .bind(&source.revision_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let rebound = original.with_extension("rebound");
    let _ = std::fs::remove_dir_all(&rebound);
    std::fs::rename(&original, &rebound).unwrap();
    std::fs::write(rebound.join(&relative), "edited elsewhere").unwrap();
    let error = rebind_vault(
        &pool,
        &vault.id,
        &rebound,
        std::slice::from_ref(&source.revision_id),
        3,
    )
    .await
    .unwrap_err();
    assert!(error.to_string().contains("hash changed"), "{error}");
    let enabled: i64 = sqlx::query_scalar("SELECT enabled FROM vaults WHERE id = ?")
        .bind(&vault.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(enabled, 1, "identity must stay untouched when the rebind is rejected");
}

#[tokio::test]
async fn invalid_project_rebind_keeps_the_existing_binding_active() {
    use crate::knowledge::vault::{rebind_project, register_project, resolve_project};
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let original = root("project-rebind-original");
    let binding = register_project(&pool, &original, 1).await.unwrap();
    assert!(
        rebind_project(&pool, &binding.id, &original.join("missing"), 2)
            .await
            .is_err()
    );
    assert_eq!(
        resolve_project(&pool, &original).await.unwrap(),
        Some(binding)
    );
}

fn root(name: &str) -> PathBuf {
    let path = crate::testtmp::dir().join(format!("vault-catalog-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}
