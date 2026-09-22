//! 새 스냅샷에 심볼·참조를 만들고 검증 후 원자 승격한다.
//!
//! 파일은 담당 서버별로 묶여 처리된다. 한 서버를 못 써도 나머지 언어의 그래프는 그대로
//! 만들어진다 — 부분 가용성은 정상이다(설계 0065 DR-3).

use std::path::Path;

use serde::Serialize;
use sqlx::SqlitePool;

use crate::lspclient::server::{self, ServerSpec};
use crate::lspclient::{LspPool, Readiness};

use super::generation::{self, RunStats};
use super::jobs::BuildGuard;
use super::manifest::ManifestFile;
use super::{manifest, snapshot};

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BuildReport {
    pub run_id: i64,
    pub state: String,
    pub files_seen: usize,
    pub files_indexed: usize,
    pub files_unchanged: usize,
    pub files_skipped: usize,
    pub symbols: usize,
    pub edges: usize,
}

pub async fn index_worktree(
    pool: &SqlitePool,
    lsp: &LspPool,
    task_id: i64,
    worktree: &Path,
    job: &BuildGuard,
    now: i64,
) -> anyhow::Result<BuildReport> {
    let before = manifest::scan(worktree)?;
    let worktree_key = worktree.to_string_lossy();
    let run_id = generation::start_run(pool, &worktree_key, &before.fingerprint, now).await?;
    let result = build_snapshot(pool, lsp, task_id, worktree, run_id, job, &before).await;
    let stats = match result {
        Ok(stats) => stats,
        Err(error) => return fail_run(pool, run_id, job, error, now).await,
    };
    if job.is_cancelled() {
        return fail_run(
            pool,
            run_id,
            job,
            anyhow::anyhow!("사용자가 취소했습니다"),
            now,
        )
        .await;
    }
    let after = match manifest::scan(worktree) {
        Ok(manifest) => manifest,
        Err(error) => return fail_run(pool, run_id, job, error, now).await,
    };
    finalize_snapshot(pool, run_id, stats, &before, &after, job, now).await?;
    Ok(BuildReport {
        run_id,
        state: "ready".to_string(),
        files_seen: stats.files_seen,
        files_indexed: stats.files_indexed,
        files_unchanged: 0,
        files_skipped: stats.files_skipped,
        symbols: stats.symbols,
        edges: stats.edges,
    })
}

pub(crate) async fn finalize_snapshot(
    pool: &SqlitePool,
    run_id: i64,
    stats: RunStats,
    before: &manifest::SourceManifest,
    after: &manifest::SourceManifest,
    job: &BuildGuard,
    now: i64,
) -> anyhow::Result<()> {
    if before.fingerprint != after.fingerprint {
        let error = anyhow::anyhow!("인덱싱 중 소스가 변경되어 새 세대를 활성화하지 않았습니다");
        generation::finish_cancelled(pool, run_id, &error.to_string(), now).await?;
        return Err(error);
    }
    if !job.begin_promotion() {
        let error = anyhow::anyhow!("사용자가 취소했습니다");
        generation::finish_cancelled(pool, run_id, &error.to_string(), now).await?;
        return Err(error);
    }
    match generation::promote(pool, run_id, stats, now).await {
        Ok(()) => Ok(()),
        Err(promotion_error) => {
            let promotion_message = promotion_error.to_string();
            match generation::finish_failed(pool, run_id, &promotion_message, now).await {
                Ok(()) => Err(promotion_error),
                Err(terminalization_error) => Err(anyhow::anyhow!(
                    "코드 그래프 승격 실패: {promotion_error}; 후보 세대 실패 기록도 실패: {terminalization_error}"
                )),
            }
        }
    }
}

/// 한 언어 서버가 맡는 파일들. `unavailable`이 차 있으면 그 서버를 쓸 수 없다는 뜻이고,
/// 그룹 전체가 `skip_reason`을 달고 넘어간다 — 런은 실패하지 않는다.
pub(crate) struct ServerGroup<'a> {
    pub spec: ServerSpec,
    pub files: Vec<&'a ManifestFile>,
    pub unavailable: Option<String>,
}

pub(crate) fn group_by_server<'a>(
    source: &'a manifest::SourceManifest,
    worktree: &Path,
) -> Vec<ServerGroup<'a>> {
    let mut groups: Vec<ServerGroup<'a>> = Vec::new();
    for file in &source.files {
        if let Some(group) = groups.iter_mut().find(|g| g.spec.key == file.spec_key) {
            group.files.push(file);
            continue;
        }
        let Some((spec, _)) = server::spec_for_path(Path::new(&file.rel_path)) else {
            continue;
        };
        groups.push(ServerGroup {
            unavailable: server::unavailable_reason(&spec, worktree),
            spec,
            files: vec![file],
        });
    }
    groups
}

async fn build_snapshot(
    pool: &SqlitePool,
    lsp: &LspPool,
    task_id: i64,
    worktree: &Path,
    run_id: i64,
    job: &BuildGuard,
    source: &manifest::SourceManifest,
) -> anyhow::Result<RunStats> {
    // `files_seen`은 상한에 잘리기 **전** 개수다 — 잘린 사실은 `files_indexed`와의 차이로 남는다.
    let mut stats = RunStats {
        files_seen: source.files_total,
        ..Default::default()
    };
    let groups = group_by_server(source, worktree);
    // 소스가 하나도 없으면 빈 그래프가 정답이다(예전부터의 동작).
    if groups.is_empty() {
        return Ok(stats);
    }
    if groups.iter().all(|group| group.unavailable.is_some()) {
        anyhow::bail!(unavailable_summary(&groups));
    }
    warm_semantic_index(lsp, task_id, worktree, job, &groups).await?;
    for group in &groups {
        collect_symbols(pool, lsp, task_id, worktree, run_id, job, group, &mut stats).await?;
    }
    link_edges(
        pool, lsp, task_id, worktree, run_id, job, &groups, &mut stats,
    )
    .await?;
    Ok(stats)
}

/// 각 서버의 첫 파일을 미리 열어 프로세스를 띄운다 — 준비 래치가 심볼 수집과 겹쳐 쌓인다.
/// **여기서 기다리지 않는다**(DR-2b). 실패는 무시한다 — 같은 파일을 심볼 루프가 다시 열고
/// 그때 사유가 `skip_reason`으로 남는다.
async fn warm_semantic_index(
    lsp: &LspPool,
    task_id: i64,
    worktree: &Path,
    job: &BuildGuard,
    groups: &[ServerGroup<'_>],
) -> anyhow::Result<()> {
    for group in groups {
        ensure_not_cancelled(job)?;
        if group.unavailable.is_some() {
            continue;
        }
        if let Some(first) = group.files.first() {
            let _ = lsp
                .document_symbols(task_id, worktree, &first.rel_path)
                .await;
        }
    }
    Ok(())
}

/// 파일 하나의 `document_symbols` 실패는 그 파일만 건너뛴다. 서버 하나가 화를 냈다고
/// 나머지 수천 개를 버릴 이유가 없다(DR-3).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn collect_symbols(
    pool: &SqlitePool,
    lsp: &LspPool,
    task_id: i64,
    worktree: &Path,
    run_id: i64,
    job: &BuildGuard,
    group: &ServerGroup<'_>,
    stats: &mut RunStats,
) -> anyhow::Result<()> {
    for file in &group.files {
        ensure_not_cancelled(job)?;
        let symbols = match &group.unavailable {
            Some(reason) => Err(reason.clone()),
            None => {
                lsp.document_symbols(task_id, worktree, &file.rel_path)
                    .await
            }
        };
        let file_id = snapshot::insert_file(
            pool,
            run_id,
            &file.rel_path,
            &file.content_hash,
            file.lang,
            symbols.as_ref().err().map(String::as_str),
            None,
        )
        .await?;
        let Ok(symbols) = symbols else {
            stats.files_skipped += 1;
            continue;
        };
        for symbol in &symbols {
            snapshot::insert_node(pool, run_id, file_id, symbol).await?;
        }
        stats.files_indexed += 1;
        stats.symbols += symbols.len();
    }
    Ok(())
}

/// 준비 대기는 **여기서만** 한다(DR-2b). 심볼 루프 앞에 두면 `Unsupported` 서버에서 상한까지
/// 기다리다 실패해 엣지뿐 아니라 심볼까지 전부 잃는다.
#[allow(clippy::too_many_arguments)]
async fn link_edges(
    pool: &SqlitePool,
    lsp: &LspPool,
    task_id: i64,
    worktree: &Path,
    run_id: i64,
    job: &BuildGuard,
    groups: &[ServerGroup<'_>],
    stats: &mut RunStats,
) -> anyhow::Result<()> {
    generation::set_state(pool, run_id, "waiting_semantic").await?;
    let mut ready: Vec<&ManifestFile> = Vec::new();
    for group in groups {
        ensure_not_cancelled(job)?;
        // 심볼조차 없는 그룹에는 `edge_state`를 적지 않는다 — `skip_reason`이 이미 말한다.
        if group.unavailable.is_some() {
            continue;
        }
        match lsp.wait_semantic_ready(task_id, worktree, group.spec).await {
            Readiness::Ready => ready.extend(group.files.iter().copied()),
            Readiness::Unsupported(reason) | Readiness::Failed(reason) => {
                mark_edge_state(pool, run_id, group, &reason).await?;
            }
        }
    }
    if ready.is_empty() {
        return Ok(());
    }
    generation::set_state(pool, run_id, "indexing_edges").await?;
    stats.edges =
        super::reference_build::link(pool, lsp, task_id, worktree, run_id, job, &ready).await?;
    Ok(())
}

/// 심볼은 있으나 엣지를 만들지 않은 파일에 사유를 적는다. `skip_reason`이 이미 붙은 파일은
/// 건드리지 않는다 — 두 컬럼은 다른 것을 말한다(DR-3b).
async fn mark_edge_state(
    pool: &SqlitePool,
    run_id: i64,
    group: &ServerGroup<'_>,
    reason: &str,
) -> anyhow::Result<()> {
    for file in &group.files {
        sqlx::query(
            "UPDATE code_graph_files SET edge_state = ? \
             WHERE run_id = ? AND rel_path = ? AND skip_reason IS NULL",
        )
        .bind(reason)
        .bind(run_id)
        .bind(&file.rel_path)
        .execute(pool)
        .await?;
    }
    Ok(())
}

fn unavailable_summary(groups: &[ServerGroup<'_>]) -> String {
    let reasons: Vec<&str> = groups
        .iter()
        .filter_map(|group| group.unavailable.as_deref())
        .collect();
    format!(
        "사용할 수 있는 언어 서버가 없습니다 — {}",
        reasons.join("; ")
    )
}

fn ensure_not_cancelled(job: &BuildGuard) -> anyhow::Result<()> {
    if job.is_cancelled() {
        anyhow::bail!("사용자가 취소했습니다");
    }
    Ok(())
}

async fn fail_run<T>(
    pool: &SqlitePool,
    run_id: i64,
    job: &BuildGuard,
    error: anyhow::Error,
    now: i64,
) -> anyhow::Result<T> {
    if job.is_cancelled() {
        generation::finish_cancelled(pool, run_id, &error.to_string(), now).await?;
    } else {
        generation::finish_failed(pool, run_id, &error.to_string(), now).await?;
    }
    Err(error)
}
