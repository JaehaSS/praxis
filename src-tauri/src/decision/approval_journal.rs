use sqlx::SqlitePool;

use crate::db;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stage {
    Prepared,
    ProjectionRetired,
    Committed,
    Merged,
    Cleaned,
    Completed,
}

impl Stage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::ProjectionRetired => "projection_retired",
            Self::Committed => "committed",
            Self::Merged => "merged",
            Self::Cleaned => "cleaned",
            Self::Completed => "completed",
        }
    }

    fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "projection_retired" => Ok(Self::ProjectionRetired),
            "committed" => Ok(Self::Committed),
            "merged" => Ok(Self::Merged),
            "cleaned" => Ok(Self::Cleaned),
            "completed" => Ok(Self::Completed),
            _ => anyhow::bail!("unsupported local approval state: {value}"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FailureCode {
    ProjectionRetirementFailed,
    ProtectedPathChanged,
    GitCommitFailed,
    GitMergeFailed,
    GitCleanupFailed,
    LedgerCommitFailed,
}

impl FailureCode {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ProjectionRetirementFailed => "projection_retirement_failed",
            Self::ProtectedPathChanged => "protected_path_changed",
            Self::GitCommitFailed => "git_commit_failed",
            Self::GitMergeFailed => "git_merge_failed",
            Self::GitCleanupFailed => "git_cleanup_failed",
            Self::LedgerCommitFailed => "ledger_commit_failed",
        }
    }
}

#[derive(Clone, Debug, sqlx::FromRow)]
pub struct Journal {
    pub task_id: i64,
    pub state: String,
    pub commit_sha: Option<String>,
    pub exclude_generated_mcp: bool,
    pub failure_code: Option<String>,
}

impl Journal {
    pub fn stage(&self) -> anyhow::Result<Stage> {
        Stage::parse(&self.state)
    }
}

pub async fn claim(
    pool: &SqlitePool,
    task_id: i64,
    exclude_generated_mcp: bool,
    now: i64,
) -> anyhow::Result<db::Task> {
    super::approval_claim::claim(pool, task_id, exclude_generated_mcp, now, false).await
}

pub(super) async fn claim_enabled(
    pool: &SqlitePool,
    task_id: i64,
    exclude_generated_mcp: bool,
    now: i64,
) -> anyhow::Result<db::Task> {
    super::approval_claim::claim(pool, task_id, exclude_generated_mcp, now, true).await
}

pub async fn load(pool: &SqlitePool, task_id: i64) -> anyhow::Result<Journal> {
    let journal: Journal = sqlx::query_as(
        "SELECT task_id, state, commit_sha, exclude_generated_mcp, failure_code \
         FROM local_approval_finalizations WHERE task_id = ?",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await?;
    journal.stage()?;
    if let Some(commit) = &journal.commit_sha {
        validate_commit_sha(commit)?;
    }
    Ok(journal)
}

pub async fn stage(
    pool: &SqlitePool,
    task_id: i64,
    stage: Stage,
    commit_sha: Option<&str>,
    now: i64,
) -> anyhow::Result<()> {
    if stage == Stage::Completed {
        anyhow::bail!("completed stage is owned by atomic ledger completion");
    }
    if let Some(commit) = commit_sha {
        validate_commit_sha(commit)?;
    }
    let updated = sqlx::query(
        "UPDATE local_approval_finalizations SET state = ?, \
         commit_sha = COALESCE(?, commit_sha), failure_code = NULL, updated_at = ? \
         WHERE task_id = ? AND state != 'completed'",
    )
    .bind(stage.as_str())
    .bind(commit_sha)
    .bind(now)
    .bind(task_id)
    .execute(pool)
    .await?;
    if updated.rows_affected() != 1 {
        anyhow::bail!("local approval journal cannot advance");
    }
    Ok(())
}

pub async fn failure(
    pool: &SqlitePool,
    task_id: i64,
    code: FailureCode,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE local_approval_finalizations SET failure_code = ?, updated_at = ? \
         WHERE task_id = ? AND state != 'completed'",
    )
    .bind(code.as_str())
    .bind(now)
    .bind(task_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn finalizing_ids(pool: &SqlitePool) -> anyhow::Result<Vec<i64>> {
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT journal.task_id FROM local_approval_finalizations journal \
         JOIN tasks task ON task.id = journal.task_id \
         WHERE journal.state != 'completed' AND task.state = ? ORDER BY journal.task_id",
    )
    .bind(db::state::FINALIZING)
    .fetch_all(pool)
    .await?;
    Ok(ids)
}

pub async fn has_incomplete(pool: &SqlitePool, task_id: i64) -> anyhow::Result<bool> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM local_approval_finalizations \
         WHERE task_id = ? AND state != 'completed'",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

pub fn validate_commit_sha(value: &str) -> anyhow::Result<()> {
    let valid_length = matches!(value.len(), 40 | 64);
    let lowercase_hex = value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !valid_length || !lowercase_hex {
        anyhow::bail!("commit identity must be a full hexadecimal commit SHA");
    }
    Ok(())
}
