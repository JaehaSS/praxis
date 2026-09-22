use std::collections::{BTreeSet, HashSet, VecDeque};

use sqlx::SqlitePool;

const MAX_DEPTH: usize = 2;
const MAX_NODES: usize = 100;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WhyTrace {
    pub nodes: Vec<TraceNode>,
    pub edges: Vec<TraceEdge>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TraceNode {
    pub key: String,
    pub kind: String,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, sqlx::FromRow)]
pub struct TraceEdge {
    pub decision_id: i64,
    pub relation: String,
    pub artifact_kind: String,
    pub artifact_ref: String,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum NodeRef {
    Decision(i64),
    Artifact { kind: String, reference: String },
}

pub async fn why_trace(
    pool: &SqlitePool,
    decision_id: i64,
    requested_depth: usize,
) -> anyhow::Result<WhyTrace> {
    if !super::is_enabled(pool).await? {
        anyhow::bail!("decision ledger is disabled");
    }
    ensure_active(pool, decision_id).await?;
    let start = NodeRef::Decision(decision_id);
    let mut nodes = vec![node(&start)];
    let mut visited = HashSet::from([start.clone()]);
    let mut queue = VecDeque::from([(start, 0_usize)]);
    let mut edges = BTreeSet::new();
    let mut truncated = false;
    let depth_limit = requested_depth.min(MAX_DEPTH);
    while let Some((current, depth)) = queue.pop_front() {
        if depth >= depth_limit {
            continue;
        }
        for (neighbor, edge) in neighbors(pool, &current).await? {
            if visited.contains(&neighbor) {
                edges.insert(edge);
                continue;
            }
            if nodes.len() == MAX_NODES {
                truncated = true;
                continue;
            }
            visited.insert(neighbor.clone());
            nodes.push(node(&neighbor));
            queue.push_back((neighbor, depth + 1));
            edges.insert(edge);
        }
    }
    Ok(WhyTrace {
        nodes,
        edges: edges.into_iter().collect(),
        truncated,
    })
}

async fn ensure_active(pool: &SqlitePool, decision_id: i64) -> anyhow::Result<()> {
    let found: Option<i64> =
        sqlx::query_scalar("SELECT id FROM decision_records WHERE id = ? AND status = 'active'")
            .bind(decision_id)
            .fetch_optional(pool)
            .await?;
    if found.is_none() {
        anyhow::bail!("active decision not found");
    }
    Ok(())
}

async fn neighbors(
    pool: &SqlitePool,
    current: &NodeRef,
) -> anyhow::Result<Vec<(NodeRef, TraceEdge)>> {
    let rows = match current {
        NodeRef::Decision(id) => decision_edges(pool, *id).await?,
        NodeRef::Artifact { kind, reference } => artifact_edges(pool, kind, reference).await?,
    };
    Ok(rows
        .into_iter()
        .map(|edge| {
            let neighbor = match current {
                NodeRef::Decision(_) => NodeRef::Artifact {
                    kind: edge.artifact_kind.clone(),
                    reference: edge.artifact_ref.clone(),
                },
                NodeRef::Artifact { .. } => NodeRef::Decision(edge.decision_id),
            };
            (neighbor, edge)
        })
        .collect())
}

async fn decision_edges(pool: &SqlitePool, decision_id: i64) -> anyhow::Result<Vec<TraceEdge>> {
    Ok(sqlx::query_as(
        "SELECT decision_id, relation, artifact_kind, artifact_ref \
         FROM decision_artifact_links WHERE decision_id = ? \
         ORDER BY artifact_kind, artifact_ref, relation",
    )
    .bind(decision_id)
    .fetch_all(pool)
    .await?)
}

async fn artifact_edges(
    pool: &SqlitePool,
    kind: &str,
    reference: &str,
) -> anyhow::Result<Vec<TraceEdge>> {
    Ok(sqlx::query_as(
        "SELECT l.decision_id, l.relation, l.artifact_kind, l.artifact_ref \
         FROM decision_artifact_links l JOIN decision_records d ON d.id = l.decision_id \
         WHERE l.artifact_kind = ? AND l.artifact_ref = ? AND d.status = 'active' \
         ORDER BY l.decision_id, l.relation",
    )
    .bind(kind)
    .bind(reference)
    .fetch_all(pool)
    .await?)
}

fn node(reference: &NodeRef) -> TraceNode {
    match reference {
        NodeRef::Decision(id) => TraceNode {
            key: format!("decision:{id}"),
            kind: "decision".into(),
        },
        NodeRef::Artifact { kind, reference } => TraceNode {
            key: format!("{kind}:{reference}"),
            kind: kind.clone(),
        },
    }
}
