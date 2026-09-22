//! 항상-적용 규칙이 검색 순위와 무관하게 투영되는지.
//!
//! 이 파일의 존재 이유는 하나다: 승인된 금지·보존 규칙이 하이브리드 검색 상위
//! `INJECTION_LIMIT`건에 못 들어 조용히 빠지던 경로를 닫는 것. 조용한 누락 대신
//! 시끄러운 실패를 택한 지점(stale 차단)도 여기서 고정한다.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::memory::application_policy::{self, policy};
use praxis_lib::{db, memory, projector};

const REPO: &str = "/repo";
/// 지정 규칙과 겹치지 않는 검색어 — 규칙이 관련도로는 절대 안 뽑히게 한다.
const INSTRUCTION: &str = "배포 파이프라인 재시도 로직";
/// 검색어와 무관한 규칙 본문. FTS로는 매치되지 않는다.
const RULE: &str = "락파일은 사람이 직접 수정하지 않는다";

/// 실제 시각 기준 epoch 초.
///
/// 고정 소값(예: 100)을 쓰면 `active_filter`의 dormant 판정(`usage_count = 0` 이면서
/// 생성 30일 경과)에 걸려 검색이 0건을 반환한다 — 그 필터만 `strftime('%s','now')`로
/// 실제 시계를 보기 때문이다.
fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

fn database_path(label: &str) -> String {
    temp_root::dir()
        .join(format!(
            "praxis-must-apply-{}-{}.sqlite",
            label,
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned()
}

fn remove_database_files(path: &str) {
    for target in [
        path.to_string(),
        format!("{path}-wal"),
        format!("{path}-shm"),
    ] {
        let _ = std::fs::remove_file(target);
    }
}

fn worktree(label: &str) -> std::path::PathBuf {
    let root = temp_root::dir().join(format!(
        "praxis-must-apply-wt-{}-{}",
        label,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

async fn fresh_pool(label: &str) -> (sqlx::SqlitePool, String) {
    let path = database_path(label);
    remove_database_files(&path);
    let pool = db::init_pool(&path).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    (pool, path)
}

async fn verified_memory(pool: &sqlx::SqlitePool, content: &str) -> i64 {
    let id = memory::create_candidate(
        pool,
        memory::tier::PROJECT,
        Some(REPO),
        memory::knowledge_type::DECISION,
        content,
        Some("test"),
        now_secs(),
    )
    .await
    .unwrap();
    memory::confirm_and_approve(pool, id, 1, now_secs())
        .await
        .unwrap();
    id
}

/// 검색어와 강하게 겹치는 메모리를 상한만큼 채운다 — 지정 규칙을 순위 밖으로 밀어내기 위해.
async fn fill_relevant(pool: &sqlx::SqlitePool, count: usize) -> Vec<i64> {
    let mut ids = Vec::new();
    for index in 0..count {
        ids.push(verified_memory(pool, &format!("배포 파이프라인 재시도 규약 {index}")).await);
    }
    ids
}

async fn designate(pool: &sqlx::SqlitePool, id: i64) {
    application_policy::set_policy(
        pool,
        id,
        policy::MUST_APPLY,
        1,
        policy::RELEVANCE,
        now_secs(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn rule_outside_the_search_window_is_still_projected_first() {
    let (pool, path) = fresh_pool("rank-nine").await;
    fill_relevant(&pool, memory::INJECTION_LIMIT as usize).await;
    let rule = verified_memory(&pool, RULE).await;

    // 지정 전에는 순위 밖이라 빠진다 — 이 플랜이 고치려는 상태를 먼저 고정한다.
    let before = application_policy::select_for_projection(
        &pool,
        REPO,
        INSTRUCTION,
        None,
        memory::INJECTION_LIMIT,
        now_secs(),
    )
    .await
    .unwrap();
    assert!(
        !before.ordered().iter().any(|m| m.id == rule),
        "지정 전에는 검색 순위 밖이어야 이 테스트가 의미를 갖는다"
    );

    designate(&pool, rule).await;

    let after = application_policy::select_for_projection(
        &pool,
        REPO,
        INSTRUCTION,
        None,
        memory::INJECTION_LIMIT,
        now_secs(),
    )
    .await
    .unwrap();
    let ordered = after.ordered();
    assert_eq!(
        ordered.first().map(|m| m.id),
        Some(rule),
        "지정 규칙이 관련 메모리보다 먼저 와야 한다"
    );
    assert_eq!(
        after.relevant.len(),
        memory::INJECTION_LIMIT as usize,
        "지정했다고 일반 컨텍스트가 줄면 안 된다"
    );

    remove_database_files(&path);
}

#[tokio::test]
async fn a_designated_rule_that_also_ranks_high_appears_once() {
    let (pool, path) = fresh_pool("dedup").await;
    fill_relevant(&pool, (memory::INJECTION_LIMIT - 1) as usize).await;
    // 검색어와 겹치는 본문을 지정한다 — 두 섹션 모두에 들어갈 수 있는 조건.
    let overlapping = verified_memory(&pool, "배포 파이프라인 재시도는 3회로 제한한다").await;
    designate(&pool, overlapping).await;

    let selection = application_policy::select_for_projection(
        &pool,
        REPO,
        INSTRUCTION,
        None,
        memory::INJECTION_LIMIT,
        now_secs(),
    )
    .await
    .unwrap();

    let ordered = selection.ordered();
    let occurrences = ordered.iter().filter(|m| m.id == overlapping).count();
    assert_eq!(occurrences, 1, "같은 메모리가 두 섹션에 중복되면 안 된다");
    assert!(
        !selection.relevant.iter().any(|m| m.id == overlapping),
        "관련 섹션에서 제거돼야 한다"
    );

    remove_database_files(&path);
}

#[tokio::test]
async fn stale_designated_rule_blocks_projection_instead_of_degrading() {
    let (pool, path) = fresh_pool("stale-block").await;
    let rule = verified_memory(&pool, RULE).await;
    designate(&pool, rule).await;

    // 지정 후 stale로 떨어진다(evidence 만료 등).
    sqlx::query("UPDATE memories SET status = ?, stale_at = ? WHERE id = ?")
        .bind(memory::knowledge_status::STALE)
        .bind(now_secs())
        .bind(rule)
        .execute(&pool)
        .await
        .unwrap();

    let error = application_policy::select_for_projection(
        &pool,
        REPO,
        INSTRUCTION,
        None,
        memory::INJECTION_LIMIT,
        now_secs(),
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("항상 적용"),
        "조용한 강등이 아니라 명시적 실패여야 한다: {error}"
    );

    remove_database_files(&path);
}

#[tokio::test]
async fn projected_block_separates_must_apply_from_relevant() {
    let (pool, path) = fresh_pool("render").await;
    let root = worktree("render");
    fill_relevant(&pool, 2).await;
    let rule = verified_memory(&pool, RULE).await;
    designate(&pool, rule).await;

    let task_id = db::insert_task(
        &pool,
        REPO,
        "praxis/x",
        "main",
        &root.to_string_lossy(),
        INSTRUCTION,
        None,
        None,
        "conversation",
        1,
    )
    .await
    .unwrap();

    let targets = projector::project_targets();
    let count = memory::inject_into_worktree(
        &pool,
        REPO,
        INSTRUCTION,
        None,
        task_id,
        now_secs(),
        &root,
        memory::INJECTION_LIMIT,
        &targets,
    )
    .await
    .unwrap();
    assert_eq!(count, 3);

    let rendered = std::fs::read_to_string(root.join("AGENTS.md")).unwrap();
    let must_at = rendered.find("## 항상 적용").expect("항상 적용 섹션");
    let relevant_at = rendered.find("## 관련 메모리").expect("관련 메모리 섹션");
    assert!(must_at < relevant_at, "항상 적용이 먼저 와야 한다");
    let rule_at = rendered.find(RULE).expect("규칙 본문");
    assert!(
        must_at < rule_at && rule_at < relevant_at,
        "규칙은 항상 적용 섹션 안에 있어야 한다"
    );

    let _ = std::fs::remove_dir_all(&root);
    remove_database_files(&path);
}

#[tokio::test]
async fn policy_change_after_apply_is_rejected_by_start_verification() {
    let (pool, path) = fresh_pool("finalize-race").await;
    let root = worktree("finalize-race");
    let rule = verified_memory(&pool, RULE).await;
    designate(&pool, rule).await;

    let task_id = db::insert_task(
        &pool,
        REPO,
        "praxis/x",
        "main",
        &root.to_string_lossy(),
        INSTRUCTION,
        None,
        None,
        "conversation",
        1,
    )
    .await
    .unwrap();
    let targets = projector::project_targets();
    memory::inject_into_worktree(
        &pool,
        REPO,
        INSTRUCTION,
        None,
        task_id,
        now_secs(),
        &root,
        memory::INJECTION_LIMIT,
        &targets,
    )
    .await
    .unwrap();

    // 투영 이후 정책이 바뀌면, 이 작업이 무엇 위에서 돌았는지가 달라진다.
    // 현재 상태로 과거 선택을 추정하지 않도록 시작 검증이 거부해야 한다.
    application_policy::set_policy(
        &pool,
        rule,
        policy::RELEVANCE,
        1,
        policy::MUST_APPLY,
        now_secs(),
    )
    .await
    .unwrap();

    let error = memory::inject_into_worktree(
        &pool,
        REPO,
        INSTRUCTION,
        None,
        task_id,
        now_secs(),
        &root,
        memory::INJECTION_LIMIT,
        &targets,
    )
    .await
    .unwrap_err()
    .to_string();
    assert!(
        error.contains("memory changed during projection"),
        "정책 변경이 감지되지 않았다: {error}"
    );

    let _ = std::fs::remove_dir_all(&root);
    remove_database_files(&path);
}

// 구 receipt JSON(정책 도입 전)이 relevance로 읽히는지는 `memory::receipt`의 단위 테스트가
// 담당한다 — 그 계약을 통합 테스트로 끌어내면 테스트 전용 공개 API가 생긴다.
