//! 이 저장소 자신을 대상으로 한 실사용 검증 (계획 0037 Task 7 · DR-4 Unknown).
//!
//! 기본 실행에서는 건너뛴다 — rust-analyzer 설치가 필요하다.
//!
//! ```text
//! cargo test --lib -- --ignored codegraph::live --nocapture
//! ```
//!
//! **워크트리 전체를 인덱싱하지 않는다.** 여기서 답할 질문은 "`impact_of`가 알려진 호출처를
//! 찾는가"이고, 그러려면 대상 심볼과 **그것을 참조하는 파일들**만 있으면 된다. 전체 처리량은
//! Task 2가 따로 쟀다.
//!
//! 전체 인덱싱을 시도했다가 접은 이유는 실측 자체가 흔들렸기 때문이다. rust-analyzer는
//! 프로젝트 로딩이 끝나기 전에도 `documentSymbol`에 **에러 없이** 응답하는데 그 응답이
//! 불완전하다 — 같은 저장소를 네 번 돌려 심볼 수가 11222·5264·4174·2468로 갈렸고
//! `skip_reason`은 0건이었다. 조용히 적게 오는 형태라, 기다리지 않으면 절반짜리 그래프
//! 위에서 검증이 통과해 버린다. 인덱싱 경로(`build::index_worktree`)를 실제로 쓸 때는
//! 서버 준비를 기다리는 장치가 필요하다.

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;

use crate::lspclient::{GotoKind, LspPool};

use super::{index, query};

async fn live_pool() -> SqlitePool {
    let path = crate::testtmp::dir().join(format!(
        "praxis-codegraph-live-{}.sqlite",
        std::process::id()
    ));
    // WAL 모드라 `-wal`·`-shm`이 따로 남는다. 본 파일만 지우면 이전 실행의 데이터가
    // 되살아나 "변경 없음"으로 판정되고, 실측이 절반만 도는 채로 통과한다.
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
    }
    let pool = crate::db::init_pool(path.to_str().unwrap()).await.unwrap();
    super::migrate(&pool).await.unwrap();
    pool
}

/// 파일 하나를 인덱싱한다. 서버가 아직 그 파일을 모르면 심볼이 0으로 오므로 재시도한다.
async fn index_until_ready(
    pool: &SqlitePool,
    lsp: &LspPool,
    worktree: &Path,
    worktree_key: &str,
    rel: &str,
) -> usize {
    for attempt in 0..40 {
        let symbols = lsp.document_symbols(0, worktree, rel).await.unwrap_or_default();
        if !symbols.is_empty() {
            let text = std::fs::read_to_string(worktree.join(rel)).unwrap();
            let file_id = index::upsert_file(pool, worktree_key, rel, &text, Some("rust"), 1)
                .await
                .unwrap();
            return index::replace_nodes(pool, file_id, &symbols).await.unwrap();
        }
        if attempt == 0 {
            println!("  {rel}: 서버가 아직 준비되지 않았다 — 기다린다");
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    panic!("{rel}: rust-analyzer가 20초 안에 심볼을 주지 않았다");
}

#[tokio::test]
#[ignore = "실측: rust-analyzer 설치 필요"]
async fn impact_of_finds_the_known_callers_of_execution_prompt() {
    let worktree = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let worktree_key = worktree.to_string_lossy().into_owned();
    let pool = live_pool().await;
    let lsp = LspPool::default();

    // ── 1단계: 대상 파일 ──
    let home = "src/goal_contract/mod.rs";
    let n = index_until_ready(&pool, &lsp, &worktree, &worktree_key, home).await;
    println!("\n=== impact_of 실측 ===");
    println!("대상 파일 {home} — 심볼 {n}");

    let target = query::find_nodes_by_name(&pool, &worktree_key, "execution_prompt")
        .await
        .unwrap()
        .into_iter()
        .find(|c| c.rel_path == home)
        .expect("goal_contract::execution_prompt 노드를 찾지 못했다");
    let (sel_char,): (i64,) = sqlx::query_as("SELECT sel_char FROM code_nodes WHERE id = ?")
        .bind(target.id)
        .fetch_one(&pool)
        .await
        .unwrap();
    println!("대상 좌표 {}:{} (0-based)", target.sel_line, sel_char);

    // ── 2단계: 참조 ──
    // 커서는 심볼 **이름 위**에 놓여야 한다. 열을 1로 두면 들여쓰기나 doc comment를 가리켜
    // references가 0건으로 돌아온다 — 실제로 그렇게 한 번 헛돌았다.
    //
    // `documentSymbol`과 달리 `references`는 **프로젝트 전체 의미 분석**이 끝나야 답한다.
    // 로딩 중에는 에러가 아니라 빈 배열이 오므로, 기다리지 않으면 "참조가 없다"로 오독한다.
    // Task 2에서 documentSymbol 중앙값이 1ms였던 것과 성격이 전혀 다르다.
    let text = std::fs::read_to_string(worktree.join(home)).unwrap();
    let started = std::time::Instant::now();
    let mut refs = Vec::new();
    for attempt in 0..120 {
        refs = lsp
            .goto(
                0,
                &worktree,
                home,
                &text,
                target.sel_line as u32 + 1,
                sel_char as u32 + 1,
                GotoKind::References,
            )
            .await
            .unwrap_or_default();
        if !refs.is_empty() {
            break;
        }
        if attempt == 0 {
            println!("references 대기 — 프로젝트 의미 분석이 끝나야 답한다");
        }
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
    }
    assert!(
        !refs.is_empty(),
        "references가 2분 안에 오지 않았다 — 커서가 심볼 위에 없거나 서버가 로딩을 못 끝냈다"
    );
    println!("references 준비까지 {:?}", started.elapsed());

    // ── 3단계: 참조가 있는 파일만 인덱싱 ──
    let mut ref_files: Vec<String> = refs.iter().filter_map(|r| r.path.clone()).collect();
    ref_files.sort();
    ref_files.dedup();
    println!("참조 {}건 · 참조가 걸친 파일 {}개", refs.len(), ref_files.len());
    for rel in &ref_files {
        if rel != home {
            index_until_ready(&pool, &lsp, &worktree, &worktree_key, rel).await;
        }
    }

    // ── 4단계: 엣지 ──
    let mut unresolved = 0usize;
    for location in &refs {
        let Some(rel) = location.path.as_deref() else {
            continue;
        };
        let line = location.line.saturating_sub(1);
        match index::enclosing_node(&pool, &worktree_key, rel, line).await.unwrap() {
            Some(src) => {
                index::add_reference_edge(&pool, src, target.id).await.unwrap();
            }
            // 어느 심볼에도 안 들어간 참조 — 최상단 `use` 등. 엣지가 아니다.
            None => unresolved += 1,
        }
    }
    lsp.shutdown_task(0).await;

    let impact = query::impact_of(&pool, target.id, 2).await.unwrap();
    println!(
        "심볼로 해석 {} · 미해석(최상단 use 등) {unresolved}",
        refs.len() - unresolved
    );
    println!("impact_of(depth=2) → {}건", impact.items.len());
    for item in &impact.items {
        println!(
            "  d{} {} :{} — {}",
            item.depth, item.rel_path, item.sel_line, item.name
        );
    }

    // 계획서가 지목한 알려진 호출처. 누락되면 Task 5의 enclosing 로직 결함이다.
    assert!(
        impact.items.iter().any(|i| i.rel_path.ends_with("capsule/mod.rs")),
        "알려진 호출처 capsule/mod.rs가 영향 범위에 없다"
    );
    assert!(
        impact.items.iter().any(|i| i.rel_path.ends_with("commands.rs")),
        "commands.rs의 호출처도 나와야 한다"
    );
}
