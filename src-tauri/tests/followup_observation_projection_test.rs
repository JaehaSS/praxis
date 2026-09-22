//! Projection receipts and follow-up observation start as one transaction.

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::db;
use praxis_lib::memory::{self, knowledge_type, tier};
use praxis_lib::projector;

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Fixture {
    pool: sqlx::SqlitePool,
    root: std::path::PathBuf,
    db_path: std::path::PathBuf,
    memory_id: i64,
    task_id: i64,
    now: i64,
}

#[tokio::test]
async fn observation_failure_rolls_back_injection_usage_and_marker() {
    let fixture = fixture().await;
    let result = inject(&fixture).await;

    assert!(result.is_err());
    assert_rolled_back(&fixture).await;
    let _ = std::fs::remove_dir_all(fixture.root);
    let _ = std::fs::remove_file(fixture.db_path);
}

async fn fixture() -> Fixture {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = temp_root::dir().join(format!(
        "praxis-followup-projection-{}-{sequence}",
        std::process::id()
    ));
    let db_path = root.with_extension("sqlite");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("CLAUDE.md"), "# owner\n").unwrap();
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let now = now_ts();
    let memory_id = verified_memory(&pool, now).await;
    let task_id = db::insert_task(
        &pool,
        "/repo",
        "branch",
        "main",
        root.to_str().unwrap(),
        "memory",
        Some("claude"),
        None,
        "terminal",
        now,
    )
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER fail_followup_observation \
         BEFORE INSERT ON task_events \
         WHEN NEW.kind = 'followup_observation_started' \
         BEGIN SELECT RAISE(ABORT, 'observation unavailable'); END",
    )
    .execute(&pool)
    .await
    .unwrap();
    Fixture {
        pool,
        root,
        db_path,
        memory_id,
        task_id,
        now,
    }
}

async fn inject(fixture: &Fixture) -> anyhow::Result<usize> {
    memory::inject_into_worktree(
        &fixture.pool,
        "/repo",
        "memory",
        None,
        fixture.task_id,
        fixture.now + 1,
        &fixture.root,
        8,
        &projector::project_targets(),
    )
    .await
}

async fn assert_rolled_back(fixture: &Fixture) {
    let counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT \
           (SELECT COUNT(*) FROM memory_injections WHERE task_id = ?), \
           (SELECT COUNT(*) FROM memory_usages WHERE task_id = ?), \
           (SELECT COUNT(*) FROM task_events WHERE task_id = ? \
             AND kind = 'followup_observation_started'), \
           (SELECT usage_count FROM memories WHERE id = ?)",
    )
    .bind(fixture.task_id)
    .bind(fixture.task_id)
    .bind(fixture.task_id)
    .bind(fixture.memory_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(counts, (0, 0, 0, 0));
    assert_eq!(
        std::fs::read_to_string(fixture.root.join("CLAUDE.md")).unwrap(),
        "# owner\n"
    );
}

async fn verified_memory(pool: &sqlx::SqlitePool, now: i64) -> i64 {
    let memory_id = memory::create_candidate(
        pool,
        tier::PROJECT,
        Some("/repo"),
        knowledge_type::CONVENTION,
        "observe follow-up input after memory injection",
        Some("test"),
        now,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(pool, memory_id, now, None)
        .await
        .unwrap();
    memory::submit_for_review(pool, memory_id, now)
        .await
        .unwrap();
    memory::approve(pool, memory_id, "human", now)
        .await
        .unwrap();
    memory_id
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
