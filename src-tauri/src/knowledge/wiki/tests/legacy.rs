use super::directory;
use crate::knowledge::config::{save_obsidian, ObsidianConfig, VaultEntry};
use crate::knowledge::wiki::replace_legacy;

#[tokio::test]
async fn invalid_legacy_set_does_not_write_either_configuration() {
    let pool = super::super::super::tests::test_pool().await;
    let root = directory("atomic-set");
    let original = ObsidianConfig {
        vaults: vec![VaultEntry {
            root: root.to_string_lossy().into_owned(),
            exclude: vec![],
            embed_exclude: vec![],
        }],
    };
    save_obsidian(&pool, &original).await.unwrap();
    let invalid = ObsidianConfig {
        vaults: vec![
            original.vaults[0].clone(),
            VaultEntry {
                root: root.join("nested").to_string_lossy().into_owned(),
                exclude: vec![],
                embed_exclude: vec![],
            },
        ],
    };
    std::fs::create_dir_all(&invalid.vaults[1].root).unwrap();
    assert!(replace_legacy(&pool, &invalid).await.is_err());
    let stored = crate::knowledge::config::load_obsidian(&pool)
        .await
        .unwrap();
    assert_eq!(stored.vaults.len(), 1);
    assert_eq!(stored.vaults[0].root, original.vaults[0].root);
}
