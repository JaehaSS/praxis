#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory};

#[path = "support/memory_version_contention.rs"]
mod memory_version_contention;

async fn setup(label: &str) -> (sqlx::SqlitePool, std::path::PathBuf) {
    let root = temp_root::dir().join(format!(
        "praxis-memory-version-atomic-{label}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let database = root.join("memory.sqlite");
    let pool = db::init_pool(database.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    (pool, root)
}

async fn two_versions(pool: &sqlx::SqlitePool) -> i64 {
    let id = memory::create_candidate(
        pool,
        memory::tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::CLAIM,
        "version one",
        Some("test"),
        100,
    )
    .await
    .unwrap();
    memory::update_knowledge(pool, id, "version two", memory::knowledge_type::CLAIM, 200)
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn stale_version_or_status_conflict_has_zero_mutations() {
    let (pool, root) = setup("conflict").await;
    let id = two_versions(&pool).await;

    for (expected_version, expected_status) in [
        (1, memory::knowledge_status::CANDIDATE),
        (2, memory::knowledge_status::ARCHIVED),
    ] {
        let error = memory::management::restore_version(
            &pool,
            id,
            1,
            expected_version,
            expected_status,
            300,
        )
        .await
        .unwrap_err();
        assert!(memory::restore_error::is_conflict(&error));
    }
    let current: (String, i64) =
        sqlx::query_as("SELECT content, current_version FROM memories WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(current, ("version two".into(), 2));
    assert_eq!(
        memory::management::versions(&pool, id).await.unwrap().len(),
        2
    );

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn restore_rolls_back_when_the_audit_event_fails() {
    let (pool, root) = setup("rollback").await;
    let id = two_versions(&pool).await;
    sqlx::query(
        "CREATE TRIGGER reject_restore_event BEFORE INSERT ON memory_events
         WHEN NEW.action = 'version_restored'
         BEGIN SELECT RAISE(ABORT, 'restore audit rejected'); END",
    )
    .execute(&pool)
    .await
    .unwrap();

    assert!(memory::management::restore_version(
        &pool,
        id,
        1,
        2,
        memory::knowledge_status::CANDIDATE,
        300,
    )
    .await
    .is_err());
    let current: (String, i64) =
        sqlx::query_as("SELECT content, current_version FROM memories WHERE id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(current, ("version two".into(), 2));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM memory_versions WHERE memory_id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        2
    );

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn restore_invalidates_the_old_embedding_before_appending() {
    let (pool, root) = setup("embedding").await;
    let id = two_versions(&pool).await;
    sqlx::query("UPDATE memories SET embedding = X'0102' WHERE id = ?")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TRIGGER require_cleared_restore_embedding
         BEFORE INSERT ON memory_versions
         WHEN NEW.editor_kind = 'human_restore'
           AND (SELECT embedding FROM memories WHERE id = NEW.memory_id) IS NOT NULL
         BEGIN SELECT RAISE(ABORT, 'old embedding survived restore'); END",
    )
    .execute(&pool)
    .await
    .unwrap();

    memory::management::restore_version(&pool, id, 1, 2, memory::knowledge_status::CANDIDATE, 300)
        .await
        .unwrap();

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn concurrent_restores_yield_one_success_and_one_typed_conflict() {
    let (pool, root) = setup("concurrent").await;
    let id = two_versions(&pool).await;
    let blocker = pool.begin_with("BEGIN IMMEDIATE").await.unwrap();
    let expected_checked_out = memory_version_contention::checked_out(&pool) + 2;
    let first_pool = pool.clone();
    let second_pool = pool.clone();
    let first = tokio::spawn(async move {
        memory::management::restore_version(
            &first_pool,
            id,
            1,
            2,
            memory::knowledge_status::CANDIDATE,
            300,
        )
        .await
    });
    let second = tokio::spawn(async move {
        memory::management::restore_version(
            &second_pool,
            id,
            1,
            2,
            memory::knowledge_status::CANDIDATE,
            300,
        )
        .await
    });
    memory_version_contention::wait_for_checked_out(&pool, expected_checked_out).await;
    blocker.commit().await.unwrap();
    let results = [first.await.unwrap(), second.await.unwrap()];

    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    let error = results.into_iter().find_map(Result::err).unwrap();
    assert!(memory::restore_error::is_conflict(&error));
    assert_eq!(
        memory::management::versions(&pool, id).await.unwrap().len(),
        3
    );

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
