//! 제안(proposal)·목표 실행(goal run) 커맨드.
//!
//! `commands.rs`에서 갈라져 나왔다 — 모든 기능이 한 파일 끝에 줄을 붙이는
//! 구조가 병행 worktree의 머지 충돌을 만들었다.


use sqlx::SqlitePool;
use tauri::State;

use crate::db::{self};
use crate::selfimprove;

use super::{AppState, now, pool_of};

/// 자기개선 제안 목록 (pending_only=true면 미결정만).
#[tauri::command]
pub async fn proposal_list(
    state: State<'_, AppState>,
    pending_only: bool,
) -> Result<Vec<selfimprove::Proposal>, String> {
    let pool = pool_of(&state)?;
    selfimprove::list_proposals(&pool, pending_only)
        .await
        .map_err(|e| e.to_string())
}

/// 제안 승인 → 메모리로 승격.
#[tauri::command]
pub async fn proposal_apply(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let pool = pool_of(&state)?;
    selfimprove::apply_proposal(&pool, id, now())
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 제안 거부.
#[tauri::command]
pub async fn proposal_reject(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let pool = pool_of(&state)?;
    selfimprove::reject_proposal(&pool, id, now())
        .await
        .map_err(|e| e.to_string())
}

/// 적용 철회 — 제안이 만든 후보 지식을 보관으로 돌린다.
#[tauri::command]
pub async fn proposal_withdraw(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let pool = pool_of(&state)?;
    selfimprove::withdraw_proposal(&pool, id, now())
        .await
        .map_err(|e| e.to_string())
}

/// Run 하나와 그 사용량 (계획 0036).
///
/// 사용량은 저장돼 있지 않고 원장에서 파생하므로 조회 시점에 계산한다(DR-3).
/// Run 수가 많지 않아 N+1을 감수한다 — 캐시를 두면 원장과 어긋날 자리가 생긴다.
#[derive(serde::Serialize)]
pub struct GoalRunView {
    pub run: crate::goal_run::Run,
    pub spent: crate::goal_run::Spent,
    pub attempt_task_ids: Vec<i64>,
}

async fn goal_run_view(
    pool: &SqlitePool,
    run: crate::goal_run::Run,
    now_ts: i64,
) -> Result<GoalRunView, String> {
    let spent = crate::goal_run::spend::collect(pool, run.id, run.created_at, now_ts)
        .await
        .map_err(|e| e.to_string())?;
    let attempt_task_ids = crate::goal_run::attempt_task_ids(pool, run.id)
        .await
        .map_err(|e| e.to_string())?;
    Ok(GoalRunView {
        run,
        spent,
        attempt_task_ids,
    })
}

/// Goal Run 생성. 만들어진 즉시 `running`이고, 첫 시도는 다음 크론 틱(최대 60초)이 만든다.
#[tauri::command]
pub async fn goal_run_create(
    state: State<'_, AppState>,
    run: crate::goal_run::NewRun,
) -> Result<i64, String> {
    let pool = pool_of(&state)?;
    crate::goal_run::insert_run(&pool, &run, now())
        .await
        .map_err(|e| e.to_string())
}

/// Run 목록. `repo`가 있으면 그 레포만.
#[tauri::command]
pub async fn goal_run_list(
    state: State<'_, AppState>,
    repo: Option<String>,
) -> Result<Vec<GoalRunView>, String> {
    let pool = pool_of(&state)?;
    let runs = crate::goal_run::list_runs(&pool, repo.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    let now_ts = now();
    let mut views = Vec::with_capacity(runs.len());
    for run in runs {
        views.push(goal_run_view(&pool, run, now_ts).await?);
    }
    Ok(views)
}

#[tauri::command]
pub async fn goal_run_detail(
    state: State<'_, AppState>,
    id: i64,
) -> Result<Option<GoalRunView>, String> {
    let pool = pool_of(&state)?;
    let Some(run) = crate::goal_run::get_run(&pool, id)
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    goal_run_view(&pool, run, now()).await.map(Some)
}

/// 사용자 중단. 다음 틱부터 재진입하지 않는다. 이미 도는 시도는 건드리지 않는다 —
/// 그것을 죽이는 것은 작업 취소의 몫이고, 여기서 겸하면 두 개념이 섞인다.
#[tauri::command]
pub async fn goal_run_stop(state: State<'_, AppState>, id: i64) -> Result<(), String> {
    let pool = pool_of(&state)?;
    let stopped = crate::goal_run::stop_run(&pool, id, now())
        .await
        .map_err(|e| e.to_string())?;
    if !stopped {
        return Err("이미 종료된 Run입니다".into());
    }
    Ok(())
}

/// 사용자 호출형 반성 — 지금 이 작업을 회고해 검토용 제안을 만든다.
/// 자동 반성과 달리 capture opt-in 설정과 무관하게 동작한다(부른 사람이 비용을 감수한 것).
/// 반환: 생성된 제안 id. 회고할 내용이 없으면 None.
#[tauri::command]
pub async fn proposal_refine(
    state: State<'_, AppState>,
    task_id: i64,
) -> Result<Option<i64>, String> {
    let pool = pool_of(&state)?;
    let repo = db::get_task(&pool, task_id)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "작업을 찾을 수 없습니다".to_string())?
        .repo;
    // claude 셸아웃은 블로킹이다 — async 워커를 수십 초 점유하지 않도록 blocking 풀로 보낸다.
    tauri::async_runtime::spawn_blocking(move || {
        tauri::async_runtime::block_on(crate::capture::generate_convo_reflection(
            &pool,
            &repo,
            task_id,
            now(),
        ))
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

// ── Phase 7: MCP 레지스트리 ──

