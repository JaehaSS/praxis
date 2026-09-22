//! 골든 쿼리 recall 실측 (설계 0020 DR-9).
//!
//! RAG 튜닝 손잡이(청크 크기·overlap·후보 수·RRF K·이웃 가중치)는 서로 얽혀 있다.
//! 측정 없이 만지면 바꿀 때마다 좋아졌는지 알 수 없고, 결국 감으로 고정된 채 방치된다.
//!
//! 실 vault가 필요해 `#[ignore]`다 — 선택 사항이라서가 아니라 경로가 환경마다 달라서다.
//! **Phase 1 완료 판정은 이 테스트를 실제로 돌려서 한다.**
//!
//! ```bash
//! PRAXIS_TEST_VAULT="$HOME/Documents/Obsidian Vault" \
//!   cargo test --lib golden -- --ignored --nocapture
//! ```
//!
//! 골든 케이스도 vault마다 다르다 — `golden.jsonl`은 형식 예시이고, 실제로 잴 때는
//! `golden.local.jsonl`이나 `PRAXIS_GOLDEN_JSONL`이 그 자리를 대신한다.

use std::time::Instant;

use super::test_pool;
use crate::knowledge::config::{MultiVault, VaultEntry};
use crate::knowledge::graph::embed_pending;
use crate::knowledge::search::search_with_stats;
use crate::knowledge::sync::sync_source;

struct GoldenCase {
    query: String,
    expect: Vec<String>,
}

/// 골든 케이스 파일. 정답은 **그 vault에만** 있는 문서 경로라 저장소에 든 것은 형식을 보이는
/// 예시일 뿐이다. 자기 vault로 재려면 옆에 `golden.local.jsonl`을 두거나(gitignore된다)
/// `PRAXIS_GOLDEN_JSONL`로 경로를 준다.
fn golden_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("PRAXIS_GOLDEN_JSONL") {
        return std::path::PathBuf::from(p);
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/knowledge/tests");
    let local = dir.join("golden.local.jsonl");
    if local.exists() {
        local
    } else {
        dir.join("golden.jsonl")
    }
}

/// 최소 의존성으로 JSONL을 읽는다 — 테스트 픽스처에 파서를 들일 이유가 없다.
fn load_golden() -> Vec<GoldenCase> {
    let path = golden_path();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("골든 파일을 읽지 못했다 ({}): {e}", path.display()));
    raw.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let v: serde_json::Value = serde_json::from_str(line).expect("golden.jsonl 파싱");
            GoldenCase {
                query: v["query"].as_str().unwrap().to_string(),
                expect: v["expect"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| e.as_str().unwrap().to_string())
                    .collect(),
            }
        })
        .collect()
}

#[tokio::test]
#[ignore = "실 vault 필요 — PRAXIS_TEST_VAULT로 경로 지정"]
async fn golden_queries_meet_recall_target() {
    let Ok(vault) = std::env::var("PRAXIS_TEST_VAULT") else {
        eprintln!("PRAXIS_TEST_VAULT 미설정 — 건너뜀");
        return;
    };
    let pool = test_pool().await;

    // 흡수 범위. 실측에서 이 vault는 청크의 98%가 Claude Code 자동 기록이었다 —
    // 전량 임베딩은 수 시간이 걸려 측정 자체가 불가능하다. 무엇을 넣고 뺄지가
    // 성능을 좌우한다는 것이 DR-11·DR-12의 요지이고, 여기서 그것을 실험할 수 있게 한다.
    let globs = |key: &str| -> Vec<String> {
        std::env::var(key)
            .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
            .unwrap_or_default()
    };
    let exclude = globs("PRAXIS_TEST_VAULT_EXCLUDE");
    let embed_exclude = globs("PRAXIS_TEST_VAULT_EMBED_EXCLUDE");
    if !exclude.is_empty() {
        println!("색인 제외: {exclude:?}");
    }
    if !embed_exclude.is_empty() {
        println!("임베딩 제외(색인은 함): {embed_exclude:?}");
    }

    let started = Instant::now();
    let source = MultiVault {
        entries: vec![VaultEntry {
            root: vault.clone(),
            exclude,
            embed_exclude,
        }],
    };
    let report = sync_source(&pool, &source, 1).await.unwrap();
    let scan_secs = started.elapsed().as_secs_f64();

    let embed_started = Instant::now();
    let mut embedded = 0usize;
    loop {
        let n = embed_pending(&pool, 256).await.unwrap();
        if n == 0 {
            break;
        }
        embedded += n;
    }
    let embed_secs = embed_started.elapsed().as_secs_f64();

    let (chunks,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM knowledge_chunks")
        .fetch_one(&pool)
        .await
        .unwrap();
    let (embeddable,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM knowledge_chunks c JOIN knowledge_nodes n ON n.id = c.node_id \
         WHERE n.embed_enabled = 1",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    println!(
        "\n색인: 문서 {} · 청크 {chunks}(임베딩 대상 {embeddable}) · 엣지 {} · 스캔 {scan_secs:.1}s · 임베딩 {embedded}건 {embed_secs:.1}s",
        report.indexed, report.edges
    );

    let cases = load_golden();
    let mut hit = 0usize;
    let mut latencies = Vec::new();
    let mut misses = Vec::new();

    for case in &cases {
        let t = Instant::now();
        let (hits, stats) = search_with_stats(&pool, &case.query, 10).await.unwrap();
        latencies.push(t.elapsed().as_secs_f64() * 1000.0);

        let found = hits
            .iter()
            .any(|h| case.expect.iter().any(|e| h.external_id.ends_with(e)));
        if found {
            hit += 1;
        } else {
            let top: Vec<&str> = hits.iter().take(3).map(|h| h.title.as_str()).collect();
            misses.push(format!(
                "  ✗ {} → 후보{} 상위{:?}",
                case.query, stats.fts_candidates, top
            ));
        }
    }

    latencies.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let idx = ((latencies.len() as f64 * 0.95) as usize).min(latencies.len() - 1);
    let p95 = latencies[idx];
    let recall = hit as f64 / cases.len() as f64;

    println!("\nrecall@10 = {hit}/{} = {recall:.2}", cases.len());
    println!("검색 p95 = {p95:.0}ms");
    if !misses.is_empty() {
        println!("\n놓친 질의:");
        for m in &misses {
            println!("{m}");
        }
    }

    assert!(
        recall >= 0.8,
        "recall@10 = {recall:.2} — 후보 수를 올리기 전에 청킹과 프리필터를 먼저 본다"
    );
    assert!(p95 < 200.0, "검색 p95 = {p95:.0}ms — 200ms 목표 초과");
}
