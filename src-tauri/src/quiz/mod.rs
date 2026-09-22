//! 대기 퀴즈 — 에이전트 응답을 기다리는 동안 출제한다.
//!
//! 설계 정본: `docs/designs/0044.2026-08-17-wait-quiz-design.md`
//!
//! **경계**: 도메인 문제를 만들려고 `knowledge`를 참조한다. 그래서 Runner는 이 모듈을
//! 참조해선 안 된다 — 참조하면 지식 그래프가 Runner로 간접 노출된다(설계 0020 DR-6).
//! `knowledge::tests::isolation::runner_does_not_reference_quiz_module`이 그것을 강제한다.

pub mod gate;
pub mod generate;
pub mod inbox;
pub mod schema;
pub mod serve;

#[cfg(test)]
mod tests;

use sqlx::SqlitePool;

/// 퀴즈 스키마를 만든다. 앱 기동마다 호출되므로 멱등해야 한다.
///
/// **`knowledge::migrate` 다음에 부를 것** — `quiz_items.chunk_id`가 `knowledge_chunks`를
/// 참조하므로 그 테이블이 없으면 CREATE부터 실패한다.
pub async fn migrate(pool: &SqlitePool) -> anyhow::Result<()> {
    // 다중문 — 수동 분할은 문자열 안의 `;`에서 깨진다(`knowledge/mod.rs:35`와 같은 이유).
    sqlx::raw_sql(schema::MIGRATION).execute(pool).await?;
    Ok(())
}

/// 도메인 문제의 재료가 될 청크를 고른다.
///
/// **Obsidian만 본다** — Gmail·Notion은 MVP 범위 밖이다(설계 0044 DR-6). 개인 메일이
/// 퀴즈 문제로 튀어나오는 사고를 소스 단계에서 막는다.
///
/// 무작위로 뽑는 이유는 같은 문서만 반복 출제되는 것을 막기 위함이다. 중복 문제는
/// `inbox`가 임베딩으로 한 번 더 거른다.
pub async fn pick_source_chunks(
    pool: &SqlitePool,
    limit: i64,
) -> anyhow::Result<Vec<generate::SourceChunk>> {
    let rows: Vec<(i64, Option<String>, Option<String>, String)> = sqlx::query_as(
        "SELECT c.id, c.doc_title, c.heading, c.content \
         FROM knowledge_chunks c \
         JOIN knowledge_nodes n ON n.id = c.node_id \
         WHERE n.source = 'obsidian' AND TRIM(c.content) <> '' \
         ORDER BY RANDOM() LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, doc_title, heading, content)| generate::SourceChunk {
            id,
            doc_title,
            heading,
            content,
        })
        .collect())
}
