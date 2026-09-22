//! 코드 그래프 스키마 검증 — 멱등성과 CASCADE, 그리고 워크트리 정리.

use std::sync::atomic::{AtomicU32, Ordering};

use sqlx::SqlitePool;

static DATABASE_COUNTER: AtomicU32 = AtomicU32::new(0);

async fn test_pool() -> SqlitePool {
    let sequence = DATABASE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = crate::testtmp::dir().join(format!(
        "praxis-codegraph-{}-{sequence}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = crate::db::init_pool(path.to_str().unwrap()).await.unwrap();
    super::migrate(&pool).await.unwrap();
    pool
}

/// 파일 하나 + 그 안의 심볼 하나를 넣고 노드 id를 준다.
async fn seed_symbol(pool: &SqlitePool, worktree: &str, rel_path: &str, name: &str) -> i64 {
    sqlx::query(
        "INSERT INTO code_files (worktree, rel_path, content_hash, lang, indexed_at) \
         VALUES (?, ?, 'hash', 'rust', 1)",
    )
    .bind(worktree)
    .bind(rel_path)
    .execute(pool)
    .await
    .unwrap();
    let (file_id,): (i64,) =
        sqlx::query_as("SELECT id FROM code_files WHERE worktree = ? AND rel_path = ?")
            .bind(worktree)
            .bind(rel_path)
            .fetch_one(pool)
            .await
            .unwrap();

    sqlx::query(
        "INSERT INTO code_nodes (file_id, name, kind, sel_line, sel_char, end_line) \
         VALUES (?, ?, 12, 1, 0, 9)",
    )
    .bind(file_id)
    .bind(name)
    .execute(pool)
    .await
    .unwrap();
    let (node_id,): (i64,) =
        sqlx::query_as("SELECT id FROM code_nodes WHERE file_id = ? AND name = ?")
            .bind(file_id)
            .bind(name)
            .fetch_one(pool)
            .await
            .unwrap();
    node_id
}

async fn count(pool: &SqlitePool, table: &str) -> i64 {
    let (n,): (i64,) = sqlx::query_as(&format!("SELECT COUNT(*) FROM {table}"))
        .fetch_one(pool)
        .await
        .unwrap();
    n
}

#[tokio::test]
async fn migrate_is_idempotent() {
    let pool = test_pool().await;
    // 두 번째 호출이 에러 없이 통과해야 한다 — 앱은 매 기동마다 migrate를 부른다.
    super::migrate(&pool).await.unwrap();

    let (tables,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM sqlite_master WHERE type='table' \
         AND name IN ('code_files','code_nodes','code_edges')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(tables, 3);
}

#[tokio::test]
async fn deleting_a_file_cascades_to_its_nodes_and_edges() {
    let pool = test_pool().await;
    let caller = seed_symbol(&pool, "/w", "src/a.rs", "caller").await;
    let callee = seed_symbol(&pool, "/w", "src/b.rs", "callee").await;
    sqlx::query("INSERT INTO code_edges (src_id, dst_id, rel) VALUES (?, ?, 'references')")
        .bind(caller)
        .bind(callee)
        .execute(&pool)
        .await
        .unwrap();

    sqlx::query("DELETE FROM code_files WHERE rel_path = 'src/a.rs'")
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(
        count(&pool, "code_nodes").await,
        1,
        "CASCADE가 걸리지 않았다 — PRAGMA foreign_keys 확인"
    );
    assert_eq!(
        count(&pool, "code_edges").await,
        0,
        "엣지는 노드를 따라 지워져야 한다 — 남으면 없는 심볼을 가리킨다"
    );
}

#[tokio::test]
async fn purging_a_worktree_leaves_other_worktrees_intact() {
    let pool = test_pool().await;
    seed_symbol(&pool, "/w/gone", "src/a.rs", "doomed").await;
    seed_symbol(&pool, "/w/gone", "src/b.rs", "also_doomed").await;
    seed_symbol(&pool, "/w/kept", "src/a.rs", "survivor").await;

    let removed = super::purge_worktree(&pool, "/w/gone").await.unwrap();

    assert_eq!(removed, 2);
    assert_eq!(count(&pool, "code_files").await, 1);
    // 같은 rel_path가 다른 워크트리에 있어도 살아남아야 한다 — 정리의 단위는 워크트리다.
    assert_eq!(count(&pool, "code_nodes").await, 1);
}

#[tokio::test]
async fn purging_an_unindexed_worktree_is_not_an_error() {
    let pool = test_pool().await;
    // 인덱싱된 적 없는 워크트리를 정리하는 것은 정상이다 — 종결 경로는 인덱싱 여부를 모른다.
    assert_eq!(super::purge_worktree(&pool, "/never").await.unwrap(), 0);
}

// ── 증분 인덱싱 (Task 4) ────────────────────────────────────────────────

use super::index::{self, Indexed};
use crate::lspclient::protocol::RawSymbol;

fn sym(name: &str, sel_line: u32, end_line: u32) -> RawSymbol {
    RawSymbol {
        name: name.to_string(),
        kind: 12,
        container: None,
        sel_line,
        sel_char: 0,
        sel_end_line: sel_line,
        sel_end_char: 1,
        body_start_line: sel_line,
        body_start_char: 0,
        body_end_line: end_line,
        body_end_char: 0,
        end_line,
    }
}

#[tokio::test]
async fn unchanged_file_is_not_reindexed() {
    let pool = test_pool().await;
    index::upsert_file(&pool, "/wt", "a.rs", "hash1", Some("rust"), 100)
        .await
        .unwrap();
    assert!(!index::needs_reindex(&pool, "/wt", "a.rs", "hash1")
        .await
        .unwrap());
    assert!(index::needs_reindex(&pool, "/wt", "a.rs", "hash2")
        .await
        .unwrap());
    // 처음 보는 파일은 항상 읽는다.
    assert!(index::needs_reindex(&pool, "/wt", "new.rs", "hash1")
        .await
        .unwrap());
}

#[tokio::test]
async fn reindex_replaces_only_that_files_nodes() {
    let pool = test_pool().await;
    let a = index::upsert_file(&pool, "/wt", "a.rs", "h1", Some("rust"), 100)
        .await
        .unwrap();
    let b = index::upsert_file(&pool, "/wt", "b.rs", "h1", Some("rust"), 100)
        .await
        .unwrap();
    index::replace_nodes(&pool, a, &[sym("one", 1, 5), sym("two", 7, 9)])
        .await
        .unwrap();
    index::replace_nodes(&pool, b, &[sym("kept", 1, 3)])
        .await
        .unwrap();

    let written = index::replace_nodes(&pool, a, &[sym("only", 1, 5)])
        .await
        .unwrap();

    assert_eq!(written, 1);
    let (survivor,): (String,) = sqlx::query_as("SELECT name FROM code_nodes WHERE file_id = ?")
        .bind(b)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(survivor, "kept", "다른 파일의 심볼은 살아 있어야 한다");
    assert_eq!(count(&pool, "code_nodes").await, 2);
}

#[tokio::test]
async fn unsupported_language_records_reason_not_silence() {
    let pool = test_pool().await;
    index::record_skip(&pool, "/wt", "x.zig", "LSP 서버 없음", 100)
        .await
        .unwrap();
    let file = index::get_file(&pool, "/wt", "x.zig")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(file.skip_reason.as_deref(), Some("LSP 서버 없음"));
}

#[tokio::test]
async fn a_skipped_file_is_retried_even_when_its_content_is_unchanged() {
    let pool = test_pool().await;
    let text = "fn main() {}";
    let hash = crate::knowledge::hash::content_hash(text);
    index::record_skip(&pool, "/wt", "a.rs", "rust-analyzer가 PATH에 없습니다", 100)
        .await
        .unwrap();

    // 서버를 설치했다. 내용은 그대로지만 다시 시도해야 한다 — 안 그러면 설치가 영원히
    // 반영되지 않는다.
    assert!(index::needs_reindex(&pool, "/wt", "a.rs", &hash)
        .await
        .unwrap());
}

#[tokio::test]
async fn a_successful_reindex_clears_the_earlier_skip_reason() {
    let pool = test_pool().await;
    index::record_skip(&pool, "/wt", "a.rs", "LSP 서버 없음", 100)
        .await
        .unwrap();

    let outcome = index::index_file(
        &pool,
        "/wt",
        "a.rs",
        "fn main() {}",
        Some("rust"),
        200,
        || async { Ok(vec![sym("main", 0, 0)]) },
    )
    .await
    .unwrap();

    assert_eq!(outcome, Indexed::Symbols(1));
    let file = index::get_file(&pool, "/wt", "a.rs")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        file.skip_reason, None,
        "성공했으면 사유가 남아 있으면 안 된다"
    );
}

#[tokio::test]
async fn index_file_skips_work_when_the_hash_matches() {
    let pool = test_pool().await;
    let text = "fn main() {}";
    index::index_file(&pool, "/wt", "a.rs", text, Some("rust"), 100, || async {
        Ok(vec![sym("main", 0, 0)])
    })
    .await
    .unwrap();

    // 두 번째 호출은 LSP를 부르면 안 된다 — 부르면 여기서 패닉한다.
    let outcome = index::index_file(&pool, "/wt", "a.rs", text, Some("rust"), 200, || async {
        panic!("변경 없는 파일에 LSP를 불렀다")
    })
    .await
    .unwrap();

    assert_eq!(outcome, Indexed::Unchanged);
}

#[tokio::test]
async fn a_failed_lsp_call_becomes_a_skip_reason_not_an_error() {
    let pool = test_pool().await;
    let outcome = index::index_file(
        &pool,
        "/wt",
        "a.rs",
        "fn main() {}",
        Some("rust"),
        100,
        || async { Err("응답이 20초 안에 오지 않았습니다".to_string()) },
    )
    .await
    .unwrap();

    assert!(matches!(outcome, Indexed::Skipped(_)));
    let file = index::get_file(&pool, "/wt", "a.rs")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        file.skip_reason.as_deref(),
        Some("응답이 20초 안에 오지 않았습니다"),
        "실패 사유가 그대로 남아야 원인을 되짚을 수 있다"
    );
}

#[tokio::test]
async fn a_file_that_stops_being_indexable_loses_its_stale_symbols() {
    let pool = test_pool().await;
    index::index_file(
        &pool,
        "/wt",
        "a.rs",
        "fn main() {}",
        Some("rust"),
        100,
        || async { Ok(vec![sym("main", 0, 0)]) },
    )
    .await
    .unwrap();
    assert_eq!(count(&pool, "code_nodes").await, 1);

    // 내용이 바뀌었는데 이번엔 LSP가 실패했다. 남은 심볼은 확인할 수 없는 과거의 주장이다.
    index::index_file(
        &pool,
        "/wt",
        "a.rs",
        "fn other() {}",
        Some("rust"),
        200,
        || async { Err("서버가 종료됐습니다".to_string()) },
    )
    .await
    .unwrap();

    assert_eq!(count(&pool, "code_nodes").await, 0);
}

#[tokio::test]
async fn duplicate_symbols_in_one_response_do_not_fail_the_file() {
    let pool = test_pool().await;
    let file_id = index::upsert_file(&pool, "/wt", "a.rs", "h", Some("rust"), 100)
        .await
        .unwrap();
    // 같은 이름·같은 좌표가 두 번 오는 응답 — UNIQUE 충돌로 파일 전체를 잃으면 안 된다.
    let written = index::replace_nodes(&pool, file_id, &[sym("dup", 1, 5), sym("dup", 1, 5)])
        .await
        .unwrap();
    assert_eq!(written, 1);
}

// ── 참조 엣지 (Task 5) ──────────────────────────────────────────────────

#[test]
fn location_maps_to_enclosing_symbol() {
    let syms = vec![sym("caller", 10, 20), sym("other", 30, 40)];
    assert_eq!(index::enclosing(&syms, 15).unwrap().name, "caller");
    // 어느 심볼에도 안 들어가면 엣지를 만들지 않는다 — 파일 노드로 떨어뜨리면
    // 영향 범위가 파일 전체로 번진다.
    assert!(index::enclosing(&syms, 25).is_none());
}

#[test]
fn nested_symbols_resolve_to_the_innermost_one() {
    // impl Foo { fn bar() {} } — bar 안의 참조는 Foo 범위에도 들어가지만 출발점은 bar다.
    let syms = vec![sym("Foo", 10, 60), sym("bar", 20, 30)];
    assert_eq!(index::enclosing(&syms, 25).unwrap().name, "bar");
    // 바깥에만 걸리는 줄은 여전히 바깥 심볼이다.
    assert_eq!(index::enclosing(&syms, 50).unwrap().name, "Foo");
}

#[test]
fn self_reference_is_not_an_edge() {
    assert!(!index::should_edge(1, 1));
    assert!(index::should_edge(1, 2));
}

#[tokio::test]
async fn enclosing_node_finds_the_innermost_symbol_in_the_database() {
    let pool = test_pool().await;
    let file_id = index::upsert_file(&pool, "/wt", "a.rs", "h", Some("rust"), 100)
        .await
        .unwrap();
    index::replace_nodes(&pool, file_id, &[sym("Foo", 10, 60), sym("bar", 20, 30)])
        .await
        .unwrap();

    let inner = index::enclosing_node(&pool, "/wt", "a.rs", 25)
        .await
        .unwrap()
        .unwrap();
    let (name,): (String,) = sqlx::query_as("SELECT name FROM code_nodes WHERE id = ?")
        .bind(inner)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(name, "bar");

    // 인덱싱되지 않은 파일은 조용히 없음이다 — 참조는 워크트리 밖에서도 온다.
    assert!(index::enclosing_node(&pool, "/wt", "never.rs", 1)
        .await
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn reference_edges_are_directional_and_deduplicated() {
    let pool = test_pool().await;
    let caller = seed_symbol(&pool, "/w", "src/a.rs", "caller").await;
    let callee = seed_symbol(&pool, "/w", "src/b.rs", "callee").await;

    assert!(index::add_reference_edge(&pool, caller, callee)
        .await
        .unwrap());
    // 같은 엣지를 다시 넣어도 늘지 않는다 — 한 심볼이 다른 심볼을 여러 번 부르는 것이 정상이다.
    assert!(!index::add_reference_edge(&pool, caller, callee)
        .await
        .unwrap());
    assert!(!index::add_reference_edge(&pool, caller, caller)
        .await
        .unwrap());

    assert_eq!(count(&pool, "code_edges").await, 1);
    let (src, dst): (i64, i64) = sqlx::query_as("SELECT src_id, dst_id FROM code_edges")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        (src, dst),
        (caller, callee),
        "방향은 참조하는 쪽 → 참조되는 쪽"
    );
}

// ── impact_of (Task 6) ──────────────────────────────────────────────────

use super::query;

/// `a → b` 참조 엣지를 만든다(a가 b를 참조한다).
async fn edge(pool: &SqlitePool, src: i64, dst: i64) {
    index::add_reference_edge(pool, src, dst).await.unwrap();
}

#[tokio::test]
async fn impact_walks_references_backwards_by_depth() {
    let pool = test_pool().await;
    let target = seed_symbol(&pool, "/w", "src/core.rs", "target").await;
    let direct = seed_symbol(&pool, "/w", "src/mid.rs", "direct").await;
    let far = seed_symbol(&pool, "/w", "src/outer.rs", "far").await;
    edge(&pool, direct, target).await; // direct가 target을 참조
    edge(&pool, far, direct).await; // far가 direct를 참조

    let one = query::impact_of(&pool, target, 1).await.unwrap();
    assert_eq!(one.items.len(), 1);
    assert_eq!(one.items[0].name, "direct");
    assert_eq!(one.items[0].depth, 1);

    let two = query::impact_of(&pool, target, 2).await.unwrap();
    let names: Vec<&str> = two.items.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, vec!["direct", "far"], "가까운 것부터 나온다");
    assert_eq!(two.items[1].depth, 2);
}

#[tokio::test]
async fn a_reference_cycle_terminates() {
    let pool = test_pool().await;
    let a = seed_symbol(&pool, "/w", "src/a.rs", "a").await;
    let b = seed_symbol(&pool, "/w", "src/b.rs", "b").await;
    // A→B→A. UNION이 방문 집합 역할을 하지 않으면 여기서 끝나지 않는다.
    edge(&pool, a, b).await;
    edge(&pool, b, a).await;

    let impact = query::impact_of(&pool, a, query::MAX_DEPTH).await.unwrap();

    // b만 남는다. UNION은 (id, depth) 튜플을 접으므로 순환에서 b가 1홉·3홉으로 두 번
    // 살아남는데, GROUP BY가 노드당 한 행으로 접고 MIN이 가장 가까운 거리를 준다.
    assert_eq!(impact.items.len(), 1);
    assert_eq!(impact.items[0].name, "b");
    assert_eq!(impact.items[0].depth, 1, "가장 가까운 거리를 준다");
}

#[tokio::test]
async fn a_node_reachable_by_two_paths_appears_once_at_its_shortest_depth() {
    let pool = test_pool().await;
    let target = seed_symbol(&pool, "/w", "src/t.rs", "t").await;
    let mid = seed_symbol(&pool, "/w", "src/m.rs", "m").await;
    let both = seed_symbol(&pool, "/w", "src/b.rs", "both").await;
    // both는 target을 직접(1홉) 참조하면서 mid를 거쳐(2홉) 닿기도 한다.
    edge(&pool, mid, target).await;
    edge(&pool, both, target).await;
    edge(&pool, both, mid).await;

    let impact = query::impact_of(&pool, target, query::MAX_DEPTH)
        .await
        .unwrap();

    assert_eq!(
        impact.items.len(),
        2,
        "영향 범위는 집합이다 — 경로 수가 아니다"
    );
    let both_row = impact.items.iter().find(|i| i.name == "both").unwrap();
    assert_eq!(both_row.depth, 1);
}

#[tokio::test]
async fn depth_is_clamped_to_the_maximum() {
    let pool = test_pool().await;
    let target = seed_symbol(&pool, "/w", "src/t.rs", "t").await;
    // 5단 사슬을 만든다. 상한이 3이므로 4·5단은 나오면 안 된다.
    let mut prev = target;
    for step in 0..5 {
        let node = seed_symbol(&pool, "/w", &format!("src/s{step}.rs"), &format!("s{step}")).await;
        edge(&pool, node, prev).await;
        prev = node;
    }

    let impact = query::impact_of(&pool, target, 99).await.unwrap();

    assert_eq!(impact.items.len(), query::MAX_DEPTH as usize);
    assert!(impact
        .items
        .iter()
        .all(|i| i.depth <= query::MAX_DEPTH as i64));
}

#[tokio::test]
async fn zero_depth_is_raised_to_one_not_treated_as_no_question() {
    let pool = test_pool().await;
    let target = seed_symbol(&pool, "/w", "src/t.rs", "t").await;
    let caller = seed_symbol(&pool, "/w", "src/c.rs", "c").await;
    edge(&pool, caller, target).await;

    // 0홉은 대상 자신뿐이라 질문이 성립하지 않는다 — 빈 답 대신 직접 참조를 준다.
    let impact = query::impact_of(&pool, target, 0).await.unwrap();
    assert_eq!(impact.items.len(), 1);
}

#[tokio::test]
async fn a_truncated_result_says_so() {
    let pool = test_pool().await;
    let target = seed_symbol(&pool, "/w", "src/hot.rs", "hot").await;
    // 상한 + 1개가 직접 참조한다.
    for n in 0..=query::MAX_RESULTS {
        let caller = seed_symbol(&pool, "/w", &format!("src/c{n}.rs"), &format!("c{n}")).await;
        edge(&pool, caller, target).await;
    }

    let impact = query::impact_of(&pool, target, 1).await.unwrap();

    assert_eq!(impact.items.len(), query::MAX_RESULTS);
    assert!(
        impact.truncated,
        "조용히 자르면 이 목록이 전부인 것으로 읽힌다"
    );
}

#[tokio::test]
async fn a_symbol_nobody_references_has_an_empty_impact() {
    let pool = test_pool().await;
    let lonely = seed_symbol(&pool, "/w", "src/a.rs", "lonely").await;
    let impact = query::impact_of(&pool, lonely, query::MAX_DEPTH)
        .await
        .unwrap();
    assert!(impact.items.is_empty());
    assert!(!impact.truncated);
}

#[tokio::test]
async fn lookup_by_name_returns_every_match() {
    let pool = test_pool().await;
    seed_symbol(&pool, "/w", "src/a.rs", "new").await;
    seed_symbol(&pool, "/w", "src/b.rs", "new").await;
    seed_symbol(&pool, "/other", "src/a.rs", "new").await;

    let found = query::find_nodes_by_name(&pool, "/w", "new").await.unwrap();

    // 하나를 임의로 고르면 엉뚱한 심볼의 영향 범위를 답하게 된다.
    assert_eq!(found.len(), 2, "같은 이름이 여러 파일에 있는 것이 정상이다");
    // 다른 워크트리는 섞이지 않는다.
    assert!(found.iter().all(|f| f.rel_path.starts_with("src/")));
}

#[tokio::test]
async fn the_same_symbol_name_can_repeat_across_files() {
    let pool = test_pool().await;
    // UNIQUE는 (file_id, name, sel_line, sel_char)다. `new`나 `fmt`처럼 흔한 이름이
    // 파일마다 있는 것이 정상인데, 이름만으로 막으면 인덱싱이 통째로 실패한다.
    seed_symbol(&pool, "/w", "src/a.rs", "new").await;
    seed_symbol(&pool, "/w", "src/b.rs", "new").await;
    assert_eq!(count(&pool, "code_nodes").await, 2);
}
