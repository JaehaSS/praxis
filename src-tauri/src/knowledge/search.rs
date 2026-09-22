//! 2단 하이브리드 검색 — FTS 프리필터 → 임베딩 rerank (설계 0020 DR-4).
//!
//! 전수 스캔을 쓰지 않는 이유는 규모다. 실측 vault만 3.9만 청크(임베딩 59MB)이고
//! 세 소스면 9.9만(152MB)이다. `@`멘션은 타이핑마다 검색을 부르므로 매번 그만큼을
//! 디코딩할 수 없다. 후보를 300으로 묶으면 읽는 양이 450KB로 **고정**되어
//! 지연이 코퍼스 크기와 무관해진다.

use std::collections::{HashMap, HashSet};

use sqlx::{Row, SqlitePool};

/// FTS가 넘기는 후보 상한. 늘리면 재현율이 오르고 지연이 나빠진다 —
/// 골든 쿼리 recall로만 조정한다.
///
/// **`ORDER BY bm25`와 짝이다.** 정렬 없이 자르면 SQLite가 rowid 순(= 색인된 순서)으로
/// 300개를 주므로, 코퍼스의 98%를 차지하는 문서군이 후보를 통째로 잠식한다.
/// 실측에서 이것 하나로 recall이 0.14까지 떨어졌다.
pub const CANDIDATE_LIMIT: i64 = 300;

/// RRF 상수. `memory::retrieve_hybrid`(K=60)와 같은 값을 쓴다 — 두 검색이 다르게
/// 동작하면 "왜 여기선 이게 위인가"를 추적하기 어려워진다.
const RRF_K: f32 = 60.0;

/// 1-hop 이웃 보너스. 링크로 이어진 노트는 같은 주제일 확률이 높다.
const NEIGHBOR_BONUS: f32 = 0.3 / RRF_K;

/// trigram 토크나이저의 최소 매칭 길이. 이보다 짧으면 FTS가 아무것도 못 찾으므로
/// 제목 매칭으로 폴백한다 — `@`멘션은 1~2글자부터 검색을 부른다.
const MIN_TRIGRAM: usize = 3;

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchHit {
    pub chunk_id: i64,
    pub node_id: i64,
    pub source: String,
    /// 소스 안에서의 문서 식별자(vault 상대 경로 등). 제목이 겹치는 노트를 구분하고
    /// 골든 세트 판정에 쓴다.
    pub external_id: String,
    pub title: String,
    pub heading: Option<String>,
    pub url: Option<String>,
    pub snippet: String,
    pub score: f32,
}

#[derive(Debug, Default)]
pub struct SearchStats {
    pub fts_candidates: usize,
    /// 실제로 디스크에서 읽어 디코딩한 임베딩 수. 코퍼스 크기와 무관해야 한다.
    pub embeddings_loaded: usize,
    /// 임베딩이 비어 있던 후보 수. 높으면 색인 파이프라인이 고장 난 것이다.
    pub missing_embeddings: usize,
}

pub async fn search(pool: &SqlitePool, query: &str, limit: i64) -> anyhow::Result<Vec<SearchHit>> {
    Ok(search_with_stats(pool, query, limit).await?.0)
}

pub async fn search_visible(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
    wiki_spaces: &[String],
) -> anyhow::Result<Vec<SearchHit>> {
    Ok(search_with_scope(pool, query, limit, wiki_spaces).await?.0)
}

pub async fn search_with_stats(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
) -> anyhow::Result<(Vec<SearchHit>, SearchStats)> {
    search_with_scope(pool, query, limit, &[]).await
}

async fn search_with_scope(
    pool: &SqlitePool,
    query: &str,
    limit: i64,
    wiki_spaces: &[String],
) -> anyhow::Result<(Vec<SearchHit>, SearchStats)> {
    let _admission = crate::knowledge::vault::shared_admission(pool).await?;
    let mut stats = SearchStats::default();
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok((Vec::new(), stats));
    }

    let candidates = collect_candidates(pool, trimmed, wiki_spaces).await?;
    stats.fts_candidates = candidates.len();
    if candidates.is_empty() {
        return Ok((Vec::new(), stats));
    }

    // ① 어휘 순위 (FTS가 준 순서)
    let mut score: HashMap<i64, f32> = HashMap::new();
    for (rank, c) in candidates.iter().enumerate() {
        *score.entry(c.chunk_id).or_insert(0.0) += 1.0 / (RRF_K + rank as f32 + 1.0);
    }

    // ② 시맨틱 순위 — **후보의 임베딩만** 읽는다. 여기가 전수 스캔과 갈리는 지점이다.
    if let Ok(query_vec) = crate::embed::embed_query(trimmed) {
        let mut semantic: Vec<(i64, f32)> = Vec::new();
        for c in &candidates {
            match &c.embedding {
                Some(blob) => {
                    stats.embeddings_loaded += 1;
                    let v = crate::memory::decode_f32(blob);
                    semantic.push((c.chunk_id, crate::memory::cosine(&query_vec, &v)));
                }
                // 임베딩이 없는 청크를 여기서 버리면 결과에서 조용히 사라진다.
                // FTS 순위는 이미 ①에서 받았으므로 그대로 남는다.
                None => stats.missing_embeddings += 1,
            }
        }
        semantic.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        for (rank, (chunk_id, _)) in semantic.iter().enumerate() {
            *score.entry(*chunk_id).or_insert(0.0) += 1.0 / (RRF_K + rank as f32 + 1.0);
        }
    }

    // ③ 그래프 이웃 보너스
    let top_nodes: HashSet<i64> = candidates.iter().take(20).map(|c| c.node_id).collect();
    let neighbors = neighbor_nodes(pool, &top_nodes).await?;
    for c in &candidates {
        if neighbors.contains(&c.node_id) && !top_nodes.contains(&c.node_id) {
            *score.entry(c.chunk_id).or_insert(0.0) += NEIGHBOR_BONUS;
        }
    }

    let mut ranked: Vec<(f32, &Candidate)> = candidates
        .iter()
        .map(|c| (score.get(&c.chunk_id).copied().unwrap_or(0.0), c))
        .collect();
    // 동점일 때 chunk_id로 갈라 순서를 고정한다 — 재현성은 계약 항목이다.
    ranked.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.chunk_id.cmp(&b.1.chunk_id))
    });

    let hits = ranked
        .into_iter()
        .take(limit.max(0) as usize)
        .map(|(score, c)| SearchHit {
            chunk_id: c.chunk_id,
            node_id: c.node_id,
            source: c.source.clone(),
            external_id: c.external_id.clone(),
            title: c.title.clone(),
            heading: c.heading.clone(),
            url: c.url.clone(),
            snippet: snippet(&c.content),
            score,
        })
        .collect();
    Ok((hits, stats))
}

struct Candidate {
    chunk_id: i64,
    node_id: i64,
    source: String,
    external_id: String,
    title: String,
    heading: Option<String>,
    url: Option<String>,
    content: String,
    embedding: Option<Vec<u8>>,
}

async fn collect_candidates(
    pool: &SqlitePool,
    query: &str,
    wiki_spaces: &[String],
) -> anyhow::Result<Vec<Candidate>> {
    // 질의 길이가 아니라 **쓸 수 있는 토큰이 남았는지**로 갈린다.
    // "AI 팀"처럼 전부 3자 미만이면 길이는 충분해도 trigram이 아무것도 못 만든다.
    let matchable = fts_query(query);
    let rows = if !matchable.is_empty() {
        let sql = format!(
            "SELECT c.id, c.node_id, c.heading, c.content, c.embedding, \
                    n.source, n.external_id, n.title, n.url \
             FROM knowledge_fts f \
             JOIN knowledge_chunks c ON c.id = f.rowid \
             JOIN knowledge_nodes n ON n.id = c.node_id \
             WHERE knowledge_fts MATCH ? {} \
             ORDER BY bm25(knowledge_fts) LIMIT ?",
            visibility_clause(wiki_spaces)
        );
        bind_visibility(sqlx::query(&sql).bind(&matchable), wiki_spaces)
            .bind(CANDIDATE_LIMIT)
            .fetch_all(pool)
            .await?
    } else {
        // 짧은 질의는 trigram이 못 잡는다. 노드 제목으로 좁힌다 —
        // 노드는 청크보다 한 자릿수 적어 LIKE 스캔이 감당된다.
        let sql = format!(
            "SELECT c.id, c.node_id, c.heading, c.content, c.embedding, \
                    n.source, n.external_id, n.title, n.url \
             FROM knowledge_nodes n \
             JOIN knowledge_chunks c ON c.node_id = n.id \
             WHERE n.title LIKE ? {} AND c.ord = 0 LIMIT ?",
            visibility_clause(wiki_spaces)
        );
        bind_visibility(sqlx::query(&sql).bind(format!("%{query}%")), wiki_spaces)
            .bind(CANDIDATE_LIMIT)
            .fetch_all(pool)
            .await?
    };

    let mut candidates = Vec::new();
    for row in &rows {
        candidates.push(Candidate {
            chunk_id: row.try_get("id")?,
            node_id: row.try_get("node_id")?,
            source: row.try_get("source")?,
            external_id: row.try_get("external_id")?,
            title: row.try_get("title")?,
            heading: row.try_get("heading")?,
            url: row.try_get("url")?,
            content: row.try_get("content")?,
            embedding: row.try_get("embedding")?,
        });
    }
    // vault가 가져간 legacy 노드는 여기서 걸러낸다. 후보마다 따로 묻지 않는 이유는 풀이다 —
    // 300건이면 900번 가까운 커넥션 대여가 되고, `@`멘션은 타이핑마다 이 경로를 부른다.
    let node_ids: Vec<i64> = candidates.iter().map(|c| c.node_id).collect();
    let owned = crate::knowledge::vault::ownership::owned_legacy_nodes(pool, &node_ids).await?;
    candidates.retain(|candidate| !owned.contains(&candidate.node_id));
    Ok(candidates)
}

fn visibility_clause(wiki_spaces: &[String]) -> String {
    if wiki_spaces.is_empty() {
        return "AND n.source <> 'wiki'".into();
    }
    format!(
        "AND (n.source <> 'wiki' OR n.space_id IN ({}))",
        vec!["?"; wiki_spaces.len()].join(",")
    )
}

fn bind_visibility<'q>(
    mut query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    wiki_spaces: &'q [String],
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    for space in wiki_spaces {
        query = query.bind(space);
    }
    query
}

async fn neighbor_nodes(pool: &SqlitePool, nodes: &HashSet<i64>) -> anyhow::Result<HashSet<i64>> {
    if nodes.is_empty() {
        return Ok(HashSet::new());
    }
    let list = nodes
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(",");
    // 노드 id는 DB에서 온 정수라 문자열 조립이 안전하다(사용자 입력 아님).
    let sql = format!(
        "SELECT dst_id AS other FROM knowledge_edges WHERE src_id IN ({list}) \
         UNION SELECT src_id AS other FROM knowledge_edges WHERE dst_id IN ({list})"
    );
    let rows = sqlx::query(&sql).fetch_all(pool).await?;
    Ok(rows
        .iter()
        .filter_map(|r| r.try_get::<i64, _>("other").ok())
        .collect())
}

/// trigram MATCH용 질의.
///
/// 세 가지를 지킨다.
/// ① **따옴표로 감싸지 않는다** — trigram은 이미 3-gram 시퀀스를 phrase로 찾는다.
///    따옴표를 더하면 중첩 phrase가 되어 의미가 없고 문법만 위태로워진다.
/// ② **3자 미만 토큰을 버린다** — trigram이 만들지 못해, AND로 묶이면 그 토큰 하나가
///    전체 결과를 0으로 만든다.
/// ③ **OR로 결합한다** — 여기서 FTS의 역할은 정밀도가 아니라 rerank 후보 수집이다.
///    좁히는 건 임베딩이 하고, 후보에 없는 것은 되살릴 방법이 없다.
///
/// `memory::fts_query`를 재사용하지 않는다 — 저쪽은 영숫자 토큰만 남겨 한국어를 버린다.
/// 반환이 빈 문자열이면 호출자가 제목 매칭으로 폴백한다.
pub fn fts_query(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= MIN_TRIGRAM)
        // 소문자로 낮춰 `AND`·`OR`·`NOT`·`NEAR`가 연산자로 해석되는 것을 막는다
        // (FTS5 연산자는 대문자일 때만 유효하다). trigram은 대소문자를 구분하지 않는다.
        .map(|t| t.to_lowercase())
        .collect::<Vec<_>>()
        .join(" OR ")
}

fn snippet(content: &str) -> String {
    const MAX: usize = 200;
    let flat = content.replace('\n', " ");
    if flat.chars().count() <= MAX {
        return flat;
    }
    flat.chars().take(MAX).collect::<String>() + "…"
}

#[cfg(test)]
mod scope_tests;
