//! 2단 검색 검증.

use super::test_pool;
use crate::knowledge::graph::{embed_pending, upsert_document, Document};
use crate::knowledge::search::{search, search_with_stats, CANDIDATE_LIMIT};

fn doc(external_id: &str, body: &str) -> Document {
    Document::embedded("obsidian", external_id, external_id.trim_end_matches(".md"), body)
}

#[tokio::test]
async fn finds_korean_substring_across_word_boundaries() {
    // 기본 토크나이저면 "지식그래프를"이 "지식그래프"에 매칭되지 않는다 (DR-3).
    let pool = test_pool().await;
    upsert_document(&pool, &doc("k.md", "지식그래프를 설계한다"), 1)
        .await
        .unwrap();
    let hits = search(&pool, "지식그래프", 10).await.unwrap();
    assert_eq!(hits.len(), 1, "한국어 substring이 안 잡힌다");
    assert_eq!(hits[0].title, "k");
}

#[tokio::test]
async fn loads_only_candidate_embeddings_not_the_whole_corpus() {
    // 전수 스캔이면 코퍼스에 비례해 느려진다. 읽는 임베딩 수가 후보 상한에
    // 묶이는지가 이 설계의 지연 목표를 지탱한다 (DR-4).
    let pool = test_pool().await;
    for i in 0..400 {
        upsert_document(&pool, &doc(&format!("n{i}.md"), "공통어휘 본문입니다"), 1)
            .await
            .unwrap();
    }
    let (_, stats) = search_with_stats(&pool, "공통어휘", 10).await.unwrap();
    assert!(
        stats.fts_candidates as i64 <= CANDIDATE_LIMIT,
        "후보가 상한을 넘었다: {}",
        stats.fts_candidates
    );
    assert!(
        stats.embeddings_loaded as i64 <= CANDIDATE_LIMIT,
        "임베딩을 {}개 읽었다 — 전수 스캔이다",
        stats.embeddings_loaded
    );
}

#[tokio::test]
async fn ranking_is_stable_across_repeated_queries() {
    // 재현성은 계약 항목이다. 순위에 시간·랜덤 요소가 섞이면 여기서 깨진다.
    let pool = test_pool().await;
    for i in 0..30 {
        upsert_document(&pool, &doc(&format!("n{i}.md"), "설계 문서 본문 반복"), 1)
            .await
            .unwrap();
    }
    let ids = |v: &[crate::knowledge::search::SearchHit]| {
        v.iter().map(|h| h.chunk_id).collect::<Vec<_>>()
    };
    let a = search(&pool, "설계 문서", 10).await.unwrap();
    let b = search(&pool, "설계 문서", 10).await.unwrap();
    assert_eq!(ids(&a), ids(&b));
}

#[tokio::test]
async fn chunks_without_embeddings_still_appear() {
    // 임베딩은 best-effort다. rerank에서 버리면 결과에서 조용히 사라진다.
    let pool = test_pool().await;
    upsert_document(&pool, &doc("n.md", "임베딩 없는 고유단어 크세논"), 1)
        .await
        .unwrap();
    // embed_pending을 부르지 않았으므로 embedding은 NULL이다.
    let (hits, stats) = search_with_stats(&pool, "크세논", 10).await.unwrap();
    assert_eq!(hits.len(), 1, "임베딩 없는 청크가 사라졌다");
    assert_eq!(stats.missing_embeddings, 1);
}

#[tokio::test]
async fn short_query_falls_back_to_title_match() {
    // trigram은 3자 미만을 못 잡는다. `@`멘션은 1~2글자부터 검색을 부른다.
    let pool = test_pool().await;
    upsert_document(&pool, &doc("설계노트.md", "본문"), 1)
        .await
        .unwrap();
    let hits = search(&pool, "설계", 10).await.unwrap();
    assert_eq!(hits.len(), 1, "짧은 질의 폴백이 동작하지 않는다");
}

#[tokio::test]
async fn special_characters_do_not_break_the_query() {
    // 사용자가 `@검색: "무엇"?` 같은 걸 친다. FTS5 문법 에러로 터지면 안 된다.
    let pool = test_pool().await;
    upsert_document(&pool, &doc("n.md", "정상 본문 데이터"), 1)
        .await
        .unwrap();
    for q in ["\"따옴표\"", "AND OR NOT", "*별표*", "정상: 본문?"] {
        assert!(search(&pool, q, 10).await.is_ok(), "질의 `{q}`에서 터졌다");
    }
}

#[tokio::test]
async fn empty_query_returns_nothing_without_touching_the_index() {
    let pool = test_pool().await;
    upsert_document(&pool, &doc("n.md", "본문"), 1).await.unwrap();
    let (hits, stats) = search_with_stats(&pool, "   ", 10).await.unwrap();
    assert!(hits.is_empty());
    assert_eq!(stats.fts_candidates, 0);
}

#[tokio::test]
async fn embedding_improves_semantic_ranking() {
    // 어휘가 겹치지 않아도 의미가 가까우면 위로 와야 한다.
    // 이것이 FTS 단독 대비 임베딩을 붙이는 유일한 이유다.
    let pool = test_pool().await;
    // 두 문서가 "설계한다"를 공유하므로 FTS만으로는 우열이 없다. 순위를 가르는 건 임베딩뿐이다.
    upsert_document(
        &pool,
        &doc("가까움.md", "로컬 지식 그래프와 검색 파이프라인을 설계한다"),
        1,
    )
    .await
    .unwrap();
    upsert_document(
        &pool,
        &doc("멂.md", "오늘 점심 메뉴로 김치찌개를 설계한다"),
        1,
    )
    .await
    .unwrap();
    embed_pending(&pool, 100).await.unwrap();

    let hits = search(&pool, "지식 그래프 검색을 설계한다", 10).await.unwrap();
    assert_eq!(hits.len(), 2);
    // 임베딩이 붙었으면 의미가 가까운 쪽이 앞이다.
    assert_eq!(hits[0].title, "가까움", "시맨틱 순위가 반영되지 않았다");
}

#[tokio::test]
async fn embed_excluded_documents_are_still_searchable() {
    // 이 기능의 요점이다. 임베딩을 건너뛰는 것은 **비용을 아끼는 것이지 버리는 것이 아니다** —
    // 색인(FTS)은 그대로라 어휘 검색으로 계속 찾힌다. 의미 검색에서만 빠진다.
    // 실측 vault의 Claude Code 세션 로그가 정확히 이 경우다(청크의 98%, 임베딩하면 6시간).
    let pool = test_pool().await;
    let mut skipped = doc("세션로그.md", "그때 그 작업의 고유단어 크세논");
    skipped.embed = false;
    upsert_document(&pool, &skipped, 1).await.unwrap();
    upsert_document(&pool, &doc("수기노트.md", "사람이 쓴 크세논 노트"), 1)
        .await
        .unwrap();

    let filled = embed_pending(&pool, 100).await.unwrap();
    assert_eq!(filled, 1, "임베딩 제외 문서까지 임베딩했다");

    let (with_vec,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM knowledge_chunks c JOIN knowledge_nodes n ON n.id = c.node_id \
         WHERE n.external_id = '세션로그.md' AND c.embedding IS NOT NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(with_vec, 0);

    // 그래도 검색에는 나온다.
    let hits = search(&pool, "크세논", 10).await.unwrap();
    assert_eq!(hits.len(), 2, "임베딩 제외 문서가 검색에서 사라졌다");
    assert!(hits.iter().any(|h| h.external_id == "세션로그.md"));
}

#[tokio::test]
async fn embed_policy_change_takes_effect_on_next_sync() {
    // 정책을 바꾸면 다음 동기화부터 반영돼야 한다. 노드에 눌러 담는 값이라
    // 갱신 경로(ON CONFLICT)가 빠지면 영원히 옛 정책으로 남는다.
    let pool = test_pool().await;
    let mut d = doc("n.md", "본문 하나");
    d.embed = false;
    upsert_document(&pool, &d, 1).await.unwrap();
    assert_eq!(embed_pending(&pool, 10).await.unwrap(), 0);

    d.embed = true;
    d.body = "본문 둘".into(); // 해시가 달라져야 재색인된다
    upsert_document(&pool, &d, 2).await.unwrap();
    assert_eq!(embed_pending(&pool, 10).await.unwrap(), 1);
}
