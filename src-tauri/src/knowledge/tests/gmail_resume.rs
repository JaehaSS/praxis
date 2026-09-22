//! 백필 중단·재개 검증 (플랜 0028 Task 7 / 설계 0020 Phase 4 완료 판정).
//!
//! 실제 Gmail 대신 **페이지를 나눠 주는 더블**을 쓴다. 검증 대상은 Gmail API가 아니라
//! "커밋한 만큼만 커서를 올린다"는 오케스트레이션 성질이고, 그건 더블로 충분히 고정된다.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::test_pool;
use crate::knowledge::graph::Document;
use crate::knowledge::sync::{
    load_cursor, sync_until_done, BoxChanges, Source, SourceChanges,
};

/// 3페이지짜리 소스. Gmail 백필과 같은 형태로 커서를 전진시킨다.
struct PagedSource {
    /// 호출 횟수 — "재처리하지 않았다"를 세는 데 쓴다.
    calls: AtomicUsize,
}

impl PagedSource {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

const PAGES: usize = 3;
const PER_PAGE: usize = 2;

impl Source for PagedSource {
    fn id(&self) -> &str {
        "gmail"
    }

    fn changes<'a>(&'a self, cursor: Option<&'a str>) -> BoxChanges<'a> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            // 커서는 "다음에 줄 페이지 번호". 없으면 0페이지부터.
            let page: usize = cursor
                .and_then(|c| c.strip_prefix("backfill:h1:"))
                .and_then(|p| p.parse().ok())
                .unwrap_or(0);

            let upserts: Vec<Document> = (0..PER_PAGE)
                .map(|i| {
                    let id = page * PER_PAGE + i;
                    Document::embedded("gmail", format!("msg-{id}"), format!("메일 {id}"), "본문")
                })
                .collect();

            let last = page + 1 >= PAGES;
            Ok(SourceChanges {
                upserts,
                deletions: Vec::new(),
                next_cursor: Some(if last {
                    "history:h1".to_string()
                } else {
                    format!("backfill:h1:{}", page + 1)
                }),
                full_scan: false,
                has_more: !last,
            })
        })
    }
}

async fn stored_ids(pool: &sqlx::SqlitePool) -> Vec<String> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT external_id FROM knowledge_nodes WHERE source = 'gmail' ORDER BY external_id",
    )
    .fetch_all(pool)
    .await
    .unwrap();
    rows.into_iter().map(|r| r.0).collect()
}

/// **Acceptance의 핵심.** 예산을 1로 끊으면 한 페이지만 처리하고, 다음 호출이
/// 이어받아야 한다. 재처리도 누락도 없어야 한다.
#[tokio::test]
async fn an_interrupted_backfill_resumes_where_it_stopped() {
    let pool = test_pool().await;
    let source = PagedSource::new();

    // ① 예산 1 — 첫 페이지만.
    let first = sync_until_done(&pool, &source, 1, 1).await.unwrap();
    assert_eq!(first.indexed, PER_PAGE, "첫 배치가 한 페이지를 넘겼다");
    assert!(first.has_more, "아직 남았는데 끝났다고 보고했다");
    assert_eq!(stored_ids(&pool).await, vec!["msg-0", "msg-1"]);

    // ② 커서가 커밋된 지점을 가리켜야 한다. 이게 없으면 재개는 처음부터가 된다.
    assert_eq!(
        load_cursor(&pool, "gmail").await.unwrap().as_deref(),
        Some("backfill:h1:1")
    );

    // ③ 재개 — 남은 두 페이지.
    let calls_before = source.calls();
    let rest = sync_until_done(&pool, &source, 2, 10).await.unwrap();
    assert!(!rest.has_more, "끝났는데 더 있다고 보고했다");

    // ④ 전량이 정확히 한 번씩. 재처리했다면 indexed가 부풀고, 누락이면 개수가 준다.
    assert_eq!(
        stored_ids(&pool).await,
        vec!["msg-0", "msg-1", "msg-2", "msg-3", "msg-4", "msg-5"]
    );
    assert_eq!(rest.indexed, PER_PAGE * 2, "재개가 이미 넣은 페이지를 다시 처리했다");
    assert_eq!(
        source.calls() - calls_before,
        2,
        "남은 페이지 수보다 많이 호출했다"
    );
}

/// 예산이 넉넉하면 한 번에 끝까지 간다.
#[tokio::test]
async fn a_full_run_consumes_every_page_in_one_call() {
    let pool = test_pool().await;
    let source = PagedSource::new();

    let report = sync_until_done(&pool, &source, 1, 10).await.unwrap();

    assert_eq!(report.indexed, PER_PAGE * PAGES);
    assert!(!report.has_more);
    assert_eq!(source.calls(), PAGES, "페이지 수만큼만 호출해야 한다");
    // 백필이 끝나면 증분 상태로 넘어간다.
    assert_eq!(
        load_cursor(&pool, "gmail").await.unwrap().as_deref(),
        Some("history:h1")
    );
}

/// 증분 단계는 커서가 **항상** 남는다. 커서 유무로 종료를 판정하면 여기서 무한 루프가 된다.
#[tokio::test]
async fn an_incremental_source_terminates_even_though_its_cursor_never_empties() {
    struct Incremental;
    impl Source for Incremental {
        fn id(&self) -> &str {
            "gmail"
        }
        fn changes<'a>(&'a self, _cursor: Option<&'a str>) -> BoxChanges<'a> {
            Box::pin(async move {
                Ok(SourceChanges {
                    upserts: Vec::new(),
                    deletions: Vec::new(),
                    next_cursor: Some("history:always-present".to_string()),
                    full_scan: false,
                    has_more: false,
                })
            })
        }
    }

    let pool = test_pool().await;
    // max_batches를 크게 줘도 has_more=false면 한 번에 멈춰야 한다.
    let report = sync_until_done(&pool, &Incremental, 1, 1000).await.unwrap();
    assert!(!report.has_more);
    assert_eq!(report.indexed, 0);
}
