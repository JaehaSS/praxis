//! Ephemeral projection cleanup before an approved worktree can be merged.

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::{db, memory, projector};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Fixture {
    pool: sqlx::SqlitePool,
    root: std::path::PathBuf,
    db_path: std::path::PathBuf,
    task_id: i64,
}

async fn fixture() -> Fixture {
    fixture_with_memory(true).await
}

async fn fixture_with_memory(approved_memory: bool) -> Fixture {
    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let base = temp_root::dir().join(format!("praxis-retire-{}-{suffix}", std::process::id()));
    let db_path = base.with_extension("sqlite");
    let root = base.with_extension("worktree");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("AGENTS.md"), "# owner content\n").unwrap();
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let now = 2_000_000_000;
    if approved_memory {
        let memory_id = memory::create_candidate(
            &pool,
            memory::tier::PROJECT,
            Some("/repo"),
            memory::knowledge_type::DECISION,
            "retain the owner file",
            Some("test"),
            now,
        )
        .await
        .unwrap();
        memory::add_user_confirmation(&pool, memory_id, now + 1, None)
            .await
            .unwrap();
        memory::submit_for_review(&pool, memory_id, now + 2)
            .await
            .unwrap();
        memory::approve(&pool, memory_id, "human", now + 3)
            .await
            .unwrap();
    }
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        root.to_str().unwrap(),
        "retain owner file",
        Some("claude"),
        None,
        "terminal",
        now + 4,
    )
    .await
    .unwrap();
    memory::inject_into_worktree(
        &pool,
        "/repo",
        "retain owner file",
        None,
        task_id,
        now + 5,
        &root,
        memory::INJECTION_LIMIT,
        &projector::project_targets(),
    )
    .await
    .unwrap();
    Fixture {
        pool,
        root,
        db_path,
        task_id,
    }
}

#[tokio::test]
async fn retirement_restores_exact_preimage_and_is_idempotent() {
    let fixture = fixture().await;
    assert!(std::fs::read_to_string(fixture.root.join("AGENTS.md"))
        .unwrap()
        .contains("PRAXIS MEMORY START"));

    memory::retire_task_projection(&fixture.pool, fixture.task_id, 2_000_000_010)
        .await
        .unwrap();
    memory::retire_task_projection(&fixture.pool, fixture.task_id, 2_000_000_011)
        .await
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(fixture.root.join("AGENTS.md")).unwrap(),
        "# owner content\n"
    );
    let journal: (String, Option<String>) = sqlx::query_as(
        "SELECT state, preimages_json FROM memory_projection_journal WHERE task_id = ?",
    )
    .bind(fixture.task_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(journal, ("retired".to_string(), None));
    cleanup(fixture);
}

#[tokio::test]
async fn retirement_refuses_to_overwrite_independent_target_edits() {
    let fixture = fixture().await;
    let path = fixture.root.join("AGENTS.md");
    let mut changed = std::fs::read_to_string(&path).unwrap();
    changed.push_str("\nagent-owned edit\n");
    std::fs::write(&path, &changed).unwrap();

    assert!(
        memory::retire_task_projection(&fixture.pool, fixture.task_id, 2_000_000_010)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read_to_string(path).unwrap(), changed);
    let journal: (String, Option<String>) = sqlx::query_as(
        "SELECT state, preimages_json FROM memory_projection_journal WHERE task_id = ?",
    )
    .bind(fixture.task_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(journal.0, "degraded");
    assert!(journal.1.is_some());
    cleanup(fixture);
}

#[tokio::test]
async fn retirement_accepts_changed_target_when_no_projection_bytes_remain() {
    let fixture = fixture_with_memory(false).await;
    let path = fixture.root.join("AGENTS.md");
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("PRAXIS MEMORY START"));
    std::fs::write(
        &path,
        "# owner content\n\nreal instructions committed later\n",
    )
    .unwrap();

    memory::retire_task_projection(&fixture.pool, fixture.task_id, 2_000_000_010)
        .await
        .unwrap();

    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "# owner content\n\nreal instructions committed later\n"
    );
    let journal: (String, Option<String>) = sqlx::query_as(
        "SELECT state, preimages_json FROM memory_projection_journal WHERE task_id = ?",
    )
    .bind(fixture.task_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(journal, ("retired".to_string(), None));
    cleanup(fixture);
}

#[tokio::test]
async fn retirement_recovers_degraded_journal_once_markers_are_gone() {
    let fixture = fixture().await;
    let path = fixture.root.join("AGENTS.md");
    let mut changed = std::fs::read_to_string(&path).unwrap();
    changed.push_str("\nagent-owned edit\n");
    std::fs::write(&path, &changed).unwrap();
    assert!(
        memory::retire_task_projection(&fixture.pool, fixture.task_id, 2_000_000_010)
            .await
            .is_err()
    );

    std::fs::write(&path, "# owner content\n\nagent-owned edit\n").unwrap();
    memory::retire_task_projection(&fixture.pool, fixture.task_id, 2_000_000_011)
        .await
        .unwrap();

    let journal: (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT state, preimages_json, failure_reason \
         FROM memory_projection_journal WHERE task_id = ?",
    )
    .bind(fixture.task_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(journal, ("retired".to_string(), None, None));
    cleanup(fixture);
}

/// 워크트리가 사라진 뒤에도 폐기가 진행돼야 한다. 여기서 막히면 `task_discard`가
/// 은퇴 단계에서 되돌려져, 워크트리 없는 작업이 목록에서 영영 지워지지 않는다.
#[tokio::test]
async fn retirement_succeeds_when_the_worktree_is_gone() {
    let fixture = fixture().await;
    assert!(std::fs::read_to_string(fixture.root.join("AGENTS.md"))
        .unwrap()
        .contains("PRAXIS MEMORY START"));
    std::fs::remove_dir_all(&fixture.root).unwrap();

    memory::retire_task_projection(&fixture.pool, fixture.task_id, 2_000_000_010)
        .await
        .unwrap();

    let journal: (String, Option<String>) = sqlx::query_as(
        "SELECT state, preimages_json FROM memory_projection_journal WHERE task_id = ?",
    )
    .bind(fixture.task_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(journal, ("retired".to_string(), None));
    cleanup(fixture);
}

fn cleanup(fixture: Fixture) {
    let _ = std::fs::remove_dir_all(fixture.root);
    let _ = std::fs::remove_file(fixture.db_path);
}
