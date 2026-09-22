//! Goal Run 테스트 (계획 0036).

use std::sync::atomic::{AtomicU32, Ordering};

use super::decide::{attempt_of_state, decide, Attempt, Decision, GateVerdict};
use super::spend::{from_events, Spend};
use super::*;
use crate::db::state as tstate;

static DATABASE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// 임시 파일 DB — `sqlite::memory:`는 풀의 커넥션마다 별개의 빈 DB를 본다
/// (`knowledge/tests/mod.rs:20-27`과 같은 관례).
async fn test_pool() -> SqlitePool {
    let sequence = DATABASE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = crate::testtmp::dir().join(format!(
        "praxis-goal-run-{}-{sequence}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = crate::db::init_pool(path.to_str().unwrap()).await.unwrap();
    migrate(&pool).await.unwrap();
    pool
}

fn contract() -> GoalContract {
    GoalContract {
        schema_version: crate::goal_contract::SCHEMA_VERSION,
        objective: "테스트 목표".into(),
        acceptance: vec![],
        stop_conditions: vec![],
        must_preserve: vec![],
        protected_paths: vec![],
        non_goals: vec![],
    }
}

fn budget() -> Budget {
    Budget {
        max_attempts: 3,
        max_tokens: 100_000,
        max_cost_usd: 5.0,
        max_wall_secs: 3600,
    }
}

fn new_run() -> NewRun {
    NewRun {
        repo: "/tmp/repo".into(),
        agent: "claude".into(),
        instruction: "테스트 지시".into(),
        goal_contract: contract(),
        budget: budget(),
    }
}

fn spent_low() -> Spent {
    Spent {
        attempts: 1,
        tokens: 10,
        cost_usd: 0.01,
        elapsed_secs: 5,
    }
}

// ─────────────────────────── Budget ───────────────────────────

#[test]
fn budget_rejects_all_zero() {
    let b = Budget {
        max_attempts: 0,
        max_tokens: 0,
        max_cost_usd: 0.0,
        max_wall_secs: 0,
    };
    assert!(
        b.validate().is_err(),
        "예산이 전부 0이면 정지 조건이 없어 무한히 돈다"
    );
}

#[test]
fn budget_accepts_partial_limits() {
    let b = Budget {
        max_attempts: 5,
        max_tokens: 0,
        max_cost_usd: 0.0,
        max_wall_secs: 0,
    };
    assert!(b.validate().is_ok(), "0인 항목은 무제한을 뜻한다");
}

#[test]
fn budget_rejects_negative() {
    let b = Budget {
        max_attempts: -1,
        ..budget()
    };
    assert!(b.validate().is_err());
}

#[test]
fn budget_rejects_non_finite_cost() {
    let b = Budget {
        max_cost_usd: f64::NAN,
        ..budget()
    };
    assert!(
        b.validate().is_err(),
        "NaN은 어떤 비교에도 false라 상한이 되지 못한다"
    );
}

// ─────────────────────────── exhausted ───────────────────────────

#[test]
fn exhausted_when_attempts_run_out() {
    let spent = Spent {
        attempts: 3,
        ..spent_low()
    };
    assert_eq!(exhausted(&budget(), &spent), Some(ExhaustReason::Attempts));
}

#[test]
fn zero_budget_field_does_not_participate() {
    let b = Budget {
        max_attempts: 0,
        max_tokens: 1000,
        max_cost_usd: 0.0,
        max_wall_secs: 0,
    };
    let spent = Spent {
        attempts: 9_999,
        tokens: 10,
        cost_usd: 9_999.0,
        elapsed_secs: 99_999,
    };
    assert_eq!(exhausted(&b, &spent), None);
}

#[test]
fn attempts_budget_is_the_backstop_when_ledger_is_empty() {
    // 크래시로 Result가 원장에 없으면 토큰·비용이 0으로 계상된다(DR-3 Unknown).
    // 시도 수는 우리 테이블에서 세므로 그때도 증가한다 — 이것이 유일한 백스톱이다.
    let spent = Spent {
        attempts: 3,
        tokens: 0,
        cost_usd: 0.0,
        elapsed_secs: 1,
    };
    assert_eq!(exhausted(&budget(), &spent), Some(ExhaustReason::Attempts));
}

#[test]
fn cost_budget_can_trigger_alone() {
    let spent = Spent {
        attempts: 1,
        tokens: 1,
        cost_usd: 5.0,
        elapsed_secs: 1,
    };
    assert_eq!(exhausted(&budget(), &spent), Some(ExhaustReason::Cost));
}

#[test]
fn earliest_reason_wins() {
    // 넷이 동시에 소진돼도 판정은 하나여야 한다 — 우선순위가 고정이어야 UI 문구가 흔들리지 않는다.
    let spent = Spent {
        attempts: 99,
        tokens: 999_999,
        cost_usd: 99.0,
        elapsed_secs: 99_999,
    };
    assert_eq!(exhausted(&budget(), &spent), Some(ExhaustReason::Attempts));
}

// ─────────────────────────── spend ───────────────────────────

fn result_event(tokens_in: i64, tokens_out: i64, cost: f64, turns: i64) -> String {
    format!(
        r#"{{"kind":"result","text":"","is_error":false,"session_id":"s","cost_usd":{cost},"num_turns":{turns},"tokens_in":{tokens_in},"tokens_out":{tokens_out}}}"#
    )
}

#[test]
fn sums_result_events_across_turns() {
    // 한 태스크가 후속 입력으로 여러 턴을 돌 수 있다 — 최댓값이 아니라 합이 소비량이다.
    let events = vec![
        result_event(100, 20, 0.5, 3),
        result_event(200, 30, 0.25, 4),
    ];
    let s = from_events(&events);
    assert_eq!(s.tokens, 350);
    assert!((s.cost_usd - 0.75).abs() < 1e-9);
    assert_eq!(s.vendor_turns, 7);
}

#[test]
fn ignores_non_result_rows() {
    // convo_events에는 ConvoEvent가 아닌 행이 섞여 있다 (#235).
    let events = vec![
        r#"{"kind":"user"}"#.to_string(),
        "not json at all".to_string(),
        r#"{"kind":"context_usage","context_tokens":50000}"#.to_string(),
        result_event(10, 5, 0.1, 1),
    ];
    let s = from_events(&events);
    assert_eq!(s.tokens, 15, "context_usage는 소비량이 아니라 포화도다");
}

#[test]
fn tolerates_vendor_without_cost() {
    // codex는 cost_usd를 주지 않는다. 추정으로 채우지 않고 0으로 둔다 (#235).
    let events = vec![
        r#"{"kind":"result","text":"","is_error":false,"session_id":"s","num_turns":2,"tokens_in":80,"tokens_out":20}"#
            .to_string(),
    ];
    let s = from_events(&events);
    assert_eq!(s.tokens, 100);
    assert_eq!(s.cost_usd, 0.0);
}

#[test]
fn empty_ledger_spends_nothing() {
    assert_eq!(from_events(&[]), Spend::default());
}

#[tokio::test]
async fn collect_counts_attempts_even_without_ledger_rows() {
    let pool = test_pool().await;
    let run = insert_run(&pool, &new_run(), 100).await.unwrap();
    record_attempt(&pool, run, 41, 110).await.unwrap();
    record_attempt(&pool, run, 42, 120).await.unwrap();
    let spent = spend::collect(&pool, run, 100, 400).await.unwrap();
    assert_eq!(spent.attempts, 2);
    assert_eq!(spent.tokens, 0);
    assert_eq!(spent.elapsed_secs, 300);
}

#[tokio::test]
async fn collect_sums_across_attempts() {
    let pool = test_pool().await;
    let run = insert_run(&pool, &new_run(), 100).await.unwrap();
    record_attempt(&pool, run, 41, 110).await.unwrap();
    record_attempt(&pool, run, 42, 120).await.unwrap();
    for (task_id, tokens) in [(41i64, 100i64), (42, 250)] {
        sqlx::query("INSERT INTO convo_events (task_id, ts, event) VALUES (?, ?, ?)")
            .bind(task_id)
            .bind(150)
            .bind(result_event(tokens, 0, 1.5, 1))
            .execute(&pool)
            .await
            .unwrap();
    }
    let spent = spend::collect(&pool, run, 100, 200).await.unwrap();
    assert_eq!(spent.tokens, 350);
    assert!((spent.cost_usd - 3.0).abs() < 1e-9);
}

#[tokio::test]
async fn collect_never_reports_negative_elapsed() {
    let pool = test_pool().await;
    let run = insert_run(&pool, &new_run(), 500).await.unwrap();
    // 시스템 시계가 뒤로 갔거나 created_at이 미래인 경우.
    let spent = spend::collect(&pool, run, 500, 100).await.unwrap();
    assert_eq!(spent.elapsed_secs, 0);
}

// ─────────────────────────── CRUD ───────────────────────────

#[tokio::test]
async fn insert_and_read_back_preserves_contract_and_budget() {
    let pool = test_pool().await;
    let id = insert_run(&pool, &new_run(), 100).await.unwrap();
    let run = get_run(&pool, id).await.unwrap().unwrap();
    assert_eq!(run.status, status::RUNNING);
    assert_eq!(run.goal_contract.objective, "테스트 목표");
    assert_eq!(run.budget.max_attempts, 3);
    assert!((run.budget.max_cost_usd - 5.0).abs() < 1e-9);
}

#[tokio::test]
async fn insert_rejects_invalid_budget() {
    let pool = test_pool().await;
    let bad = NewRun {
        budget: Budget {
            max_attempts: 0,
            max_tokens: 0,
            max_cost_usd: 0.0,
            max_wall_secs: 0,
        },
        ..new_run()
    };
    assert!(insert_run(&pool, &bad, 100).await.is_err());
}

#[tokio::test]
async fn insert_rejects_empty_instruction() {
    let pool = test_pool().await;
    let bad = NewRun {
        instruction: "   ".into(),
        ..new_run()
    };
    assert!(insert_run(&pool, &bad, 100).await.is_err());
}

#[tokio::test]
async fn list_active_excludes_ended_runs() {
    let pool = test_pool().await;
    let a = insert_run(&pool, &new_run(), 100).await.unwrap();
    let b = insert_run(&pool, &new_run(), 100).await.unwrap();
    end_run(&pool, b, status::SATISFIED, "증거 통과", 200)
        .await
        .unwrap();
    let active = list_active_runs(&pool).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, a);
}

#[tokio::test]
async fn first_end_reason_wins() {
    // 같은 틱에서 두 경로가 종료를 시도해도 사유가 덮어써지면 안 된다.
    let pool = test_pool().await;
    let id = insert_run(&pool, &new_run(), 100).await.unwrap();
    assert!(end_run(&pool, id, status::EXHAUSTED, "예산 소진", 200)
        .await
        .unwrap());
    assert!(!end_run(&pool, id, status::STOPPED, "중단", 300)
        .await
        .unwrap());
    let run = get_run(&pool, id).await.unwrap().unwrap();
    assert_eq!(run.status, status::EXHAUSTED);
    assert_eq!(run.end_reason.as_deref(), Some("예산 소진"));
    assert_eq!(run.ended_at, Some(200));
}

#[tokio::test]
async fn attempts_get_increasing_sequence() {
    let pool = test_pool().await;
    let run = insert_run(&pool, &new_run(), 100).await.unwrap();
    record_attempt(&pool, run, 7, 110).await.unwrap();
    record_attempt(&pool, run, 9, 120).await.unwrap();
    assert_eq!(attempt_task_ids(&pool, run).await.unwrap(), vec![7, 9]);
}

#[tokio::test]
async fn a_task_belongs_to_at_most_one_run() {
    let pool = test_pool().await;
    let a = insert_run(&pool, &new_run(), 100).await.unwrap();
    let b = insert_run(&pool, &new_run(), 100).await.unwrap();
    record_attempt(&pool, a, 7, 110).await.unwrap();
    assert!(
        record_attempt(&pool, b, 7, 120).await.is_err(),
        "같은 태스크가 두 Run의 시도가 되면 예산이 이중 계상된다"
    );
    assert_eq!(run_id_of_task(&pool, 7).await.unwrap(), Some(a));
}

#[tokio::test]
async fn latest_attempt_returns_the_newest_seq() {
    let pool = test_pool().await;
    let run = insert_run(&pool, &new_run(), 100).await.unwrap();
    record_attempt(&pool, run, 7, 110).await.unwrap();
    record_attempt(&pool, run, 9, 120).await.unwrap();
    let latest = latest_attempt(&pool, run).await.unwrap().unwrap();
    assert_eq!(latest.task_id, 9);
    assert_eq!(latest.seq, 2);
    assert!(!latest.gate_evaluated(), "새 시도는 아직 평가되지 않았다");
}

#[tokio::test]
async fn recording_a_verdict_marks_it_evaluated() {
    let pool = test_pool().await;
    let run = insert_run(&pool, &new_run(), 100).await.unwrap();
    record_attempt(&pool, run, 7, 110).await.unwrap();
    record_gate(&pool, 7, Some(true), 200).await.unwrap();
    let latest = latest_attempt(&pool, run).await.unwrap().unwrap();
    assert!(latest.gate_evaluated());
    assert_eq!(latest.gate_ready, Some(true));
}

#[tokio::test]
async fn missing_verify_spec_is_distinguishable_from_failure() {
    // "검증 커맨드가 없다"(ready=None)와 "아직 평가 안 함"을 구분하지 못하면
    // 매 틱마다 빌드를 다시 돌리거나, 커맨드 없음을 실패로 읽어 무한 재시도가 된다.
    let pool = test_pool().await;
    let run = insert_run(&pool, &new_run(), 100).await.unwrap();
    record_attempt(&pool, run, 7, 110).await.unwrap();
    record_gate(&pool, 7, None, 200).await.unwrap();
    let latest = latest_attempt(&pool, run).await.unwrap().unwrap();
    assert!(latest.gate_evaluated(), "평가는 했다");
    assert_eq!(latest.gate_ready, None, "판정할 증거가 없었다");
}

#[tokio::test]
async fn no_attempt_yet_reads_as_none() {
    let pool = test_pool().await;
    let run = insert_run(&pool, &new_run(), 100).await.unwrap();
    assert!(latest_attempt(&pool, run).await.unwrap().is_none());
}

#[tokio::test]
async fn unknown_task_has_no_run() {
    let pool = test_pool().await;
    assert_eq!(run_id_of_task(&pool, 999).await.unwrap(), None);
}

#[tokio::test]
async fn stop_marks_stopped() {
    let pool = test_pool().await;
    let id = insert_run(&pool, &new_run(), 100).await.unwrap();
    stop_run(&pool, id, 200).await.unwrap();
    let run = get_run(&pool, id).await.unwrap().unwrap();
    assert_eq!(run.status, status::STOPPED);
}

// ─────────────────────────── decide ───────────────────────────

#[test]
fn state_maps_to_attempt() {
    assert_eq!(attempt_of_state(tstate::DISCARDED), Attempt::Rejected);
    assert_eq!(attempt_of_state(tstate::DONE), Attempt::Settled);
    assert_eq!(attempt_of_state(tstate::FAILED), Attempt::Settled);
    assert_eq!(attempt_of_state(tstate::PENDING_APPROVAL), Attempt::Busy);
    assert_eq!(attempt_of_state(tstate::RUNNING), Attempt::Busy);
    // 에이전트는 끝냈지만 사람이 아직 안 봤다 — 여기서 재진입하면 검토 중인 결과 위에 쌓인다.
    assert_eq!(attempt_of_state(tstate::AWAITING_REVIEW), Attempt::Busy);
}

#[test]
fn first_attempt_is_created() {
    assert_eq!(
        decide(&budget(), &Spent::default(), Attempt::None, None),
        Decision::Reenter
    );
}

#[test]
fn passing_gate_satisfies() {
    let d = decide(
        &budget(),
        &spent_low(),
        Attempt::Settled,
        Some(GateVerdict::Pass),
    );
    assert_eq!(d, Decision::Satisfy);
}

#[test]
fn failing_gate_within_budget_reenters() {
    let d = decide(
        &budget(),
        &spent_low(),
        Attempt::Settled,
        Some(GateVerdict::Fail),
    );
    assert_eq!(d, Decision::Reenter);
}

#[test]
fn exhausted_budget_beats_failing_gate() {
    let spent = Spent {
        attempts: 3,
        ..spent_low()
    };
    let d = decide(&budget(), &spent, Attempt::Settled, Some(GateVerdict::Fail));
    assert_eq!(d, Decision::Exhaust(ExhaustReason::Attempts));
}

#[test]
fn missing_evidence_hands_over_to_human() {
    let d = decide(&budget(), &spent_low(), Attempt::Settled, None);
    assert_eq!(d, Decision::HandOff);
}

#[test]
fn rejected_attempt_stops_the_run() {
    // DR-6. 거부는 목표 판정이 아니라 사람의 정지 신호다.
    let d = decide(
        &budget(),
        &spent_low(),
        Attempt::Rejected,
        Some(GateVerdict::Fail),
    );
    assert!(matches!(d, Decision::Stop(_)));
}

#[test]
fn rejection_beats_a_passing_gate() {
    let d = decide(
        &budget(),
        &spent_low(),
        Attempt::Rejected,
        Some(GateVerdict::Pass),
    );
    assert!(matches!(d, Decision::Stop(_)));
}

#[test]
fn busy_attempt_does_nothing() {
    // 승인 대기·실행 중에는 다음 시도를 만들지 않는다 — Run 1건은 태스크 1개만 갖는다.
    assert_eq!(
        decide(&budget(), &spent_low(), Attempt::Busy, None),
        Decision::Wait
    );
}

#[test]
fn exhausted_budget_beats_busy_attempt() {
    // 진행 중이어도 예산이 끝났으면 Run은 끝난다 — 다음 시도를 안 만드는 것만으로는
    // 시간 예산이 영원히 안 걸린다.
    let spent = Spent {
        elapsed_secs: 3600,
        ..spent_low()
    };
    let d = decide(&budget(), &spent, Attempt::Busy, None);
    assert_eq!(d, Decision::Exhaust(ExhaustReason::WallClock));
}
