//! 자기개선 제안(proposal) 테스트. `cargo test`

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::db;
use praxis_lib::memory::{self, tier};
use praxis_lib::selfimprove::{self, pkind, status};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// 이 실행 전용 DB 경로.
///
/// **실행 간 격리는 `temp_root`가 맡는다** — 루트가 프로세스마다 새로 생기므로 여기서는
/// 같은 프로세스 안의 테스트끼리만 구분하면 된다.
fn temp_db() -> String {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    temp_root::dir()
        .join(format!("praxis-si-test-{}-{n}.sqlite", std::process::id()))
        .to_string_lossy()
        .into_owned()
}

/// DB 본체와 WAL 부산물을 함께 지운다.
///
/// 루트가 통째로 정리되므로 정확성에는 이것 없이도 문제가 없다. 그래도 지우는 이유는
/// 디스크다 — 루트는 다음 실행의 청소를 기다리는 동안 남아 있고, WAL은 파일당 1MB를 넘는다.
fn remove_db(path: &str) {
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{path}{suffix}"));
    }
}

async fn setup() -> (sqlx::SqlitePool, String) {
    let path = temp_db();
    let pool = db::init_pool(&path).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    selfimprove::migrate(&pool).await.unwrap();
    (pool, path)
}

#[tokio::test]
async fn insert_and_list_pending() {
    let (pool, path) = setup().await;
    selfimprove::insert_proposal(
        &pool,
        "/repo",
        pkind::REFLECTION,
        "테스트는 항상 먼저 작성됐다",
        Some("s1"),
        1,
    )
    .await
    .unwrap();
    let pending = selfimprove::list_proposals(&pool, true).await.unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].status, status::PROPOSED);
    assert_eq!(pending[0].kind, pkind::REFLECTION);
    remove_db(&path);
}

#[tokio::test]
async fn apply_routes_proposal_to_candidate_review_and_is_idempotent() {
    let (pool, path) = setup().await;
    let id = selfimprove::insert_proposal(
        &pool,
        "/repo",
        pkind::REFLECTION,
        "auth 리팩토링은 작은 PR로 쪼개라",
        None,
        1,
    )
    .await
    .unwrap();
    let mem_id = selfimprove::apply_proposal(&pool, id, 2).await.unwrap();
    assert!(mem_id.is_some());
    // 검증되지 않은 반성은 주입 가능한 메모리가 아니라 candidate로 이동한다.
    let mems = memory::list_project(&pool, "/repo").await.unwrap();
    assert_eq!(mems.len(), 1);
    assert!(mems[0].content.contains("작은 PR"));
    assert_eq!(mems[0].tier, tier::PROJECT);
    assert_eq!(mems[0].status, memory::knowledge_status::CANDIDATE);
    assert_eq!(mems[0].knowledge_type, memory::knowledge_type::OBSERVATION);
    // 상태 applied, pending 목록에서 빠짐
    assert_eq!(
        selfimprove::list_proposals(&pool, true)
            .await
            .unwrap()
            .len(),
        0
    );
    // 재적용 불가 (이미 처리됨)
    assert!(selfimprove::apply_proposal(&pool, id, 3).await.is_err());
    remove_db(&path);
}

#[tokio::test]
async fn reject_does_not_create_memory() {
    let (pool, path) = setup().await;
    let id = selfimprove::insert_proposal(&pool, "/repo", pkind::REFLECTION, "버림", None, 1)
        .await
        .unwrap();
    selfimprove::reject_proposal(&pool, id, 2).await.unwrap();
    assert_eq!(memory::list_project(&pool, "/repo").await.unwrap().len(), 0);
    assert_eq!(
        selfimprove::list_proposals(&pool, true)
            .await
            .unwrap()
            .len(),
        0
    );
    let all = selfimprove::list_proposals(&pool, false).await.unwrap();
    assert_eq!(all[0].status, status::REJECTED);
    // 재거부 불가
    assert!(selfimprove::reject_proposal(&pool, id, 3).await.is_err());
    remove_db(&path);
}
