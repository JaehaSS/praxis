//! 활성 코드 그래프 freshness와 최신 빌드 상태.

use std::path::Path;

use serde::Serialize;
use sqlx::{Executor, Sqlite, SqlitePool};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodeGraphStatus {
    pub active_state: String,
    pub active_run_id: Option<i64>,
    pub indexed_at: Option<i64>,
    pub files: usize,
    pub symbols: usize,
    pub edges: usize,
    pub build_state: String,
    pub build_run_id: Option<i64>,
    pub detail: Option<String>,
    /// 완결성. `None`이면 활성 그래프가 온전하다.
    pub incomplete: Option<Incompleteness>,
}

/// 완결성은 신선도와 **직교하는 축**이다(설계 0065 DR-6). `active_state`는 "fingerprint가 현재
/// 소스와 일치하는가"만 답하므로, **최신이면서 불완전한 그래프가 정상 상태다.** 저장 컬럼이 아니라
/// 활성 run의 `code_graph_files`를 집계해 내는 파생값이다.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Incompleteness {
    /// 심볼조차 만들지 못한 파일 수(`skip_reason`).
    pub files_skipped: usize,
    /// 심볼은 있으나 엣지를 만들지 않은 파일 수(`edge_state`).
    pub files_without_edges: usize,
    /// 엣지가 빠진 `languageId`들. 사전순.
    pub languages_without_edges: Vec<String>,
    /// 언어별 사유.
    pub detail: String,
}

type ActiveRow = (i64, String, i64, i64, i64, i64);
type BuildRow = (i64, String, Option<String>);
/// `(lang, skip_reason 개수, edge_state 개수, 대표 사유)`.
type IncompleteRow = (String, i64, i64, String);

/// 폴링 경로다 — 파일 수만큼 행을 끌어오지 않고 언어당 한 행으로 접는다.
/// `code_graph_files_run_path`가 `run_id` 선두라 이 필터가 그 인덱스를 탄다.
const INCOMPLETE_SQL: &str = "\
SELECT lang, sum(skip_reason IS NOT NULL), sum(edge_state IS NOT NULL), \
       min(coalesce(edge_state, skip_reason)) \
  FROM code_graph_files \
 WHERE run_id=? AND (skip_reason IS NOT NULL OR edge_state IS NOT NULL) \
 GROUP BY lang ORDER BY lang";

pub async fn load(pool: &SqlitePool, worktree: &Path) -> anyhow::Result<CodeGraphStatus> {
    let worktree_key = worktree.to_string_lossy();
    let active: Option<ActiveRow> = sqlx::query_as(
        "SELECT r.id, r.source_fingerprint, r.finished_at, r.files_seen, r.symbols, r.edges \
         FROM code_graph_active a JOIN code_graph_runs r ON r.id=a.run_id \
         WHERE a.worktree=?",
    )
    .bind(worktree_key.as_ref())
    .fetch_optional(pool)
    .await?;
    let latest: Option<BuildRow> = sqlx::query_as(
        "SELECT id, state, failure_reason FROM code_graph_runs \
         WHERE worktree=? ORDER BY id DESC LIMIT 1",
    )
    .bind(worktree_key.as_ref())
    .fetch_optional(pool)
    .await?;
    let (current_fingerprint, incomplete) = match active.as_ref() {
        None => (String::new(), None),
        Some(row) => (
            super::manifest::scan(worktree)?.fingerprint,
            incompleteness(pool, row.0).await?,
        ),
    };
    Ok(assemble(active, latest, &current_fingerprint, incomplete))
}

pub(crate) async fn incompleteness<'e, E>(
    executor: E,
    run_id: i64,
) -> anyhow::Result<Option<Incompleteness>>
where
    E: Executor<'e, Database = Sqlite>,
{
    let rows: Vec<IncompleteRow> = sqlx::query_as(INCOMPLETE_SQL)
        .bind(run_id)
        .fetch_all(executor)
        .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut incomplete = Incompleteness {
        files_skipped: 0,
        files_without_edges: 0,
        languages_without_edges: Vec::new(),
        detail: String::new(),
    };
    let mut details = Vec::with_capacity(rows.len());
    for (lang, skipped, without_edges, reason) in rows {
        incomplete.files_skipped += skipped as usize;
        incomplete.files_without_edges += without_edges as usize;
        details.push(format!("{lang}: {reason}"));
        if without_edges > 0 {
            incomplete.languages_without_edges.push(lang);
        }
    }
    incomplete.detail = details.join(" · ");
    Ok(Some(incomplete))
}

fn assemble(
    active: Option<ActiveRow>,
    latest: Option<BuildRow>,
    current_fingerprint: &str,
    incomplete: Option<Incompleteness>,
) -> CodeGraphStatus {
    let active_id = active.as_ref().map(|row| row.0);
    let (build_state, build_run_id, detail) = match latest {
        Some((id, state, _)) if Some(id) == active_id && state == "ready" => {
            ("idle".to_string(), None, None)
        }
        Some((id, state, detail)) => (state, Some(id), detail),
        None => ("idle".to_string(), None, None),
    };
    let Some((run_id, fingerprint, indexed_at, files, symbols, edges)) = active else {
        return CodeGraphStatus {
            active_state: "absent".to_string(),
            active_run_id: None,
            indexed_at: None,
            files: 0,
            symbols: 0,
            edges: 0,
            build_state,
            build_run_id,
            detail,
            incomplete: None,
        };
    };
    CodeGraphStatus {
        active_state: if fingerprint == current_fingerprint {
            "ready"
        } else {
            "stale"
        }
        .into(),
        active_run_id: Some(run_id),
        indexed_at: Some(indexed_at),
        files: files as usize,
        symbols: symbols as usize,
        edges: edges as usize,
        build_state,
        build_run_id,
        detail,
        incomplete,
    }
}
