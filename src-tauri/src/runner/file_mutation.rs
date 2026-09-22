//! Worktree-locked Runner file mutations.

use std::path::Path;

use sqlx::SqlitePool;
use tokio::sync::OwnedMutexGuard;

use crate::fsapi::guard::{assert_owners_mutable, owner_tasks, OwnerTask};
use crate::review_ops::{ReviewClaim, ReviewClaims};

use super::worktree_lock::WorktreeLocks;

pub async fn write_file(
    pool: &SqlitePool,
    worktree_locks: &WorktreeLocks,
    review_claims: &ReviewClaims,
    root: &Path,
    relative_path: &str,
    content: &str,
) -> anyhow::Result<i64> {
    let target = crate::fsapi::safe_join(root, relative_path)?;
    let owners = owner_tasks(pool, &target).await?;
    let _claims = claim_owners(review_claims, &owners)?;
    let _guards = lock_owner_roots(worktree_locks, &owners).await;
    assert_owners_mutable(pool, &owners).await?;
    crate::fsapi::write_file(root, relative_path, content)
}

fn claim_owners(
    review_claims: &ReviewClaims,
    owners: &[OwnerTask],
) -> anyhow::Result<Vec<ReviewClaim>> {
    let mut claims = Vec::with_capacity(owners.len());
    for owner in owners {
        claims.push(
            review_claims
                .claim_finalization(owner.id)
                .map_err(anyhow::Error::msg)?,
        );
    }
    Ok(claims)
}

async fn lock_owner_roots(
    worktree_locks: &WorktreeLocks,
    owners: &[OwnerTask],
) -> Vec<OwnedMutexGuard<()>> {
    let mut roots = owners
        .iter()
        .map(|owner| owner.root.clone())
        .collect::<Vec<_>>();
    roots.sort();
    roots.dedup();
    let mut guards = Vec::with_capacity(roots.len());
    for root in roots {
        guards.push(worktree_locks.acquire(&root).await);
    }
    guards
}
