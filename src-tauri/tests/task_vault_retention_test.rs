#![cfg(target_os = "macos")]

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::knowledge::vault::provenance::{
    grant_consent, record_terminal_snapshot, record_user_input, start_attempt,
};
use praxis_lib::knowledge::vault::{
    create_text_source, register_project, register_vault, shared_admission, Scope, ScopeRequest,
    TextSourceDraft,
};
use praxis_lib::{db, knowledge};

struct Fixture {
    pool: sqlx::SqlitePool,
    root: std::path::PathBuf,
    task_id: i64,
    vault_id: String,
    binding: praxis_lib::knowledge::vault::ProjectBinding,
}

async fn fixture(label: &str) -> Fixture {
    let root = temp_root::dir().join(format!(
        "praxis-vault-retention-{label}-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let pool = db::init_pool(root.join("tasks.sqlite").to_str().unwrap())
        .await
        .unwrap();
    knowledge::vault::migrate(&pool).await.unwrap();
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        root.to_string_lossy().as_ref(),
        "task",
        None,
        None,
        "conversation",
        1,
    )
    .await
    .unwrap();
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let binding = register_project(&pool, &root, 1).await.unwrap();
    let provider = "claude:sonnet:low:lean";
    grant_consent(&pool, &binding, provider, 2).await.unwrap();
    start_attempt(&pool, task_id, &vault.id, &binding, provider, None, 3)
        .await
        .unwrap();
    record_user_input(&pool, task_id, "input", 4).await.unwrap();
    assert!(record_terminal_snapshot(&pool, task_id, "done", "completion", 5)
        .await
        .unwrap()
        .is_some());
    db::update_state(&pool, task_id, db::state::DONE, 6)
        .await
        .unwrap();
    Fixture {
        pool,
        root,
        task_id,
        vault_id: vault.id,
        binding,
    }
}

fn scope(binding: &praxis_lib::knowledge::vault::ProjectBinding) -> ScopeRequest {
    ScopeRequest {
        scope: Scope::Project {
            key: binding.id.clone(),
            binding_epoch: binding.epoch.clone(),
        },
    }
}

#[tokio::test]
async fn shared_admission_blocks_task_deletion_without_mutation() {
    let fixture = fixture("busy").await;
    let capture = fixture
        .root
        .join(".praxis/captures")
        .join(fixture.task_id.to_string())
        .join("capture.json");
    std::fs::create_dir_all(capture.parent().unwrap()).unwrap();
    std::fs::write(&capture, "capture").unwrap();
    let shared = shared_admission(&fixture.pool).await.unwrap();
    let error = db::delete_task(&fixture.pool, fixture.task_id)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("deletion_busy"));
    assert!(db::get_task(&fixture.pool, fixture.task_id)
        .await
        .unwrap()
        .is_some());
    assert!(capture.is_file());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vault_terminal_snapshots")
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
        1
    );
    drop(shared);

    db::delete_task(&fixture.pool, fixture.task_id)
        .await
        .unwrap();
    assert!(!capture.exists());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vault_terminal_snapshots")
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn stored_knowledge_survives_task_deletion() {
    let fixture = fixture("stored").await;
    create_text_source(
        &fixture.pool,
        &TextSourceDraft {
            vault_id: fixture.vault_id.clone(),
            title: "kept".into(),
            body: "kept body".into(),
            scope: scope(&fixture.binding),
        },
        7,
    )
    .await
    .unwrap();
    let paths: Vec<String> =
        sqlx::query_scalar("SELECT relative_path FROM vault_revisions ORDER BY id")
            .fetch_all(&fixture.pool)
            .await
            .unwrap();
    db::delete_task(&fixture.pool, fixture.task_id)
        .await
        .unwrap();
    assert_eq!(paths.len(), 1);
    assert!(paths
        .into_iter()
        .all(|path| fixture.root.join(path).is_file()));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vault_documents")
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM vault_task_attempts")
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
        0
    );
}
