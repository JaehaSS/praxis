//! Approval must atomically bind the fresh backend check generation it trusted.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory};

#[tokio::test]
async fn approval_records_immutable_fresh_check_receipt() {
    let db_path = temp_root::dir().join(format!(
        "praxis-evidence-approval-receipt-{}.sqlite",
        std::process::id()
    ));
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let memory_id = memory::create_candidate(
        &pool,
        memory::tier::GLOBAL,
        None,
        memory::knowledge_type::CLAIM,
        "approval receipt claim",
        Some("test"),
        99,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(&pool, memory_id, 100, None)
        .await
        .unwrap();
    memory::submit_for_review(&pool, memory_id, 101)
        .await
        .unwrap();

    memory::approve(&pool, memory_id, "human", 102)
        .await
        .unwrap();

    let check_ids_json: String = sqlx::query_scalar(
        "SELECT source_check_ids_json FROM memory_approval_receipts WHERE memory_id = ?",
    )
    .bind(memory_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let check_ids: Vec<i64> = serde_json::from_str(&check_ids_json).unwrap();
    assert_eq!(check_ids.len(), 1);
    let checked_at: i64 =
        sqlx::query_scalar("SELECT checked_at FROM memory_evidence_checks WHERE id = ?")
            .bind(check_ids[0])
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(checked_at, 102);
    assert!(
        sqlx::query("DELETE FROM memory_approval_receipts WHERE memory_id = ?")
            .bind(memory_id)
            .execute(&pool)
            .await
            .is_err()
    );
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn evidence_listing_excludes_receipts_from_superseded_versions() {
    let db_path = temp_root::dir().join(format!(
        "praxis-evidence-current-version-{}.sqlite",
        std::process::id()
    ));
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let memory_id = memory::create_candidate(
        &pool,
        memory::tier::GLOBAL,
        None,
        memory::knowledge_type::CLAIM,
        "version one",
        Some("test"),
        99,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(&pool, memory_id, 100, None)
        .await
        .unwrap();
    memory::update_knowledge(
        &pool,
        memory_id,
        "version two",
        memory::knowledge_type::CLAIM,
        101,
    )
    .await
    .unwrap();

    assert!(praxis_lib::evidence::list_evidence(&pool, memory_id)
        .await
        .unwrap()
        .is_empty());
    let _ = std::fs::remove_file(db_path);
}
