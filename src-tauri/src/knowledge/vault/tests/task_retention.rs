#![cfg(target_os = "macos")]

use crate::knowledge::vault::provenance::start_attempt;
use crate::knowledge::vault::task_retention::{delete_task_data, try_admission_for_task};
use crate::knowledge::vault::{
    create_text_source, register_project, register_vault, shared_admission, Scope, ScopeRequest,
    TextSourceDraft,
};

#[tokio::test]
async fn task_retention_removes_owned_rows_and_preserves_other_tasks() {
    let root = crate::testtmp::dir().join(format!("vault-retention-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let pool = crate::knowledge::tests::test_pool().await;
    let root_text = root.to_string_lossy().into_owned();
    insert_task(&pool, 1, &root_text).await;
    insert_task(&pool, 2, &root_text).await;
    let vault = register_vault(&pool, &root, 1).await.unwrap();
    let binding = register_project(&pool, &root, 1).await.unwrap();
    let source = create_text_source(
        &pool,
        &TextSourceDraft {
            vault_id: vault.id.clone(),
            title: "source".into(),
            body: "preserved source".into(),
            scope: ScopeRequest {
                scope: Scope::PrivateData,
            },
        },
        2,
    )
    .await
    .unwrap();
    seed_task(
        &pool,
        1,
        &vault.id,
        &binding,
        &source.revision_id,
        &source.sha256,
    )
    .await;
    let shared = shared_admission(&pool).await.unwrap();
    assert!(try_admission_for_task(&pool, 1).await.is_err());
    assert_eq!(count(&pool, "vault_task_attempts", "task_id = 1").await, 1);
    drop(shared);
    let mut unguarded = pool.begin().await.unwrap();
    assert!(delete_task_data(&mut unguarded, 1, false).await.is_err());
    unguarded.rollback().await.unwrap();
    let _admission = try_admission_for_task(&pool, 1).await.unwrap();
    seed_task(
        &pool,
        2,
        &vault.id,
        &binding,
        &source.revision_id,
        &source.sha256,
    )
    .await;
    let mut tx = pool.begin().await.unwrap();
    delete_task_data(&mut tx, 1, true).await.unwrap();
    tx.commit().await.unwrap();

    assert_eq!(count(&pool, "vault_task_attempts", "task_id = 1").await, 0);
    assert_eq!(count(&pool, "vault_usages", "task_id = 1").await, 0);
    assert_eq!(
        count(&pool, "vault_reference_previews", "consumed_task_id = 1").await,
        0
    );
    assert_eq!(
        count(&pool, "vault_draft_policies", "consumed_task_id = 1").await,
        0
    );
    assert_eq!(count(&pool, "vault_task_attempts", "task_id = 2").await, 1);
    // 다른 작업의 완료 스냅샷은 남는다 — 출처 기록은 작업 단위로만 지운다.
    assert_eq!(
        count(&pool, "vault_terminal_snapshots", "task_id = 2").await,
        1
    );
    assert_eq!(
        count(&pool, "vault_terminal_snapshots", "task_id = 1").await,
        0
    );
    assert!(!root.join("notes").exists());
    let violations: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM pragma_foreign_key_check")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(violations, 0);
}

async fn insert_task(pool: &sqlx::SqlitePool, id: i64, root: &str) {
    sqlx::query("INSERT INTO tasks (id, repo, branch, base, worktree_path, instruction, state, created_at, updated_at) VALUES (?, ?, 'main', 'base', ?, 'task', 'done', 1, 1)")
        .bind(id).bind(root).bind(root).execute(pool).await.unwrap();
}

async fn seed_task(
    pool: &sqlx::SqlitePool,
    task_id: i64,
    vault_id: &str,
    binding: &crate::knowledge::vault::ProjectBinding,
    revision_id: &str,
    revision_hash: &str,
) {
    let attempt = start_attempt(pool, task_id, vault_id, binding, "test", None, 3)
        .await
        .unwrap();
    let input = format!("input-{task_id}");
    let snapshot = format!("snapshot-{task_id}");
    let (root, device, inode): (String, i64, i64) =
        sqlx::query_as("SELECT canonical_root, root_device, root_inode FROM vaults WHERE id = ?")
            .bind(vault_id)
            .fetch_one(pool)
            .await
            .unwrap();
    sqlx::query("INSERT INTO vault_attempt_inputs (id, attempt_id, origin_kind, payload_hash, declared_scope, capture_purpose, created_at) VALUES (?, ?, 'user_message', 'payload', 'private-data', 'capture_allowed', 4)")
        .bind(format!("attempt-input-{task_id}")).bind(&attempt).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO vault_input_snapshots (id, attempt_id, vault_id, vault_root, vault_device, vault_inode, body_hash, created_at) VALUES (?, ?, ?, ?, ?, ?, 'input-hash', 4)")
        .bind(&input).bind(&attempt).bind(vault_id).bind(&root).bind(device).bind(inode).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO vault_terminal_snapshots (id, attempt_id, task_id, vault_id, vault_root, vault_device, vault_inode, binding_id, binding_epoch, scope, bounded_body, body_hash, input_snapshot_id, terminal_hash, terminal_state, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, 'private-data', 'snapshot body', ?, ?, 'terminal', 'done', 5)")
        .bind(&snapshot).bind(&attempt).bind(task_id).bind(vault_id).bind(&root).bind(device).bind(inode).bind(&binding.id).bind(&binding.epoch).bind(format!("body-hash-{task_id}")).bind(&input).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO vault_usages (id, task_id, attempt_id, revision_id, revision_hash, snippet, snippet_hash, delivery_state, citation_state, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 'snippet', 'snippet-hash', 'pending', 'unknown', 6, 6)")
        .bind(format!("usage-{task_id}")).bind(task_id).bind(&attempt).bind(revision_id).bind(revision_hash).execute(pool).await.unwrap();
    insert_consumed_delivery(pool, task_id, binding, revision_id, revision_hash).await;
}

async fn insert_consumed_delivery(
    pool: &sqlx::SqlitePool,
    task_id: i64,
    binding: &crate::knowledge::vault::ProjectBinding,
    revision_id: &str,
    revision_hash: &str,
) {
    sqlx::query("INSERT INTO vault_reference_previews (id, binding_id, binding_epoch, query_hash, client_ref, state, created_at, consumed_task_id) VALUES (?, ?, ?, 'query', ?, 'consumed', 6, ?)")
        .bind(format!("preview-{task_id}")).bind(&binding.id).bind(&binding.epoch).bind(format!("preview-client-{task_id}")).bind(task_id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO vault_reference_preview_items (preview_id, revision_id, revision_hash, snippet, reason) VALUES (?, ?, ?, 'snippet', 'reason')")
        .bind(format!("preview-{task_id}")).bind(revision_id).bind(revision_hash).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO vault_draft_policies (id, binding_id, binding_epoch, client_ref, query_hash, input_mode, state, created_at, consumed_task_id) VALUES (?, ?, ?, ?, 'query', 'private_attachment', 'consumed', 6, ?)")
        .bind(format!("policy-{task_id}")).bind(&binding.id).bind(&binding.epoch).bind(format!("policy-client-{task_id}")).bind(task_id).execute(pool).await.unwrap();
    sqlx::query("INSERT INTO vault_draft_policy_sources (policy_id, document_id, revision_id, revision_hash, grant_fingerprint, scope) SELECT ?, document_id, id, ?, 'grant', 'private-data' FROM vault_revisions WHERE id = ?")
        .bind(format!("policy-{task_id}")).bind(revision_hash).bind(revision_id).execute(pool).await.unwrap();
}

async fn count(pool: &sqlx::SqlitePool, table: &str, condition: &str) -> i64 {
    sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE {condition}"))
        .fetch_one(pool)
        .await
        .unwrap()
}
