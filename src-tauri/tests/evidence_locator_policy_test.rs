//! Revalidation re-enforces locator schema and URL policy for legacy database rows.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, evidence, memory};

#[tokio::test]
async fn unsafe_or_unknown_external_locators_become_unknown() {
    let path = temp_root::dir().join(format!(
        "praxis-evidence-locator-policy-{}.sqlite",
        std::process::id()
    ));
    let pool = db::init_pool(path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let memory_id = memory::create_candidate(
        &pool,
        memory::tier::GLOBAL,
        None,
        memory::knowledge_type::CLAIM,
        "legacy locator policy",
        Some("test"),
        99,
    )
    .await
    .unwrap();
    for locator in [
        r#"{"schema_version":1,"url":"https://example.test/guide?token=secret"}"#,
        r#"{"schema_version":2,"url":"https://example.test/guide"}"#,
    ] {
        sqlx::query(
            "INSERT INTO memory_evidence \
             (memory_id, version, kind, locator_json, status, observed_at, checked_at, expires_at) \
             VALUES (?, 1, 'document', ?, 'valid', 100, 100, 200)",
        )
        .bind(memory_id)
        .bind(locator)
        .execute(&pool)
        .await
        .unwrap();
    }

    let report = evidence::revalidate_memory(&pool, memory_id, 101)
        .await
        .unwrap();

    assert_eq!(
        report.statuses,
        vec![
            memory::evidence_status::UNKNOWN,
            memory::evidence_status::UNKNOWN
        ]
    );
    assert!(!report.stale);
    let _ = std::fs::remove_file(path);
}
