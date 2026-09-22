//! Manual project memories use canonical repository identity.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory};

#[cfg(unix)]
#[tokio::test]
async fn manual_memory_canonicalizes_repository_aliases() {
    let root = temp_root::dir().join(format!("praxis-memory-scope-{}", std::process::id()));
    let alias = root.with_extension("alias");
    let db_path = root.with_extension("sqlite");
    let _ = std::fs::remove_file(&alias);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&root, &alias).unwrap();
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();

    let id = memory::management::create_manual(
        &pool,
        alias.to_string_lossy().as_ref(),
        memory::knowledge_type::CLAIM,
        "canonical scope",
        100,
    )
    .await
    .unwrap();
    let scope: Option<String> = sqlx::query_scalar("SELECT scope_key FROM memories WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(scope.as_deref(), root.canonicalize().unwrap().to_str());
    assert!(memory::management::create_manual(
        &pool,
        root.join("missing").to_string_lossy().as_ref(),
        memory::knowledge_type::CLAIM,
        "missing scope",
        101,
    )
    .await
    .is_err());
    let _ = std::fs::remove_file(alias);
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(root);
}
