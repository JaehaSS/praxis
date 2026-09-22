use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::knowledge::vault::{
    create_text_source, create_url_source, current_revision, import_file, migrate, read_revision,
    register_vault, scan_vault_with_exclusions, ImportRequest, Scope, ScopeRequest, TextSourceDraft,
    UrlSourceDraft,
};

fn directory(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let path = crate::testtmp::dir().join(format!(
        "vault-{name}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn private_scope() -> ScopeRequest {
    ScopeRequest {
        scope: Scope::PrivateData,
    }
}

#[tokio::test]
async fn imports_external_file_once_without_overwriting() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("copy-root");
    let external = directory("copy-external").join("source.txt");
    std::fs::write(&external, "original bytes").unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let imported = import_file(
        &pool,
        &ImportRequest {
            vault_id: vault.id,
            source: external,
            title: "source".into(),
            scope: private_scope(),
        },
        2,
    )
    .await
    .unwrap();
    assert_eq!(
        read_revision(&pool, &imported.revision_id).await.unwrap(),
        b"original bytes"
    );
    let target = root.join("sources").join(&imported.document.id);
    assert!(target.exists());
}

#[tokio::test]
async fn duplicate_import_reuses_the_existing_revision() {
    let pool = crate::knowledge::tests::raw_pool().await;
    crate::knowledge::migrate(&pool).await.unwrap();
    let root = directory("duplicate-root");
    let source = directory("duplicate-source").join("source.bin");
    let bytes = (0..3 * 1024 * 1024)
        .map(|index| (index % 251) as u8)
        .collect::<Vec<_>>();
    std::fs::write(&source, &bytes).unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let request = ImportRequest {
        vault_id: vault.id,
        source,
        title: "binary".into(),
        scope: private_scope(),
    };
    let first = import_file(&pool, &request, 2).await.unwrap();
    let second = import_file(&pool, &request, 3).await.unwrap();
    assert_eq!(first.revision_id, second.revision_id);
    assert_eq!(
        read_revision(&pool, &first.revision_id).await.unwrap(),
        bytes
    );
}

#[tokio::test]
async fn duplicate_import_requires_the_stored_file_to_still_match() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("duplicate-drift-root");
    let source = directory("duplicate-drift-source").join("source.txt");
    std::fs::write(&source, "original").unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let request = ImportRequest {
        vault_id: vault.id,
        source,
        title: "source".into(),
        scope: private_scope(),
    };
    let first = import_file(&pool, &request, 2).await.unwrap();
    let revision = current_revision(&pool, &first.document.id)
        .await
        .unwrap()
        .unwrap();
    std::fs::write(root.join(revision.relative_path), "changed").unwrap();
    let second = import_file(&pool, &request, 3).await.unwrap();
    assert_ne!(first.revision_id, second.revision_id);
    assert_eq!(
        read_revision(&pool, &second.revision_id).await.unwrap(),
        b"original"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn import_rejects_symlinks_and_files_over_the_limit() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("rejected-root");
    let external = directory("rejected-external");
    let regular = external.join("regular.txt");
    let symlink = external.join("source-link.txt");
    std::fs::write(&regular, "safe").unwrap();
    std::os::unix::fs::symlink(&regular, &symlink).unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let request = |source| ImportRequest {
        vault_id: vault.id.clone(),
        source,
        title: "source".into(),
        scope: private_scope(),
    };
    assert!(import_file(&pool, &request(symlink), 2).await.is_err());
    let oversized = external.join("oversized.bin");
    std::fs::File::create(&oversized)
        .unwrap()
        .set_len(101 * 1024 * 1024)
        .unwrap();
    assert!(import_file(&pool, &request(oversized), 3).await.is_err());
    let excluded = external.join(".env.secrets");
    std::fs::write(&excluded, "secret").unwrap();
    assert!(import_file(&pool, &request(excluded), 4).await.is_err());
}

#[tokio::test]
async fn saves_text_and_url_without_fetching() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let vault = register_vault(&pool, &directory("manual-root"), 1)
        .await
        .unwrap();
    let text = create_text_source(
        &pool,
        &TextSourceDraft {
            vault_id: vault.id.clone(),
            title: "research".into(),
            body: "local body".into(),
            scope: private_scope(),
        },
        2,
    )
    .await
    .unwrap();
    let url = create_url_source(
        &pool,
        &UrlSourceDraft {
            vault_id: vault.id,
            title: "reference".into(),
            url: "https://example.test/reference".into(),
            memo: "review later".into(),
            scope: private_scope(),
        },
        3,
    )
    .await
    .unwrap();
    assert_eq!(
        read_revision(&pool, &text.revision_id).await.unwrap(),
        b"local body"
    );
    assert_eq!(
        read_revision(&pool, &url.revision_id).await.unwrap(),
        b"URL: https://example.test/reference\n\nreview later"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn scan_skips_default_and_configured_exclusions() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("scan-root");
    std::fs::write(root.join("keep.txt"), "keep").unwrap();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/ignored.txt"), "ignored").unwrap();
    std::fs::create_dir_all(root.join("private")).unwrap();
    std::fs::write(root.join("private/ignored.txt"), "ignored").unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let result = scan_vault_with_exclusions(
        &pool,
        &vault.id,
        private_scope(),
        &[PathBuf::from("private")],
        2,
    )
    .await
    .unwrap();
    assert_eq!(result.indexed, 1);
    assert!(!result.partial);
}

#[cfg(unix)]
#[tokio::test]
async fn root_replacement_blocks_revision_read() {
    let pool = crate::knowledge::tests::raw_pool().await;
    migrate(&pool).await.unwrap();
    let root = directory("swap-root");
    let source = directory("swap-source").join("source.txt");
    std::fs::write(&source, "safe bytes").unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let imported = import_file(
        &pool,
        &ImportRequest {
            vault_id: vault.id,
            source,
            title: "source".into(),
            scope: private_scope(),
        },
        2,
    )
    .await
    .unwrap();
    let outside = directory("swap-outside");
    std::fs::remove_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside, &root).unwrap();
    assert!(read_revision(&pool, &imported.revision_id).await.is_err());
}
