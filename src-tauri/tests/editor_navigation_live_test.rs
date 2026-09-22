//! Real-language-server acceptance tests for the editor navigation fixtures.
//!
//! Run intentionally and serially because each test starts an LSP subprocess:
//! `cargo test --manifest-path src-tauri/Cargo.toml --test editor_navigation_live_test -- --ignored --test-threads=1 --nocapture`

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use praxis_lib::codegraph::{self, build, jobs::BuildJobs, manifest, neighborhood};
use praxis_lib::db;
use praxis_lib::lspclient::server;
use praxis_lib::lspclient::{GotoKind, LspPool, Readiness};

static TEMP_SEQUENCE: AtomicU32 = AtomicU32::new(0);

#[derive(Clone, Copy)]
struct Caller {
    name: &'static str,
    path: &'static str,
    line: u32,
}

#[derive(Clone, Copy)]
struct Location {
    path: &'static str,
    line: u32,
}

#[derive(Clone, Copy)]
struct Fixture {
    name: &'static str,
    target: &'static str,
    declaration: &'static str,
    callers: [Caller; 2],
    excluded: Location,
    task_id: i64,
}

struct Evidence {
    report: build::BuildReport,
    readiness: Readiness,
    graph: neighborhood::Neighborhood,
    references: Vec<praxis_lib::lspclient::LspTarget>,
}

fn fixture_source(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/editor-navigation")
        .join(name)
}

fn unique_root(name: &str) -> PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::SeqCst);
    std::env::temp_dir().join(format!(
        "praxis-editor-navigation-live-{name}-{}-{sequence}",
        std::process::id(),
    ))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination).map_err(|error| error.to_string())?;
    for entry in fs::read_dir(source).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let target = destination.join(entry.file_name());
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), target).map_err(|error| error.to_string())?;
        } else {
            return Err(format!(
                "fixture contains unsupported entry: {}",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn locate(text: &str, line_contains: &str, needle: &str) -> Result<(u32, u32), String> {
    let (index, line) = text
        .lines()
        .enumerate()
        .find(|(_, line)| line.contains(line_contains))
        .ok_or_else(|| format!("fixture line missing: {line_contains}"))?;
    let column = line
        .find(needle)
        .ok_or_else(|| format!("fixture token missing: {needle}"))?;
    Ok((index as u32 + 1, column as u32 + 1))
}

async fn execute(fixture: Fixture) -> Result<Evidence, String> {
    let root = unique_root(fixture.name);
    let result = execute_in_root(fixture, &root).await;
    let _ = fs::remove_dir_all(&root);
    result
}

async fn execute_in_root(fixture: Fixture, root: &Path) -> Result<Evidence, String> {
    copy_tree(&fixture_source(fixture.name), root)?;
    let database = root.with_extension("sqlite");
    let pool = db::init_pool(database.to_str().ok_or("non-UTF8 database path")?)
        .await
        .map_err(|error| error.to_string())?;
    codegraph::migrate(&pool)
        .await
        .map_err(|error| error.to_string())?;
    let lsp = LspPool::default();
    let result = query_fixture(&pool, &lsp, fixture, root).await;
    lsp.shutdown_all().await;
    pool.close().await;
    for suffix in ["", "-wal", "-shm"] {
        let _ = fs::remove_file(format!("{}{suffix}", database.display()));
    }
    result
}

async fn query_fixture(
    pool: &sqlx::SqlitePool,
    lsp: &LspPool,
    fixture: Fixture,
    root: &Path,
) -> Result<Evidence, String> {
    let (spec, _) = server::spec_for_path(&root.join(fixture.target))
        .ok_or_else(|| format!("unsupported fixture target: {}", fixture.target))?;
    // The progress/status readiness signals begin only after a document opens.
    // This matches the graph builder's warm phase without retrying for counts.
    let _warm_symbols = tokio::time::timeout(
        Duration::from_secs(spec.init_timeout_secs + 60),
        lsp.document_symbols(fixture.task_id, root, fixture.target),
    )
    .await
    .map_err(|_| format!("{} document-symbol warmup timed out", fixture.name))?
    .map_err(|error| error.to_string())?;
    let readiness = tokio::time::timeout(
        Duration::from_secs(spec.ready_timeout_secs + 60),
        lsp.wait_semantic_ready(fixture.task_id, root, spec),
    )
    .await
    .map_err(|_| format!("{} readiness timed out", fixture.name))?;
    let jobs = BuildJobs::default();
    let guard = jobs
        .start(fixture.task_id)
        .map_err(|error| error.to_string())?;
    let report = tokio::time::timeout(
        Duration::from_secs(spec.ready_timeout_secs + 90),
        build::index_worktree(pool, lsp, fixture.task_id, root, &guard, 100),
    )
    .await
    .map_err(|_| format!("{} graph build timed out", fixture.name))?
    .map_err(|error| error.to_string())?;
    drop(guard);

    let source = manifest::scan(root).map_err(|error| error.to_string())?;
    let target_text =
        fs::read_to_string(root.join(fixture.target)).map_err(|error| error.to_string())?;
    let (line, column) = locate(&target_text, fixture.declaration, "target")?;
    let graph = neighborhood::at(
        pool,
        &root.to_string_lossy(),
        fixture.target,
        line.saturating_sub(1),
        column.saturating_sub(1),
        neighborhood::Direction::Incoming,
        1,
        &source,
    )
    .await
    .map_err(|error| error.to_string())?;
    let references = lsp
        .goto(
            fixture.task_id,
            root,
            fixture.target,
            &target_text,
            line,
            column,
            GotoKind::References,
        )
        .await
        .map_err(|error| error.to_string())?;
    Ok(Evidence {
        report,
        readiness,
        graph,
        references,
    })
}

fn assert_ready(readiness: &Readiness, fixture: &Fixture) {
    assert!(
        matches!(readiness, Readiness::Ready),
        "{} did not become semantically ready: {readiness:?}",
        fixture.name
    );
}

fn assert_two_callers(evidence: &Evidence, fixture: &Fixture) {
    assert!(
        evidence.report.symbols > 0,
        "{} indexed no symbols",
        fixture.name
    );
    for caller in fixture.callers {
        assert!(
            evidence
                .graph
                .nodes
                .iter()
                .any(|node| node.name == caller.name),
            "{} graph omitted {}; nodes={:?}; direct references={:?}",
            fixture.name,
            caller.name,
            evidence.graph.nodes,
            evidence.references,
        );
    }
    for caller in fixture.callers {
        assert!(
            evidence.references.iter().any(|target| {
                target.path.as_deref() == Some(caller.path) && target.line == caller.line
            }),
            "{} references omitted {} at {}:{}: {:?}",
            fixture.name,
            caller.name,
            caller.path,
            caller.line,
            evidence.references
        );
    }
    assert!(
        evidence.references.iter().all(|target| {
            target.path.as_deref() != Some(fixture.excluded.path)
                || target.line != fixture.excluded.line
        }),
        "{} references leaked same-named target: {:?}",
        fixture.name,
        evidence.references
    );
}

#[tokio::test]
#[ignore = "live acceptance: rust-analyzer required"]
async fn should_return_rust_callers_without_the_same_named_target() {
    let fixture = Fixture {
        name: "rust",
        target: "src/lib.rs",
        declaration: "pub fn target()",
        callers: [
            Caller {
                name: "caller_a",
                path: "src/lib.rs",
                line: 4,
            },
            Caller {
                name: "caller_b",
                path: "src/lib.rs",
                line: 8,
            },
        ],
        excluded: Location {
            path: "src/lib.rs",
            line: 12,
        },
        task_id: 901,
    };
    let evidence = execute(fixture).await.expect("rust fixture acceptance");

    assert_ready(&evidence.readiness, &fixture);
    assert_two_callers(&evidence, &fixture);
}

#[tokio::test]
#[ignore = "live acceptance: typescript-language-server required"]
async fn should_return_typescript_callers_without_the_same_named_target() {
    let fixture = Fixture {
        name: "typescript",
        target: "src/target.ts",
        declaration: "export function target()",
        callers: [
            Caller {
                name: "callerA",
                path: "src/callers.ts",
                line: 4,
            },
            Caller {
                name: "callerB",
                path: "src/callers.ts",
                line: 8,
            },
        ],
        excluded: Location {
            path: "src/other.ts",
            line: 1,
        },
        task_id: 902,
    };
    let evidence = execute(fixture)
        .await
        .expect("typescript fixture acceptance");

    assert_ready(&evidence.readiness, &fixture);
    assert_two_callers(&evidence, &fixture);
}

#[tokio::test]
#[ignore = "live acceptance: pyright required"]
async fn should_keep_python_symbols_when_reference_edges_are_unsupported() {
    let fixture = Fixture {
        name: "python",
        target: "target.py",
        declaration: "def target()",
        callers: [
            Caller {
                name: "caller_a",
                path: "callers.py",
                line: 5,
            },
            Caller {
                name: "caller_b",
                path: "callers.py",
                line: 9,
            },
        ],
        excluded: Location {
            path: "other.py",
            line: 1,
        },
        task_id: 903,
    };
    let evidence = execute(fixture).await.expect("python fixture acceptance");

    assert!(matches!(evidence.readiness, Readiness::Unsupported(_)));
    assert!(
        evidence.report.symbols > 0,
        "python symbols were not retained"
    );
    assert!(
        evidence.graph.edges_unavailable.is_some(),
        "python edge state missing"
    );
    println!(
        "python live references (diagnostic only): {:?}",
        evidence.references
    );
}

#[tokio::test]
#[ignore = "live acceptance: jdtls required"]
async fn should_return_java_callers_without_the_same_named_target() {
    let fixture = Fixture {
        name: "java",
        target: "src/sample/Target.java",
        declaration: "public static void target()",
        callers: [
            Caller {
                name: "callerA()",
                path: "src/sample/Callers.java",
                line: 7,
            },
            Caller {
                name: "callerB()",
                path: "src/sample/Callers.java",
                line: 11,
            },
        ],
        excluded: Location {
            path: "src/sample/Callers.java",
            line: 14,
        },
        task_id: 904,
    };
    let evidence = execute(fixture).await.expect("java fixture acceptance");

    assert_ready(&evidence.readiness, &fixture);
    assert_two_callers(&evidence, &fixture);
}
