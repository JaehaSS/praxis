//! Local and external document evidence policies.
#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::{db, evidence, memory};

static COUNTER: AtomicU32 = AtomicU32::new(0);

#[tokio::test]
async fn missing_local_document_stales_an_approved_memory() {
    let (pool, root, db_path) = project_fixture("missing-document").await;
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::fs::write(root.join("docs/guide.md"), "authoritative guide\n").unwrap();
    let memory_id = candidate(&pool, Some(root.to_string_lossy().as_ref()), 90).await;
    evidence::add_local_document(
        &pool,
        memory_id,
        evidence::LocalDocumentInput {
            relative_path: "docs/guide.md".into(),
            expires_at: None,
        },
        100,
    )
    .await
    .unwrap();
    approve(&pool, memory_id, 101).await;

    std::fs::remove_file(root.join("docs/guide.md")).unwrap();
    let report = evidence::revalidate_memory(&pool, memory_id, 102)
        .await
        .unwrap();
    assert_eq!(report.statuses, vec![memory::evidence_status::MISSING]);
    assert!(report.stale);
    let status: (String,) = sqlx::query_as("SELECT status FROM memories WHERE id = ?")
        .bind(memory_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status.0, memory::knowledge_status::STALE);
    cleanup(root, db_path);
}

#[tokio::test]
async fn external_documents_require_safe_https_and_expire_fail_closed() {
    let (pool, root, db_path) = project_fixture("external-document").await;
    let memory_id = candidate(&pool, None, 90).await;
    for unsafe_url in [
        "http://example.test/guide",
        "https:///guide",
        "https://user@example.test/guide",
        "https://example.test/guide?token=secret",
        "https://example.test/guide#section",
    ] {
        let result = evidence::add_external_document(
            &pool,
            memory_id,
            evidence::ExternalDocumentInput {
                url: unsafe_url.into(),
                expires_at: 110,
            },
            100,
        )
        .await;
        assert!(result.is_err(), "unsafe URL was accepted: {unsafe_url}");
    }
    evidence::add_external_document(
        &pool,
        memory_id,
        evidence::ExternalDocumentInput {
            url: "https://example.test/guide".into(),
            expires_at: 105,
        },
        100,
    )
    .await
    .unwrap();
    approve(&pool, memory_id, 101).await;
    let report = evidence::revalidate_memory(&pool, memory_id, 105)
        .await
        .unwrap();
    assert_eq!(report.statuses, vec![memory::evidence_status::EXPIRED]);
    assert!(report.stale);
    cleanup(root, db_path);
}

#[tokio::test]
async fn unknown_test_evidence_blocks_approval_without_claiming_stale() {
    let (pool, root, db_path) = project_fixture("unknown-test").await;
    let memory_id = candidate(&pool, Some(root.to_string_lossy().as_ref()), 90).await;
    sqlx::query(
        "INSERT INTO memory_evidence \
         (memory_id, version, kind, locator_json, status, observed_at, checked_at) \
         VALUES (?, 1, 'test_run', '{}', 'valid', 100, 100)",
    )
    .bind(memory_id)
    .execute(&pool)
    .await
    .unwrap();
    memory::submit_for_review(&pool, memory_id, 101)
        .await
        .unwrap();
    assert!(memory::approve(&pool, memory_id, "human", 102)
        .await
        .is_err());
    let row: (String, String) = sqlx::query_as(
        "SELECT m.status, e.status FROM memories m \
         JOIN memory_evidence e ON e.memory_id = m.id WHERE m.id = ?",
    )
    .bind(memory_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        (
            memory::knowledge_status::PENDING_REVIEW.into(),
            memory::evidence_status::UNKNOWN.into()
        )
    );
    cleanup(root, db_path);
}

#[tokio::test]
async fn unknown_evidence_blocks_verified_memory_without_overstating_staleness() {
    let (pool, root, db_path) = project_fixture("unknown-verified").await;
    let memory_id = candidate(&pool, Some(root.to_string_lossy().as_ref()), 90).await;
    memory::add_user_confirmation(&pool, memory_id, 100, None)
        .await
        .unwrap();
    approve(&pool, memory_id, 101).await;
    sqlx::query(
        "INSERT INTO memory_evidence \
         (memory_id, version, kind, locator_json, status, observed_at, checked_at) \
         VALUES (?, 1, 'test_run', '{}', 'valid', 102, 102)",
    )
    .bind(memory_id)
    .execute(&pool)
    .await
    .unwrap();

    let report = evidence::revalidate_memory(&pool, memory_id, 103)
        .await
        .unwrap();
    assert!(!report.stale);
    assert!(report
        .statuses
        .contains(&memory::evidence_status::UNKNOWN.into()));
    let status: String = sqlx::query_scalar("SELECT status FROM memories WHERE id = ?")
        .bind(memory_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(status, memory::knowledge_status::VERIFIED);
    assert!(!memory::has_valid_evidence(&pool, memory_id, 1, 103)
        .await
        .unwrap());
    cleanup(root, db_path);
}

async fn project_fixture(
    label: &str,
) -> (sqlx::SqlitePool, std::path::PathBuf, std::path::PathBuf) {
    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = temp_root::dir().join(format!(
        "praxis-document-{label}-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let db_path = root.with_extension("sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    (pool, root, db_path)
}

async fn candidate(pool: &sqlx::SqlitePool, scope: Option<&str>, now: i64) -> i64 {
    memory::create_candidate(
        pool,
        scope.map_or(memory::tier::GLOBAL, |_| memory::tier::PROJECT),
        scope,
        memory::knowledge_type::CLAIM,
        "document-backed claim",
        Some("test"),
        now,
    )
    .await
    .unwrap()
}

async fn approve(pool: &sqlx::SqlitePool, memory_id: i64, now: i64) {
    memory::submit_for_review(pool, memory_id, now)
        .await
        .unwrap();
    memory::approve(pool, memory_id, "human", now)
        .await
        .unwrap();
}
fn cleanup(root: std::path::PathBuf, db_path: std::path::PathBuf) {
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_dir_all(root);
}
