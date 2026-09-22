use std::collections::HashSet;

use sqlx::{Row, SqlitePool};

use super::bindings::ProjectBinding;
use super::platform;

const MAX_SOURCE_NODES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Scope {
    PrivateData,
    Common,
    Project { key: String, binding_epoch: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeRequest {
    pub scope: Scope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    Revision(String),
    CompletionSnapshot(String),
}

/// Return the narrowest permitted automatic scope for every current source.
/// Incompatible project grants are terminal: later sources cannot widen them.
pub async fn scope_for_sources(
    pool: &SqlitePool,
    sources: &[String],
) -> anyhow::Result<Option<Scope>> {
    let sets = source_grant_sets(pool, sources).await?;
    intersect_grant_sets(&sets)
}

pub async fn scope_allows_binding(
    pool: &SqlitePool,
    sources: &[String],
    binding: &ProjectBinding,
) -> anyhow::Result<bool> {
    if !binding_is_current(pool, binding).await? {
        return Ok(false);
    }
    let Some(scope) = scope_for_sources(pool, sources).await? else {
        return Ok(false);
    };
    Ok(match scope {
        Scope::Common => true,
        Scope::Project { key, binding_epoch } => {
            key == binding.id && binding_epoch == binding.epoch
        }
        Scope::PrivateData => false,
    })
}

pub fn scope_allows(scope: &Scope, request: &ScopeRequest) -> bool {
    match (scope, &request.scope) {
        (Scope::PrivateData, Scope::PrivateData)
        | (Scope::Common, _)
        | (Scope::Project { .. }, Scope::PrivateData) => true,
        (
            Scope::Project { key, binding_epoch },
            Scope::Project {
                key: requested,
                binding_epoch: epoch,
            },
        ) => key == requested && binding_epoch == epoch,
        _ => false,
    }
}

async fn source_grant_sets(
    pool: &SqlitePool,
    sources: &[String],
) -> anyhow::Result<Vec<HashSet<Scope>>> {
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let mut pending = sources
        .iter()
        .cloned()
        .map(|id| (id, false))
        .collect::<Vec<_>>();
    let mut visited = HashSet::new();
    let mut active = HashSet::new();
    let mut sets = Vec::new();
    while let Some((revision_id, exiting)) = pending.pop() {
        if exiting {
            active.remove(&revision_id);
            continue;
        }
        if active.contains(&revision_id) {
            anyhow::bail!("vault source graph contains a cycle")
        }
        if !visited.insert(revision_id.clone()) {
            continue;
        }
        if visited.len() > MAX_SOURCE_NODES {
            anyhow::bail!("vault source graph exceeds 64 nodes")
        }
        active.insert(revision_id.clone());
        let grants = current_grants(pool, &revision_id).await?;
        if grants.is_empty() {
            return Ok(Vec::new());
        }
        sets.push(grants);
        let rows = sqlx::query(
            "SELECT source_revision_id FROM vault_revision_sources WHERE revision_id = ?",
        )
        .bind(&revision_id)
        .fetch_all(pool)
        .await?;
        pending.push((revision_id.clone(), true));
        for row in rows {
            pending.push((row.try_get("source_revision_id")?, false));
        }
    }
    Ok(sets)
}

fn intersect_grant_sets(sets: &[HashSet<Scope>]) -> anyhow::Result<Option<Scope>> {
    if sets.is_empty() {
        return Ok(None);
    }
    if sets.iter().all(|set| set.contains(&Scope::Common)) {
        return Ok(Some(Scope::Common));
    }
    let projects: HashSet<Scope> = sets
        .iter()
        .flat_map(|set| set.iter())
        .filter_map(|scope| match scope {
            Scope::Project { .. } => Some(scope.clone()),
            _ => None,
        })
        .collect();
    if projects.len() > 1 {
        return Ok(None);
    }
    if sets.iter().any(|set| set.contains(&Scope::PrivateData)) {
        return Ok(Some(Scope::PrivateData));
    }
    let compatible: Vec<Scope> = projects
        .into_iter()
        .filter(|project| {
            sets.iter()
                .all(|set| set.contains(&Scope::Common) || set.contains(project))
        })
        .collect();
    if compatible.len() == 1 {
        return Ok(compatible.into_iter().next());
    }
    if compatible.len() > 1 {
        anyhow::bail!("vault source grants have ambiguous project scope")
    }
    let has_project = sets.iter().any(|set| {
        set.iter()
            .any(|scope| matches!(scope, Scope::Project { .. }))
    });
    if has_project {
        return Ok(None);
    }
    Ok(Some(Scope::PrivateData))
}

async fn current_grants(pool: &SqlitePool, revision_id: &str) -> anyhow::Result<HashSet<Scope>> {
    if super::files::read_revision(pool, revision_id)
        .await
        .is_err()
    {
        return Ok(HashSet::new());
    }
    let rows = sqlx::query("SELECT g.scope, g.project_key, g.binding_epoch, v.canonical_root, v.root_device, v.root_inode, p.canonical_root AS binding_root, p.root_device AS binding_device, p.root_inode AS binding_inode FROM vault_grants g JOIN vault_revisions r ON r.id = g.revision_id JOIN vault_documents d ON d.id = r.document_id JOIN vaults v ON v.id = d.vault_id LEFT JOIN vault_project_bindings p ON p.id = g.project_key AND p.epoch = g.binding_epoch AND p.active = 1 WHERE g.revision_id = ? AND g.revoked_at IS NULL AND d.state = 'active' AND d.current_revision = r.id AND v.enabled = 1 AND (g.scope <> 'project' OR p.id IS NOT NULL)")
        .bind(revision_id).fetch_all(pool).await?;
    let mut scopes = HashSet::new();
    for row in &rows {
        let root: String = row.try_get("canonical_root")?;
        let device: i64 = row.try_get("root_device")?;
        let inode: i64 = row.try_get("root_inode")?;
        let identity = platform::verified_root(std::path::Path::new(&root))?;
        if identity.canonical_root != root || identity.device != device || identity.inode != inode {
            continue;
        }
        let scope = scope_from_row(row)?;
        if let Scope::Project { .. } = &scope {
            let binding_root: String = row.try_get("binding_root")?;
            let Ok(binding) = platform::verified_root(std::path::Path::new(&binding_root)) else {
                continue;
            };
            if binding.canonical_root != binding_root
                || binding.device != row.try_get::<i64, _>("binding_device")?
                || binding.inode != row.try_get::<i64, _>("binding_inode")?
            {
                continue;
            }
        }
        scopes.insert(scope);
    }
    Ok(scopes)
}

fn scope_from_row(row: &sqlx::sqlite::SqliteRow) -> anyhow::Result<Scope> {
    let scope: String = row.try_get("scope")?;
    match scope.as_str() {
        "private-data" => Ok(Scope::PrivateData),
        "common" => Ok(Scope::Common),
        "project" => Ok(Scope::Project {
            key: row
                .try_get::<Option<String>, _>("project_key")?
                .ok_or_else(|| anyhow::anyhow!("project grant has no project key"))?,
            binding_epoch: row
                .try_get::<Option<String>, _>("binding_epoch")?
                .ok_or_else(|| anyhow::anyhow!("project grant has no binding epoch"))?,
        }),
        _ => anyhow::bail!("unknown vault scope"),
    }
}

async fn binding_is_current(pool: &SqlitePool, binding: &ProjectBinding) -> anyhow::Result<bool> {
    let row: Option<(String, i64, i64, String)> = sqlx::query_as("SELECT canonical_root, root_device, root_inode, epoch FROM vault_project_bindings WHERE id = ? AND active = 1")
        .bind(&binding.id).fetch_optional(pool).await?;
    let Some((root, device, inode, epoch)) = row else {
        return Ok(false);
    };
    let identity = platform::verified_root(std::path::Path::new(&root))?;
    Ok(identity.canonical_root == root
        && identity.device == device
        && identity.inode == inode
        && epoch == binding.epoch)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{intersect_grant_sets, Scope};

    #[test]
    fn private_source_narrows_a_project_to_private_data() {
        let project = Scope::Project {
            key: "project".into(),
            binding_epoch: "epoch".into(),
        };
        let result = intersect_grant_sets(&[
            HashSet::from([project]),
            HashSet::from([Scope::PrivateData]),
        ])
        .unwrap();
        assert_eq!(result, Some(Scope::PrivateData));
    }

    #[test]
    fn incompatible_projects_are_denied_even_with_private_data() {
        let first = Scope::Project {
            key: "first".into(),
            binding_epoch: "epoch".into(),
        };
        let second = Scope::Project {
            key: "second".into(),
            binding_epoch: "epoch".into(),
        };
        let result = intersect_grant_sets(&[
            HashSet::from([first]),
            HashSet::from([second]),
            HashSet::from([Scope::PrivateData]),
        ])
        .unwrap();
        assert_eq!(result, None);
    }
}
