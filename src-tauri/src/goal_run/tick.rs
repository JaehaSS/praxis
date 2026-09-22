//! Goal Run 재진입 구동 (계획 0036 DR-2).
//!
//! 새 상주 루프를 만들지 않고 크론 틱(`schedule::runner::tick_loop`)에 얹는다. 그 루프는
//! "틱당 최대 1회"라는 멱등 불변식이 구조로 보장돼 있고(`schedule/runner.rs:34-37`), 이미
//! `goal_contract`를 실어 태스크를 만든다. 같은 성질이 여기에도 필요하다.
//!
//! 오케스트레이션 계층이라 `AppHandle`/`AppState`에 의존한다 — 판정 로직은 `super::decide`에
//! 순수 함수로 두고 여기서는 조회·부작용만 맡는다.

use std::path::PathBuf;

use tauri::Manager;

use crate::commands::{self, AppState};
use crate::db;
use crate::goal_run::decide::{attempt_of_state, decide, Attempt, Decision, GateVerdict};
use crate::goal_run::{self, status};
use crate::verify;

/// 활성 Run을 한 번씩 평가한다. 개별 Run의 실패는 로그 후 skip — 하나가 넘어져도
/// 나머지 Run과 크론 스케줄은 계속 돈다(크론의 best-effort 관례와 같다).
pub async fn run_due_goal_runs(app: &tauri::AppHandle, pool: &sqlx::SqlitePool) {
    let runs = match goal_run::list_active_runs(pool).await {
        Ok(runs) => runs,
        Err(e) => {
            eprintln!("goal_run 목록 조회 실패: {e}");
            return;
        }
    };
    for run in runs {
        evaluate_one(app, pool, &run).await;
    }
}

async fn evaluate_one(app: &tauri::AppHandle, pool: &sqlx::SqlitePool, run: &goal_run::Run) {
    let now = crate::now();
    let spent = match goal_run::spend::collect(pool, run.id, run.created_at, now).await {
        Ok(spent) => spent,
        Err(e) => {
            eprintln!("goal_run #{} 사용량 집계 실패 — 이번 틱 skip: {e}", run.id);
            return;
        }
    };
    let (attempt, gate) = match observe_attempt(pool, run.id, now).await {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("goal_run #{} 시도 관측 실패 — 이번 틱 skip: {e}", run.id);
            return;
        }
    };
    match decide(&run.budget, &spent, attempt, gate) {
        Decision::Wait => {}
        Decision::Reenter => spawn_attempt(app, pool, run, now).await,
        Decision::Satisfy => end(pool, run.id, status::SATISFIED, "증거 게이트 통과", now).await,
        Decision::Exhaust(reason) => {
            end(pool, run.id, status::EXHAUSTED, reason.as_str(), now).await
        }
        Decision::Stop(why) => end(pool, run.id, status::STOPPED, why, now).await,
        Decision::HandOff => {
            end(
                pool,
                run.id,
                status::STOPPED,
                "검증 커맨드가 없어 자동 판정할 수 없습니다 — 사람이 확인해야 합니다",
                now,
            )
            .await
        }
    }
}

/// 직전 시도의 처지와 게이트 판정을 읽는다. 끝난 시도인데 아직 평가하지 않았으면 **여기서
/// 한 번** 평가하고 기록한다.
async fn observe_attempt(
    pool: &sqlx::SqlitePool,
    run_id: i64,
    now: i64,
) -> anyhow::Result<(Attempt, Option<GateVerdict>)> {
    let Some(row) = goal_run::latest_attempt(pool, run_id).await? else {
        return Ok((Attempt::None, None));
    };
    // 태스크가 사라졌으면(수동 삭제 등) 판정할 근거가 없다. 진행 중으로 보아 기다리면
    // Run이 영원히 멈추므로, 끝난 것으로 보되 증거는 없다고 알린다.
    let Some(task) = db::get_task(pool, row.task_id).await? else {
        return Ok((Attempt::Settled, None));
    };
    let attempt = attempt_of_state(&task.state);
    if attempt != Attempt::Settled {
        return Ok((attempt, None));
    }
    if row.gate_evaluated() {
        return Ok((attempt, row.gate_ready.map(GateVerdict::from_ready)));
    }
    let ready = evaluate_gate(task.worktree_path.clone()).await;
    goal_run::record_gate(pool, row.task_id, ready, now).await?;
    Ok((attempt, ready.map(GateVerdict::from_ready)))
}

/// 워크트리에서 빌드·테스트를 돌려 게이트를 판정한다. `None`은 **검증 커맨드를 찾지 못했다**는
/// 뜻이지 실패가 아니다 — 그 구분이 없으면 검증 설정이 없는 프로젝트에서 무한 재시도가 된다.
///
/// 블로킹 실행이라 `spawn_blocking`으로 옮긴다. 그래도 이 await 동안 크론 틱은 멈추는데,
/// 이는 시도당 1회이고 `is_due`가 "1회 이상 도래" 여부만 보므로 밀린 스케줄이 누락되지는
/// 않는다(`schedule/runner.rs:34-37`).
async fn evaluate_gate(worktree_path: String) -> Option<bool> {
    tokio::task::spawn_blocking(move || {
        let root = PathBuf::from(worktree_path);
        if !root.is_dir() {
            return None;
        }
        let spec = verify::detect_spec(&root);
        if spec.build.is_none() && spec.test.is_none() {
            return None;
        }
        let build = spec
            .build
            .as_ref()
            .map(|command| verify::run_check(&root, command, spec.timeout_secs));
        let tests = spec
            .test
            .as_ref()
            .map(|command| verify::run_check(&root, command, spec.timeout_secs));
        let test_summary = tests
            .as_ref()
            .and_then(|result| verify::parse_test_summary(&result.tail));
        Some(
            verify::gate(&verify::EvidenceBundle {
                build,
                tests,
                test_summary,
                changed_files: vec![],
                created_at: 0,
            })
            .ready,
        )
    })
    .await
    .unwrap_or(None)
}

/// 다음 시도를 만든다.
///
/// 크론은 부작용 전에 `mark_schedule_ran`을 먼저 쓰지만(`schedule/runner.rs:63-65`) 여기서는
/// **그럴 수 없다** — attempt 행은 `task_id`를 담는데 그 값이 태스크를 만들어야 나온다.
///
/// 그래서 순서가 뒤집힌 대가를 따로 막는다. 기록이 실패하면 그 태스크는 Run에 묶이지 않고,
/// 다음 틱은 이전 시도를 최신으로 보아 **또 만든다.** attempt 행이 안 늘어 예산도 소모되지
/// 않으므로 무한 재생성이다. 이 기능의 최악 시나리오라, 기록 실패는 로그로 넘기지 않고
/// **Run을 즉시 정지**시킨다. 묶이지 않은 태스크 하나는 승인 대기로 남아 사람이 처리한다.
async fn spawn_attempt(
    app: &tauri::AppHandle,
    pool: &sqlx::SqlitePool,
    run: &goal_run::Run,
    now: i64,
) {
    let state = app.state::<AppState>();
    let instruction = next_instruction(pool, run).await;
    let mut params = commands::CreateTaskParams::headless_terminal(
        run.repo.clone(),
        instruction,
        run.agent.clone(),
        // 자율 재진입도 승인을 우회하지 않는다 (DR-5). 예산은 비용을 막지 손상을 막지 못한다.
        commands::TaskOrigin::External,
    );
    params.goal_contract = Some(run.goal_contract.clone());
    match commands::create_task_internal(app, &state, params).await {
        Ok(task) => {
            if let Err(e) = goal_run::record_attempt(pool, run.id, task.id, now).await {
                eprintln!(
                    "goal_run #{} 시도 기록 실패 — 작업 #{}가 Run에 묶이지 않아 정지합니다: {e}",
                    run.id, task.id
                );
                end(
                    pool,
                    run.id,
                    status::STOPPED,
                    "시도 기록에 실패했습니다 — 중복 생성을 막기 위해 정지했습니다",
                    now,
                )
                .await;
            }
        }
        Err(e) => eprintln!("goal_run #{} 작업 생성 실패: {e}", run.id),
    }
}

/// 재시도 지시문. 직전 시도의 **결정적 증거만** 덧붙인다 — 자유 텍스트 소감을 나르면
/// DR-1이 막은 자리(자연어를 판정 재료로 쓰는 것)로 뒷문이 열린다.
async fn next_instruction(pool: &sqlx::SqlitePool, run: &goal_run::Run) -> String {
    let Ok(Some(row)) = goal_run::latest_attempt(pool, run.id).await else {
        return run.instruction.clone();
    };
    let failed = row.gate_ready == Some(false);
    if !failed {
        return run.instruction.clone();
    }
    format!(
        "{}\n\n---\n직전 시도(작업 #{})의 검증이 통과하지 못했습니다. \
         빌드·테스트가 통과하도록 이어서 작업하세요.",
        run.instruction, row.task_id
    )
}

async fn end(pool: &sqlx::SqlitePool, id: i64, new_status: &str, reason: &str, now: i64) {
    if let Err(e) = goal_run::end_run(pool, id, new_status, reason, now).await {
        eprintln!("goal_run #{id} 종료 기록 실패: {e}");
    }
}
