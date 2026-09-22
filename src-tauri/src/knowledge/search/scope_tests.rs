use sqlx::{Row, SqlitePool};

use super::collect_candidates;

#[tokio::test]
async fn wiki_visibility_applies_before_fts_and_title_candidate_limits() {
    let pool = crate::knowledge::tests::test_pool().await;
    let root = crate::testtmp::dir().join(format!("praxis-search-scope-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let space = crate::knowledge::wiki::connect(&pool, root.to_str().unwrap())
        .await
        .unwrap();
    for index in 0..super::CANDIDATE_LIMIT {
        insert(&pool, "wiki", Some("removed"), &format!("removed/{index}")).await;
    }
    insert(&pool, "wiki", Some(&space.id), "active/note").await;
    insert(&pool, "gmail", None, "mail/valid").await;
    let spaces = crate::knowledge::wiki::active_space_ids(&pool)
        .await
        .unwrap();
    assert_candidates(
        &collect_candidates(&pool, "floodterm", &spaces)
            .await
            .unwrap(),
    );
    assert_candidates(&collect_candidates(&pool, "xy", &spaces).await.unwrap());
}

async fn insert(pool: &SqlitePool, source: &str, space_id: Option<&str>, external_id: &str) {
    let node_id: i64 = sqlx::query(
        "INSERT INTO knowledge_nodes (source, space_id, external_id, kind, title, updated_at, synced_at) \
         VALUES (?, ?, ?, 'document', 'xy', 1, 1) RETURNING id",
    )
    .bind(source)
    .bind(space_id)
    .bind(external_id)
    .fetch_one(pool)
    .await
    .unwrap()
    .try_get("id")
    .unwrap();
    sqlx::query(
        "INSERT INTO knowledge_chunks (node_id, ord, doc_title, content) \
         VALUES (?, 0, 'xy', 'floodterm')",
    )
    .bind(node_id)
    .execute(pool)
    .await
    .unwrap();
}

fn assert_candidates(candidates: &[super::Candidate]) {
    assert_eq!(candidates.len(), 2);
    assert!(candidates
        .iter()
        .any(|candidate| candidate.external_id == "active/note"));
    assert!(candidates
        .iter()
        .any(|candidate| candidate.external_id == "mail/valid"));
    assert!(candidates
        .iter()
        .all(|candidate| candidate.external_id != "removed/0"));
}
