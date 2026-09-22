use super::test_support::test_pool;
use super::{generation, manifest, status};

#[tokio::test]
async fn status_distinguishes_ready_stale_and_failed_rebuild() {
    let pool = test_pool("status").await;
    let root = crate::testtmp::dir().join(format!("codegraph-status-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    let source = root.join("src/lib.rs");
    std::fs::write(&source, "fn ready() {}\n").unwrap();
    let first_manifest = manifest::scan(&root).unwrap();
    let worktree = root.to_string_lossy();
    let first = generation::start_run(&pool, &worktree, &first_manifest.fingerprint, 10)
        .await
        .unwrap();
    generation::promote(&pool, first, generation::RunStats::default(), 20)
        .await
        .unwrap();

    let ready = status::load(&pool, &root).await.unwrap();
    assert_eq!(ready.active_state, "ready");
    assert_eq!(ready.build_state, "idle");

    std::fs::write(&source, "fn changed() {}\n").unwrap();
    let stale = status::load(&pool, &root).await.unwrap();
    assert_eq!(stale.active_state, "stale");

    let second = generation::start_run(&pool, &worktree, "new", 30)
        .await
        .unwrap();
    generation::finish_failed(&pool, second, "references 실패", 40)
        .await
        .unwrap();
    let failed = status::load(&pool, &root).await.unwrap();
    assert_eq!(failed.active_run_id, Some(first));
    assert_eq!(failed.build_state, "degraded");
    assert_eq!(failed.detail.as_deref(), Some("references 실패"));
}

#[tokio::test]
async fn running_rebuild_keeps_changed_active_generation_stale() {
    let pool = test_pool("status-running-stale").await;
    let root = crate::testtmp::dir().join(format!(
        "codegraph-status-running-stale-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    let source = root.join("src/lib.rs");
    std::fs::write(&source, "fn ready() {}\n").unwrap();
    let first_manifest = manifest::scan(&root).unwrap();
    let worktree = root.to_string_lossy();
    let first = generation::start_run(&pool, &worktree, &first_manifest.fingerprint, 10)
        .await
        .unwrap();
    generation::promote(&pool, first, generation::RunStats::default(), 20)
        .await
        .unwrap();

    std::fs::write(&source, "fn changed_during_rebuild() {}\n").unwrap();
    let second_manifest = manifest::scan(&root).unwrap();
    let second = generation::start_run(&pool, &worktree, &second_manifest.fingerprint, 30)
        .await
        .unwrap();

    let status = status::load(&pool, &root).await.unwrap();
    assert_eq!(status.active_run_id, Some(first));
    assert_eq!(status.active_state, "stale");
    assert_eq!(status.build_run_id, Some(second));
    assert_eq!(status.build_state, "indexing_symbols");
}

/// 완결성은 신선도와 직교한다(설계 0065 DR-6) — `edge_state`가 섞여 있어도 `active_state`는
/// `ready`다. 하나의 enum에 접으면 `generation.rs`의 세대 보존 SQL이 그 런을 지워
/// 롤백 지점을 잃는다.
#[tokio::test]
async fn incompleteness_is_reported_beside_a_ready_freshness() {
    let pool = test_pool("status-incomplete").await;
    let root = crate::testtmp::dir().join(format!(
        "codegraph-status-incomplete-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "fn ready() {}\n").unwrap();
    let scanned = manifest::scan(&root).unwrap();
    let worktree = root.to_string_lossy();
    let run = generation::start_run(&pool, &worktree, &scanned.fingerprint, 10)
        .await
        .unwrap();
    super::snapshot::insert_file(&pool, run, "src/lib.rs", "a", "rust", None, None)
        .await
        .unwrap();
    super::snapshot::insert_file(
        &pool,
        run,
        "src/app.py",
        "b",
        "python",
        None,
        Some("pyright 준비 신호 없음"),
    )
    .await
    .unwrap();
    super::snapshot::insert_file(
        &pool,
        run,
        "src/App.java",
        "c",
        "java",
        Some("jdtls 없음"),
        None,
    )
    .await
    .unwrap();
    generation::promote(&pool, run, generation::RunStats::default(), 20)
        .await
        .unwrap();

    let status = status::load(&pool, &root).await.unwrap();
    assert_eq!(status.active_state, "ready");
    let incomplete = status.incomplete.expect("불완전 집계");
    assert_eq!(incomplete.files_skipped, 1);
    assert_eq!(incomplete.files_without_edges, 1);
    assert_eq!(incomplete.languages_without_edges, vec!["python".to_owned()]);
    assert_eq!(
        incomplete.detail,
        "java: jdtls 없음 · python: pyright 준비 신호 없음"
    );
}

#[tokio::test]
async fn a_complete_run_reports_no_incompleteness() {
    let pool = test_pool("status-complete").await;
    let root = crate::testtmp::dir().join(format!("codegraph-status-complete-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "fn ready() {}\n").unwrap();
    let scanned = manifest::scan(&root).unwrap();
    let worktree = root.to_string_lossy();
    let run = generation::start_run(&pool, &worktree, &scanned.fingerprint, 10)
        .await
        .unwrap();
    super::snapshot::insert_file(&pool, run, "src/lib.rs", "a", "rust", None, None)
        .await
        .unwrap();
    generation::promote(&pool, run, generation::RunStats::default(), 20)
        .await
        .unwrap();

    assert_eq!(status::load(&pool, &root).await.unwrap().incomplete, None);
}
