//! 워크트리 소유 판정 — 로컬 커맨드와 Runner가 공유한다.
//!
//! 원래 `runner/file_mutation.rs`에 있었다. Runner 전용일 이유가 없어 승격했다
//! (설계 0024 D1). 로컬이 별도 판정을 만들면 규칙이 두 벌로 갈라진다.

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;

pub struct OwnerTask {
    pub id: i64,
    pub state: String,
    pub root: PathBuf,
}

/// 대상 경로를 워크트리 하위에 두는 작업들.
///
/// canonicalize가 실패했는데 대상이 그 raw root 하위면 **판정 불능이므로 에러**다 —
/// 막는 쪽으로 기운다. 무관한 경로의 실패는 건너뛴다.
pub async fn owner_tasks(pool: &SqlitePool, target: &Path) -> anyhow::Result<Vec<OwnerTask>> {
    let rows: Vec<(i64, String, String)> =
        sqlx::query_as("SELECT id, state, worktree_path FROM tasks ORDER BY id")
            .fetch_all(pool)
            .await?;
    let mut owners = Vec::new();
    for (id, state, stored_root) in rows {
        let raw_root = PathBuf::from(&stored_root);
        let canonical = match raw_root.canonicalize() {
            Ok(canonical) => canonical,
            Err(error) if target.starts_with(&raw_root) => return Err(error.into()),
            Err(_) => continue,
        };
        if target.starts_with(&canonical) {
            owners.push(OwnerTask {
                id,
                state,
                root: canonical,
            });
        }
    }
    Ok(owners)
}

/// 소유 작업이 변경을 허용하는 상태인지. Finalizing과 fenced는 거부.
pub async fn assert_owners_mutable(pool: &SqlitePool, owners: &[OwnerTask]) -> anyhow::Result<()> {
    for owner in owners {
        if owner.state == crate::db::state::FINALIZING {
            anyhow::bail!("Finalizing task의 worktree는 수정할 수 없습니다");
        }
        crate::runner::review_process::assert_task_unfenced(pool, owner.id)
            .await
            .map_err(anyhow::Error::msg)?;
    }
    Ok(())
}
