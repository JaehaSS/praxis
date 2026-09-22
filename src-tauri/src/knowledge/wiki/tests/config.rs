use super::directory;
use crate::knowledge::config::{save_obsidian, ObsidianConfig, VaultEntry};
use crate::knowledge::wiki::{connect, legacy_view, replace_legacy, spaces};

#[tokio::test]
async fn legacy_vault_migration_preserves_node_chunk_and_edge_ids() {
    let pool = super::super::super::tests::test_pool().await;
    let root = directory("legacy").join("한%_vault");
    std::fs::create_dir_all(&root).unwrap();
    save_obsidian(
        &pool,
        &ObsidianConfig {
            vaults: vec![VaultEntry {
                root: root.to_string_lossy().into_owned(),
                exclude: vec![],
                embed_exclude: vec![],
            }],
        },
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO knowledge_nodes (source, external_id, kind, title, updated_at, synced_at) VALUES ('obsidian', '한%_vault/a.md', 'document', 'a', 1, 1)")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO knowledge_nodes (source, external_id, kind, title, updated_at, synced_at) VALUES ('obsidian', '한%_vault/b.md', 'document', 'b', 1, 1)")
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO knowledge_chunks (node_id, ord, content) VALUES (1, 0, 'legacy body')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO knowledge_edges (src_id, dst_id, rel) VALUES (1, 2, 'links_to')")
        .execute(&pool)
        .await
        .unwrap();
    let result = spaces(&pool).await.unwrap();
    assert_eq!(result.len(), 1);
    let row: (i64, i64, String, String) = sqlx::query_as(
        "SELECT n.id, c.id, n.space_id, n.external_id FROM knowledge_nodes n JOIN knowledge_chunks c ON c.node_id = n.id",
    ).fetch_one(&pool).await.unwrap();
    assert_eq!((row.0, row.1), (1, 1));
    assert_eq!(row.2, result[0].id);
    assert_eq!(row.3, format!("{}/a.md", result[0].id));
    let edge: (i64, i64) = sqlx::query_as("SELECT src_id, dst_id FROM knowledge_edges")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(edge, (1, 2));
}

#[tokio::test]
async fn missing_legacy_root_does_not_block_new_connections_or_erase_space_ids() {
    let pool = super::super::super::tests::test_pool().await;
    save_obsidian(
        &pool,
        &ObsidianConfig {
            vaults: vec![VaultEntry {
                root: directory("missing")
                    .join("gone")
                    .to_string_lossy()
                    .into_owned(),
                exclude: vec![],
                embed_exclude: vec![],
            }],
        },
    )
    .await
    .unwrap();
    let root = directory("new-connection");
    let connected = connect(&pool, root.to_str().unwrap()).await.unwrap();
    save_obsidian(&pool, &ObsidianConfig::default())
        .await
        .unwrap();
    assert_eq!(spaces(&pool).await.unwrap()[0].id, connected.id);
}

#[tokio::test]
async fn same_basename_folders_are_isolated_and_nested_roots_are_rejected() {
    let pool = super::super::super::tests::test_pool().await;
    let parent = directory("same-name");
    let first = parent.join("a").join("notes");
    let second = parent.join("b").join("notes");
    std::fs::create_dir_all(&first).unwrap();
    std::fs::create_dir_all(&second).unwrap();
    let a = connect(&pool, first.to_str().unwrap()).await.unwrap();
    let b = connect(&pool, second.to_str().unwrap()).await.unwrap();
    assert_ne!(a.id, b.id);
    assert!(connect(&pool, first.to_str().unwrap()).await.is_err());
    let child = first.join("nested");
    std::fs::create_dir_all(&child).unwrap();
    assert!(connect(&pool, child.to_str().unwrap()).await.is_err());
}

#[tokio::test]
async fn missing_registered_root_does_not_block_a_distinct_connection() {
    let pool = super::super::super::tests::test_pool().await;
    let vanished = directory("vanished");
    connect(&pool, vanished.to_str().unwrap()).await.unwrap();
    std::fs::remove_dir_all(&vanished).unwrap();
    let distinct = directory("distinct");
    assert!(connect(&pool, distinct.to_str().unwrap()).await.is_ok());
}

#[tokio::test]
async fn legacy_settings_replace_removed_roots_without_changing_kept_ids() {
    let pool = super::super::super::tests::test_pool().await;
    let kept_root = directory("kept");
    let removed_root = directory("removed");
    let kept = connect(&pool, kept_root.to_str().unwrap()).await.unwrap();
    connect(&pool, removed_root.to_str().unwrap())
        .await
        .unwrap();
    replace_legacy(
        &pool,
        &ObsidianConfig {
            vaults: vec![VaultEntry {
                root: kept_root.to_string_lossy().into_owned(),
                exclude: vec![],
                embed_exclude: vec![],
            }],
        },
    )
    .await
    .unwrap();
    let spaces = spaces(&pool).await.unwrap();
    assert_eq!(spaces.len(), 1);
    assert_eq!(spaces[0].id, kept.id);
}

#[tokio::test]
async fn malformed_legacy_root_set_does_not_partially_migrate_nodes() {
    let pool = super::super::super::tests::test_pool().await;
    let root = directory("transaction");
    let nested = root.join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    save_obsidian(
        &pool,
        &ObsidianConfig {
            vaults: vec![
                VaultEntry {
                    root: root.to_string_lossy().into_owned(),
                    exclude: vec![],
                    embed_exclude: vec![],
                },
                VaultEntry {
                    root: nested.to_string_lossy().into_owned(),
                    exclude: vec![],
                    embed_exclude: vec![],
                },
            ],
        },
    )
    .await
    .unwrap();
    sqlx::query("INSERT INTO knowledge_nodes (source, external_id, kind, title, updated_at, synced_at) VALUES ('obsidian', 'transaction/a.md', 'document', 'a', 1, 1)")
        .execute(&pool).await.unwrap();
    assert!(spaces(&pool).await.is_err());
    let source: (String,) = sqlx::query_as("SELECT source FROM knowledge_nodes")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(source.0, "obsidian");
}

#[tokio::test]
async fn offline_legacy_root_stays_visible_and_a_failed_set_keeps_configuration() {
    let pool = super::super::super::tests::test_pool().await;
    let offline = directory("offline").join("gone");
    let config = ObsidianConfig {
        vaults: vec![VaultEntry {
            root: offline.to_string_lossy().into_owned(),
            exclude: vec!["private/**".into()],
            embed_exclude: vec!["large/**".into()],
        }],
    };
    save_obsidian(&pool, &config).await.unwrap();
    assert_eq!(
        legacy_view(&pool).await.unwrap().vaults[0].root,
        config.vaults[0].root
    );
    assert!(replace_legacy(&pool, &config).await.is_err());
    let stored = crate::knowledge::config::load_obsidian(&pool)
        .await
        .unwrap();
    assert_eq!(stored.vaults[0].root, config.vaults[0].root);
}
