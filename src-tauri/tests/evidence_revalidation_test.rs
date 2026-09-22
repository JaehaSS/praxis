//! Backend-minted code evidence and fail-closed source revalidation.

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::{db, evidence, memory};

static COUNTER: AtomicU32 = AtomicU32::new(0);

struct Fixture {
    pool: sqlx::SqlitePool,
    root: std::path::PathBuf,
    db_path: std::path::PathBuf,
    memory_id: i64,
}

#[tokio::test]
async fn code_evidence_is_minted_by_the_backend_and_source_changes_stale_memory() {
    let fixture = fixture("code-change", memory::tier::PROJECT).await;
    std::fs::write(fixture.root.join("src/lib.rs"), b"alpha\nbeta\ngamma\n").unwrap();
    git(&fixture.root, &["add", "src/lib.rs"]);
    git(&fixture.root, &["commit", "-qm", "add source"]);

    evidence::add_code_location(
        &fixture.pool,
        fixture.memory_id,
        evidence::CodeLocationInput {
            relative_path: "src/lib.rs".into(),
            line_start: 2,
            line_end: 2,
        },
        100,
    )
    .await
    .unwrap();
    memory::submit_for_review(&fixture.pool, fixture.memory_id, 101)
        .await
        .unwrap();
    memory::approve(&fixture.pool, fixture.memory_id, "human", 102)
        .await
        .unwrap();

    std::fs::write(fixture.root.join("src/lib.rs"), b"alpha\nchanged\ngamma\n").unwrap();
    evidence::revalidate_memory(&fixture.pool, fixture.memory_id, 103)
        .await
        .unwrap();

    let evidence_status: (String,) = sqlx::query_as(
        "SELECT status FROM memory_evidence WHERE memory_id = ? AND kind = 'code_location'",
    )
    .bind(fixture.memory_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    let memory_status: (String, Option<i64>) =
        sqlx::query_as("SELECT status, stale_at FROM memories WHERE id = ?")
            .bind(fixture.memory_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap();
    assert_eq!(evidence_status.0, memory::evidence_status::CHANGED);
    assert_eq!(
        memory_status,
        (memory::knowledge_status::STALE.into(), Some(103))
    );
    cleanup(fixture);
}

#[tokio::test]
async fn code_evidence_rejects_scope_escape_global_scope_and_unsafe_test_runs() {
    let fixture = fixture("code-policy", memory::tier::PROJECT).await;
    let escaped = evidence::add_code_location(
        &fixture.pool,
        fixture.memory_id,
        evidence::CodeLocationInput {
            relative_path: "../outside.rs".into(),
            line_start: 1,
            line_end: 1,
        },
        100,
    )
    .await;
    assert!(escaped.is_err());
    assert!(
        evidence::add_test_run(&fixture.pool, fixture.memory_id, 1, 100)
            .await
            .is_err()
    );

    let global = memory::create_candidate(
        &fixture.pool,
        memory::tier::GLOBAL,
        None,
        memory::knowledge_type::CLAIM,
        "global claim",
        Some("test"),
        100,
    )
    .await
    .unwrap();
    assert!(evidence::add_code_location(
        &fixture.pool,
        global,
        evidence::CodeLocationInput {
            relative_path: "src/lib.rs".into(),
            line_start: 1,
            line_end: 1,
        },
        100,
    )
    .await
    .is_err());
    cleanup(fixture);
}

async fn fixture(label: &str, tier: &str) -> Fixture {
    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let root = temp_root::dir().join(format!(
        "praxis-evidence-{label}-{}-{suffix}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "evidence@example.test"]);
    git(&root, &["config", "user.name", "Evidence Test"]);
    let root = root.canonicalize().unwrap();
    let db_path = root.with_extension("sqlite");
    let pool = db::init_pool(db_path.to_str().unwrap()).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    let scope = (tier == memory::tier::PROJECT).then(|| root.to_string_lossy().into_owned());
    let memory_id = memory::create_candidate(
        &pool,
        tier,
        scope.as_deref(),
        memory::knowledge_type::CLAIM,
        "backend evidence claim",
        Some("test"),
        99,
    )
    .await
    .unwrap();
    Fixture {
        pool,
        root,
        db_path,
        memory_id,
    }
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
    let _ = std::fs::remove_file(&fixture.db_path);
    let _ = std::fs::remove_dir_all(fixture.root);
}
