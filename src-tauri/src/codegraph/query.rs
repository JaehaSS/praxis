//! `impact_of` — "이 심볼을 고치면 무엇이 깨지나" (계획 0037 Task 6).
//!
//! 참조 엣지를 **거꾸로** 타고 오른다. 엣지는 참조하는 쪽 → 참조되는 쪽이므로,
//! 영향 범위는 `dst_id = 대상`인 엣지의 `src_id`들이고 그것을 N홉 반복한다.

use serde::Serialize;
use sqlx::SqlitePool;

/// 깊이 상한. 4홉이면 이 저장소 규모에서 사실상 전체가 나와 답이 되지 못한다.
pub const MAX_DEPTH: u32 = 3;
/// 결과 상한. 넘으면 `truncated`로 알린다 — 조용히 자르면 "이게 전부"로 읽힌다.
pub const MAX_RESULTS: usize = 500;

/// 재귀 CTE.
///
/// `UNION`(ALL 아님)이 **종료**를 보장한다 — 순환 참조가 흔해서 중복을 접지 않으면
/// A→B→A에서 끝나지 않는다.
///
/// 다만 `UNION`이 접는 것은 `(id, depth)` **튜플**이라, 순환에서는 같은 노드가 다른 깊이로
/// 여러 번 살아남는다(A→B→A는 B를 1홉과 3홉으로 두 번 낸다). 영향 범위는 집합이어야
/// 하므로 `GROUP BY`로 노드당 한 행만 남기고 `MIN(depth)`, 즉 **가장 가까운 거리**를 준다.
///
/// 대상 자신(`up.id = ?1`)도 뺀다. 순환을 한 바퀴 돌면 자기 자신이 "영향받는 심볼"로
/// 돌아오는데, 그것은 답이 아니라 순환이 있다는 사실일 뿐이다.
const SQL: &str = r#"
WITH RECURSIVE up(id, depth) AS (
  SELECT ?1, 0
  UNION
  SELECT e.src_id, up.depth + 1
    FROM code_edges e JOIN up ON e.dst_id = up.id
   WHERE up.depth < ?2 AND e.rel = 'references'
)
SELECT n.id, n.name, n.container, f.rel_path, n.sel_line, MIN(up.depth) AS depth
  FROM up JOIN code_nodes n ON n.id = up.id
          JOIN code_files f ON f.id = n.file_id
 WHERE up.depth > 0 AND up.id <> ?1
 GROUP BY n.id
 ORDER BY depth, f.rel_path
 LIMIT ?3
"#;

/// 영향 범위에 들어온 심볼 하나.
#[derive(Debug, Clone, Serialize, sqlx::FromRow, PartialEq, Eq)]
pub struct Impacted {
    pub id: i64,
    pub name: String,
    pub container: Option<String>,
    pub rel_path: String,
    /// LSP 원본 0-based. UI 경계에서만 +1 한다.
    pub sel_line: i64,
    /// 대상에서 몇 홉 떨어져 있는가. 1이 직접 참조다.
    pub depth: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Impact {
    pub items: Vec<Impacted>,
    /// 상한에 걸려 잘렸는가. 참이면 이 목록은 영향 범위의 **일부**다.
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GenerationImpacted {
    pub id: i64,
    pub name: String,
    pub container: Option<String>,
    pub rel_path: String,
    pub line: i64,
    pub character: i64,
    pub depth: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GenerationImpact {
    pub run_id: i64,
    pub indexed_at: i64,
    pub freshness: String,
    pub items: Vec<GenerationImpacted>,
    pub truncated: bool,
    /// `Some`이면 대상 파일의 엣지를 만들지 않았다 — 빈 `items`는 "영향 없음"이 아니라
    /// "참조를 알 수 없음"이다(설계 0065 DR-6, ADR 0099).
    pub edges_unavailable: Option<String>,
}

const GENERATION_SQL: &str = r#"
WITH RECURSIVE up(id, depth) AS (
  SELECT ?1, 0
  UNION
  SELECT e.src_id, up.depth + 1
    FROM code_graph_edges e JOIN up ON e.dst_id = up.id
   WHERE e.run_id = ?2 AND up.depth < ?3 AND e.rel = 'references'
)
SELECT n.id, n.name, n.container, f.rel_path,
       n.sel_start_line AS line, n.sel_start_char AS character, MIN(up.depth) AS depth
  FROM up JOIN code_graph_nodes n ON n.id = up.id AND n.run_id = ?2
          JOIN code_graph_files f ON f.id = n.file_id
 WHERE up.depth > 0 AND up.id <> ?1
 GROUP BY n.id
 ORDER BY depth, f.rel_path, n.sel_start_line
 LIMIT ?4
"#;

pub async fn impact_at(
    pool: &SqlitePool,
    worktree: &str,
    rel_path: &str,
    line: u32,
    character: u32,
    depth: u32,
    freshness: &str,
) -> anyhow::Result<Option<GenerationImpact>> {
    let active: Option<(i64, i64)> = sqlx::query_as(
        "SELECT a.run_id, r.finished_at FROM code_graph_active a \
         JOIN code_graph_runs r ON r.id=a.run_id WHERE a.worktree=?",
    )
    .bind(worktree)
    .fetch_optional(pool)
    .await?;
    let Some((run_id, indexed_at)) = active else {
        return Ok(None);
    };
    let Some(node) = super::snapshot::find_node_at(pool, run_id, rel_path, line, character).await?
    else {
        return Ok(None);
    };
    let edges_unavailable: Option<String> = sqlx::query_scalar(
        "SELECT edge_state FROM code_graph_files WHERE run_id=? AND rel_path=?",
    )
    .bind(run_id)
    .bind(rel_path)
    .fetch_optional(pool)
    .await?
    .flatten();
    let depth = depth.clamp(1, MAX_DEPTH);
    let mut items = sqlx::query_as::<_, GenerationImpacted>(GENERATION_SQL)
        .bind(node.id)
        .bind(run_id)
        .bind(depth)
        .bind(MAX_RESULTS as i64 + 1)
        .fetch_all(pool)
        .await?;
    let truncated = items.len() > MAX_RESULTS;
    items.truncate(MAX_RESULTS);
    Ok(Some(GenerationImpact {
        run_id,
        indexed_at,
        freshness: freshness.to_string(),
        items,
        truncated,
        edges_unavailable,
    }))
}

/// 이 노드를 참조하는 심볼들을 N홉까지 거슬러 찾는다.
///
/// `depth`는 1..=[`MAX_DEPTH`]로 clamp한다. 0을 받아도 1로 올린다 — 0홉은 대상 자신뿐이라
/// 질문이 성립하지 않는다.
pub async fn impact_of(pool: &SqlitePool, node_id: i64, depth: u32) -> anyhow::Result<Impact> {
    let depth = depth.clamp(1, MAX_DEPTH);
    // 상한을 넘겼는지 알려면 상한보다 하나 더 받아 봐야 한다.
    let mut items = sqlx::query_as::<_, Impacted>(SQL)
        .bind(node_id)
        .bind(depth)
        .bind(MAX_RESULTS as i64 + 1)
        .fetch_all(pool)
        .await?;
    let truncated = items.len() > MAX_RESULTS;
    items.truncate(MAX_RESULTS);
    Ok(Impact { items, truncated })
}

/// 이름으로 노드를 찾는다. 같은 이름이 여러 파일에 있는 것이 정상이므로 **목록**을 준다 —
/// 하나를 임의로 고르면 엉뚱한 심볼의 영향 범위를 답하게 된다.
pub async fn find_nodes_by_name(
    pool: &SqlitePool,
    worktree: &str,
    name: &str,
) -> anyhow::Result<Vec<Impacted>> {
    let rows = sqlx::query_as::<_, Impacted>(
        "SELECT n.id, n.name, n.container, f.rel_path, n.sel_line, 0 AS depth \
         FROM code_nodes n JOIN code_files f ON f.id = n.file_id \
         WHERE f.worktree = ? AND n.name = ? \
         ORDER BY f.rel_path, n.sel_line",
    )
    .bind(worktree)
    .bind(name)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}
