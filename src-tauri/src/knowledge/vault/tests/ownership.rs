use sqlx::Row;

use crate::knowledge::vault::{migrate, rebind_vault, register_vault};

#[tokio::test]
async fn claiming_legacy_node_keeps_ledger_row_and_blocks_missing_path() {
    let pool = crate::knowledge::tests::raw_pool().await;
    crate::knowledge::migrate(&pool).await.unwrap();
    migrate(&pool).await.unwrap();
    let root = crate::testtmp::dir().join(format!("vault-owned-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("owned.md");
    std::fs::write(&file, "owned").unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let node: i64 = sqlx::query("INSERT INTO knowledge_nodes (source, external_id, kind, title, url, updated_at, synced_at) VALUES ('wiki', 'owned.md', 'document', 'owned', ?, 1, 1) RETURNING id")
        .bind(format!("file://{}", file.display())).fetch_one(&pool).await.unwrap().try_get("id").unwrap();
    sqlx::query("INSERT INTO knowledge_chunks (node_id, ord, content) VALUES (?, 0, 'owned')")
        .bind(node)
        .execute(&pool)
        .await
        .unwrap();
    crate::knowledge::vault::ownership::claim_legacy_owned_nodes(&pool, 2)
        .await
        .unwrap();
    let nodes: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge_nodes WHERE id = ?")
        .bind(node)
        .fetch_one(&pool)
        .await
        .unwrap();
    let chunks: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge_chunks WHERE node_id = ?")
        .bind(node)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(nodes.0, 1);
    assert_eq!(chunks.0, 0);
    assert_eq!(
        vault.canonical_root,
        root.canonicalize().unwrap().to_string_lossy()
    );
    std::fs::remove_dir_all(&root).unwrap();
    assert!(
        crate::knowledge::vault::ownership::owns_legacy_node(&pool, node)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn rebind_keeps_claimed_legacy_nodes_owned() {
    let pool = crate::knowledge::tests::raw_pool().await;
    crate::knowledge::migrate(&pool).await.unwrap();
    migrate(&pool).await.unwrap();
    let root = crate::testtmp::dir().join(format!("vault-rebind-owned-{}", std::process::id()));
    let next = crate::testtmp::dir().join(format!("vault-rebind-next-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&next).unwrap();
    let file = root.join("owned.md");
    std::fs::write(&file, "owned").unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let node: i64 = sqlx::query("INSERT INTO knowledge_nodes (source, external_id, kind, title, url, updated_at, synced_at) VALUES ('wiki', 'owned.md', 'document', 'owned', ?, 1, 1) RETURNING id").bind(format!("file://{}", file.display())).fetch_one(&pool).await.unwrap().try_get("id").unwrap();
    crate::knowledge::vault::ownership::claim_legacy_owned_nodes(&pool, 2)
        .await
        .unwrap();
    rebind_vault(&pool, &vault.id, &next, &[], 3).await.unwrap();
    assert!(
        crate::knowledge::vault::ownership::owns_legacy_node(&pool, node)
            .await
            .unwrap()
    );
}

/// 묶음 판정은 낱개 판정과 **한 글자도 다르지 않아야** 한다. 여기가 갈라지면 검색 결과에
/// vault 문서가 새거나(누락 판정) 멀쩡한 노트가 조용히 사라진다(과잉 판정).
#[tokio::test]
async fn batch_ownership_matches_one_by_one() {
    let pool = crate::knowledge::tests::raw_pool().await;
    crate::knowledge::migrate(&pool).await.unwrap();
    migrate(&pool).await.unwrap();
    let root = crate::testtmp::dir().join(format!("vault-batch-{}", std::process::id()));
    let outside = crate::testtmp::dir().join(format!("vault-batch-out-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(root.join("inside.md"), "inside").unwrap();
    std::fs::write(outside.join("outside.md"), "outside").unwrap();
    register_vault(&pool, &root, 1).await.unwrap();

    let mut ids = Vec::new();
    // ① vault 루트 안의 파일 — 등재 없이도 경로만으로 소유.
    // ② 루트 밖의 파일 — 소유 아님.
    // ③ url이 NULL인 노드 — file:// 접두가 없으므로 소유 아님.
    for (external, url) in [
        ("inside.md", Some(format!("file://{}", root.join("inside.md").display()))),
        ("outside.md", Some(format!("file://{}", outside.join("outside.md").display()))),
        ("no-url", None),
    ] {
        let id: i64 = sqlx::query("INSERT INTO knowledge_nodes (source, external_id, kind, title, url, updated_at, synced_at) VALUES ('wiki', ?, 'document', ?, ?, 1, 1) RETURNING id")
            .bind(external).bind(external).bind(url)
            .fetch_one(&pool).await.unwrap().try_get("id").unwrap();
        ids.push(id);
    }
    // ④ 등재로 소유된 노드 — 경로 판정을 타지 않는 경로.
    let claimed: i64 = sqlx::query("INSERT INTO knowledge_nodes (source, external_id, kind, title, url, updated_at, synced_at) VALUES ('wiki', 'claimed', 'document', 'claimed', NULL, 1, 1) RETURNING id")
        .fetch_one(&pool).await.unwrap().try_get("id").unwrap();
    sqlx::query("INSERT INTO vault_legacy_ownership (node_id, vault_id, claimed_at) SELECT ?, id, 1 FROM vaults LIMIT 1")
        .bind(claimed).execute(&pool).await.unwrap();
    ids.push(claimed);

    // 같은 노드를 두 번 넣어 중복 접기까지 확인한다 — 청크가 여럿이면 실제로 이렇게 들어온다.
    let mut probed = ids.clone();
    probed.push(ids[0]);
    let batch = crate::knowledge::vault::ownership::owned_legacy_nodes(&pool, &probed)
        .await
        .unwrap();
    for id in &ids {
        let single = crate::knowledge::vault::ownership::owns_legacy_node(&pool, *id)
            .await
            .unwrap();
        assert_eq!(batch.contains(id), single, "node {id}");
    }
    assert!(batch.contains(&ids[0]));
    assert!(!batch.contains(&ids[1]));
    assert!(!batch.contains(&ids[2]));
    assert!(batch.contains(&claimed));
    assert!(
        crate::knowledge::vault::ownership::owned_legacy_nodes(&pool, &[])
            .await
            .unwrap()
            .is_empty()
    );

    std::fs::remove_dir_all(&root).unwrap();
    std::fs::remove_dir_all(&outside).unwrap();
}
