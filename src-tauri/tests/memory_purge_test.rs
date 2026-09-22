//! 영구 삭제(purge)의 계약 — 본문은 사라지고 감사 행은 남는다(이슈 #149).

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory};

async fn setup(name: &str) -> (sqlx::SqlitePool, std::path::PathBuf) {
    let root = temp_root::dir().join(format!(
        "praxis-memory-purge-{name}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let database = root.join("memory.sqlite");
    let pool = db::init_pool(database.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    (pool, root)
}

async fn candidate(pool: &sqlx::SqlitePool, content: &str) -> i64 {
    memory::create_candidate(
        pool,
        memory::tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::CONVENTION,
        content,
        Some("test"),
        2_000_000_000,
    )
    .await
    .unwrap()
}

async fn archived(pool: &sqlx::SqlitePool, content: &str) -> i64 {
    let id = candidate(pool, content).await;
    memory::archive(pool, id, 2_000_000_001).await.unwrap();
    id
}

#[tokio::test]
async fn purge_removes_the_row_and_leaves_the_audit_trail() {
    let (pool, root) = setup("removes").await;
    let id = archived(&pool, "purge target body").await;
    let events_before: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM memory_events WHERE memory_id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();

    memory::purge(&pool, id, 2_000_000_002).await.unwrap();

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 0, "memories 행이 남았다");

    // 감사 행은 줄지 않는다 — purge 이벤트가 하나 늘어난다.
    let events_after: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM memory_events WHERE memory_id = ?")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(events_after, events_before + 1);
    let purged: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memory_events WHERE memory_id = ? AND action = 'purged'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(purged, 1);

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn purge_erases_the_body_from_versions_and_search_index() {
    let (pool, root) = setup("erases").await;
    let id = archived(&pool, "sensitive purge body").await;

    // 전제: 버전 이력에 본문이 실제로 남아 있다 — 여기를 덮지 않으면 삭제가 아니다.
    let before: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memory_versions WHERE memory_id = ? AND content = ?",
    )
    .bind(id)
    .bind("sensitive purge body")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before, 1, "전제 실패: 버전 이력에 본문이 없다");

    memory::purge(&pool, id, 2_000_000_002).await.unwrap();

    let leaked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memory_versions WHERE memory_id = ? AND content = ?",
    )
    .bind(id)
    .bind("sensitive purge body")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(leaked, 0, "버전 이력에 본문이 남았다");

    // 행 자체는 남는다 — no_delete 계약을 깨지 않았다는 뜻.
    let versions: (i64, String) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(MAX(content), '') FROM memory_versions WHERE memory_id = ?",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(versions.0, 1);
    assert_eq!(versions.1, memory::PURGED_TOMBSTONE);

    // FTS 인덱스에서도 조회되지 않는다.
    let hits: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM memories_fts WHERE memories_fts MATCH ?")
            .bind("sensitive")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(hits, 0, "FTS 인덱스에 본문이 남았다");

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn only_archived_memories_can_be_purged() {
    let (pool, root) = setup("guard").await;
    let id = candidate(&pool, "still active").await;

    let error = memory::purge(&pool, id, 2_000_000_002)
        .await
        .expect_err("보관되지 않은 메모리가 삭제됐다");
    assert!(error.to_string().contains("보관된 메모리만"));

    let rows: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM memories WHERE id = ?")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(rows, 1, "실패했는데 행이 사라졌다");

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn missing_memory_is_reported_not_silently_ignored() {
    let (pool, root) = setup("missing").await;

    let error = memory::purge(&pool, 9_999, 2_000_000_002)
        .await
        .expect_err("없는 메모리인데 성공했다");
    assert!(error.to_string().contains("찾을 수 없습니다"));

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn tombstone_is_the_only_hole_in_the_version_immutability_contract() {
    let (pool, root) = setup("contract").await;
    let id = archived(&pool, "contract body").await;
    memory::purge(&pool, id, 2_000_000_002).await.unwrap();

    // tombstone을 되돌리는 UPDATE는 계속 막힌다.
    let restore = sqlx::query("UPDATE memory_versions SET content = ? WHERE memory_id = ?")
        .bind("contract body")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(restore.is_err(), "tombstone을 되돌릴 수 있었다");

    // 본문 외 컬럼을 tombstone과 함께 고치는 것도 막힌다.
    let sneak = sqlx::query(
        "UPDATE memory_versions SET content = ?, editor_kind = 'forged' WHERE memory_id = ?",
    )
    .bind(memory::PURGED_TOMBSTONE)
    .bind(id)
    .execute(&pool)
    .await;
    assert!(sneak.is_err(), "tombstone 예외로 다른 컬럼이 바뀌었다");

    // 삭제는 여전히 금지다.
    let delete = sqlx::query("DELETE FROM memory_versions WHERE memory_id = ?")
        .bind(id)
        .execute(&pool)
        .await;
    assert!(delete.is_err(), "감사 행이 삭제됐다");

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

/// 트리거는 SQL 리터럴, 코드는 상수 — 갈라지면 purge가 조용히 막히거나 예외가 넓어진다.
#[tokio::test]
async fn migration_trigger_and_constant_do_not_drift() {
    let (pool, root) = setup("drift").await;

    let sql: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'memory_versions_immutable'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(
        sql.contains(memory::PURGED_TOMBSTONE),
        "트리거가 tombstone 상수와 갈라졌다: {sql}"
    );

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}

/// 기존 DB에는 예외 없는 옛 트리거가 설치돼 있다. `CREATE TRIGGER IF NOT EXISTS`는 그것을
/// 바꾸지 않으므로, migrate가 옛 정의를 감지해 교체하지 못하면 purge가 통째로 막힌다.
#[tokio::test]
async fn migrate_upgrades_the_legacy_trigger_on_existing_databases() {
    let (pool, root) = setup("upgrade").await;

    // 규칙 도입 이전 상태를 만든다 — 예외 없는 옛 정의로 되돌린다.
    sqlx::raw_sql(
        "DROP TRIGGER IF EXISTS memory_versions_immutable; \
         CREATE TRIGGER memory_versions_immutable \
         BEFORE UPDATE ON memory_versions \
         BEGIN SELECT RAISE(ABORT, 'memory version is immutable'); END;",
    )
    .execute(&pool)
    .await
    .unwrap();
    let legacy: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'memory_versions_immutable'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(!legacy.contains(memory::PURGED_TOMBSTONE), "전제 실패: 옛 정의가 아니다");

    memory::migrate(&pool).await.unwrap();

    let upgraded: String = sqlx::query_scalar(
        "SELECT sql FROM sqlite_master WHERE type = 'trigger' AND name = 'memory_versions_immutable'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(upgraded.contains(memory::PURGED_TOMBSTONE), "옛 트리거가 교체되지 않았다");

    // 교체가 실제로 통하는지 — 옛 트리거가 남아 있으면 여기서 막힌다.
    let id = archived(&pool, "legacy db body").await;
    memory::purge(&pool, id, 2_000_000_002).await.unwrap();
    let leaked: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM memory_versions WHERE memory_id = ? AND content = ?",
    )
    .bind(id)
    .bind("legacy db body")
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(leaked, 0);

    pool.close().await;
    std::fs::remove_dir_all(root).unwrap();
}
