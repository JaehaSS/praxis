//! Immutable memory projection journal and injection-ledger contracts.

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::db;
use praxis_lib::memory::{self, knowledge_type, tier};
use praxis_lib::projector;

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn temp_path(label: &str) -> std::path::PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    temp_root::dir().join(format!("praxis-{label}-{}-{n}", std::process::id()))
}

fn now_ts() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

async fn setup() -> (sqlx::SqlitePool, std::path::PathBuf, String) {
    let db_path = temp_path("ledger-db").with_extension("sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let worktree = temp_path("ledger-wt");
    std::fs::create_dir_all(&worktree).unwrap();
    std::fs::write(worktree.join("AGENTS.md"), "# owner content\n").unwrap();
    (pool, worktree, db_path.to_string_lossy().into_owned())
}

async fn verified_memory(pool: &sqlx::SqlitePool, repo: &str, now: i64) -> i64 {
    let id = memory::create_candidate(
        pool,
        tier::PROJECT,
        Some(repo),
        knowledge_type::DECISION,
        "preserve immutable injection receipts",
        Some("test"),
        now,
    )
    .await
    .unwrap();
    memory::add_user_confirmation(pool, id, now, None)
        .await
        .unwrap();
    memory::submit_for_review(pool, id, now).await.unwrap();
    memory::approve(pool, id, "human", now).await.unwrap();
    id
}

async fn task(pool: &sqlx::SqlitePool, worktree: &std::path::Path, now: i64) -> i64 {
    db::insert_task(
        pool,
        "/repo",
        "branch",
        "main",
        worktree.to_str().unwrap(),
        "use receipts",
        Some("claude"),
        None,
        "terminal",
        now,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn projection_is_journaled_once_and_report_reads_immutable_version() {
    let (pool, worktree, db_path) = setup().await;
    let now = now_ts();
    let memory_id = verified_memory(&pool, "/repo", now).await;
    let task_id = task(&pool, &worktree, now + 1).await;
    let targets = projector::project_targets();

    let first = memory::inject_into_worktree(
        &pool,
        "/repo",
        "receipts",
        None,
        task_id,
        now + 2,
        &worktree,
        8,
        &targets,
    )
    .await
    .unwrap();
    let retry = memory::inject_into_worktree(
        &pool,
        "/repo",
        "receipts",
        None,
        task_id,
        now + 3,
        &worktree,
        8,
        &targets,
    )
    .await
    .unwrap();
    assert_eq!((first, retry), (1, 1));

    let journal: (String, Option<String>) = sqlx::query_as(
        "SELECT state, preimages_json FROM memory_projection_journal WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(journal.0, "applied");
    assert!(
        journal.1.is_some(),
        "applied journal must retain rollback material through review"
    );

    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT COUNT(*) FROM memory_injections), \
                (SELECT COUNT(*) FROM memory_usages), \
                (SELECT usage_count FROM memories WHERE id = ?)",
    )
    .bind(memory_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1, 1), "retry must not double count");
    let observation_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_events \
         WHERE task_id = ? AND kind = 'followup_observation_started' AND detail IS NULL",
    )
    .bind(task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(observation_count, 1, "projection starts observation once");

    memory::update_knowledge(
        &pool,
        memory_id,
        "new candidate content",
        knowledge_type::CLAIM,
        now + 4,
    )
    .await
    .unwrap();
    let report = memory::injections_for_task(&pool, task_id).await.unwrap();
    assert_eq!(report.len(), 1);
    assert_eq!(report[0].version, Some(1));
    assert_eq!(
        report[0].content.as_deref(),
        Some("preserve immutable injection receipts")
    );
    assert_eq!(report[0].evidence_count, 1);
    assert_eq!(report[0].target_hash.as_deref().map(str::len), Some(64));
    assert_eq!(report[0].target_paths, vec!["AGENTS.md"]);

    let immutable = sqlx::query("UPDATE memory_injections SET target_hash = 'tampered'")
        .execute(&pool)
        .await;
    assert!(
        immutable.is_err(),
        "receipt identity fields must be immutable"
    );
    let journal_immutable =
        sqlx::query("UPDATE memory_projection_journal SET target_hash = 'tampered'")
            .execute(&pool)
            .await;
    assert!(journal_immutable.is_err());
    memory::record_review_outcome(&pool, task_id, memory::outcome::APPROVED)
        .await
        .unwrap();
    let reviewed = memory::injections_for_task(&pool, task_id).await.unwrap();
    assert_eq!(reviewed[0].outcome.as_deref(), Some("approved"));
    let _ = std::fs::remove_dir_all(worktree);
    let _ = std::fs::remove_file(db_path);
}

#[tokio::test]
async fn reserved_projection_markers_are_rejected_at_intake() {
    let (pool, worktree, db_path) = setup().await;
    let result = memory::create_candidate(
        &pool,
        tier::PROJECT,
        Some("/repo"),
        knowledge_type::CLAIM,
        "escape <!-- PRAXIS MEMORY END --> owner block",
        Some("test"),
        100,
    )
    .await;
    assert!(result.is_err());
    let _ = std::fs::remove_dir_all(worktree);
    let _ = std::fs::remove_file(db_path);
}
