//! 출제·채점 — 상태 필터와 이어 풀기.

use super::test_pool;
use crate::quiz::serve;

/// 검수 전 도메인 문제는 출제되지 않는다 (설계 0044 비즈니스 규칙 1).
#[tokio::test]
async fn pending_items_are_never_served() {
    let pool = test_pool().await;
    insert(&pool, "domain", "검수중", "답", "pending").await;

    assert!(serve::next(&pool, &[], 100).await.unwrap().is_none());
}

/// 신고된 문제는 다시 나오지 않는다 (설계 0044 비즈니스 규칙 5).
#[tokio::test]
async fn retired_items_are_never_served() {
    let pool = test_pool().await;
    let id = insert(&pool, "vocab", "신고됨", "답", "approved").await;

    serve::report(&pool, id, 100).await.unwrap();

    assert!(serve::next(&pool, &[], 100).await.unwrap().is_none());
}

#[tokio::test]
async fn approving_makes_an_item_servable() {
    let pool = test_pool().await;
    let id = insert(&pool, "domain", "검수중", "답", "pending").await;

    serve::approve(&pool, id, 100).await.unwrap();

    let item = serve::next(&pool, &[], 100).await.unwrap().unwrap();
    assert_eq!(item.id, id);
}

/// 풀다 만 문제가 있으면 그것부터 준다 — 대기가 끊겨도 이어서 푼다 (설계 0044 DR-3).
#[tokio::test]
async fn an_open_attempt_is_resumed_before_a_new_one() {
    let pool = test_pool().await;
    let first = insert(&pool, "vocab", "첫문제", "답", "approved").await;
    let served = serve::next(&pool, &[], 100).await.unwrap().unwrap();
    assert_eq!(served.id, first);

    // 두 번째 문제를 넣어도, 아직 답하지 않은 첫 문제가 계속 나와야 한다.
    insert(&pool, "vocab", "둘째문제", "답", "approved").await;
    let again = serve::next(&pool, &[], 200).await.unwrap().unwrap();

    assert_eq!(again.id, first, "풀던 문제를 두고 새 문제를 줬다");
}

#[tokio::test]
async fn answering_closes_the_attempt_and_lets_the_next_one_through() {
    let pool = test_pool().await;
    let first = insert(&pool, "vocab", "첫문제", "정답", "approved").await;
    serve::next(&pool, &[], 100).await.unwrap();

    let result = serve::answer(&pool, first, "정답", 150)
        .await
        .unwrap()
        .unwrap();
    assert!(result.correct);
    assert_eq!(result.answer, "정답");

    insert(&pool, "vocab", "둘째문제", "답", "approved").await;
    let next = serve::next(&pool, &[], 200).await.unwrap().unwrap();
    assert_ne!(next.id, first, "답한 문제가 다시 나왔다");
}

#[tokio::test]
async fn a_wrong_answer_reports_the_right_one() {
    let pool = test_pool().await;
    let id = insert(&pool, "vocab", "질문", "정답", "approved").await;
    serve::next(&pool, &[], 100).await.unwrap();

    let result = serve::answer(&pool, id, "오답", 150)
        .await
        .unwrap()
        .unwrap();

    assert!(!result.correct);
    assert_eq!(result.answer, "정답", "틀렸을 때 정답을 알려주지 않는다");
}

#[tokio::test]
async fn kind_filter_limits_what_is_served() {
    let pool = test_pool().await;
    insert(&pool, "trivia", "상식문제", "답", "approved").await;

    let filtered = serve::next(&pool, &["vocab".into()], 100).await.unwrap();
    assert!(filtered.is_none(), "요청하지 않은 종류가 나왔다");

    let matched = serve::next(&pool, &["trivia".into()], 100).await.unwrap();
    assert!(matched.is_some());
}

/// 알 수 없는 종류만 요청하면 필터가 비어 전부 통과하는 함정이 있다 — 그렇게 되면 안 된다.
#[tokio::test]
async fn an_unknown_kind_filter_serves_nothing() {
    let pool = test_pool().await;
    insert(&pool, "trivia", "상식문제", "답", "approved").await;

    let served = serve::next(&pool, &["게임".into()], 100).await.unwrap();

    assert!(served.is_none(), "모르는 종류를 무시하고 전부 출제했다");
}

/// 출제 응답에 정답이 실리면 화면 소스에서 답이 보인다.
#[tokio::test]
async fn the_served_item_does_not_leak_the_answer() {
    let pool = test_pool().await;
    insert(&pool, "vocab", "질문", "비밀정답", "approved").await;

    let item = serve::next(&pool, &[], 100).await.unwrap().unwrap();
    let json = serde_json::to_string(&item).unwrap();

    assert!(!json.contains("비밀정답"), "출제 응답에 정답이 실렸다");
}

#[tokio::test]
async fn pending_list_carries_the_source() {
    let pool = test_pool().await;
    sqlx::query(
        "INSERT INTO knowledge_nodes (source, external_id, kind, title, updated_at, synced_at) \
         VALUES ('obsidian', 'n.md', 'document', '제목', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO knowledge_chunks (node_id, ord, doc_title, heading, content) \
         VALUES ((SELECT id FROM knowledge_nodes), 0, '배포 절차', '롤백', '본문')",
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO quiz_items (kind, question, answer, chunk_id, source_excerpt, status, created_at) \
         VALUES ('domain', '롤백?', '태그', (SELECT id FROM knowledge_chunks), '본문', 'pending', 1)",
    )
    .execute(&pool)
    .await
    .unwrap();

    let list = serve::pending(&pool, 10).await.unwrap();

    assert_eq!(list.len(), 1);
    assert_eq!(list[0].doc_title.as_deref(), Some("배포 절차"));
    assert_eq!(list[0].heading.as_deref(), Some("롤백"));
    assert_eq!(list[0].source_excerpt.as_deref(), Some("본문"));
}

/// 큐가 비면 둘 다 0 — 화면은 이 값을 보고 패널을 아예 열지 않는다 (이슈 #87).
#[tokio::test]
async fn availability_is_zero_on_an_empty_queue() {
    let pool = test_pool().await;

    let counts = serve::availability(&pool, &[]).await.unwrap();

    assert_eq!(counts.askable, 0);
    assert_eq!(counts.pending_review, 0);
}

/// 검수 대기는 출제 가능과 **따로** 센다. 도메인 문제는 승인 전엔 안 나오므로
/// "낼 것은 없지만 검수할 것은 있다"가 흔한 상태다 — 그때 할 일은 검수다.
#[tokio::test]
async fn availability_separates_askable_from_pending_review() {
    let pool = test_pool().await;
    insert(&pool, "vocab", "낼수있음1", "답", "approved").await;
    insert(&pool, "vocab", "낼수있음2", "답", "approved").await;
    insert(&pool, "domain", "검수중", "답", "pending").await;

    let counts = serve::availability(&pool, &[]).await.unwrap();

    assert_eq!(counts.askable, 2, "승인된 것만 세야 한다");
    assert_eq!(counts.pending_review, 1);
}

/// 프로브가 문제를 소비하면 안 된다 — 폴링이 부르는 자리라, 시도를 열면 아무도 보지 않은
/// 문제가 계속 "풀던 문제"로 쌓인다.
#[tokio::test]
async fn availability_does_not_open_an_attempt() {
    let pool = test_pool().await;
    let id = insert(&pool, "vocab", "문제", "답", "approved").await;

    serve::availability(&pool, &[]).await.unwrap();

    let (attempts,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM quiz_attempts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(attempts, 0, "프로브가 시도를 열었다");
    // 소비되지 않았으니 그대로 나와야 한다.
    assert_eq!(serve::next(&pool, &[], 100).await.unwrap().unwrap().id, id);
}

/// 답한 문제는 `next`가 다시 내지 않으므로 여기서도 빠져야 한다. 어긋나면 "있다"고 열어
/// 놓고 빈 화면을 보여준다.
#[tokio::test]
async fn answered_items_are_not_askable() {
    let pool = test_pool().await;
    let id = insert(&pool, "vocab", "문제", "정답", "approved").await;
    serve::next(&pool, &[], 100).await.unwrap();
    serve::answer(&pool, id, "정답", 150).await.unwrap();

    assert_eq!(serve::availability(&pool, &[]).await.unwrap().askable, 0);
}

/// 반대로 풀다 만 문제는 여전히 낼 수 있다 — 0으로 세면 이어 풀 문제를 두고 패널을 닫는다.
#[tokio::test]
async fn an_open_attempt_is_still_askable() {
    let pool = test_pool().await;
    insert(&pool, "vocab", "문제", "답", "approved").await;
    serve::next(&pool, &[], 100).await.unwrap();

    assert_eq!(serve::availability(&pool, &[]).await.unwrap().askable, 1);
}

/// 신고된 문제는 출제 대상이 아니다 — `next`와 같은 규칙(설계 0044 비즈니스 규칙 5).
#[tokio::test]
async fn retired_items_are_not_askable() {
    let pool = test_pool().await;
    let id = insert(&pool, "vocab", "신고됨", "답", "approved").await;

    serve::report(&pool, id, 100).await.unwrap();

    assert_eq!(serve::availability(&pool, &[]).await.unwrap().askable, 0);
}

async fn insert(
    pool: &sqlx::SqlitePool,
    kind: &str,
    question: &str,
    answer: &str,
    status: &str,
) -> i64 {
    sqlx::query(
        "INSERT INTO quiz_items (kind, question, answer, status, created_at) \
         VALUES (?, ?, ?, ?, 1)",
    )
    .bind(kind)
    .bind(question)
    .bind(answer)
    .bind(status)
    .execute(pool)
    .await
    .unwrap();
    let (id,): (i64,) = sqlx::query_as("SELECT id FROM quiz_items WHERE question = ?")
        .bind(question)
        .fetch_one(pool)
        .await
        .unwrap();
    id
}
