#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::db::{self, state};
use praxis_lib::goal_contract::GoalContract;
use praxis_lib::orchestrator::{CreateTaskParams, TaskDraft, TaskOrigin, TaskService};

static COUNTER: AtomicU32 = AtomicU32::new(0);

async fn service() -> (TaskService, std::path::PathBuf) {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = temp_root::dir().join(format!(
        "praxis-orchestrator-test-{}-{n}.sqlite",
        std::process::id()
    ));
    let pool = db::init_pool(path.to_str().unwrap()).await.unwrap();
    (TaskService::new(pool), path)
}

fn draft() -> TaskDraft {
    TaskDraft {
        repo: "/repo".to_string(),
        branch: "praxis/test".to_string(),
        base: "main".to_string(),
        worktree_path: "/repo/.praxis/worktrees/test".to_string(),
        instruction: "add runner boundary".to_string(),
        agent: Some("claude".to_string()),
        role: "implementer".to_string(),
        ensemble: None,
        mode: "terminal".to_string(),
        goal_contract: None,
        ambiguity: None,
    }
}

fn contract() -> GoalContract {
    GoalContract {
        schema_version: 1,
        objective: "persist through TaskService".into(),
        acceptance: vec!["round-trip".into()],
        stop_conditions: vec![],
        must_preserve: vec![],
        protected_paths: vec![],
        non_goals: vec![],
    }
}

#[tokio::test]
async fn task_service_creates_task_and_persists_model_override() {
    let (service, path) = service().await;

    let task = service
        .create_task(draft(), Some("haiku"), None, 100)
        .await
        .unwrap();

    assert_eq!(task.state, state::CREATED);
    assert_eq!(task.model.as_deref(), Some("haiku"));
    assert_eq!(task.agent.as_deref(), Some("claude"));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn task_service_persists_codex_reasoning_effort_override() {
    let (service, path) = service().await;
    let mut draft = draft();
    draft.agent = Some("codex".to_string());

    let task = service
        .create_task(draft, Some("gpt-5.6-sol"), Some("high"), 100)
        .await
        .unwrap();

    assert_eq!(task.reasoning_effort.as_deref(), Some("high"));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn task_service_rejects_invalid_or_unsupported_reasoning_effort() {
    let (service, path) = service().await;
    let mut codex_draft = draft();
    codex_draft.agent = Some("codex".to_string());

    let invalid = service
        .create_task(codex_draft, None, Some("extreme"), 100)
        .await
        .unwrap_err();
    // Antigravity도 --effort를 받으므로 거부 대상은 지원 벤더 밖의 에이전트다.
    let mut other_draft = draft();
    other_draft.agent = Some("opencode".to_string());
    let unsupported_agent = service
        .create_task(other_draft, None, Some("high"), 100)
        .await
        .unwrap_err();
    let mut luna_draft = draft();
    luna_draft.agent = Some("codex".to_string());
    let unsupported_luna = service
        .create_task(luna_draft, Some("gpt-5.6-luna"), Some("ultra"), 100)
        .await
        .unwrap_err();
    let mut legacy_draft = draft();
    legacy_draft.agent = Some("codex".to_string());
    let unsupported_legacy = service
        .create_task(legacy_draft, Some("gpt-5.4"), Some("max"), 100)
        .await
        .unwrap_err();

    assert!(invalid.contains("reasoning effort"));
    assert!(unsupported_agent.contains("Codex/Claude/Antigravity"));
    assert!(unsupported_luna.contains("gpt-5.6-luna"));
    assert!(unsupported_legacy.contains("gpt-5.4"));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn task_service_persists_trimmed_agy_alias_reasoning_effort() {
    let (service, path) = service().await;

    for (agent, effort, expected) in [
        ("agy", " high ", "high"),
        ("gemini", "medium", "medium"),
        ("antigravity", "low", "low"),
    ] {
        let mut agy_draft = draft();
        agy_draft.agent = Some(agent.to_string());
        let task = service
            .create_task(agy_draft, Some("gemini-3.6-flash-high"), Some(effort), 100)
            .await
            .unwrap();

        assert_eq!(task.reasoning_effort.as_deref(), Some(expected));
    }
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn task_service_rejects_invalid_agy_reasoning_effort() {
    let (service, path) = service().await;

    for effort in ["xhigh", "max", "ultra"] {
        let mut agy_draft = draft();
        agy_draft.agent = Some("agy".to_string());
        let error = service
            .create_task(agy_draft, Some("gemini-3.6-flash-high"), Some(effort), 100)
            .await
            .unwrap_err();

        assert!(error.contains("지원하지 않는 reasoning effort"));
    }
    let _ = std::fs::remove_file(path);
}

/// claude는 모델과 무관하게 effort를 받는다(codex만 모델별 목록으로 좁힌다).
#[tokio::test]
async fn task_service_accepts_claude_reasoning_effort_for_any_model() {
    let (service, path) = service().await;
    let task = service
        .create_task(draft(), None, Some("high"), 100)
        .await
        .unwrap();

    assert_eq!(task.reasoning_effort.as_deref(), Some("high"));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn task_service_persists_reasoning_effort_in_the_initial_insert() {
    let (service, path) = service().await;
    sqlx::query(
        "CREATE TRIGGER reject_effort_insert \
         BEFORE INSERT ON tasks WHEN NEW.reasoning_effort IS NOT NULL \
         BEGIN SELECT RAISE(ABORT, 'effort insert rejected'); END",
    )
    .execute(service.pool())
    .await
    .unwrap();
    let mut draft = draft();
    draft.agent = Some("codex".to_string());

    let error = service
        .create_task(draft, Some("gpt-5.6-sol"), Some("high"), 100)
        .await
        .unwrap_err();
    let (task_count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tasks")
        .fetch_one(service.pool())
        .await
        .unwrap();

    assert!(error.contains("effort insert rejected"));
    assert_eq!(task_count, 0);
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn task_service_persists_goal_contract_in_the_task_insert() {
    let (service, path) = service().await;
    let mut draft = draft();
    draft.goal_contract = Some(contract());

    let task = service.create_task(draft, None, None, 100).await.unwrap();

    assert_eq!(task.goal_contract.as_deref(), Some(&contract()));
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn active_conversation_cannot_claim_review_finalization() {
    let (service, path) = service().await;
    let task = service.create_task(draft(), None, None, 100).await.unwrap();
    db::update_state(service.pool(), task.id, state::AWAITING_REVIEW, 101)
        .await
        .unwrap();

    let error = service
        .claim_review_finalization(task.id, true, 102)
        .await
        .unwrap_err();

    assert!(error.contains("대화 턴"));
    assert_eq!(
        db::get_task(service.pool(), task.id)
            .await
            .unwrap()
            .unwrap()
            .state,
        state::AWAITING_REVIEW
    );
    let _ = std::fs::remove_file(path);
}

#[tokio::test]
async fn idle_conversation_claims_review_finalization_once() {
    let (service, path) = service().await;
    let task = service.create_task(draft(), None, None, 100).await.unwrap();
    db::update_state(service.pool(), task.id, state::AWAITING_REVIEW, 101)
        .await
        .unwrap();

    let claimed = service
        .claim_review_finalization(task.id, false, 102)
        .await
        .unwrap();

    assert_eq!(claimed.id, task.id);
    assert_eq!(claimed.state, state::FINALIZING);
    assert!(service
        .claim_review_finalization(task.id, false, 103)
        .await
        .is_err());
    let _ = std::fs::remove_file(path);
}

#[test]
fn headless_terminal_params_are_runner_safe_defaults() {
    let params = CreateTaskParams::headless_terminal(
        "/repo".to_string(),
        "scheduled task".to_string(),
        "claude".to_string(),
        TaskOrigin::External,
    );

    assert!(params.headless);
    assert_eq!(params.mode, "terminal");
    assert_eq!(params.cols, 80);
    assert_eq!(params.rows, 24);
    assert!(params.reasoning_effort.is_empty());
    assert!(params.goal_contract.is_none());
}
