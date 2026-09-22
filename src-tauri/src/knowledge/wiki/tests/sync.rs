use super::directory;
use crate::knowledge::config::{save_obsidian, ObsidianConfig, VaultEntry};
use crate::knowledge::wiki::{connect, sync};

#[tokio::test]
async fn partial_scan_keeps_previously_indexed_documents() {
    let pool = super::super::super::tests::test_pool().await;
    let root = directory("partial");
    let note = root.join("note.md");
    std::fs::write(&note, "readable body").unwrap();
    let space = connect(&pool, root.to_str().unwrap()).await.unwrap();
    sync(&pool, 1).await.unwrap();
    std::fs::write(&note, [0xff]).unwrap();
    let result = sync(&pool, 2).await.unwrap();
    assert!(!result.complete);
    assert!(!result.warnings.is_empty());
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge_nodes WHERE space_id = ?")
        .bind(space.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count.0, 1);
}

#[tokio::test]
async fn migrated_exclusions_apply_to_indexing_and_reads() {
    let pool = super::super::super::tests::test_pool().await;
    let root = directory("legacy-exclusions");
    std::fs::create_dir_all(root.join("private")).unwrap();
    std::fs::write(root.join("public.md"), "public").unwrap();
    std::fs::write(root.join("private").join("hidden.md"), "hidden").unwrap();
    save_obsidian(
        &pool,
        &ObsidianConfig {
            vaults: vec![VaultEntry {
                root: root.to_string_lossy().into_owned(),
                exclude: vec!["private/**".into()],
                embed_exclude: vec![],
            }],
        },
    )
    .await
    .unwrap();
    let result = sync(&pool, 1).await.unwrap();
    assert_eq!(result.indexed, 1);
    assert!(super::super::files::read(&root, "private/hidden.md", &["private/**".into()]).is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn replaced_root_symlink_cannot_expand_the_registered_scope() {
    let pool = super::super::super::tests::test_pool().await;
    let root = directory("root-replacement");
    std::fs::write(root.join("inside.md"), "inside").unwrap();
    connect(&pool, root.to_str().unwrap()).await.unwrap();
    sync(&pool, 1).await.unwrap();
    let outside = directory("outside");
    std::fs::write(outside.join("outside.md"), "outside").unwrap();
    std::fs::remove_dir_all(&root).unwrap();
    std::os::unix::fs::symlink(&outside, &root).unwrap();
    let result = sync(&pool, 2).await.unwrap();
    assert!(!result.complete);
    let count: (i64,) =
        sqlx::query_as("SELECT COUNT(*) FROM knowledge_nodes WHERE source = 'wiki'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count.0, 1);
}

#[tokio::test]
async fn markdown_extension_is_case_insensitive_and_excluded_directories_stay_out() {
    let pool = super::super::super::tests::test_pool().await;
    let root = directory("extensions");
    std::fs::write(root.join("note.MD"), "upper case extension").unwrap();
    std::fs::create_dir_all(root.join(".git")).unwrap();
    std::fs::write(root.join(".git").join("hidden.md"), "excluded").unwrap();
    connect(&pool, root.to_str().unwrap()).await.unwrap();
    let result = sync(&pool, 1).await.unwrap();
    assert_eq!(result.indexed, 1);
}

#[tokio::test]
async fn links_resolve_within_their_own_wiki_space() {
    let pool = super::super::super::tests::test_pool().await;
    let first = directory("links-first");
    let second = directory("links-second");
    for root in [&first, &second] {
        std::fs::write(root.join("source.md"), "[[target]]").unwrap();
        std::fs::write(root.join("target.md"), root.to_string_lossy().as_bytes()).unwrap();
    }
    let first_space = connect(&pool, first.to_str().unwrap()).await.unwrap();
    let second_space = connect(&pool, second.to_str().unwrap()).await.unwrap();
    sync(&pool, 1).await.unwrap();
    let rows = sqlx::query_as::<_, (String, String)>(
        "SELECT src.space_id, dst.space_id FROM knowledge_edges e \
         JOIN knowledge_nodes src ON src.id = e.src_id \
         JOIN knowledge_nodes dst ON dst.id = e.dst_id WHERE e.rel = 'links_to'",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|(source, target)| source == target));
    assert!(rows.iter().any(|(source, _)| source == &first_space.id));
    assert!(rows.iter().any(|(source, _)| source == &second_space.id));
}
