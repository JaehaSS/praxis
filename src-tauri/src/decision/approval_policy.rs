use crate::{db, worktree::Worktree};

pub(super) fn enforce_goal_contract(task: &db::Task, worktree: &Worktree) -> anyhow::Result<()> {
    let changed = worktree.changed_paths()?;
    let violations = task
        .goal_contract
        .as_deref()
        .map(|contract| {
            crate::goal_contract::protected_path_violations(&contract.protected_paths, &changed)
        })
        .unwrap_or_default();
    if !violations.is_empty() {
        anyhow::bail!(
            "Goal Contract 보호 경로가 변경되어 승인할 수 없습니다: {}",
            violations.join(", ")
        );
    }
    Ok(())
}
