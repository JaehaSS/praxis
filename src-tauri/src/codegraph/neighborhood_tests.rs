use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use sqlx::SqlitePool;

use super::test_support::{symbol, test_pool};
use super::{
    generation, manifest,
    neighborhood::{self, Direction},
    snapshot,
};

fn manifest(path: &str, hash: &str) -> manifest::SourceManifest {
    manifest::SourceManifest {
        files: vec![manifest::ManifestFile {
            rel_path: path.into(),
            abs_path: PathBuf::from(path),
            content_hash: hash.into(),
            lang: "rust",
            spec_key: "rust-analyzer",
        }],
        files_total: 1,
        fingerprint: "fingerprint".into(),
    }
}

async fn file(
    pool: &SqlitePool,
    run: i64,
    path: &str,
    hash: &str,
    edge_state: Option<&str>,
) -> i64 {
    snapshot::insert_file(pool, run, path, hash, "rust", None, edge_state)
        .await
        .unwrap()
}

async fn file_with_reasons(
    pool: &SqlitePool,
    run: i64,
    path: &str,
    hash: &str,
    lang: &str,
    skip_reason: Option<&str>,
    edge_state: Option<&str>,
) -> i64 {
    snapshot::insert_file(pool, run, path, hash, lang, skip_reason, edge_state)
        .await
        .unwrap()
}

async fn node(pool: &SqlitePool, run: i64, file: i64, name: &str, line: u32) -> i64 {
    snapshot::insert_node(
        pool,
        run,
        file,
        &symbol(name, (line, 0, line, 6), (line, 0, line + 1, 0)),
    )
    .await
    .unwrap()
}

async fn ready(pool: &SqlitePool, run: i64) {
    generation::promote(pool, run, generation::RunStats::default(), 20)
        .await
        .unwrap();
}

#[tokio::test]
async fn neighborhood_preserves_direction_diamond_and_cycles() {
    let pool = test_pool("neighborhood").await;
    let run = generation::start_run(&pool, "/w", "fingerprint", 10)
        .await
        .unwrap();
    let root_file = file(&pool, run, "src/c.rs", "c", None).await;
    let a_file = file(&pool, run, "src/a.rs", "a", None).await;
    let b_file = file(&pool, run, "src/b.rs", "b", None).await;
    let d_file = file(&pool, run, "src/d.rs", "d", Some("partial edges")).await;
    let a = node(&pool, run, a_file, "a", 0).await;
    let b = node(&pool, run, b_file, "b", 0).await;
    let c = node(&pool, run, root_file, "target", 0).await;
    let d = node(&pool, run, d_file, "d", 0).await;
    for (src, dst) in [(a, b), (b, c), (a, d), (d, c), (c, b)] {
        snapshot::add_edge(&pool, run, src, dst).await.unwrap();
    }
    ready(&pool, run).await;
    let graph = neighborhood::at(
        &pool,
        "/w",
        "src/c.rs",
        0,
        1,
        Direction::Incoming,
        2,
        &manifest("src/c.rs", "c"),
    )
    .await
    .unwrap();
    assert_eq!(graph.root_id, c);
    assert_eq!(
        graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>(),
        vec![a, b, c, d]
    );
    assert_eq!(
        graph
            .edges
            .iter()
            .map(|edge| (edge.source_id, edge.target_id))
            .collect::<Vec<_>>(),
        vec![(a, b), (a, d), (b, c), (c, b), (d, c)]
    );
    assert_eq!(graph.encountered_incomplete[0].rel_path, "src/d.rs");
    let outgoing = neighborhood::at(
        &pool,
        "/w",
        "src/a.rs",
        0,
        1,
        Direction::Outgoing,
        2,
        &manifest("src/a.rs", "a"),
    )
    .await
    .unwrap();
    assert_eq!(
        outgoing
            .nodes
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![a, b, c, d]
    );
}

#[tokio::test]
async fn neighborhood_isolates_same_names_and_active_generations() {
    let pool = test_pool("generations").await;
    let first = generation::start_run(&pool, "/w", "fingerprint", 10)
        .await
        .unwrap();
    let first_file = file(&pool, first, "src/lib.rs", "hash", None).await;
    let old = node(&pool, first, first_file, "target", 0).await;
    ready(&pool, first).await;
    let second = generation::start_run(&pool, "/w", "fingerprint", 11)
        .await
        .unwrap();
    let second_file = file(&pool, second, "src/lib.rs", "hash", None).await;
    let selected = node(&pool, second, second_file, "target", 0).await;
    let other_file = file(&pool, second, "src/other.rs", "other", None).await;
    let same_name = node(&pool, second, other_file, "target", 0).await;
    let caller = node(&pool, second, second_file, "caller", 3).await;
    snapshot::add_edge(&pool, second, caller, selected)
        .await
        .unwrap();
    snapshot::add_edge(&pool, second, caller, same_name)
        .await
        .unwrap();
    ready(&pool, second).await;
    let graph = neighborhood::at(
        &pool,
        "/w",
        "src/lib.rs",
        0,
        1,
        Direction::Incoming,
        1,
        &manifest("src/lib.rs", "hash"),
    )
    .await
    .unwrap();
    assert_eq!(graph.run_id, second);
    assert_ne!(graph.root_id, old);
    assert_eq!(graph.root_id, selected);
    assert_eq!(
        graph.edges,
        vec![neighborhood::Edge {
            source_id: caller,
            target_id: selected,
            relation: "references"
        }]
    );
}

#[tokio::test]
async fn neighborhood_distinguishes_exact_and_exceeded_edge_caps() {
    let pool = test_pool("edge-cap").await;
    let run = generation::start_run(&pool, "/w", "fingerprint", 10)
        .await
        .unwrap();
    let root_file = file(&pool, run, "src/lib.rs", "hash", None).await;
    let root = node(&pool, run, root_file, "target", 0).await;
    let b = node(&pool, run, root_file, "b", 1).await;
    let c = node(&pool, run, root_file, "c", 2).await;
    let mut callers = Vec::new();
    for index in 3..neighborhood::MAX_NODES {
        callers.push(
            node(
                &pool,
                run,
                root_file,
                &format!("caller_{index}"),
                index as u32,
            )
            .await,
        );
    }
    for caller in &callers {
        for target in [b, c] {
            snapshot::add_edge(&pool, run, *caller, target)
                .await
                .unwrap();
        }
    }
    for (source, target) in [(b, root), (c, root), (root, b), (root, c), (b, c), (c, b)] {
        snapshot::add_edge(&pool, run, source, target)
            .await
            .unwrap();
    }
    ready(&pool, run).await;
    let exact = neighborhood::at(
        &pool,
        "/w",
        "src/lib.rs",
        0,
        1,
        Direction::Incoming,
        2,
        &manifest("src/lib.rs", "hash"),
    )
    .await
    .unwrap();
    assert_eq!(exact.edges.len(), neighborhood::MAX_EDGES);
    assert!(!exact.truncated);
    snapshot::add_edge(&pool, run, callers[0], root)
        .await
        .unwrap();
    let exceeded = neighborhood::at(
        &pool,
        "/w",
        "src/lib.rs",
        0,
        1,
        Direction::Incoming,
        2,
        &manifest("src/lib.rs", "hash"),
    )
    .await
    .unwrap();
    assert_eq!(exceeded.edges.len(), neighborhood::MAX_EDGES);
    assert!(exceeded.truncated);
}

#[tokio::test]
async fn neighborhood_reports_caps_and_source_changes() {
    let pool = test_pool("caps").await;
    let run = generation::start_run(&pool, "/w", "fingerprint", 10)
        .await
        .unwrap();
    let root_file = file(&pool, run, "src/lib.rs", "hash", None).await;
    let root = node(&pool, run, root_file, "target", 0).await;
    for index in 1..=neighborhood::MAX_NODES {
        let caller = node(
            &pool,
            run,
            root_file,
            &format!("caller_{index}"),
            index as u32,
        )
        .await;
        snapshot::add_edge(&pool, run, caller, root).await.unwrap();
    }
    ready(&pool, run).await;
    let graph = neighborhood::at(
        &pool,
        "/w",
        "src/lib.rs",
        0,
        1,
        Direction::Incoming,
        1,
        &manifest("src/lib.rs", "hash"),
    )
    .await
    .unwrap();
    assert_eq!(graph.nodes.len(), neighborhood::MAX_NODES);
    assert!(graph.truncated);
    let changed = neighborhood::at(
        &pool,
        "/w",
        "src/lib.rs",
        0,
        1,
        Direction::Incoming,
        1,
        &manifest("src/lib.rs", "changed"),
    )
    .await;
    assert!(changed
        .unwrap_err()
        .to_string()
        .starts_with("source-changed:"));
}

#[tokio::test]
async fn neighborhood_keeps_exact_node_limit_untruncated_and_edges_inside_nodes() {
    let pool = test_pool("exact-node-cap").await;
    let run = generation::start_run(&pool, "/w", "fingerprint", 10)
        .await
        .unwrap();
    let root_file = file(&pool, run, "src/lib.rs", "hash", None).await;
    let root = node(&pool, run, root_file, "target", 0).await;
    for index in 1..neighborhood::MAX_NODES {
        let caller = node(
            &pool,
            run,
            root_file,
            &format!("caller_{index}"),
            index as u32,
        )
        .await;
        snapshot::add_edge(&pool, run, caller, root).await.unwrap();
    }
    ready(&pool, run).await;

    let graph = neighborhood::at(
        &pool,
        "/w",
        "src/lib.rs",
        0,
        1,
        Direction::Incoming,
        1,
        &manifest("src/lib.rs", "hash"),
    )
    .await
    .unwrap();

    assert_eq!(graph.nodes.len(), neighborhood::MAX_NODES);
    assert_eq!(graph.edges.len(), neighborhood::MAX_NODES - 1);
    assert!(!graph.truncated);
    let ids: std::collections::BTreeSet<_> = graph.nodes.iter().map(|node| node.id).collect();
    assert!(graph
        .edges
        .iter()
        .all(|edge| ids.contains(&edge.source_id) && ids.contains(&edge.target_id)));
}

#[tokio::test]
async fn neighborhood_keeps_root_edge_unavailability_and_run_incompleteness_in_both_directions() {
    let pool = test_pool("partial-directions").await;
    let run = generation::start_run(&pool, "/w", "fingerprint", 10)
        .await
        .unwrap();
    let root_file = file_with_reasons(
        &pool,
        run,
        "src/target.py",
        "hash",
        "python",
        None,
        Some("pyright does not provide readiness"),
    )
    .await;
    let caller_file = file(&pool, run, "src/caller.rs", "caller", None).await;
    file_with_reasons(
        &pool,
        run,
        "src/skipped.java",
        "skipped",
        "java",
        Some("jdtls unavailable"),
        None,
    )
    .await;
    let root = node(&pool, run, root_file, "target", 0).await;
    let caller = node(&pool, run, caller_file, "caller", 0).await;
    let callee = node(&pool, run, caller_file, "callee", 2).await;
    snapshot::add_edge(&pool, run, caller, root).await.unwrap();
    snapshot::add_edge(&pool, run, root, callee).await.unwrap();
    ready(&pool, run).await;

    for direction in [Direction::Incoming, Direction::Outgoing] {
        let graph = neighborhood::at(
            &pool,
            "/w",
            "src/target.py",
            0,
            1,
            direction,
            1,
            &manifest("src/target.py", "hash"),
        )
        .await
        .unwrap();
        assert_eq!(
            graph.edges_unavailable.as_deref(),
            Some("pyright does not provide readiness")
        );
        let incomplete = graph
            .incomplete
            .expect("run-wide partial state is returned");
        assert_eq!(incomplete.files_skipped, 1);
        assert_eq!(incomplete.files_without_edges, 1);
    }
}

#[tokio::test]
async fn neighborhood_isolates_identical_paths_in_distinct_worktrees() {
    let pool = test_pool("worktree-isolation").await;
    let first = generation::start_run(&pool, "/one", "fingerprint", 10)
        .await
        .unwrap();
    let first_file = file(&pool, first, "src/lib.rs", "hash", None).await;
    let first_root = node(&pool, first, first_file, "target", 0).await;
    ready(&pool, first).await;
    let second = generation::start_run(&pool, "/two", "fingerprint", 30)
        .await
        .unwrap();
    let second_file = file(&pool, second, "src/lib.rs", "hash", None).await;
    let second_root = node(&pool, second, second_file, "target", 0).await;
    ready(&pool, second).await;

    let graph = neighborhood::at(
        &pool,
        "/one",
        "src/lib.rs",
        0,
        1,
        Direction::Incoming,
        1,
        &manifest("src/lib.rs", "hash"),
    )
    .await
    .unwrap();

    assert_eq!(graph.run_id, first);
    assert_eq!(graph.root_id, first_root);
    assert_ne!(graph.root_id, second_root);
}

#[tokio::test]
async fn neighborhood_keeps_every_field_in_the_generation_read_before_promotion() {
    let pool = test_pool("generation-read-barrier").await;
    let preexisting = generation::start_run(&pool, "/w", "pre", 1).await.unwrap();
    ready(&pool, preexisting).await;
    let old = generation::start_run(&pool, "/w", "fingerprint", 10)
        .await
        .unwrap();
    let old_file = file_with_reasons(
        &pool,
        old,
        "src/lib.rs",
        "hash",
        "rust",
        None,
        Some("old edges partial"),
    )
    .await;
    let old_root = node(&pool, old, old_file, "old_target", 0).await;
    let old_caller = node(&pool, old, old_file, "old_caller", 2).await;
    snapshot::add_edge(&pool, old, old_caller, old_root)
        .await
        .unwrap();
    ready(&pool, old).await;
    let next = generation::start_run(&pool, "/w", "fingerprint", 30)
        .await
        .unwrap();
    let next_file = file_with_reasons(
        &pool,
        next,
        "src/lib.rs",
        "hash",
        "java",
        Some("new symbols skipped"),
        None,
    )
    .await;
    let next_root = node(&pool, next, next_file, "new_target", 0).await;

    let gate = neighborhood::ActiveReadGate {
        entered: Arc::new(tokio::sync::Barrier::new(2)),
        release: Arc::new(tokio::sync::Barrier::new(2)),
    };
    let query_pool = pool.clone();
    let query_gate = gate.clone();
    let query = tokio::spawn(async move {
        neighborhood::ACTIVE_READ_GATE
            .scope(
                query_gate,
                neighborhood::at(
                    &query_pool,
                    "/w",
                    "src/lib.rs",
                    0,
                    1,
                    Direction::Incoming,
                    1,
                    &manifest("src/lib.rs", "hash"),
                ),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), gate.entered.wait())
        .await
        .expect("query did not read the active generation");
    generation::promote(&pool, next, generation::RunStats::default(), 40)
        .await
        .unwrap();
    let final_run = generation::start_run(&pool, "/w", "fingerprint", 50)
        .await
        .unwrap();
    generation::promote(&pool, final_run, generation::RunStats::default(), 60)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM code_graph_runs WHERE id=?")
            .bind(old)
            .fetch_one(&pool)
            .await
            .unwrap(),
        0,
        "writer cleanup must remove the generation held by the read snapshot"
    );
    tokio::time::timeout(Duration::from_secs(5), gate.release.wait())
        .await
        .expect("query did not resume after promotion");
    let graph = tokio::time::timeout(Duration::from_secs(5), query)
        .await
        .expect("query did not finish")
        .unwrap()
        .unwrap();

    assert_eq!(graph.run_id, old);
    assert_eq!(graph.indexed_at, 20);
    assert_eq!(graph.root_id, old_root);
    assert_eq!(
        graph.nodes.iter().map(|node| node.id).collect::<Vec<_>>(),
        vec![old_root, old_caller]
    );
    assert_eq!(
        graph.edges,
        vec![neighborhood::Edge {
            source_id: old_caller,
            target_id: old_root,
            relation: "references",
        }]
    );
    assert_eq!(
        graph.edges_unavailable.as_deref(),
        Some("old edges partial")
    );
    assert_eq!(
        graph
            .incomplete
            .expect("old generation's partial state")
            .languages_without_edges,
        vec!["rust"]
    );
    assert_ne!(graph.root_id, next_root);
    assert_eq!(
        generation::active_run_id(&pool, "/w").await.unwrap(),
        Some(final_run)
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM code_graph_runs WHERE id=?")
            .bind(preexisting)
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn neighborhood_at_path_resolves_the_indexed_relative_source() {
    let root =
        crate::testtmp::dir().join(format!("praxis-neighborhood-path-{}", std::process::id()));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "fn target() {}\n").unwrap();
    let source = manifest::scan(&root).unwrap();
    let pool = test_pool("path-success").await;
    let key = root.to_string_lossy().into_owned();
    let run = generation::start_run(&pool, &key, &source.fingerprint, 10)
        .await
        .unwrap();
    let source_file = source
        .files
        .iter()
        .find(|file| file.rel_path == "src/lib.rs")
        .unwrap();
    let stored = file(&pool, run, "src/lib.rs", &source_file.content_hash, None).await;
    let target = node(&pool, run, stored, "target", 0).await;
    ready(&pool, run).await;

    let graph = neighborhood::at_path(&pool, &root, "src/lib.rs", 0, 1, Direction::Incoming, 1)
        .await
        .unwrap();

    assert_eq!(graph.root_id, target);
}

#[tokio::test]
async fn neighborhood_at_path_rejects_parent_and_absolute_paths_before_scanning() {
    let root = crate::testtmp::dir().join(format!(
        "praxis-neighborhood-path-reject-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let pool = test_pool("path-reject").await;
    let parent =
        neighborhood::at_path(&pool, &root, "../escape.rs", 0, 0, Direction::Incoming, 1).await;
    let absolute =
        neighborhood::at_path(&pool, &root, "/tmp/escape.rs", 0, 0, Direction::Incoming, 1).await;

    assert!(parent.is_err());
    assert!(absolute.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn neighborhood_at_path_rejects_a_symlinked_source() {
    use std::os::unix::fs::symlink;

    let root = crate::testtmp::dir().join(format!(
        "praxis-neighborhood-path-symlink-{}",
        std::process::id()
    ));
    let outside = crate::testtmp::dir().join(format!(
        "praxis-neighborhood-outside-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(&outside, "fn outside() {}\n").unwrap();
    symlink(&outside, root.join("escape.rs")).unwrap();
    let pool = test_pool("path-symlink").await;
    let result =
        neighborhood::at_path(&pool, &root, "escape.rs", 0, 0, Direction::Incoming, 1).await;

    assert!(result.is_err());
}

#[tokio::test]
#[ignore = "benchmark: 30-sample 200-node/400-edge neighborhood timing"]
async fn neighborhood_full_query_p95_stays_within_the_budget() {
    let root = crate::testtmp::dir().join(format!(
        "praxis-neighborhood-benchmark-{}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("src/lib.rs"), "fn target() {}\n").unwrap();
    let source = manifest::scan(&root).unwrap();
    let pool = test_pool("benchmark").await;
    let key = root.to_string_lossy().into_owned();
    let run = generation::start_run(&pool, &key, &source.fingerprint, 10)
        .await
        .unwrap();
    let source_file = source
        .files
        .iter()
        .find(|file| file.rel_path == "src/lib.rs")
        .unwrap();
    let stored = file(&pool, run, "src/lib.rs", &source_file.content_hash, None).await;
    let root_node = node(&pool, run, stored, "target", 0).await;
    let mut callers = Vec::new();
    for index in 1..neighborhood::MAX_NODES {
        callers.push(node(&pool, run, stored, &format!("caller_{index}"), index as u32).await);
    }
    for caller in &callers {
        snapshot::add_edge(&pool, run, *caller, root_node)
            .await
            .unwrap();
    }
    for index in 0..callers.len() {
        snapshot::add_edge(
            &pool,
            run,
            callers[index],
            callers[(index + 1) % callers.len()],
        )
        .await
        .unwrap();
    }
    for (source, target) in [(callers[0], callers[2]), (callers[1], callers[3])] {
        snapshot::add_edge(&pool, run, source, target)
            .await
            .unwrap();
    }
    ready(&pool, run).await;

    let mut scan_samples = Vec::with_capacity(30);
    let mut full_samples = Vec::with_capacity(30);
    for _ in 0..30 {
        let scan_started = std::time::Instant::now();
        manifest::scan(&root).unwrap();
        scan_samples.push(scan_started.elapsed());
        let query_started = std::time::Instant::now();
        let graph = neighborhood::at_path(&pool, &root, "src/lib.rs", 0, 1, Direction::Incoming, 3)
            .await
            .unwrap();
        full_samples.push(query_started.elapsed());
        assert_eq!(graph.nodes.len(), neighborhood::MAX_NODES);
        assert_eq!(graph.edges.len(), neighborhood::MAX_EDGES);
    }
    scan_samples.sort_unstable();
    full_samples.sort_unstable();
    let p95 = |samples: &[std::time::Duration]| samples[(samples.len() * 95).div_ceil(100) - 1];
    let scan_p95 = p95(&scan_samples);
    let full_p95 = p95(&full_samples);
    println!(
        "neighborhood benchmark: os={} arch={} samples=30 manifest_scan_p95={scan_p95:?} full_query_p95={full_p95:?}",
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    assert!(
        full_p95 <= std::time::Duration::from_millis(500),
        "full neighborhood query p95 exceeded 500ms: {full_p95:?}"
    );
}
