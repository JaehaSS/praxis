//! 한 코드 그래프 실행에 귀속되는 파일·심볼·엣지 저장소.

use sqlx::SqlitePool;

use crate::lspclient::protocol::RawSymbol;

#[derive(Debug, Clone, sqlx::FromRow, PartialEq, Eq)]
pub struct SnapshotNode {
    pub id: i64,
    pub name: String,
    pub sel_line: i64,
    pub sel_char: i64,
}

/// `lang`은 LSP `languageId`다 — `"python"`·`"typescriptreact"`. **서버 키가 아니다**(설계 0065 DR-4).
///
/// `skip_reason`(심볼조차 못 만들었다)과 `edge_state`(심볼은 있으나 엣지가 없다)는 판단하지
/// 않고 그대로 싣는다 — 사유를 아는 것은 build 계층이다(DR-3b).
pub async fn insert_file(
    pool: &SqlitePool,
    run_id: i64,
    rel_path: &str,
    content_hash: &str,
    lang: &str,
    skip_reason: Option<&str>,
    edge_state: Option<&str>,
) -> anyhow::Result<i64> {
    let result = sqlx::query(
        "INSERT INTO code_graph_files (run_id, rel_path, content_hash, lang, skip_reason, edge_state) \
         VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(run_id)
    .bind(rel_path)
    .bind(content_hash)
    .bind(lang)
    .bind(skip_reason)
    .bind(edge_state)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn insert_node(
    pool: &SqlitePool,
    run_id: i64,
    file_id: i64,
    symbol: &RawSymbol,
) -> anyhow::Result<i64> {
    let result = sqlx::query(
        "INSERT INTO code_graph_nodes \
         (run_id, file_id, name, kind, container, sel_start_line, sel_start_char, \
          sel_end_line, sel_end_char, body_start_line, body_start_char, body_end_line, \
          body_end_char) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(run_id)
    .bind(file_id)
    .bind(&symbol.name)
    .bind(symbol.kind as i64)
    .bind(&symbol.container)
    .bind(symbol.sel_line as i64)
    .bind(symbol.sel_char as i64)
    .bind(symbol.sel_end_line as i64)
    .bind(symbol.sel_end_char as i64)
    .bind(symbol.body_start_line as i64)
    .bind(symbol.body_start_char as i64)
    .bind(symbol.body_end_line as i64)
    .bind(symbol.body_end_char as i64)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn add_edge(
    pool: &SqlitePool,
    run_id: i64,
    src_id: i64,
    dst_id: i64,
) -> anyhow::Result<bool> {
    if src_id == dst_id {
        return Ok(false);
    }
    let result = sqlx::query(
        "INSERT OR IGNORE INTO code_graph_edges (run_id, src_id, dst_id, rel) \
         VALUES (?, ?, ?, 'references')",
    )
    .bind(run_id)
    .bind(src_id)
    .bind(dst_id)
    .execute(pool)
    .await?;
    Ok(result.rows_affected() > 0)
}

const FIND_NODE_AT: &str = r#"
SELECT n.id, n.name, n.sel_start_line AS sel_line, n.sel_start_char AS sel_char
  FROM code_graph_nodes n JOIN code_graph_files f ON f.id = n.file_id
 WHERE n.run_id = ?1 AND f.rel_path = ?2 AND (
   ((n.sel_start_line < ?3 OR (n.sel_start_line = ?3 AND n.sel_start_char <= ?4))
    AND (n.sel_end_line > ?3 OR (n.sel_end_line = ?3 AND n.sel_end_char > ?4)))
   OR
   ((n.body_start_line < ?3 OR (n.body_start_line = ?3 AND n.body_start_char <= ?4))
    AND (n.body_end_line > ?3 OR (n.body_end_line = ?3 AND n.body_end_char > ?4)))
 )
 ORDER BY
   CASE WHEN
     (n.sel_start_line < ?3 OR (n.sel_start_line = ?3 AND n.sel_start_char <= ?4))
     AND (n.sel_end_line > ?3 OR (n.sel_end_line = ?3 AND n.sel_end_char > ?4))
   THEN 0 ELSE 1 END,
   (n.body_end_line - n.body_start_line) ASC,
   n.body_start_line DESC,
   n.body_end_line ASC,
   n.body_start_char DESC,
   n.body_end_char ASC
 LIMIT 1
"#;

pub async fn find_node_at(
    pool: &SqlitePool,
    run_id: i64,
    rel_path: &str,
    line: u32,
    character: u32,
) -> anyhow::Result<Option<SnapshotNode>> {
    let node = sqlx::query_as::<_, SnapshotNode>(FIND_NODE_AT)
        .bind(run_id)
        .bind(rel_path)
        .bind(line as i64)
        .bind(character as i64)
        .fetch_optional(pool)
        .await?;
    Ok(node)
}

pub async fn nodes_for_file(
    pool: &SqlitePool,
    run_id: i64,
    rel_path: &str,
) -> anyhow::Result<Vec<SnapshotNode>> {
    let nodes = sqlx::query_as::<_, SnapshotNode>(
        "SELECT n.id, n.name, n.sel_start_line AS sel_line, n.sel_start_char AS sel_char \
         FROM code_graph_nodes n JOIN code_graph_files f ON f.id=n.file_id \
         WHERE n.run_id=? AND f.rel_path=? ORDER BY n.id",
    )
    .bind(run_id)
    .bind(rel_path)
    .fetch_all(pool)
    .await?;
    Ok(nodes)
}

#[cfg(test)]
pub async fn insert_test_node(
    pool: &SqlitePool,
    run_id: i64,
    file_id: i64,
    name: &str,
) -> anyhow::Result<i64> {
    let symbol = RawSymbol {
        name: name.to_string(),
        kind: 12,
        container: None,
        sel_line: 0,
        sel_char: 3,
        sel_end_line: 0,
        sel_end_char: 9,
        body_start_line: 0,
        body_start_char: 0,
        body_end_line: 2,
        body_end_char: 1,
        end_line: 2,
    };
    insert_node(pool, run_id, file_id, &symbol).await
}
