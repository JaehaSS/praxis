#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::{db, memory};

fn database_path() -> String {
    temp_root::dir()
        .join(format!(
            "praxis-memory-management-{}.sqlite",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

#[tokio::test]
async fn list_summarizes_only_current_version_usable_and_blocking_evidence() {
    let path = database_path();
    remove_database_files(&path);
    let pool = db::init_pool(&path).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let memory_id = memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::DECISION,
        "version one",
        Some("test"),
        100,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(&pool, memory_id, 110, None)
        .await
        .unwrap();
    memory::update_knowledge(
        &pool,
        memory_id,
        "version two",
        memory::knowledge_type::DECISION,
        200,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(&pool, memory_id, 210, None)
        .await
        .unwrap();
    memory::add_user_confirmation(&pool, memory_id, 220, Some(250))
        .await
        .unwrap();
    let changed_id = memory::add_user_confirmation(&pool, memory_id, 230, None)
        .await
        .unwrap();
    sqlx::query("UPDATE memory_evidence SET status = 'changed' WHERE id = ?")
        .bind(changed_id)
        .execute(&pool)
        .await
        .unwrap();
    let empty_id = memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some("/repo"),
        memory::knowledge_type::CLAIM,
        "no evidence",
        Some("test"),
        240,
    )
    .await
    .unwrap();

    let rows = memory::management::list(&pool, 300).await.unwrap();
    let row = rows
        .iter()
        .find(|item| item.memory.id == memory_id)
        .unwrap();
    assert_eq!(
        row.evidence_count, 3,
        "old-version evidence must be excluded"
    );
    assert_eq!(
        row.blocking_evidence_count, 2,
        "changed and expired evidence block"
    );
    let empty = rows.iter().find(|item| item.memory.id == empty_id).unwrap();
    assert_eq!(empty.evidence_count, 0);
    assert_eq!(empty.blocking_evidence_count, 0);
    let json = serde_json::to_value(row).unwrap();
    assert_eq!(json["evidence_count"], 3);
    assert_eq!(json["blocking_evidence_count"], 2);

    drop(pool);
    remove_database_files(&path);
}

fn remove_database_files(path: &str) {
    for target in [
        path.to_string(),
        format!("{path}-wal"),
        format!("{path}-shm"),
    ] {
        let _ = std::fs::remove_file(target);
    }
}
