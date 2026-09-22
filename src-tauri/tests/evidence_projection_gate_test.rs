//! Source evidence must be re-observed at selection and pre-spawn gates.

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::{db, evidence, memory, projector};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Fixture {
    pool: sqlx::SqlitePool,
    root: std::path::PathBuf,
    db_path: std::path::PathBuf,
    memory_id: i64,
    now: i64,
}

#[tokio::test]
async fn projection_selection_revalidates_changed_sources() {
    let fixture = fixture("selection").await;
    std::fs::write(fixture.root.join("src/lib.rs"), "changed\n").unwrap();
    let task_id = task(&fixture).await;
    let targets = projector::project_targets();

    let count = memory::inject_into_worktree(
        &fixture.pool,
        fixture.root.to_string_lossy().as_ref(),
        "source-backed claim",
        None,
        task_id,
        fixture.now + 3,
        &fixture.root,
        8,
        &targets,
    )
    .await
    .unwrap();

    assert_eq!(count, 0);
    assert_eq!(
        memory_status(&fixture).await,
        memory::knowledge_status::STALE
    );
    cleanup(fixture);
}

#[tokio::test]
async fn pre_spawn_projection_gate_reobserves_original_sources() {
    let fixture = fixture("pre-spawn").await;
    let task_id = task(&fixture).await;
    let targets = projector::project_targets();
    assert_eq!(
        memory::inject_into_worktree(
            &fixture.pool,
            fixture.root.to_string_lossy().as_ref(),
            "source-backed claim",
            None,
            task_id,
            fixture.now + 3,
            &fixture.root,
            8,
            &targets,
        )
        .await
        .unwrap(),
        1
    );
    let checks: (String, String) = sqlx::query_as(
        "SELECT j.source_check_ids_json, i.source_check_ids_json \
         FROM memory_projection_journal j JOIN memory_injections i ON i.projection_id = j.id \
         WHERE j.task_id = ?",
    )
    .bind(task_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(checks.0, checks.1);
    assert!(!serde_json::from_str::<Vec<i64>>(&checks.0)
        .unwrap()
        .is_empty());
    std::fs::write(fixture.root.join("src/lib.rs"), "changed\n").unwrap();

    assert!(
        memory::verify_task_projection(&fixture.pool, task_id, fixture.now + 4)
            .await
            .is_err()
    );
    assert_eq!(
        memory_status(&fixture).await,
        memory::knowledge_status::STALE
    );
    cleanup(fixture);
}

async fn fixture(label: &str) -> Fixture {
    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = temp_root::dir().join(format!(
        "praxis-evidence-gate-{label}-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "original\n").unwrap();
    std::fs::write(root.join("CLAUDE.md"), "# owner\n").unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "gate@example.test"]);
    git(&root, &["config", "user.name", "Gate Test"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "initial"]);
    let root = root.canonicalize().unwrap();
    let db_path = root.with_extension("sqlite");
    let _ = std::fs::remove_file(&db_path);
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let now = 2_000_000_000;
    let memory_id = memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some(root.to_string_lossy().as_ref()),
        memory::knowledge_type::CLAIM,
        "source-backed claim",
        Some("test"),
        now,
    )
    .await
    .unwrap();
    evidence::add_code_location(
        &pool,
        memory_id,
        evidence::CodeLocationInput {
            relative_path: "src/lib.rs".into(),
            line_start: 1,
            line_end: 1,
        },
        now,
    )
    .await
    .unwrap();
    memory::submit_for_review(&pool, memory_id, now + 1)
        .await
        .unwrap();
    memory::approve(&pool, memory_id, "human", now + 2)
        .await
        .unwrap();
    Fixture {
        pool,
        root,
        db_path,
        memory_id,
        now,
    }
}

async fn task(fixture: &Fixture) -> i64 {
    db::insert_task(
        &fixture.pool,
        fixture.root.to_string_lossy().as_ref(),
        "branch",
        "main",
        fixture.root.to_string_lossy().as_ref(),
        "source-backed claim",
        Some("claude"),
        None,
        "terminal",
        fixture.now,
    )
    .await
    .unwrap()
}

async fn memory_status(fixture: &Fixture) -> String {
    sqlx::query_scalar("SELECT status FROM memories WHERE id = ?")
        .bind(fixture.memory_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap()
}

fn git(root: &std::path::Path, args: &[&str]) {
    assert!(std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn cleanup(fixture: Fixture) {
    let _ = std::fs::remove_file(fixture.db_path);
    let _ = std::fs::remove_dir_all(fixture.root);
}
