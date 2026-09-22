use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sqlx::{Sqlite, SqlitePool, Transaction};

use super::{manifest::SourceManifest, status};

pub const MAX_NODES: usize = 200;
pub const MAX_EDGES: usize = 400;

/// Command-boundary entry point. The DB key is captured before the root is
/// canonicalized because it is the exact key stored in `code_graph_active`.
/// Coordinates are already database/LSP 0-based here.
#[allow(clippy::too_many_arguments)]
pub async fn at_path(
    pool: &SqlitePool,
    worktree_root: &Path,
    path: &str,
    line: u32,
    character: u32,
    direction: Direction,
    depth: u32,
) -> anyhow::Result<Neighborhood> {
    let worktree_key = worktree_root.to_string_lossy().into_owned();
    let abs = crate::fsapi::safe_join(worktree_root, path)?;
    let canonical_root = worktree_root.canonicalize()?;
    let rel_path = abs
        .strip_prefix(&canonical_root)
        .map_err(|_| anyhow::anyhow!("워크트리 밖 경로입니다"))?
        .to_string_lossy()
        .into_owned();
    let scan_root = canonical_root.clone();
    let manifest = tokio::task::spawn_blocking(move || super::manifest::scan(&scan_root)).await??;
    at(
        pool,
        &worktree_key,
        &rel_path,
        line,
        character,
        direction,
        depth,
        &manifest,
    )
    .await
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    Incoming,
    Outgoing,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Node {
    pub id: i64,
    pub name: String,
    pub rel_path: String,
    pub line: i64,
    pub character: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub source_id: i64,
    pub target_id: i64,
    pub relation: &'static str,
}

#[derive(Debug, Clone, Serialize, sqlx::FromRow, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EncounteredIncomplete {
    pub rel_path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Neighborhood {
    pub run_id: i64,
    pub indexed_at: i64,
    pub freshness: String,
    pub root_id: i64,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub truncated: bool,
    pub incomplete: Option<status::Incompleteness>,
    pub edges_unavailable: Option<String>,
    pub encountered_incomplete: Vec<EncounteredIncomplete>,
}

#[derive(sqlx::FromRow)]
struct Run {
    id: i64,
    source_fingerprint: String,
    indexed_at: i64,
}

#[derive(sqlx::FromRow)]
struct EdgeRow {
    source_id: i64,
    target_id: i64,
    id: i64,
    name: String,
    rel_path: String,
    line: i64,
    character: i64,
}

pub async fn at(
    pool: &SqlitePool,
    worktree: &str,
    rel_path: &str,
    line: u32,
    character: u32,
    direction: Direction,
    depth: u32,
    manifest: &SourceManifest,
) -> anyhow::Result<Neighborhood> {
    let hash = manifest
        .files
        .iter()
        .find(|file| file.rel_path == rel_path)
        .map(|file| file.content_hash.as_str())
        .ok_or_else(|| anyhow::anyhow!("source-changed: 인덱싱 대상 소스가 아닙니다"))?;
    let mut tx = pool.begin().await?;
    let run = active(&mut tx, worktree).await?;
    #[cfg(test)]
    wait_after_active_read().await;
    let stored = file_hash(&mut tx, run.id, rel_path).await?;
    if stored.as_deref() != Some(hash) {
        anyhow::bail!("source-changed: 그래프 좌표를 사용할 수 없습니다");
    }
    let root = root(&mut tx, run.id, rel_path, line, character).await?;
    let edges_unavailable = edge_state(&mut tx, run.id, rel_path).await?;
    let (nodes, edges, truncated) =
        traverse(&mut tx, run.id, root.clone(), direction, depth).await?;
    let incomplete = status::incompleteness(&mut *tx, run.id).await?;
    let encountered_incomplete = encountered(&mut tx, run.id, &nodes).await?;
    tx.commit().await?;
    Ok(Neighborhood {
        run_id: run.id,
        indexed_at: run.indexed_at,
        freshness: if run.source_fingerprint == manifest.fingerprint {
            "ready"
        } else {
            "stale"
        }
        .into(),
        root_id: root.id,
        nodes,
        edges,
        truncated,
        incomplete,
        edges_unavailable,
        encountered_incomplete,
    })
}

#[cfg(test)]
#[derive(Clone)]
pub(super) struct ActiveReadGate {
    pub entered: std::sync::Arc<tokio::sync::Barrier>,
    pub release: std::sync::Arc<tokio::sync::Barrier>,
}

#[cfg(test)]
tokio::task_local! {
    pub(super) static ACTIVE_READ_GATE: ActiveReadGate;
}

#[cfg(test)]
async fn wait_after_active_read() {
    if let Ok(gate) = ACTIVE_READ_GATE.try_with(Clone::clone) {
        gate.entered.wait().await;
        gate.release.wait().await;
    }
}

async fn active(tx: &mut Transaction<'_, Sqlite>, worktree: &str) -> anyhow::Result<Run> {
    sqlx::query_as(
        "SELECT r.id, r.source_fingerprint, r.finished_at AS indexed_at FROM code_graph_active a JOIN code_graph_runs r ON r.id=a.run_id WHERE a.worktree=?",
    )
    .bind(worktree)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| anyhow::anyhow!("활성 코드 그래프가 없습니다"))
}

async fn file_hash(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: i64,
    path: &str,
) -> anyhow::Result<Option<String>> {
    Ok(sqlx::query_scalar(
        "SELECT content_hash FROM code_graph_files WHERE run_id=? AND rel_path=?",
    )
    .bind(run_id)
    .bind(path)
    .fetch_optional(&mut **tx)
    .await?)
}

async fn edge_state(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: i64,
    path: &str,
) -> anyhow::Result<Option<String>> {
    Ok(
        sqlx::query_scalar("SELECT edge_state FROM code_graph_files WHERE run_id=? AND rel_path=?")
            .bind(run_id)
            .bind(path)
            .fetch_optional(&mut **tx)
            .await?
            .flatten(),
    )
}

async fn root(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: i64,
    path: &str,
    line: u32,
    character: u32,
) -> anyhow::Result<Node> {
    sqlx::query_as(
        "SELECT n.id, n.name, f.rel_path, n.sel_start_line AS line, n.sel_start_char AS character FROM code_graph_nodes n JOIN code_graph_files f ON f.id=n.file_id AND f.run_id=? WHERE n.run_id=? AND f.rel_path=? AND ((n.sel_start_line<? OR (n.sel_start_line=? AND n.sel_start_char<=?)) AND (n.sel_end_line>? OR (n.sel_end_line=? AND n.sel_end_char>?)) OR (n.body_start_line<? OR (n.body_start_line=? AND n.body_start_char<=?)) AND (n.body_end_line>? OR (n.body_end_line=? AND n.body_end_char>?))) ORDER BY CASE WHEN (n.sel_start_line<? OR (n.sel_start_line=? AND n.sel_start_char<=?)) AND (n.sel_end_line>? OR (n.sel_end_line=? AND n.sel_end_char>?)) THEN 0 ELSE 1 END, (n.body_end_line-n.body_start_line), n.body_start_line DESC, n.body_end_line, n.body_start_char DESC, n.body_end_char LIMIT 1",
    ).bind(run_id).bind(run_id).bind(path).bind(line).bind(line).bind(character).bind(line).bind(line).bind(character).bind(line).bind(line).bind(character).bind(line).bind(line).bind(character).bind(line).bind(line).bind(character).bind(line).bind(line).bind(character).fetch_optional(&mut **tx).await?.ok_or_else(|| anyhow::anyhow!("커서 위치에서 활성 코드 그래프 심볼을 찾지 못했습니다"))
}

async fn traverse(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: i64,
    root: Node,
    direction: Direction,
    depth: u32,
) -> anyhow::Result<(Vec<Node>, Vec<Edge>, bool)> {
    let mut nodes = BTreeMap::from([(root.id, root)]);
    let mut edges = BTreeSet::new();
    let mut frontier = vec![*nodes.keys().next().unwrap()];
    let mut truncated = false;
    let max_depth = depth.clamp(1, 3);
    for step in 0..max_depth {
        if frontier.is_empty() || edges.len() == MAX_EDGES {
            break;
        }
        let rows = edge_rows(
            tx,
            run_id,
            &frontier,
            direction,
            MAX_EDGES - edges.len() + 1,
        )
        .await?;
        let mut next = Vec::new();
        for row in rows {
            if edges.len() == MAX_EDGES {
                truncated = true;
                break;
            }
            if !nodes.contains_key(&row.id) && nodes.len() == MAX_NODES {
                truncated = true;
                continue;
            }
            if nodes
                .insert(
                    row.id,
                    Node {
                        id: row.id,
                        name: row.name,
                        rel_path: row.rel_path,
                        line: row.line,
                        character: row.character,
                    },
                )
                .is_none()
            {
                next.push(row.id);
            }
            edges.insert((row.source_id, row.target_id));
        }
        frontier = next;
        if edges.len() == MAX_EDGES && step + 1 < max_depth {
            truncated =
                truncated || has_more_edges(tx, run_id, &frontier, direction, &edges).await?;
            break;
        }
    }
    let mut nodes: Vec<_> = nodes.into_values().collect();
    nodes.sort_by(|a, b| {
        (&a.rel_path, a.line, a.character, a.id).cmp(&(&b.rel_path, b.line, b.character, b.id))
    });
    Ok((
        nodes,
        edges
            .into_iter()
            .map(|(source_id, target_id)| Edge {
                source_id,
                target_id,
                relation: "references",
            })
            .collect(),
        truncated,
    ))
}

async fn has_more_edges(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: i64,
    frontier: &[i64],
    direction: Direction,
    edges: &BTreeSet<(i64, i64)>,
) -> anyhow::Result<bool> {
    if frontier.is_empty() {
        return Ok(false);
    }
    Ok(edge_rows(tx, run_id, frontier, direction, MAX_EDGES + 1)
        .await?
        .into_iter()
        .any(|row| !edges.contains(&(row.source_id, row.target_id))))
}

async fn edge_rows(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: i64,
    frontier: &[i64],
    direction: Direction,
    limit: usize,
) -> anyhow::Result<Vec<EdgeRow>> {
    let (column, node) = match direction {
        Direction::Incoming => ("dst_id", "src_id"),
        Direction::Outgoing => ("src_id", "dst_id"),
    };
    let marks = std::iter::repeat_n("?", frontier.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT e.src_id AS source_id, e.dst_id AS target_id, n.id, n.name, f.rel_path, n.sel_start_line AS line, n.sel_start_char AS character FROM code_graph_edges e JOIN code_graph_nodes n ON n.id=e.{node} AND n.run_id=? JOIN code_graph_files f ON f.id=n.file_id AND f.run_id=? WHERE e.run_id=? AND e.rel='references' AND e.{column} IN ({marks}) ORDER BY f.rel_path, n.sel_start_line, n.sel_start_char, n.id LIMIT ?");
    let mut query = sqlx::query_as::<_, EdgeRow>(&sql)
        .bind(run_id)
        .bind(run_id)
        .bind(run_id);
    for id in frontier {
        query = query.bind(id);
    }
    Ok(query.bind(limit as i64).fetch_all(&mut **tx).await?)
}

async fn encountered(
    tx: &mut Transaction<'_, Sqlite>,
    run_id: i64,
    nodes: &[Node],
) -> anyhow::Result<Vec<EncounteredIncomplete>> {
    let marks = std::iter::repeat_n("?", nodes.len())
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("SELECT DISTINCT f.rel_path, coalesce(f.edge_state, f.skip_reason) AS reason FROM code_graph_files f JOIN code_graph_nodes n ON n.file_id=f.id AND n.run_id=? WHERE f.run_id=? AND n.id IN ({marks}) AND (f.edge_state IS NOT NULL OR f.skip_reason IS NOT NULL) ORDER BY f.rel_path");
    let mut query = sqlx::query_as::<_, EncounteredIncomplete>(&sql)
        .bind(run_id)
        .bind(run_id);
    for node in nodes {
        query = query.bind(node.id);
    }
    Ok(query.fetch_all(&mut **tx).await?)
}
