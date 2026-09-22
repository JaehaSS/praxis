//! 노드·청크 upsert와 삭제.
//!
//! 임베딩은 여기서 만들지 않는다. 청크를 `embedding IS NULL`로 저장하고
//! `embed_pending`이 배치로 채운다. 이유는 셋이다 —
//! ① 백필 중단 후 재개할 때 남은 것만 골라내기 쉽다,
//! ② 임베딩 실패(모델 다운로드·네트워크)가 색인 자체를 막지 않는다,
//! ③ 테스트가 모델 로드 없이 돈다.

use sqlx::{Row, SqlitePool};

use super::{chunk, hash};

/// 커넥터가 넘기는 정규화된 문서 1건.
#[derive(Debug, Clone)]
pub struct Document {
    pub source: String,
    pub external_id: String,
    pub kind: String,
    pub title: String,
    pub url: Option<String>,
    pub body: String,
    pub updated_at: i64,
    /// false면 임베딩 대상에서 뺀다. **색인(FTS)은 그대로** 되므로 검색에서 사라지지 않고,
    /// 어휘 검색만 되고 의미 검색이 안 될 뿐이다. 임베딩은 청크당 ~190ms라
    /// 어떤 문서에 그 비용을 쓸지는 사람이 정해야 한다 (설계 0020 DR-11).
    pub embed: bool,
}

impl Document {
    /// 임베딩까지 하는 기본 문서. 테스트·단순 커넥터용.
    pub fn embedded(
        source: impl Into<String>,
        external_id: impl Into<String>,
        title: impl Into<String>,
        body: impl Into<String>,
    ) -> Self {
        Self {
            source: source.into(),
            external_id: external_id.into(),
            kind: "document".into(),
            title: title.into(),
            url: None,
            body: body.into(),
            updated_at: 1,
            embed: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome {
    /// 새로 색인했거나 변경분을 반영했다.
    Indexed,
    /// 내용 해시가 같아 아무것도 하지 않았다.
    Skipped,
}

/// 문서 1건을 색인한다. **문서 하나가 트랜잭션 하나**다 —
/// 중간에 죽어도 반쯤 색인된 문서가 남지 않는다.
pub async fn upsert_document(
    pool: &SqlitePool,
    doc: &Document,
    now: i64,
) -> anyhow::Result<UpsertOutcome> {
    let digest = hash::content_hash(&doc.body);

    let existing: Option<(i64, Option<String>)> = sqlx::query_as(
        "SELECT id, content_hash FROM knowledge_nodes WHERE source = ? AND external_id = ?",
    )
    .bind(&doc.source)
    .bind(&doc.external_id)
    .fetch_optional(pool)
    .await?;

    if let Some((id, Some(prev))) = &existing {
        if prev == &digest {
            // 본문은 유지하되, 설정 변경에 따른 임베딩 대상 여부는 즉시 반영한다.
            let mut tx = pool.begin().await?;
            sqlx::query("UPDATE knowledge_nodes SET synced_at = ?, embed_enabled = ? WHERE id = ?")
                .bind(now)
                .bind(i64::from(doc.embed))
                .bind(id)
                .execute(&mut *tx)
                .await?;
            if !doc.embed {
                sqlx::query("UPDATE knowledge_chunks SET embedding = NULL, embed_model = NULL WHERE node_id = ? AND (embedding IS NOT NULL OR embed_model IS NOT NULL)")
                    .bind(id)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await?;
            return Ok(UpsertOutcome::Skipped);
        }
    }

    let chunks = chunk::split_markdown(&doc.body);
    let mut tx = pool.begin().await?;

    let node_id: i64 = sqlx::query(
        "INSERT INTO knowledge_nodes \
           (source, external_id, kind, title, url, content_hash, updated_at, synced_at, \
            embed_enabled) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(source, external_id) DO UPDATE SET \
           kind = excluded.kind, title = excluded.title, url = excluded.url, \
           content_hash = excluded.content_hash, updated_at = excluded.updated_at, \
           synced_at = excluded.synced_at, embed_enabled = excluded.embed_enabled \
         RETURNING id",
    )
    .bind(&doc.source)
    .bind(&doc.external_id)
    .bind(&doc.kind)
    .bind(&doc.title)
    .bind(&doc.url)
    .bind(&digest)
    .bind(doc.updated_at)
    .bind(now)
    .bind(i64::from(doc.embed))
    .fetch_one(&mut *tx)
    .await?
    .try_get("id")?;

    // 부분 갱신은 `ord` 정합을 깨뜨린다(문단이 하나 줄면 뒤가 전부 밀린다).
    // 통째로 지우고 다시 넣는 편이 단순하고, FTS 트리거도 delete/insert를 정확히 받는다.
    sqlx::query("DELETE FROM knowledge_chunks WHERE node_id = ?")
        .bind(node_id)
        .execute(&mut *tx)
        .await?;

    for c in &chunks {
        sqlx::query(
            "INSERT INTO knowledge_chunks (node_id, ord, doc_title, heading, content) \
             VALUES (?, ?, ?, ?, ?)",
        )
        .bind(node_id)
        .bind(c.ord)
        .bind(&doc.title)
        .bind(&c.heading)
        .bind(&c.content)
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;
    Ok(UpsertOutcome::Indexed)
}

/// 원본에서 사라진 문서를 지운다. 청크·엣지는 CASCADE로 함께 사라지고,
/// FTS는 DELETE 트리거가 정리한다.
pub async fn delete_document(
    pool: &SqlitePool,
    source: &str,
    external_id: &str,
) -> anyhow::Result<()> {
    sqlx::query("DELETE FROM knowledge_nodes WHERE source = ? AND external_id = ?")
        .bind(source)
        .bind(external_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 저장된 `external_id` 중 이번 스캔에 없는 것 = 삭제분.
/// 파일 삭제는 목록의 "부재"로만 드러나므로 명시적 diff가 없으면 영원히 남는다.
pub fn deleted_ids(stored: &[String], present: &[String]) -> Vec<String> {
    let live: std::collections::HashSet<&String> = present.iter().collect();
    stored
        .iter()
        .filter(|id| !live.contains(id))
        .cloned()
        .collect()
}

/// 임베딩이 비어 있는 청크를 배치로 채운다. 반환값은 채운 개수.
///
/// best-effort다 — 모델을 못 불러오면 0을 반환하고 청크는 NULL로 남는다.
/// NULL 청크도 FTS로는 검색되므로 결과에서 사라지지는 않는다.
pub async fn embed_pending(pool: &SqlitePool, limit: i64) -> anyhow::Result<usize> {
    let _admission = crate::knowledge::vault::shared_admission(pool).await?;
    let rows = sqlx::query(
        "SELECT c.id, c.content, c.node_id FROM knowledge_chunks c \
         JOIN knowledge_nodes n ON n.id = c.node_id \
         WHERE c.embedding IS NULL AND n.embed_enabled = 1 ORDER BY c.id LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(0);
    }

    // id와 본문을 **한 번에** 짝지어 뽑는다. 각각 따로 `filter_map`한 뒤 `zip`하면
    // 한쪽에서 행 하나가 빠지는 순간 이후 전부가 한 칸씩 밀려, **다른 문서의 벡터가
    // 이 청크에 저장된다.** 검색이 조용히 틀린 결과를 내고 원인을 추적하기 어렵다.
    let mut triples = Vec::new();
    for row in &rows {
        let id: i64 = row.try_get("id")?;
        let content: String = row.try_get("content")?;
        let node_id: i64 = row.try_get("node_id")?;
        triples.push((id, content, node_id));
    }
    // 소유 판정은 묶음으로 한 번에. 낱개로 물으면 배치(limit=256)마다 700번 넘게 풀에서
    // 커넥션을 빌렸다 놓아, 백그라운드 임베딩이 전면 조회의 대기 시간을 밀어 올린다.
    let node_ids: Vec<i64> = triples.iter().map(|(_, _, node_id)| *node_id).collect();
    let owned = crate::knowledge::vault::ownership::owned_legacy_nodes(pool, &node_ids).await?;
    let pairs: Vec<(i64, String)> = triples
        .into_iter()
        .filter(|(_, _, node_id)| !owned.contains(node_id))
        .map(|(id, content, _)| (id, content))
        .collect();
    if pairs.is_empty() {
        return Ok(0);
    }
    let texts: Vec<String> = pairs.iter().map(|(_, t)| t.clone()).collect();

    let vectors = match crate::embed::embed_passages(&texts) {
        Ok(v) => v,
        Err(_) => return Ok(0), // 모델 부재·네트워크 실패 — 다음 동기화에서 재시도
    };
    // 모델이 입력과 다른 개수를 돌려주면 짝이 어긋난다. `zip`은 짧은 쪽에 맞춰 조용히
    // 잘라내므로 여기서 막지 않으면 증상이 "일부 청크만 임베딩됨"으로 위장된다.
    if vectors.len() != texts.len() {
        anyhow::bail!(
            "임베딩 {}건을 요청했는데 {}건이 왔다",
            texts.len(),
            vectors.len()
        );
    }

    let mut tx = pool.begin().await?;
    for ((id, _), vector) in pairs.iter().zip(vectors.iter()) {
        sqlx::query("UPDATE knowledge_chunks SET embedding = ?, embed_model = ? WHERE id = ?")
            .bind(crate::memory::encode_f32(vector))
            .bind(crate::embed::KNOWLEDGE_MODEL)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(pairs.len())
}
