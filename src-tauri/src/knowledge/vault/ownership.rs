use std::collections::HashSet;
use std::path::{Path, PathBuf};

use sqlx::{Row, SqlitePool};

/// 한 번에 물어볼 노드 수. SQLite의 바인드 변수 상한(기본 32,766)보다 한참 아래로 잡는다 —
/// 후보 상한이 300이라 실제로는 한 묶음으로 끝난다.
const OWNERSHIP_BATCH: usize = 256;

/// Legacy graph readers call this before returning indexed content.  A path
/// owned by a vault may only be read through the vault's scoped catalog.
pub async fn is_owned_path(pool: &SqlitePool, path: &Path) -> anyhow::Result<bool> {
    Ok(path_under_roots(&owned_roots(pool).await?, path))
}

/// 경로 판정만 떼어낸 것. `is_owned_path`와 묶음 판정이 **같은 규칙**을 쓰게 하려고 둔다 —
/// 두 벌로 두면 한쪽만 canonicalize 폴백을 잃고, 그 차이는 "가끔 안 걸러진다"로만 보인다.
fn path_under_roots(roots: &[PathBuf], path: &Path) -> bool {
    if roots.iter().any(|root| path.starts_with(root.as_path())) {
        return true;
    }
    path.canonicalize()
        .ok()
        .is_some_and(|path| roots.iter().any(|root| path.starts_with(root.as_path())))
}

/// `owns_legacy_node`를 노드 묶음에 한 번에 적용한다. 소유된 node_id만 돌려준다.
///
/// 낱개로 물으면 노드마다 최대 세 번(소유 등재 조회 · url 조회 · vault 루트 조회) 풀에서
/// 커넥션을 빌렸다 놓는다. 검색 후보는 300개까지라 **검색 한 번이 900번 가까이** 그 짓을
/// 하고, 그 사이에 낀 다른 조회는 매번 줄 뒤로 밀린다. 커넥션이 다섯뿐이던 시절 이것이
/// `pool timed out while waiting for an open connection`의 실질적인 압력원이었다.
/// 묶음당 쿼리 두 번 + 루트 조회 한 번으로 줄인다.
pub async fn owned_legacy_nodes(
    pool: &SqlitePool,
    node_ids: &[i64],
) -> anyhow::Result<HashSet<i64>> {
    let mut owned = HashSet::new();
    if node_ids.is_empty() {
        return Ok(owned);
    }
    // 한 노드가 청크 여러 개로 후보에 들어오므로 중복부터 접는다.
    let mut unique: Vec<i64> = node_ids.to_vec();
    unique.sort_unstable();
    unique.dedup();
    let roots = owned_roots(pool).await?;
    for batch in unique.chunks(OWNERSHIP_BATCH) {
        let placeholders = vec!["?"; batch.len()].join(", ");
        let claimed_sql = format!(
            "SELECT node_id FROM vault_legacy_ownership WHERE node_id IN ({placeholders})"
        );
        let mut claimed = sqlx::query(&claimed_sql);
        for id in batch {
            claimed = claimed.bind(id);
        }
        for row in claimed.fetch_all(pool).await? {
            owned.insert(row.try_get::<i64, _>("node_id")?);
        }
        let urls_sql = format!("SELECT id, url FROM knowledge_nodes WHERE id IN ({placeholders})");
        let mut urls = sqlx::query(&urls_sql);
        for id in batch {
            urls = urls.bind(id);
        }
        for row in urls.fetch_all(pool).await? {
            let id: i64 = row.try_get("id")?;
            if owned.contains(&id) {
                continue;
            }
            let url: Option<String> = row.try_get("url")?;
            let Some(path) = url
                .as_deref()
                .and_then(|url| url.strip_prefix("file://"))
                .map(PathBuf::from)
            else {
                continue;
            };
            if path_under_roots(&roots, &path) {
                owned.insert(id);
            }
        }
    }
    Ok(owned)
}

pub async fn owns_legacy_node(pool: &SqlitePool, node_id: i64) -> anyhow::Result<bool> {
    let claimed: Option<(i64,)> =
        sqlx::query_as("SELECT 1 FROM vault_legacy_ownership WHERE node_id = ?")
            .bind(node_id)
            .fetch_optional(pool)
            .await?;
    if claimed.is_some() {
        return Ok(true);
    }
    let url: Option<String> = sqlx::query_scalar("SELECT url FROM knowledge_nodes WHERE id = ?")
        .bind(node_id)
        .fetch_optional(pool)
        .await?
        .flatten();
    let Some(path) = url.and_then(|url| url.strip_prefix("file://").map(PathBuf::from)) else {
        return Ok(false);
    };
    is_owned_path(pool, &path).await
}

pub async fn reject_overlapping_root(pool: &SqlitePool, root: &Path) -> anyhow::Result<()> {
    let root = root.canonicalize()?;
    for owned in owned_roots(pool).await? {
        if root == owned || root.starts_with(&owned) || owned.starts_with(&root) {
            anyhow::bail!("Wiki root overlaps a personal knowledge vault")
        }
    }
    Ok(())
}

pub async fn claim_legacy_owned_nodes(pool: &SqlitePool, now: i64) -> anyhow::Result<usize> {
    let _admission = super::exclusive_admission(pool).await?;
    claim_legacy_owned_nodes_unlocked(pool, now).await
}

pub(crate) async fn claim_legacy_owned_nodes_unlocked(
    pool: &SqlitePool,
    now: i64,
) -> anyhow::Result<usize> {
    let legacy_exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'knowledge_nodes')")
        .fetch_one(pool).await?;
    if !legacy_exists {
        return Ok(0);
    }
    let rows = sqlx::query("SELECT id, url FROM knowledge_nodes WHERE url LIKE 'file://%'")
        .fetch_all(pool)
        .await?;
    let mut claimed = 0;
    for row in rows {
        let id: i64 = row.try_get("id")?;
        let url: String = row.try_get("url")?;
        let Some(path) = url.strip_prefix("file://") else {
            continue;
        };
        if !is_owned_path(pool, Path::new(path)).await? {
            continue;
        }
        let vault_id = vault_for_path(pool, Path::new(path))
            .await?
            .ok_or_else(|| anyhow::anyhow!("vault ownership root is missing"))?;
        let inserted = sqlx::query("INSERT INTO vault_legacy_ownership (node_id, vault_id, claimed_at) VALUES (?, ?, ?) ON CONFLICT(node_id) DO NOTHING")
            .bind(id).bind(vault_id).bind(now).execute(pool).await?.rows_affected();
        if inserted == 0 {
            continue;
        }
        sqlx::query("DELETE FROM knowledge_chunks WHERE node_id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        sqlx::query("DELETE FROM knowledge_edges WHERE src_id = ? OR dst_id = ?")
            .bind(id)
            .bind(id)
            .execute(pool)
            .await?;
        claimed += 1;
    }
    Ok(claimed)
}

pub async fn owned_roots(pool: &SqlitePool) -> anyhow::Result<Vec<PathBuf>> {
    let rows = sqlx::query("SELECT canonical_root FROM vaults")
        .fetch_all(pool)
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| row.try_get::<String, _>("canonical_root").ok())
        .map(PathBuf::from)
        .collect())
}

async fn vault_for_path(pool: &SqlitePool, path: &Path) -> anyhow::Result<Option<String>> {
    let canonical = path.canonicalize().ok();
    let rows = sqlx::query("SELECT id, canonical_root FROM vaults")
        .fetch_all(pool)
        .await?;
    Ok(rows.into_iter().find_map(|row| {
        let root: String = row.try_get("canonical_root").ok()?;
        (path.starts_with(Path::new(&root))
            || canonical
                .as_ref()
                .is_some_and(|path| path.starts_with(Path::new(&root))))
        .then(|| row.try_get("id").ok())
        .flatten()
    }))
}
