use crate::lspclient::LspPool;

use super::jobs::BuildJobs;
use super::test_support::test_pool;
use super::{build, generation, manifest};

/// 서버 가용성은 개발 기계마다 다르다. 여기 쓰는 워크트리는 **루트 마커가 없어** 서버 설치와
/// 무관하게 항상 `unavailable`이 되는 조합만 고른다 — 그래야 단언이 결정적이다.
fn worktree(label: &str) -> std::path::PathBuf {
    let root = crate::testtmp::dir().join(format!("codegraph-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[tokio::test]
async fn cancelled_finalization_keeps_previous_generation_active() {
    let pool = test_pool("build-finalize-cancelled").await;
    let root = crate::testtmp::dir().join(format!(
        "codegraph-build-finalize-cancelled-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "fn stable() {}\n").unwrap();
    let before = manifest::scan(&root).unwrap();
    let worktree = root.to_string_lossy();
    let active = generation::start_run(&pool, &worktree, &before.fingerprint, 10)
        .await
        .unwrap();
    generation::promote(&pool, active, generation::RunStats::default(), 20)
        .await
        .unwrap();
    let candidate = generation::start_run(&pool, &worktree, &before.fingerprint, 30)
        .await
        .unwrap();
    let after = manifest::scan(&root).unwrap();
    let jobs = BuildJobs::default();
    let guard = jobs.start(101).unwrap();
    assert!(jobs.cancel(101));

    let result = build::finalize_snapshot(
        &pool,
        candidate,
        generation::RunStats::default(),
        &before,
        &after,
        &guard,
        40,
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        generation::active_run_id(&pool, &worktree).await.unwrap(),
        Some(active)
    );
    assert_eq!(
        generation::run_state(&pool, candidate).await.unwrap(),
        "cancelled"
    );
}

#[tokio::test]
async fn promotion_cutoff_rejects_late_cancel_before_publish() {
    let pool = test_pool("build-promotion-cutoff").await;
    let worktree = "/codegraph/build-promotion-cutoff";
    let active = generation::start_run(&pool, worktree, "active", 10)
        .await
        .unwrap();
    generation::promote(&pool, active, generation::RunStats::default(), 20)
        .await
        .unwrap();
    let candidate = generation::start_run(&pool, worktree, "candidate", 30)
        .await
        .unwrap();
    let jobs = BuildJobs::default();
    let guard = jobs.start(102).unwrap();

    assert!(guard.begin_promotion());
    assert!(!jobs.cancel(102));
    generation::promote(&pool, candidate, generation::RunStats::default(), 40)
        .await
        .unwrap();

    assert_eq!(
        generation::active_run_id(&pool, worktree).await.unwrap(),
        Some(candidate)
    );
    assert_eq!(
        generation::run_state(&pool, candidate).await.unwrap(),
        "ready"
    );
}

#[tokio::test]
async fn promotion_failure_keeps_old_active_and_terminalizes_candidate() {
    let pool = test_pool("build-promotion-failure").await;
    let root = crate::testtmp::dir().join(format!(
        "codegraph-build-promotion-failure-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "fn stable() {}\n").unwrap();
    let before = manifest::scan(&root).unwrap();
    let after = manifest::scan(&root).unwrap();
    let worktree = root.to_string_lossy();
    let active = generation::start_run(&pool, &worktree, &before.fingerprint, 10)
        .await
        .unwrap();
    generation::promote(&pool, active, generation::RunStats::default(), 20)
        .await
        .unwrap();
    let candidate = generation::start_run(&pool, &worktree, &before.fingerprint, 30)
        .await
        .unwrap();
    sqlx::query(
        "CREATE TRIGGER fail_active_promotion BEFORE UPDATE ON code_graph_active \
         BEGIN SELECT RAISE(ABORT, 'forced promotion failure'); END",
    )
    .execute(&pool)
    .await
    .unwrap();
    let jobs = BuildJobs::default();
    let guard = jobs.start(103).unwrap();

    let result = build::finalize_snapshot(
        &pool,
        candidate,
        generation::RunStats::default(),
        &before,
        &after,
        &guard,
        40,
    )
    .await;

    assert!(result.is_err());
    assert_eq!(
        generation::active_run_id(&pool, &worktree).await.unwrap(),
        Some(active)
    );
    assert_eq!(
        generation::run_state(&pool, candidate).await.unwrap(),
        "degraded"
    );
}

const CARGO_TOML: &str = "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2021\"\n";

#[test]
fn groups_split_by_server_and_flag_the_missing_one() {
    let root = worktree("groups-split");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), CARGO_TOML).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn stable() {}\n").unwrap();
    std::fs::write(root.join("Api.java"), "class Api {}\n").unwrap();
    let source = manifest::scan(&root).unwrap();

    let groups = build::group_by_server(&source, &root);

    assert_eq!(groups.len(), 2);
    let java = groups.iter().find(|g| g.spec.key == "jdtls").unwrap();
    let rust = groups
        .iter()
        .find(|g| g.spec.key == "rust-analyzer")
        .unwrap();
    assert_eq!(
        java.files
            .iter()
            .map(|f| f.rel_path.as_str())
            .collect::<Vec<_>>(),
        ["Api.java"]
    );
    assert_eq!(
        rust.files
            .iter()
            .map(|f| f.rel_path.as_str())
            .collect::<Vec<_>>(),
        ["src/lib.rs"]
    );
    // 루트 마커가 없으므로 jdtls 설치 여부와 무관하게 항상 사유가 붙는다.
    assert!(java.unavailable.as_deref().unwrap().contains("pom.xml"));
}

#[tokio::test]
async fn unavailable_group_records_skip_reason_and_language_id() {
    let pool = test_pool("build-skip-reason").await;
    let root = worktree("skip-reason");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), CARGO_TOML).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn stable() {}\n").unwrap();
    let source = manifest::scan(&root).unwrap();
    let mut groups = build::group_by_server(&source, &root);
    groups[0].unavailable = Some("테스트: 서버를 찾지 못했습니다".to_string());
    let run = generation::start_run(&pool, &root.to_string_lossy(), &source.fingerprint, 10)
        .await
        .unwrap();
    let jobs = BuildJobs::default();
    let guard = jobs.start(201).unwrap();
    let mut stats = generation::RunStats::default();

    build::collect_symbols(
        &pool,
        &LspPool::default(),
        201,
        &root,
        run,
        &guard,
        &groups[0],
        &mut stats,
    )
    .await
    .unwrap();

    assert_eq!(stats.files_skipped, 1);
    assert_eq!(stats.files_indexed, 0);
    assert_eq!(stats.symbols, 0);
    let (lang, skip_reason, edge_state): (String, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT lang, skip_reason, edge_state FROM code_graph_files WHERE run_id = ?",
    )
    .bind(run)
    .fetch_one(&pool)
    .await
    .unwrap();
    // languageId여야 한다 — 서버 키가 아니다(설계 0065 DR-4).
    assert_eq!(lang, "rust");
    assert_ne!(lang, "rust-analyzer");
    assert_eq!(
        skip_reason.as_deref(),
        Some("테스트: 서버를 찾지 못했습니다")
    );
    assert_eq!(edge_state, None);
}

#[tokio::test]
async fn run_fails_only_when_every_server_is_unavailable() {
    let pool = test_pool("build-all-unavailable").await;
    let root = worktree("all-unavailable");
    std::fs::write(root.join("Api.java"), "class Api {}\n").unwrap();
    let jobs = BuildJobs::default();
    let guard = jobs.start(202).unwrap();

    let result = build::index_worktree(&pool, &LspPool::default(), 202, &root, &guard, 10).await;

    let message = result.unwrap_err().to_string();
    assert!(
        message.contains("사용할 수 있는 언어 서버가 없습니다"),
        "{message}"
    );
    assert_eq!(
        generation::active_run_id(&pool, &root.to_string_lossy())
            .await
            .unwrap(),
        None
    );
    let (state,): (String,) = sqlx::query_as("SELECT state FROM code_graph_runs")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "degraded");
}

#[tokio::test]
async fn empty_worktree_still_promotes_an_empty_graph() {
    let pool = test_pool("build-empty-worktree").await;
    let root = worktree("empty-worktree");
    let jobs = BuildJobs::default();
    let guard = jobs.start(203).unwrap();

    let report = build::index_worktree(&pool, &LspPool::default(), 203, &root, &guard, 10)
        .await
        .unwrap();

    assert_eq!(report.state, "ready");
    assert_eq!(report.files_seen, 0);
    assert_eq!(report.files_indexed, 0);
    assert_eq!(report.files_skipped, 0);
}

#[tokio::test]
#[ignore = "실측: rust-analyzer 설치 필요"]
async fn missing_server_skips_its_language_and_keeps_the_other() {
    let pool = test_pool("build-partial-availability").await;
    let root = worktree("partial-availability");
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), CARGO_TOML).unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn stable() {}\n").unwrap();
    std::fs::write(root.join("Api.java"), "class Api {}\n").unwrap();
    let jobs = BuildJobs::default();
    let guard = jobs.start(204).unwrap();

    let report = build::index_worktree(&pool, &LspPool::default(), 204, &root, &guard, 10)
        .await
        .unwrap();

    assert_eq!(report.state, "ready");
    assert_eq!(report.files_skipped, 1);
    assert_eq!(report.files_indexed, 1);
    assert!(report.symbols > 0, "러스트 심볼이 남아야 한다");
    let rows: Vec<(String, String, Option<String>)> = sqlx::query_as(
        "SELECT rel_path, lang, skip_reason FROM code_graph_files WHERE run_id = ? ORDER BY rel_path",
    )
    .bind(report.run_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows[0].0, "Api.java");
    assert_eq!(rows[0].1, "java");
    assert!(rows[0].2.is_some(), "jdtls가 없으면 사유가 남아야 한다");
    assert_eq!(rows[1].0, "src/lib.rs");
    assert_eq!(rows[1].1, "rust");
    assert_eq!(rows[1].2, None);
}

#[tokio::test]
#[ignore = "실측: pyright 설치 필요"]
async fn unsupported_readiness_keeps_symbols_and_marks_edge_state() {
    let pool = test_pool("build-unsupported-edges").await;
    let root = worktree("unsupported-edges");
    std::fs::write(root.join("pyproject.toml"), "[project]\nname = \"probe\"\n").unwrap();
    std::fs::write(root.join("m.py"), "def stable():\n    return 1\n").unwrap();
    let jobs = BuildJobs::default();
    let guard = jobs.start(205).unwrap();

    let report = build::index_worktree(&pool, &LspPool::default(), 205, &root, &guard, 10)
        .await
        .unwrap();

    // 이번 작업의 핵심 단언 — 엣지가 없어도 런은 성공이고 심볼은 남는다(DR-2b·DR-3).
    assert_eq!(report.state, "ready");
    assert_eq!(report.files_skipped, 0);
    assert!(
        report.symbols > 0,
        "pyright의 심볼은 준비와 무관하게 정확하다"
    );
    assert_eq!(report.edges, 0);
    let (skip_reason, edge_state): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT skip_reason, edge_state FROM code_graph_files WHERE run_id = ?")
            .bind(report.run_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(skip_reason, None);
    assert!(edge_state.is_some(), "엣지가 없는 사유가 남아야 한다");
}
