use std::collections::HashSet;

use sqlx::{Row, SqlitePool};

use super::config::active_space_ids;
use super::SOURCE_ID;

pub async fn active_node_ids(pool: &SqlitePool, node_ids: &[i64]) -> anyhow::Result<HashSet<i64>> {
    if node_ids.is_empty() {
        return Ok(HashSet::new());
    }
    let spaces: HashSet<String> = active_space_ids(pool).await?.into_iter().collect();
    let placeholders = vec!["?"; node_ids.len()].join(",");
    let sql =
        format!("SELECT id, source, space_id FROM knowledge_nodes WHERE id IN ({placeholders})");
    let mut query = sqlx::query(&sql);
    for id in node_ids {
        query = query.bind(id);
    }
    let rows = query.fetch_all(pool).await?;
    let mut active = HashSet::new();
    for row in rows {
        let id: i64 = row.try_get("id")?;
        let source: String = row.try_get("source")?;
        let space_id: Option<String> = row.try_get("space_id")?;
        if source == SOURCE_ID && !space_id.is_some_and(|space| spaces.contains(&space)) {
            continue;
        }
        if crate::knowledge::vault::ownership::owns_legacy_node(pool, id).await? {
            continue;
        }
        active.insert(id);
    }
    Ok(active)
}
