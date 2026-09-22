use std::path::{Path, PathBuf};

use super::{current_branch, current_revision, run_git, Worktree};

impl Worktree {
    pub fn validate_isolated_approval(&self) -> anyhow::Result<()> {
        let repo = canonical(&self.repo, "repository")?;
        let checkout = canonical(&self.path, "approval worktree")?;
        if repo == checkout {
            anyhow::bail!("direct mode decision ledger is not supported");
        }
        let registered = registered_worktrees(&self.repo)?;
        if !registered.iter().any(|path| path == &checkout) {
            anyhow::bail!("decision ledger requires a registered isolated worktree");
        }
        if current_branch(&self.path)? != self.branch {
            anyhow::bail!("approval worktree branch does not match the task");
        }
        Ok(())
    }

    pub fn validate_commit_object(&self, commit: &str) -> anyhow::Result<()> {
        crate::decision::approval_journal::validate_commit_sha(commit)?;
        let kind = run_git(&self.repo, &["cat-file", "-t", commit])?;
        if kind.trim() != "commit" {
            anyhow::bail!("recorded approval identity is not a commit object");
        }
        Ok(())
    }

    pub fn validate_decision_commit(
        &self,
        commit: &str,
        exclude_generated_mcp: bool,
    ) -> anyhow::Result<()> {
        self.validate_commit_object(commit)?;
        if exclude_generated_mcp && self.commit_history_touches_generated_mcp(commit)? {
            anyhow::bail!("generated .mcp.json is present in approval commit history");
        }
        let paths = self.changed_paths()?;
        let has_task_change = paths.iter().any(|path| path != ".mcp.json");
        if !has_task_change {
            anyhow::bail!("decision ledger approval contains no task changes");
        }
        Ok(())
    }

    pub fn validate_recorded_checkout(
        &self,
        commit: &str,
        exclude_generated_mcp: bool,
    ) -> anyhow::Result<()> {
        self.validate_commit_object(commit)?;
        self.validate_isolated_approval()?;
        if self.has_unexpected_checkout_changes(exclude_generated_mcp)?
            || current_revision(&self.path)? != commit
        {
            anyhow::bail!("recorded approval checkout changed after commit");
        }
        Ok(())
    }

    pub(super) fn unstage_generated_mcp(&self) -> anyhow::Result<()> {
        run_git(&self.path, &["reset", "--quiet", "HEAD", "--", ".mcp.json"])?;
        Ok(())
    }

    fn commit_history_touches_generated_mcp(&self, commit: &str) -> anyhow::Result<bool> {
        let base = run_git(&self.path, &["merge-base", commit, &self.base])?;
        let range = format!("{}..{commit}", base.trim());
        let paths = run_git(
            &self.path,
            &["log", "--format=", "--name-only", &range, "--", ".mcp.json"],
        )?;
        Ok(!paths.trim().is_empty())
    }

    fn has_unexpected_checkout_changes(&self, exclude_generated_mcp: bool) -> anyhow::Result<bool> {
        let raw = run_git(
            &self.path,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )?;
        if raw.is_empty() {
            return Ok(false);
        }
        if !exclude_generated_mcp {
            return Ok(true);
        }
        Ok(!raw
            .split('\0')
            .filter(|entry| !entry.is_empty())
            .all(|entry| entry.get(3..) == Some(".mcp.json")))
    }
}

fn canonical(path: &Path, label: &str) -> anyhow::Result<PathBuf> {
    std::fs::canonicalize(path).map_err(|error| anyhow::anyhow!("{label} is unavailable: {error}"))
}

fn registered_worktrees(repo: &Path) -> anyhow::Result<Vec<PathBuf>> {
    let raw = run_git(repo, &["worktree", "list", "--porcelain", "-z"])?;
    raw.split('\0')
        .filter_map(|field| field.strip_prefix("worktree "))
        .map(|path| canonical(Path::new(path), "registered worktree"))
        .collect()
}
