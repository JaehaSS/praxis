use sqlx::SqlitePool;

use super::test_support::{symbol, test_pool};
use super::{generation, query, snapshot};

async fn run_and_file(pool: &SqlitePool, label: &str) -> (i64, i64) {
    let run = generation::start_run(pool, "/w", label, 10).await.unwrap();
    let file = snapshot::insert_file(pool, run, "src/lib.rs", "hash", "rust", None, None)
        .await
        .unwrap();
    (run, file)
}

async fn node(pool: &SqlitePool, run: i64, file: i64, name: &str, line: u32) -> i64 {
    snapshot::insert_node(
        pool,
        run,
        file,
        &symbol(name, (line, 3, line, 9), (line, 0, line + 1, 0)),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn cursor_prefers_selection_range_over_a_smaller_body() {
    let pool = test_pool("selection").await;
    let (run, file) = run_and_file(&pool, "fingerprint").await;
    let selected = snapshot::insert_node(
        &pool,
        run,
        file,
        &symbol("selected", (10, 4, 10, 12), (0, 0, 20, 0)),
    )
    .await
    .unwrap();
    snapshot::insert_node(
        &pool,
        run,
        file,
        &symbol("smaller_body", (11, 0, 11, 12), (10, 6, 10, 8)),
    )
    .await
    .unwrap();

    let found = snapshot::find_node_at(&pool, run, "src/lib.rs", 10, 7)
        .await
        .unwrap();
    assert_eq!(found.map(|item| item.id), Some(selected));

    sqlx::query("DELETE FROM code_graph_nodes WHERE id = ?")
        .bind(selected)
        .execute(&pool)
        .await
        .unwrap();
    let fallback = snapshot::find_node_at(&pool, run, "src/lib.rs", 10, 7)
        .await
        .unwrap();
    assert_eq!(
        fallback.map(|item| item.name).as_deref(),
        Some("smaller_body")
    );
}

#[tokio::test]
async fn impact_at_reads_only_the_active_generation() {
    let pool = test_pool("impact").await;
    let run = generation::start_run(&pool, "/w", "fingerprint", 10)
        .await
        .unwrap();
    let target_file = snapshot::insert_file(&pool, run, "src/target.rs", "a", "rust", None, None)
        .await
        .unwrap();
    let caller_file = snapshot::insert_file(&pool, run, "src/caller.rs", "b", "rust", None, None)
        .await
        .unwrap();
    let target = node(&pool, run, target_file, "target", 1).await;
    let caller = node(&pool, run, caller_file, "caller", 0).await;
    snapshot::add_edge(&pool, run, caller, target)
        .await
        .unwrap();
    generation::promote(&pool, run, generation::RunStats::default(), 20)
        .await
        .unwrap();

    let impact = query::impact_at(&pool, "/w", "src/target.rs", 1, 4, 2, "ready")
        .await
        .unwrap()
        .expect("커서 심볼");
    assert_eq!(impact.run_id, run);
    assert_eq!(impact.items[0].name, "caller");
    assert_eq!(impact.items[0].depth, 1);
    let json = serde_json::to_value(&impact).unwrap();
    assert_eq!(json["runId"], run);
    assert_eq!(json["freshness"], "ready");
    assert_eq!(json["items"][0]["relPath"], "src/caller.rs");
}

#[tokio::test]
async fn same_named_symbols_in_other_files_do_not_leak_into_cursor_impact() {
    let pool = test_pool("same-name").await;
    let run = generation::start_run(&pool, "/w", "fingerprint", 10)
        .await
        .unwrap();
    let a_file = snapshot::insert_file(&pool, run, "src/a.rs", "a", "rust", None, None)
        .await
        .unwrap();
    let b_file = snapshot::insert_file(&pool, run, "src/b.rs", "b", "rust", None, None)
        .await
        .unwrap();
    let caller_file = snapshot::insert_file(&pool, run, "src/caller.rs", "c", "rust", None, None)
        .await
        .unwrap();
    let a = node(&pool, run, a_file, "new", 0).await;
    node(&pool, run, b_file, "new", 0).await;
    let caller = node(&pool, run, caller_file, "caller", 0).await;
    snapshot::add_edge(&pool, run, caller, a).await.unwrap();
    generation::promote(&pool, run, generation::RunStats::default(), 20)
        .await
        .unwrap();

    let impact = query::impact_at(&pool, "/w", "src/b.rs", 0, 4, 2, "ready")
        .await
        .unwrap()
        .unwrap();
    assert!(impact.items.is_empty());
}

#[tokio::test]
async fn cycles_terminate_at_the_shortest_depth() {
    let pool = test_pool("cycle").await;
    let (run, file) = run_and_file(&pool, "fingerprint").await;
    let a = node(&pool, run, file, "a", 0).await;
    let b = node(&pool, run, file, "b", 3).await;
    snapshot::add_edge(&pool, run, b, a).await.unwrap();
    snapshot::add_edge(&pool, run, a, b).await.unwrap();
    generation::promote(&pool, run, generation::RunStats::default(), 20)
        .await
        .unwrap();

    let impact = query::impact_at(&pool, "/w", "src/lib.rs", 0, 4, 3, "ready")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(impact.items.len(), 1);
    assert_eq!(
        (impact.items[0].name.as_str(), impact.items[0].depth),
        ("b", 1)
    );
}

#[tokio::test]
async fn depth_is_clamped_to_three() {
    let pool = test_pool("depth").await;
    let (run, file) = run_and_file(&pool, "fingerprint").await;
    let target = node(&pool, run, file, "target", 0).await;
    let mut previous = target;
    for step in 1..=5 {
        let current = node(&pool, run, file, &format!("step_{step}"), step * 3).await;
        snapshot::add_edge(&pool, run, current, previous)
            .await
            .unwrap();
        previous = current;
    }
    generation::promote(&pool, run, generation::RunStats::default(), 20)
        .await
        .unwrap();

    let impact = query::impact_at(&pool, "/w", "src/lib.rs", 0, 4, 99, "ready")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(impact.items.len(), 3);
    assert!(impact.items.iter().all(|item| item.depth <= 3));
}

#[tokio::test]
async fn the_five_hundred_result_limit_is_reported() {
    let pool = test_pool("limit").await;
    let (run, file) = run_and_file(&pool, "fingerprint").await;
    let target = node(&pool, run, file, "target", 0).await;
    for n in 0..=query::MAX_RESULTS {
        let caller = node(&pool, run, file, &format!("caller_{n}"), n as u32 + 3).await;
        snapshot::add_edge(&pool, run, caller, target)
            .await
            .unwrap();
    }
    generation::promote(&pool, run, generation::RunStats::default(), 20)
        .await
        .unwrap();

    let impact = query::impact_at(&pool, "/w", "src/lib.rs", 0, 4, 1, "ready")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(impact.items.len(), query::MAX_RESULTS);
    assert!(impact.truncated);
}

/// ADR 0099가 금지한 것 — 엣지를 만들지 못한 파일이 `items: []`를 `freshness: "ready"`와 함께
/// 내면 "영향 없음"으로 읽힌다. 두 경우가 같은 저장 형태(엣지 0개)를 갖기 때문에,
/// 구분은 `edge_state`를 실어 내는 것뿐이다(설계 0065 DR-6 2항).
#[tokio::test]
async fn a_file_without_edges_is_distinguishable_from_a_file_with_no_impact() {
    let pool = test_pool("edges-unavailable").await;
    let run = generation::start_run(&pool, "/w", "fingerprint", 10)
        .await
        .unwrap();
    let unknown = snapshot::insert_file(
        &pool,
        run,
        "src/app.py",
        "a",
        "python",
        None,
        Some("pyright: 준비 신호가 없어 참조를 생략했다"),
    )
    .await
    .unwrap();
    let empty = snapshot::insert_file(&pool, run, "src/lib.rs", "b", "rust", None, None)
        .await
        .unwrap();
    node(&pool, run, unknown, "handler", 1).await;
    node(&pool, run, empty, "unused", 1).await;
    generation::promote(&pool, run, generation::RunStats::default(), 20)
        .await
        .unwrap();

    let unknown = query::impact_at(&pool, "/w", "src/app.py", 1, 4, 2, "ready")
        .await
        .unwrap()
        .expect("커서 심볼");
    let empty = query::impact_at(&pool, "/w", "src/lib.rs", 1, 4, 2, "ready")
        .await
        .unwrap()
        .expect("커서 심볼");

    assert!(unknown.items.is_empty() && empty.items.is_empty());
    assert_eq!(
        unknown.edges_unavailable.as_deref(),
        Some("pyright: 준비 신호가 없어 참조를 생략했다")
    );
    assert_eq!(empty.edges_unavailable, None);
    let json = serde_json::to_value(&unknown).unwrap();
    assert_eq!(json["freshness"], "ready");
    assert!(json["edgesUnavailable"].is_string());
}
