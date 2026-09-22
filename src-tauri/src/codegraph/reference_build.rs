//! 활성 후보 스냅샷의 심볼 위치를 언어 서버 참조로 연결한다.
//!
//! **준비된 서버의 파일만 들어온다**(설계 0065 DR-2b). 준비 전 pyright는 빈 배열이 아니라
//! 자기 선언 1건을 돌려주므로 — 그럴듯해 보이는 거짓이다 — 요청을 아예 보내지 않는 것이
//! 유일하게 안전한 처리다.

use std::path::Path;

use sqlx::SqlitePool;

use crate::lspclient::{GotoKind, LspPool};

use super::jobs::BuildGuard;
use super::{manifest, snapshot};

pub async fn link(
    pool: &SqlitePool,
    lsp: &LspPool,
    task_id: i64,
    worktree: &Path,
    run_id: i64,
    job: &BuildGuard,
    files: &[&manifest::ManifestFile],
) -> anyhow::Result<usize> {
    let mut edges = 0usize;
    for file in files {
        let text = std::fs::read_to_string(&file.abs_path)?;
        let nodes = snapshot::nodes_for_file(pool, run_id, &file.rel_path).await?;
        for node in nodes {
            ensure_not_cancelled(job)?;
            let targets = lsp
                .goto(
                    task_id,
                    worktree,
                    &file.rel_path,
                    &text,
                    node.sel_line as u32 + 1,
                    node.sel_char as u32 + 1,
                    GotoKind::References,
                )
                .await
                .map_err(anyhow::Error::msg)?;
            for target in targets {
                let Some(rel_path) = target.path else {
                    continue;
                };
                let line = target.line.saturating_sub(1);
                let character = target.column.saturating_sub(1);
                let Some(src) =
                    snapshot::find_node_at(pool, run_id, &rel_path, line, character).await?
                else {
                    continue;
                };
                edges += snapshot::add_edge(pool, run_id, src.id, node.id).await? as usize;
            }
        }
    }
    Ok(edges)
}

fn ensure_not_cancelled(job: &BuildGuard) -> anyhow::Result<()> {
    if job.is_cancelled() {
        anyhow::bail!("사용자가 취소했습니다");
    }
    Ok(())
}
