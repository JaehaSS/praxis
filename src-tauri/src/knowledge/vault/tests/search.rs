use std::path::PathBuf;

use crate::knowledge::vault::{
    create_text_source, migrate, register_project, register_vault, scan_vault, scope_for_sources,
    search, search_browse, Scope, ScopeRequest, TextSourceDraft,
};

fn root(name: &str) -> PathBuf {
    let path = crate::testtmp::dir().join(format!("vault-search-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

async fn source(
    pool: &sqlx::SqlitePool,
    vault_id: &str,
    title: &str,
    scope: Scope,
    now: i64,
) -> crate::knowledge::vault::ImportedFile {
    create_text_source(
        pool,
        &TextSourceDraft {
            vault_id: vault_id.into(),
            title: title.into(),
            body: "자료 검색 내용".into(),
            scope: ScopeRequest { scope },
        },
        now,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn automatic_search_skips_more_than_one_page_of_denied_hits() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = root("acl-page");
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let binding = register_project(&pool, &root, 2).await.unwrap();
    for index in 0..101 {
        source(
            &pool,
            &vault.id,
            &format!("shared phrase {index:03}"),
            Scope::PrivateData,
            index + 3,
        )
        .await;
    }
    let allowed = source(
        &pool,
        &vault.id,
        "shared phrase",
        Scope::Project {
            key: binding.id.clone(),
            binding_epoch: binding.epoch.clone(),
        },
        4,
    )
    .await;
    let hits = search(
        &pool,
        "shared phrase",
        &ScopeRequest {
            scope: Scope::Project {
                key: binding.id,
                binding_epoch: binding.epoch,
            },
        },
        1,
    )
    .await
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].revision_id, allowed.revision_id);
}

#[tokio::test]
async fn short_queries_browse_private_data_and_zero_limit_returns_empty() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let vault = register_vault(&pool, &root("short-query"), 1)
        .await
        .unwrap();
    let private = source(&pool, &vault.id, "AI 자료", Scope::PrivateData, 2).await;
    let browse = search_browse(&pool, "자료", 0).await.unwrap();
    assert_eq!(browse.hits[0].revision_id, private.revision_id);
    assert!(search(
        &pool,
        "AI",
        &ScopeRequest {
            scope: Scope::Common
        },
        0
    )
    .await
    .unwrap()
    .is_empty());
    assert!(search(
        &pool,
        "자료",
        &ScopeRequest {
            scope: Scope::Common
        },
        10
    )
    .await
    .unwrap()
    .is_empty());
}

#[tokio::test]
async fn browse_pages_are_stable_at_one_hundred_results() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let vault = register_vault(&pool, &root("browse-pages"), 1)
        .await
        .unwrap();
    for index in 0..101 {
        source(
            &pool,
            &vault.id,
            &format!("pagination 자료 {index:03}"),
            Scope::PrivateData,
            index + 2,
        )
        .await;
    }
    let first = search_browse(&pool, "자료", 0).await.unwrap();
    let second = search_browse(&pool, "자료", 100).await.unwrap();
    assert_eq!(first.hits.len(), 100);
    assert!(first.has_more);
    assert_eq!(second.hits.len(), 1);
    assert!(!second.has_more);
    assert_ne!(first.hits[0].revision_id, second.hits[0].revision_id);
}

#[tokio::test]
async fn scope_detects_cycles_and_enforces_the_sixty_four_node_boundary() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let vault = register_vault(&pool, &root("scope-graph"), 1)
        .await
        .unwrap();
    let mut revisions = Vec::new();
    for index in 0..65 {
        revisions.push(
            source(
                &pool,
                &vault.id,
                &format!("source {index}"),
                Scope::PrivateData,
                index + 2,
            )
            .await
            .revision_id,
        );
    }
    for pair in revisions.windows(2) {
        sqlx::query(
            "INSERT INTO vault_revision_sources (revision_id, source_revision_id) VALUES (?, ?)",
        )
        .bind(&pair[0])
        .bind(&pair[1])
        .execute(&pool)
        .await
        .unwrap();
    }
    assert_eq!(
        scope_for_sources(&pool, &revisions[1..]).await.unwrap(),
        Some(Scope::PrivateData)
    );
    assert!(scope_for_sources(&pool, &[revisions[0].clone()])
        .await
        .is_err());
    sqlx::query(
        "INSERT INTO vault_revision_sources (revision_id, source_revision_id) VALUES (?, ?)",
    )
    .bind(&revisions[64])
    .bind(&revisions[63])
    .execute(&pool)
    .await
    .unwrap();
    assert!(scope_for_sources(&pool, &[revisions[63].clone()])
        .await
        .is_err());
}

#[tokio::test]
async fn moved_project_binding_revokes_project_source_admission() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let vault = register_vault(&pool, &root("binding-vault"), 1)
        .await
        .unwrap();
    let original = root("binding-original");
    let binding = register_project(&pool, &original, 2).await.unwrap();
    let source = source(
        &pool,
        &vault.id,
        "project source",
        Scope::Project {
            key: binding.id.clone(),
            binding_epoch: binding.epoch.clone(),
        },
        3,
    )
    .await;
    assert!(scope_for_sources(&pool, std::slice::from_ref(&source.revision_id))
        .await
        .unwrap()
        .is_some());
    let moved = original.with_extension("moved");
    let _ = std::fs::remove_dir_all(&moved);
    std::fs::rename(&original, moved).unwrap();
    assert_eq!(
        scope_for_sources(&pool, &[source.revision_id])
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn projection_layout_migration_only_rebuilds_derived_rows_once() {
    use super::super::index::{index_revision, migrate_projection_layout, rebuild_needed};

    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let vault = register_vault(&pool, &root("projection-layout"), 1)
        .await
        .unwrap();
    let source = source(&pool, &vault.id, "projection", Scope::PrivateData, 2).await;
    sqlx::query("UPDATE vault_index_layout SET version = 0 WHERE id = 1")
        .execute(&pool)
        .await
        .unwrap();
    assert!(migrate_projection_layout(&pool).await.unwrap());
    assert!(rebuild_needed(&pool).await.unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vault_documents")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vault_fts")
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    index_revision(&pool, &source.revision_id).await.unwrap();
    index_revision(&pool, &source.revision_id).await.unwrap();
    assert!(!migrate_projection_layout(&pool).await.unwrap());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vault_fts")
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn complete_scan_repairs_index_and_tracks_drift_or_missing_without_partial_deletes() {
    use super::super::scan::{scan_vault_with_limits, ScanLimits};

    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = root("refresh");
    std::fs::write(root.join("tracked.txt"), "first").unwrap();
    std::fs::write(root.join(".env.local"), "excluded").unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let first = scan_vault(
        &pool,
        &vault.id,
        ScopeRequest {
            scope: Scope::PrivateData,
        },
        2,
    )
    .await
    .unwrap();
    assert_eq!(first.indexed, 1);
    let document_id: String =
        sqlx::query_scalar("SELECT id FROM vault_documents WHERE title = 'tracked'")
            .fetch_one(&pool)
            .await
            .unwrap();
    let revision_id: String =
        sqlx::query_scalar("SELECT current_revision FROM vault_documents WHERE id = ?")
            .bind(&document_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    sqlx::query("DELETE FROM vault_fts WHERE revision_id = ?")
        .bind(&revision_id)
        .execute(&pool)
        .await
        .unwrap();
    scan_vault(
        &pool,
        &vault.id,
        ScopeRequest {
            scope: Scope::PrivateData,
        },
        3,
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vault_fts WHERE revision_id = ?")
            .bind(&revision_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        1
    );
    std::fs::write(root.join("tracked.txt"), "changed").unwrap();
    scan_vault(
        &pool,
        &vault.id,
        ScopeRequest {
            scope: Scope::PrivateData,
        },
        4,
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM vault_documents WHERE id = ?")
            .bind(&document_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        "drifted"
    );
    std::fs::remove_file(root.join("tracked.txt")).unwrap();
    let partial = scan_vault_with_limits(
        &pool,
        &vault.id,
        ScopeRequest {
            scope: Scope::PrivateData,
        },
        &[],
        5,
        ScanLimits {
            max_files: 0,
            max_text_bytes: 1,
        },
    )
    .await
    .unwrap();
    assert!(partial.partial);
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM vault_documents WHERE id = ?")
            .bind(&document_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        "drifted"
    );
    scan_vault(
        &pool,
        &vault.id,
        ScopeRequest {
            scope: Scope::PrivateData,
        },
        6,
    )
    .await
    .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT state FROM vault_documents WHERE id = ?")
            .bind(&document_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        "missing"
    );
}
