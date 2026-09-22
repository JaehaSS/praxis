use std::sync::atomic::{AtomicU32, Ordering};

use super::*;
use crate::codegraph::test_support::{symbol, test_pool};
use crate::codegraph::{generation, manifest, snapshot};

static COUNTER: AtomicU32 = AtomicU32::new(0);

async fn fixture(label: &str) -> (SqlitePool, std::path::PathBuf) {
    let root = crate::testtmp::dir().join(format!(
        "codewiki-{label}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname='wiki-fixture'\n").unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn api() {}\n").unwrap();
    std::fs::write(root.join("src/deep/my file.rs"), "pub fn caller() {}\n").unwrap();
    let pool = test_pool(label).await;
    let manifest = manifest::scan(&root).unwrap();
    let worktree = root.to_string_lossy();
    let run = generation::start_run(&pool, &worktree, &manifest.fingerprint, 1)
        .await
        .unwrap();
    let lib = manifest
        .files
        .iter()
        .find(|file| file.rel_path == "src/lib.rs")
        .unwrap();
    let deep = manifest
        .files
        .iter()
        .find(|file| file.rel_path == "src/deep/my file.rs")
        .unwrap();
    let lib_id = snapshot::insert_file(
        &pool,
        run,
        &lib.rel_path,
        &lib.content_hash,
        "rust",
        None,
        None,
    )
    .await
    .unwrap();
    let deep_id = snapshot::insert_file(
        &pool,
        run,
        &deep.rel_path,
        &deep.content_hash,
        "rust",
        None,
        None,
    )
    .await
    .unwrap();
    let api = snapshot::insert_node(
        &pool,
        run,
        lib_id,
        &symbol("api", (0, 0, 0, 3), (0, 0, 0, 12)),
    )
    .await
    .unwrap();
    let caller = snapshot::insert_node(
        &pool,
        run,
        deep_id,
        &symbol("caller", (0, 0, 0, 6), (0, 0, 0, 15)),
    )
    .await
    .unwrap();
    snapshot::add_edge(&pool, run, caller, api).await.unwrap();
    generation::promote(
        &pool,
        run,
        generation::RunStats {
            files_seen: 2,
            symbols: 2,
            edges: 1,
            ..Default::default()
        },
        2,
    )
    .await
    .unwrap();
    (pool, root)
}

#[tokio::test]
async fn writes_a_selected_page_and_index_with_resolved_reference_evidence() {
    let (pool, root) = fixture("selected").await;
    let result = generate(&pool, &root, Some("src/deep/my file.rs"), 3)
        .await
        .unwrap();
    assert_eq!(result.index_state, CodeWikiPageState::Ready);
    assert_eq!(
        result
            .modules
            .iter()
            .find(|module| module.source_path == "src/deep/my file.rs")
            .unwrap()
            .state,
        CodeWikiPageState::Ready
    );
    assert_eq!(
        result
            .modules
            .iter()
            .find(|module| module.source_path == "src/lib.rs")
            .unwrap()
            .state,
        CodeWikiPageState::Missing
    );
    let page =
        std::fs::read_to_string(root.join("docs/codebase/modules/src/deep/my file.rs.md")).unwrap();
    assert!(page.contains("../../../../../src/deep/my%20file.rs"));
    assert!(page.contains("`caller` at src/deep/my file.rs:1"));
    assert!(page.contains("`api` at src/lib.rs:1"));
    let index = std::fs::read_to_string(root.join("docs/codebase/index.md")).unwrap();
    assert!(index.contains("modules/src/deep/my%20file.rs.md"));
    assert!(index.contains("`src/lib.rs` — missing"));
    assert!(!index.contains("(modules/src/lib.rs.md)"));
}

#[tokio::test]
async fn detects_source_and_configuration_staleness_without_writing() {
    let (pool, root) = fixture("stale").await;
    generate(&pool, &root, None, 3).await.unwrap();
    std::fs::write(root.join("Cargo.lock"), "lockfile\n").unwrap();
    let state = status(&pool, &root).await.unwrap();
    assert_eq!(state.graph_state, "stale");
    assert!(state
        .modules
        .iter()
        .all(|module| module.state == CodeWikiPageState::Stale));
}

#[tokio::test]
async fn protects_modified_metadata_body_and_manual_targets() {
    let (pool, root) = fixture("conflict").await;
    generate(&pool, &root, None, 3).await.unwrap();
    let page = root.join("docs/codebase/modules/src/lib.rs.md");
    std::fs::write(&page, "manual note\n").unwrap();
    let state = status(&pool, &root).await.unwrap();
    assert_eq!(
        state
            .modules
            .iter()
            .find(|module| module.source_path == "src/lib.rs")
            .unwrap()
            .state,
        CodeWikiPageState::Conflict
    );
    assert!(generate(&pool, &root, Some("src/lib.rs"), 4).await.is_err());
    assert_eq!(std::fs::read_to_string(page).unwrap(), "manual note\n");
}

#[tokio::test]
async fn detects_metadata_formatting_edits_and_preserves_orphaned_pages() {
    let (pool, root) = fixture("metadata-orphan").await;
    generate(&pool, &root, None, 3).await.unwrap();
    let page = root.join("docs/codebase/modules/src/lib.rs.md");
    let original = std::fs::read_to_string(&page).unwrap();
    std::fs::write(&page, original.replacen("sourceHash: ", "sourceHash:  ", 1)).unwrap();
    let state = status(&pool, &root).await.unwrap();
    assert_eq!(
        state
            .modules
            .iter()
            .find(|module| module.source_path == "src/lib.rs")
            .unwrap()
            .state,
        CodeWikiPageState::Conflict
    );
    std::fs::write(&page, original).unwrap();
    std::fs::remove_file(root.join("src/lib.rs")).unwrap();
    let state = status(&pool, &root).await.unwrap();
    assert_eq!(
        state
            .modules
            .iter()
            .find(|module| module.source_path == "src/lib.rs")
            .unwrap()
            .state,
        CodeWikiPageState::Orphaned
    );
}

#[tokio::test]
async fn rejects_a_changed_preimage_without_overwriting_it() {
    let (pool, root) = fixture("preimage").await;
    generate(&pool, &root, None, 3).await.unwrap();
    let path = "docs/codebase/modules/src/lib.rs.md";
    let preimage = storage::preflight(&root, &[(path, "module")]).unwrap();
    std::fs::write(root.join(path), "x".repeat(model::MAX_INDEX_BYTES + 1)).unwrap();
    assert!(storage::write(&root, &preimage[0], "replacement\n").is_err());
    assert!(std::fs::metadata(root.join(path)).unwrap().len() > model::MAX_INDEX_BYTES as u64);
    let leftovers: Vec<_> = std::fs::read_dir(root.join("docs/codebase/modules/src"))
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .contains(".praxis-codewiki-")
        })
        .collect();
    assert!(leftovers.is_empty());
}

#[tokio::test]
async fn marks_a_symbol_list_that_exceeds_its_bound() {
    let (pool, root) = fixture("bounds").await;
    let run = generation::active_run_id(&pool, &root.to_string_lossy())
        .await
        .unwrap()
        .unwrap();
    let (file_id,): (i64,) =
        sqlx::query_as("SELECT id FROM code_graph_files WHERE run_id=? AND rel_path='src/lib.rs'")
            .bind(run)
            .fetch_one(&pool)
            .await
            .unwrap();
    for line in 1..=model::MAX_SYMBOLS {
        snapshot::insert_node(
            &pool,
            run,
            file_id,
            &symbol(
                &format!("extra_{line}"),
                (line as u32, 0, line as u32, 1),
                (line as u32, 0, line as u32, 1),
            ),
        )
        .await
        .unwrap();
    }
    generate(&pool, &root, Some("src/lib.rs"), 3).await.unwrap();
    let page = std::fs::read_to_string(root.join("docs/codebase/modules/src/lib.rs.md")).unwrap();
    assert!(page.contains("Symbol list truncated at 200 entries."));
}

#[tokio::test]
async fn marks_a_reference_list_that_exceeds_its_bound() {
    let (pool, root) = fixture("relation-bounds").await;
    let run = generation::active_run_id(&pool, &root.to_string_lossy())
        .await
        .unwrap()
        .unwrap();
    let (file_id, target): (i64, i64) = sqlx::query_as("SELECT f.id, n.id FROM code_graph_files f JOIN code_graph_nodes n ON n.file_id=f.id WHERE f.run_id=? AND f.rel_path='src/lib.rs' AND n.name='api'").bind(run).fetch_one(&pool).await.unwrap();
    for line in 1..=model::MAX_RELATIONS {
        let caller = snapshot::insert_node(
            &pool,
            run,
            file_id,
            &symbol(
                &format!("caller_{line}"),
                (line as u32, 0, line as u32, 1),
                (line as u32, 0, line as u32, 1),
            ),
        )
        .await
        .unwrap();
        snapshot::add_edge(&pool, run, caller, target)
            .await
            .unwrap();
    }
    generate(&pool, &root, Some("src/lib.rs"), 3).await.unwrap();
    let page = std::fs::read_to_string(root.join("docs/codebase/modules/src/lib.rs.md")).unwrap();
    assert!(page.contains("Reference list truncated at 100 entries."));
    assert_eq!(page.matches(" references ").count(), model::MAX_RELATIONS);
}

#[tokio::test]
async fn run_fingerprint_mismatch_refuses_publication() {
    let (pool, root) = fixture("run-fingerprint").await;
    let run = generation::active_run_id(&pool, &root.to_string_lossy())
        .await
        .unwrap()
        .unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn changed() {}\n").unwrap();
    let manifest = manifest::scan(&root).unwrap();

    assert!(ensure_run_fingerprint(&pool, run, &manifest.fingerprint)
        .await
        .is_err());
    assert_eq!(status(&pool, &root).await.unwrap().graph_state, "stale");
    assert!(generate(&pool, &root, None, 3).await.is_err());
    assert!(!root.join("docs/codebase").exists());
}

#[test]
fn refuses_an_oversized_index_before_any_output_preflight() {
    let long = "x".repeat(500);
    let modules: Vec<_> = (0..model::MAX_FILES)
        .map(|number| {
            (
                format!("src/{number:04}-{long}.rs"),
                "modules/p.md".to_owned(),
                CodeWikiPageState::Missing,
            )
        })
        .collect();
    let index = render::index("fingerprint", 1, 1, &modules, &[]);

    assert!(index.len() > model::MAX_INDEX_BYTES);
    assert!(size_check(&index, model::MAX_INDEX_BYTES, "index").is_err());
}

#[tokio::test]
async fn refuses_more_than_five_thousand_sources_without_creating_output() {
    let root = crate::testtmp::dir().join(format!(
        "codewiki-file-cap-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname='file-cap'\n").unwrap();
    for number in 0..=model::MAX_FILES {
        std::fs::write(root.join(format!("src/file-{number:04}.rs")), "\n").unwrap();
    }
    let manifest = manifest::scan(&root).unwrap();
    let pool = test_pool("file-cap").await;
    let run = generation::start_run(&pool, &root.to_string_lossy(), &manifest.fingerprint, 1)
        .await
        .unwrap();
    generation::promote(&pool, run, generation::RunStats::default(), 2)
        .await
        .unwrap();

    assert!(generate(&pool, &root, None, 3).await.is_err());
    assert!(!root.join("docs/codebase").exists());
}

#[tokio::test]
async fn a_rerun_recovers_the_index_after_a_selected_partial_publication() {
    let (pool, root) = fixture("rerun").await;
    generate(&pool, &root, Some("src/deep/my file.rs"), 3)
        .await
        .unwrap();
    std::fs::remove_file(root.join("docs/codebase/index.md")).unwrap();

    let state = generate(&pool, &root, None, 4).await.unwrap();

    assert_eq!(state.index_state, CodeWikiPageState::Ready);
    assert!(root.join("docs/codebase/modules/src/lib.rs.md").is_file());
    assert!(root.join("docs/codebase/index.md").is_file());
}

#[cfg(unix)]
#[test]
fn output_lock_rejects_contention_and_releases_on_drop() {
    let root = crate::testtmp::dir().join(format!(
        "codewiki-lock-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    let lock = storage::lock(&root).unwrap();

    assert!(storage::lock(&root).is_err());
    drop(lock);
    assert!(storage::lock(&root).is_ok());
}

#[test]
fn resource_bounds_reject_before_output_or_graph_materialization() {
    let mut output = Vec::new();
    let mut bytes = model::MAX_OUTPUT_BYTES;
    assert!(push_output(
        &mut output,
        &mut bytes,
        "page.md".into(),
        "module",
        "x".into()
    )
    .is_err());
    assert!(validate_graph_bounds(((model::MAX_GRAPH_NODE_ROWS + 1) as i64, 0), (0, 0)).is_err());
    assert!(validate_graph_bounds((0, model::MAX_GRAPH_BYTES as i64), (0, 1)).is_err());
}

#[test]
fn index_links_existing_stale_and_conflict_pages_with_their_states() {
    let index = render::index(
        "fingerprint",
        1,
        1,
        &[
            (
                "src/ready.rs".into(),
                "modules/src/ready.rs.md".into(),
                CodeWikiPageState::Ready,
            ),
            (
                "src/stale.rs".into(),
                "modules/src/stale.rs.md".into(),
                CodeWikiPageState::Stale,
            ),
            (
                "src/conflict.rs".into(),
                "modules/src/conflict.rs.md".into(),
                CodeWikiPageState::Conflict,
            ),
            (
                "src/missing.rs".into(),
                "modules/src/missing.rs.md".into(),
                CodeWikiPageState::Missing,
            ),
        ],
        &[],
    );
    assert!(index.contains("(modules/src/ready.rs.md)"));
    assert!(index.contains("(modules/src/stale.rs.md) — stale"));
    assert!(index.contains("(modules/src/conflict.rs.md) — conflict"));
    assert!(index.contains("`src/missing.rs` — missing"));
}

#[test]
fn escapes_markdown_labels_and_percent_encodes_every_reserved_url_byte() {
    let page = render::page(
        "src/deep/[name])#.rs",
        "hash",
        "fingerprint",
        1,
        1,
        &[],
        &[],
        None,
    );
    assert!(page.contains("src/deep/\\[name\\]\\)\\#.rs"));
    assert!(page.contains("src/deep/%5Bname%5D%29%23.rs"));
}

#[tokio::test]
async fn graph_byte_budget_counts_multibyte_text_as_utf8_bytes() {
    let (pool, root) = fixture("utf8-budget").await;
    let run = generation::active_run_id(&pool, &root.to_string_lossy())
        .await
        .unwrap()
        .unwrap();
    let (file_id,): (i64,) =
        sqlx::query_as("SELECT id FROM code_graph_files WHERE run_id=? LIMIT 1")
            .bind(run)
            .fetch_one(&pool)
            .await
            .unwrap();
    snapshot::insert_node(
        &pool,
        run,
        file_id,
        &symbol("한글", (3, 0, 3, 1), (3, 0, 3, 1)),
    )
    .await
    .unwrap();
    let (bytes,): (i64,) = sqlx::query_as(
        "SELECT length(CAST(name AS BLOB)) FROM code_graph_nodes WHERE run_id=? AND name='한글'",
    )
    .bind(run)
    .fetch_one(&pool)
    .await
    .unwrap();

    assert_eq!(bytes as usize, "한글".len());
}

#[cfg(unix)]
#[test]
fn preflight_rejects_aggregate_preimages_before_writes() {
    let root = crate::testtmp::dir().join(format!("codewiki-preimage-cap-{}", std::process::id()));
    std::fs::create_dir_all(root.join("docs/codebase")).unwrap();
    let modules: Vec<_> = (0..2_000)
        .map(|number| {
            (
                format!("src/{number:04}-{}.rs", "x".repeat(500)),
                "modules/missing.md".into(),
                CodeWikiPageState::Missing,
            )
        })
        .collect();
    let content = render::index("fingerprint", 1, 1, &modules, &[]);
    assert!(content.len() < model::MAX_INDEX_BYTES);
    let paths: Vec<_> = (0..17)
        .map(|number| format!("docs/codebase/index-{number}.md"))
        .collect();
    for path in &paths {
        std::fs::write(root.join(path), &content).unwrap();
    }
    let outputs: Vec<_> = paths.iter().map(|path| (path.as_str(), "index")).collect();
    assert!(storage::preflight(&root, &outputs).is_err());
}

#[cfg(unix)]
#[test]
fn write_rejects_a_fifo_replacing_a_captured_preimage_without_blocking() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::FileTypeExt;

    let root = crate::testtmp::dir().join(format!("codewiki-fifo-{}", std::process::id()));
    std::fs::create_dir_all(root.join("docs/codebase")).unwrap();
    let relative = "docs/codebase/fifo.md";
    let preimage = storage::preflight(&root, &[(relative, "index")]).unwrap();
    let path = root.join(relative);
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { nix::libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);

    assert!(storage::write(&root, &preimage[0], "replacement\n").is_err());
    assert!(std::fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_fifo());
    assert!(std::fs::read_dir(root.join("docs/codebase"))
        .unwrap()
        .flatten()
        .all(|entry| !entry
            .file_name()
            .to_string_lossy()
            .contains(".praxis-codewiki-")));
}

#[cfg(unix)]
#[tokio::test]
async fn rejects_a_symlinked_output_directory_before_writing() {
    let (pool, root) = fixture("symlink").await;
    let outside = crate::testtmp::dir().join("codewiki-outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::create_dir_all(root.join("docs")).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("docs/codebase")).unwrap();
    assert!(generate(&pool, &root, None, 3).await.is_err());
    assert!(!outside.join("index.md").exists());
}

/// 설계 0065 R8. 서버를 설치하고 다시 빌드하면 소스는 그대로라 `source_hash`도
/// `source_fingerprint`도 **둘 다 같다.** run을 함께 보지 않으면 "참조 분석 없음"이라는 거짓이
/// `Ready`인 채로 남는다.
#[tokio::test]
async fn installing_a_server_makes_a_no_reference_page_stale_without_touching_the_source() {
    let root = crate::testtmp::dir().join(format!(
        "codewiki-edge-state-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(root.join("src")).unwrap();
    std::fs::write(root.join("Cargo.toml"), "[package]\nname='edge-state'\n").unwrap();
    std::fs::write(root.join("src/lib.rs"), "pub fn api() {}\n").unwrap();
    let pool = test_pool("edge-state").await;
    let scanned = manifest::scan(&root).unwrap();
    let worktree = root.to_string_lossy().into_owned();
    let source = scanned.files[0].clone();

    let publish = |edge_state: Option<&'static str>, now: i64| {
        let pool = pool.clone();
        let worktree = worktree.clone();
        let fingerprint = scanned.fingerprint.clone();
        let source = source.clone();
        async move {
            let run = generation::start_run(&pool, &worktree, &fingerprint, now)
                .await
                .unwrap();
            let file = snapshot::insert_file(
                &pool,
                run,
                &source.rel_path,
                &source.content_hash,
                "rust",
                None,
                edge_state,
            )
            .await
            .unwrap();
            snapshot::insert_node(&pool, run, file, &symbol("api", (0, 7, 0, 10), (0, 0, 0, 15)))
                .await
                .unwrap();
            generation::promote(&pool, run, generation::RunStats::default(), now + 1)
                .await
                .unwrap();
        }
    };

    publish(Some("rust-analyzer 준비 신호 없음"), 1).await;
    let published = generate(&pool, &root, None, 3).await.unwrap();
    assert_eq!(published.modules[0].state, CodeWikiPageState::Ready);
    let page = std::fs::read_to_string(root.join("docs/codebase/modules/src/lib.rs.md")).unwrap();
    assert!(page.contains("Reference analysis did not run for this file"));
    assert!(page.contains("rust-analyzer 준비 신호 없음"));

    publish(None, 10).await;

    let after = status(&pool, &root).await.unwrap();
    assert_eq!(after.graph_state, "ready");
    assert_eq!(scanned.fingerprint, manifest::scan(&root).unwrap().fingerprint);
    assert_eq!(after.modules[0].state, CodeWikiPageState::Stale);
    assert_eq!(after.index_state, CodeWikiPageState::Stale);
}
