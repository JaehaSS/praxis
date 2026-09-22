//! Approval observation, separate from the existing durable finalization journals.
pub mod repair;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{
    db,
    worktree::{readiness::Readiness, Worktree},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub attempt_id: i64,
    pub task_id: i64,
    pub ts: i64,
    pub stage: String,
    pub outcome: String,
    pub base: String,
    pub source_sha: Option<String>,
    pub target_sha: Option<String>,
    pub direct: bool,
    pub error: Option<String>,
}

impl Attempt {
    pub async fn start(pool: &SqlitePool, task: &db::Task) -> anyhow::Result<Self> {
        let mut attempt = Self {
            attempt_id: 0,
            task_id: task.id,
            ts: chrono::Utc::now().timestamp(),
            stage: "admission".into(),
            outcome: "started".into(),
            base: task.base.clone(),
            source_sha: revision(&task.worktree_path, "HEAD"),
            target_sha: revision(&task.repo, &format!("refs/heads/{}", task.base)),
            direct: task.repo == task.worktree_path,
            error: None,
        };
        let row = sqlx::query(
            "INSERT INTO task_events(task_id,ts,kind,detail) VALUES(?,?,'approval_attempt',?)",
        )
        .bind(task.id)
        .bind(attempt.ts)
        .bind(serde_json::to_string(&attempt)?)
        .execute(pool)
        .await?;
        attempt.attempt_id = row.last_insert_rowid();
        Ok(attempt)
    }

    pub async fn finish(&mut self, pool: &SqlitePool, result: &Result<(), String>) {
        self.ts = chrono::Utc::now().timestamp();
        self.outcome = if result.is_ok() {
            "succeeded"
        } else {
            "failed"
        }
        .into();
        self.error = result
            .as_ref()
            .err()
            .map(|error| error.chars().take(4000).collect());
        let saved = async {
            db::append_event(
                pool,
                self.task_id,
                "approval_outcome",
                Some(&serde_json::to_string(self)?),
                self.ts,
            )
            .await
        }
        .await;
        // Never turn a completed merge into a retryable failure due to telemetry loss.
        if let Err(error) = saved {
            eprintln!("승인 결과 이력 저장 실패 (task {}): {error}", self.task_id);
        }
    }
}

fn revision(path: &str, reference: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .current_dir(path)
        .args(["rev-parse", "--verify", reference])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub async fn history(pool: &SqlitePool, id: i64) -> anyhow::Result<Vec<Attempt>> {
    let rows: Vec<db::TaskEvent> = sqlx::query_as(
        "SELECT * FROM task_events WHERE task_id=? AND kind IN ('approval_attempt','approval_outcome') ORDER BY id DESC LIMIT 40")
        .bind(id).fetch_all(pool).await?;
    let mut results = vec![];
    let mut seen = std::collections::HashSet::new();
    for row in rows {
        let mut item: Attempt = serde_json::from_str(row.detail.as_deref().unwrap_or(""))?;
        if item.attempt_id == 0 {
            item.attempt_id = row.id;
        }
        if seen.insert(item.attempt_id) {
            results.push(item);
        }
        if results.len() == 5 {
            break;
        }
    }
    Ok(results)
}

#[derive(Serialize)]
pub struct Status {
    pub readiness: Option<Readiness>,
    pub inspection_error: Option<String>,
    pub attempts: Vec<Attempt>,
}

pub async fn inspect(
    pool: &SqlitePool,
    task: &db::Task,
    worktree: Worktree,
) -> anyhow::Result<Status> {
    let observed = tokio::task::spawn_blocking(move || worktree.approval_readiness()).await?;
    let (readiness, inspection_error) = match observed {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    Ok(Status {
        readiness,
        inspection_error,
        attempts: history(pool, task.id).await?,
    })
}
