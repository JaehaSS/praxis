//! 코드 그래프 실행 세대와 활성 포인터.

use serde::Serialize;
use sqlx::SqlitePool;

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct RunStats {
    /// 상한에 잘리기 **전** 소스 수. `files_indexed`와 벌어지면 잘렸거나 건너뛴 것이다.
    pub files_seen: usize,
    /// 실제로 심볼을 만든 파일 수. `code_graph_runs`에는 열이 없어 저장되지 않는다.
    pub files_indexed: usize,
    /// `skip_reason`이 붙은 파일 수.
    pub files_skipped: usize,
    pub symbols: usize,
    pub edges: usize,
}

pub async fn start_run(
    pool: &SqlitePool,
    worktree: &str,
    fingerprint: &str,
    now: i64,
) -> anyhow::Result<i64> {
    let result = sqlx::query(
        "INSERT INTO code_graph_runs \
         (worktree, state, source_fingerprint, started_at) VALUES (?, 'indexing_symbols', ?, ?)",
    )
    .bind(worktree)
    .bind(fingerprint)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(result.last_insert_rowid())
}

pub async fn set_state(pool: &SqlitePool, run_id: i64, state: &str) -> anyhow::Result<()> {
    sqlx::query("UPDATE code_graph_runs SET state = ? WHERE id = ?")
        .bind(state)
        .bind(run_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn finish_failed(
    pool: &SqlitePool,
    run_id: i64,
    reason: &str,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE code_graph_runs SET state = 'degraded', failure_reason = ?, finished_at = ? \
         WHERE id = ?",
    )
    .bind(reason)
    .bind(now)
    .bind(run_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn finish_cancelled(
    pool: &SqlitePool,
    run_id: i64,
    reason: &str,
    now: i64,
) -> anyhow::Result<()> {
    sqlx::query(
        "UPDATE code_graph_runs SET state = 'cancelled', failure_reason = ?, finished_at = ? \
         WHERE id = ?",
    )
    .bind(reason)
    .bind(now)
    .bind(run_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn promote(
    pool: &SqlitePool,
    run_id: i64,
    stats: RunStats,
    now: i64,
) -> anyhow::Result<()> {
    let mut tx = pool.begin().await?;
    let (worktree,): (String,) =
        sqlx::query_as("SELECT worktree FROM code_graph_runs WHERE id = ?")
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await?;
    sqlx::query(
        "UPDATE code_graph_runs SET state='ready', finished_at=?, files_seen=?, \
         files_skipped=?, symbols=?, edges=?, failure_reason=NULL WHERE id=?",
    )
    .bind(now)
    .bind(stats.files_seen as i64)
    .bind(stats.files_skipped as i64)
    .bind(stats.symbols as i64)
    .bind(stats.edges as i64)
    .bind(run_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO code_graph_active (worktree, run_id) VALUES (?, ?) \
         ON CONFLICT(worktree) DO UPDATE SET run_id=excluded.run_id",
    )
    .bind(&worktree)
    .bind(run_id)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM code_graph_runs WHERE worktree=? AND id<>? AND id<>COALESCE( \
         (SELECT MAX(id) FROM code_graph_runs WHERE worktree=? AND state='ready' AND id<>?), -1)",
    )
    .bind(&worktree)
    .bind(run_id)
    .bind(&worktree)
    .bind(run_id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn active_run_id(pool: &SqlitePool, worktree: &str) -> anyhow::Result<Option<i64>> {
    let row: Option<(i64,)> =
        sqlx::query_as("SELECT run_id FROM code_graph_active WHERE worktree = ?")
            .bind(worktree)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(id,)| id))
}

pub async fn run_state(pool: &SqlitePool, run_id: i64) -> anyhow::Result<String> {
    let (state,): (String,) = sqlx::query_as("SELECT state FROM code_graph_runs WHERE id = ?")
        .bind(run_id)
        .fetch_one(pool)
        .await?;
    Ok(state)
}
