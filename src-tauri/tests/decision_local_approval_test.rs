#[path = "support/temp_root.rs"]
mod temp_root;

use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::decision::local_approval::{self, Route};
use praxis_lib::{db, decision, memory, worktree};

static COUNTER: AtomicU32 = AtomicU32::new(0);

fn git(root: &std::path::Path, args: &[&str]) {
    assert!(Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap()
        .success());
}

async fn fixture(label: &str) -> (std::path::PathBuf, String, sqlx::SqlitePool, i64) {
    let sequence = COUNTER.fetch_add(1, Ordering::SeqCst);
    let repo = temp_root::dir().join(format!(
        "praxis-local-approval-{label}-{}-{sequence}",
        std::process::id()
    ));
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["config", "user.email", "approval@example.test"]);
    git(&repo, &["config", "user.name", "Approval Test"]);
    std::fs::write(repo.join("README.md"), "before\n").unwrap();
    git(&repo, &["add", "README.md"]);
    git(&repo, &["commit", "-qm", "initial"]);
    let isolated = worktree::create_plain(&repo, &format!("praxis/{label}-{sequence}"), None).unwrap();
    let db_path = repo.with_extension("sqlite").to_string_lossy().into_owned();
    let pool = db::init_pool(&db_path).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let task_id = db::insert_task(
        &pool,
        repo.to_str().unwrap(),
        &isolated.branch,
        &isolated.base,
        isolated.path.to_str().unwrap(),
        "PRIVATE_LOCAL_APPROVAL_INSTRUCTION",
        None,
        None,
        "terminal",
        10,
    )
    .await
    .unwrap();
    db::update_state(&pool, task_id, db::state::AWAITING_REVIEW, 11)
        .await
        .unwrap();
    (repo, db_path, pool, task_id)
}

#[tokio::test]
async fn flag_routing_preserves_legacy_and_rejects_direct_before_claim() {
    let (repo, db_path, pool, isolated_id) = fixture("route").await;
    let isolated = db::get_task(&pool, isolated_id).await.unwrap().unwrap();
    assert_eq!(
        local_approval::route(&pool, &isolated).await.unwrap(),
        Route::Legacy
    );

    db::set_setting(&pool, decision::FLAG_KEY, "true")
        .await
        .unwrap();
    assert_eq!(
        local_approval::route(&pool, &isolated).await.unwrap(),
        Route::Ledger
    );
    let direct_id = insert_direct_task(&pool, &repo).await;
    assert_direct_rejected(&pool, direct_id).await;
    drop(pool);
    let _ = std::fs::remove_dir_all(repo);
    let _ = std::fs::remove_file(db_path);
}

async fn insert_direct_task(pool: &sqlx::SqlitePool, repo: &std::path::Path) -> i64 {
    let id = db::insert_task(
        pool,
        repo.to_str().unwrap(),
        "main",
        "main",
        repo.to_str().unwrap(),
        "direct",
        None,
        None,
        "terminal",
        20,
    )
    .await
    .unwrap();
    db::update_state(pool, id, db::state::AWAITING_REVIEW, 21)
        .await
        .unwrap();
    id
}

async fn assert_direct_rejected(pool: &sqlx::SqlitePool, task_id: i64) {
    let task = db::get_task(pool, task_id).await.unwrap().unwrap();
    let route_error = local_approval::route(pool, &task).await.unwrap_err();
    assert!(route_error
        .to_string()
        .contains("direct mode decision ledger"));
    let finalize_error = local_approval::finalize(pool, task_id, false, 22)
        .await
        .unwrap_err();
    assert!(finalize_error
        .to_string()
        .contains("direct mode decision ledger"));
    assert_eq!(
        db::get_task(pool, task_id).await.unwrap().unwrap().state,
        db::state::AWAITING_REVIEW
    );
    let journals: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM local_approval_finalizations WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(journals, 0);
}

#[tokio::test]
async fn isolated_approval_completes_git_and_the_ledger() {
    let (repo, db_path, pool, task_id) = fixture("happy").await;
    let task = db::get_task(&pool, task_id).await.unwrap().unwrap();
    write_approval_changes(&task);
    db::set_setting(&pool, decision::FLAG_KEY, "true")
        .await
        .unwrap();

    local_approval::finalize(&pool, task_id, true, 30)
        .await
        .unwrap();

    assert_approval_success(&pool, task_id, &repo, &task.worktree_path).await;
    assert_eq!(local_approval::reconcile(&pool, 31).await.unwrap(), 0);
    drop(pool);
    let _ = std::fs::remove_dir_all(repo);
    let _ = std::fs::remove_file(db_path);
}

fn write_approval_changes(task: &db::Task) {
    let worktree = std::path::Path::new(&task.worktree_path);
    std::fs::write(worktree.join("result.txt"), "approved\n").unwrap();
    std::fs::write(worktree.join(".mcp.json"), "PRIVATE_GENERATED_MCP\n").unwrap();
}

async fn assert_approval_success(
    pool: &sqlx::SqlitePool,
    task_id: i64,
    repo: &std::path::Path,
    worktree_path: &str,
) {
    assert_eq!(
        db::get_task(pool, task_id).await.unwrap().unwrap().state,
        db::state::DONE
    );
    assert_eq!(
        std::fs::read_to_string(repo.join("result.txt")).unwrap(),
        "approved\n"
    );
    assert!(!repo.join(".mcp.json").exists());
    assert!(!std::path::Path::new(worktree_path).exists());
    let facts: (String, bool, i64) = sqlx::query_as(
        "SELECT (SELECT state FROM local_approval_finalizations WHERE task_id = ?), \
                (SELECT exclude_generated_mcp FROM local_approval_finalizations WHERE task_id = ?), \
                (SELECT COUNT(*) FROM decision_records WHERE task_id = ?)",
    )
    .bind(task_id)
    .bind(task_id)
    .bind(task_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(facts, ("completed".into(), true, 1));
}
