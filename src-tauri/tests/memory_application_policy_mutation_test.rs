//! 항상-적용(`must_apply`) 지정의 변경 계약.
//!
//! 지정은 사람의 개별 동작으로만 일어나고, 자격·상한·중복·CAS를 모두 통과해야 한다.
//! 본문이 바뀌는 lifecycle(편집·복원·보관)에서는 같은 transaction에서 지정이 풀린다 —
//! 승인은 그 version의 본문에 대한 것이지 메모리 식별자에 대한 것이 아니다.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::memory::application_policy::{
    self, policy, PolicyFailure, MAX_MUST_APPLY_PER_PROJECT,
};
use praxis_lib::{db, memory};
use sqlx::SqlitePool;

const REPO: &str = "/repo";

fn database_path(label: &str) -> String {
    temp_root::dir()
        .join(format!(
            "praxis-memory-policy-mutation-{}-{}.sqlite",
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

async fn fresh_pool(label: &str) -> (SqlitePool, String) {
    let path = database_path(label);
    remove_database_files(&path);
    let pool = db::init_pool(&path).await.unwrap();
    memory::migrate(&pool).await.unwrap();
    (pool, path)
}

/// verified 상태의 project decision 메모리 — 지정 가능한 최소 조건.
async fn verified_memory(pool: &SqlitePool, content: &str) -> i64 {
    let id = memory::create_candidate(
        pool,
        memory::tier::PROJECT,
        Some(REPO),
        memory::knowledge_type::DECISION,
        content,
        Some("test"),
        100,
    )
    .await
    .unwrap();
    memory::confirm_and_approve(pool, id, 1, 110).await.unwrap();
    id
}

async fn policy_of(pool: &SqlitePool, id: i64) -> String {
    sqlx::query_scalar::<_, String>("SELECT application_policy FROM memories WHERE id = ?")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn policy_event_count(pool: &SqlitePool, id: i64) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM memory_events WHERE memory_id = ? AND action = ?",
    )
    .bind(id)
    .bind(application_policy::EVENT_ACTION)
    .fetch_one(pool)
    .await
    .unwrap()
}

/// 지정만 해 두고 되돌아오는 헬퍼 — 대부분의 테스트가 "이미 지정된 상태"에서 시작한다.
async fn designate(pool: &SqlitePool, id: i64) {
    let changed =
        application_policy::set_policy(pool, id, policy::MUST_APPLY, 1, policy::RELEVANCE, 200)
            .await
            .unwrap();
    assert!(changed);
}

#[tokio::test]
async fn ineligible_targets_are_rejected_without_mutation() {
    let (pool, path) = fresh_pool("ineligible").await;

    // global tier — 전역 규칙 강제 주입은 MVP 범위 밖이다.
    let global = memory::create_candidate(
        &pool,
        memory::tier::GLOBAL,
        None,
        memory::knowledge_type::DECISION,
        "전역 결정",
        Some("test"),
        100,
    )
    .await
    .unwrap();
    memory::confirm_and_approve(&pool, global, 1, 110)
        .await
        .unwrap();

    // claim 유형 — 사실 주장은 강제 주입 대상이 아니다.
    let claim = memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some(REPO),
        memory::knowledge_type::CLAIM,
        "사실 주장",
        Some("test"),
        100,
    )
    .await
    .unwrap();
    memory::confirm_and_approve(&pool, claim, 1, 110)
        .await
        .unwrap();

    // 미승인 후보 — 진실성 gate를 우회할 수 없다.
    let candidate = memory::create_candidate(
        &pool,
        memory::tier::PROJECT,
        Some(REPO),
        memory::knowledge_type::DECISION,
        "미승인 결정",
        Some("test"),
        100,
    )
    .await
    .unwrap();

    for id in [global, claim, candidate] {
        let failure = application_policy::set_policy(
            &pool,
            id,
            policy::MUST_APPLY,
            1,
            policy::RELEVANCE,
            200,
        )
        .await
        .unwrap_err();
        assert!(
            matches!(failure, PolicyFailure::Ineligible(_)),
            "id={id}: {failure:?}"
        );
        assert_eq!(policy_of(&pool, id).await, policy::RELEVANCE);
        assert_eq!(
            policy_event_count(&pool, id).await,
            0,
            "거부는 event를 남기지 않는다"
        );
    }

    remove_database_files(&path);
}

#[tokio::test]
async fn count_cap_rejects_the_designation_past_the_limit() {
    let (pool, path) = fresh_pool("count-cap").await;
    for index in 0..MAX_MUST_APPLY_PER_PROJECT {
        let id = verified_memory(&pool, &format!("규칙 {index}")).await;
        designate(&pool, id).await;
    }

    let overflow = verified_memory(&pool, "한 건 더").await;
    let failure = application_policy::set_policy(
        &pool,
        overflow,
        policy::MUST_APPLY,
        1,
        policy::RELEVANCE,
        300,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(failure, PolicyFailure::CapacityExceeded(_)),
        "{failure:?}"
    );
    assert_eq!(policy_of(&pool, overflow).await, policy::RELEVANCE);

    remove_database_files(&path);
}

#[tokio::test]
async fn byte_cap_rejects_when_the_designated_total_would_overflow() {
    let (pool, path) = fresh_pool("byte-cap").await;
    // 개수는 상한 안이지만 합계 바이트가 넘는 경우 — 개수만 세면 놓친다.
    let half = "가".repeat(1_400); // 한 글자 3바이트 → 약 4,200바이트
    let first = verified_memory(&pool, &format!("{half}1")).await;
    designate(&pool, first).await;

    let second = verified_memory(&pool, &format!("{half}2")).await;
    let failure = application_policy::set_policy(
        &pool,
        second,
        policy::MUST_APPLY,
        1,
        policy::RELEVANCE,
        300,
    )
    .await
    .unwrap_err();
    assert!(
        matches!(failure, PolicyFailure::CapacityExceeded(_)),
        "합계 바이트 초과는 거부돼야 한다: {failure:?}"
    );
    assert_eq!(policy_of(&pool, second).await, policy::RELEVANCE);

    remove_database_files(&path);
}

#[tokio::test]
async fn normalized_duplicate_is_rejected() {
    let (pool, path) = fresh_pool("duplicate").await;
    let first = verified_memory(&pool, "락파일은 직접 수정하지 않는다").await;
    designate(&pool, first).await;

    // 공백만 다른 같은 규칙 — 두 번 주입할 이유가 없다.
    let second = verified_memory(&pool, "락파일은   직접\n수정하지 않는다").await;
    let failure = application_policy::set_policy(
        &pool,
        second,
        policy::MUST_APPLY,
        1,
        policy::RELEVANCE,
        300,
    )
    .await
    .unwrap_err();
    assert_eq!(failure, PolicyFailure::Duplicate);
    assert_eq!(policy_of(&pool, second).await, policy::RELEVANCE);

    remove_database_files(&path);
}

#[tokio::test]
async fn stale_version_or_policy_expectation_conflicts() {
    let (pool, path) = fresh_pool("cas").await;
    let id = verified_memory(&pool, "규칙").await;

    // version 기대값 불일치 — 본문이 바뀌었을 수 있다.
    let failure =
        application_policy::set_policy(&pool, id, policy::MUST_APPLY, 99, policy::RELEVANCE, 300)
            .await
            .unwrap_err();
    assert_eq!(failure, PolicyFailure::Conflict);

    // policy 기대값 불일치 — 다른 곳에서 먼저 바꿨다.
    let failure =
        application_policy::set_policy(&pool, id, policy::MUST_APPLY, 1, policy::MUST_APPLY, 300)
            .await
            .unwrap_err();
    assert_eq!(failure, PolicyFailure::Conflict);
    assert_eq!(policy_of(&pool, id).await, policy::RELEVANCE);

    remove_database_files(&path);
}

#[tokio::test]
async fn retry_toward_the_same_target_is_idempotent() {
    let (pool, path) = fresh_pool("idempotent").await;
    let id = verified_memory(&pool, "규칙").await;
    designate(&pool, id).await;
    assert_eq!(policy_event_count(&pool, id).await, 1);

    // 응답을 잃은 클라이언트가 같은 요청을 되보낸다 — 이미 목표 상태이므로
    // Conflict가 아니라 "변경 없음"으로 흡수돼야 한다.
    let changed =
        application_policy::set_policy(&pool, id, policy::MUST_APPLY, 1, policy::RELEVANCE, 300)
            .await
            .unwrap();
    assert!(!changed);
    assert_eq!(
        policy_event_count(&pool, id).await,
        1,
        "재시도가 중복 event를 만들면 원장이 거짓말을 한다"
    );

    remove_database_files(&path);
}

#[tokio::test]
async fn downgrade_to_relevance_is_always_allowed() {
    let (pool, path) = fresh_pool("downgrade").await;
    let id = verified_memory(&pool, "규칙").await;
    designate(&pool, id).await;

    // 지정 후 stale이 되어도 해제 경로는 열려 있어야 한다 —
    // 막히면 fail-closed 주입 게이트가 사용자를 가둔다.
    // (stale 전이는 evidence 만료로 일어나므로 여기서는 상태만 직접 만든다.)
    sqlx::query("UPDATE memories SET status = ?, stale_at = ? WHERE id = ?")
        .bind(memory::knowledge_status::STALE)
        .bind(400)
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    let changed =
        application_policy::set_policy(&pool, id, policy::RELEVANCE, 1, policy::MUST_APPLY, 410)
            .await
            .unwrap();
    assert!(changed);
    assert_eq!(policy_of(&pool, id).await, policy::RELEVANCE);

    remove_database_files(&path);
}

#[tokio::test]
async fn content_update_resets_policy_in_the_same_transaction() {
    let (pool, path) = fresh_pool("content-update").await;
    let id = verified_memory(&pool, "원래 규칙").await;
    designate(&pool, id).await;

    memory::update_knowledge(
        &pool,
        id,
        "바뀐 규칙",
        memory::knowledge_type::DECISION,
        500,
    )
    .await
    .unwrap();

    assert_eq!(
        policy_of(&pool, id).await,
        policy::RELEVANCE,
        "편집된 본문에 이전 승인이 상속되면 안 된다"
    );
    assert_eq!(
        policy_event_count(&pool, id).await,
        2,
        "해제도 원장에 남는다"
    );

    remove_database_files(&path);
}

#[tokio::test]
async fn archive_resets_policy_in_the_same_transaction() {
    let (pool, path) = fresh_pool("archive").await;
    let id = verified_memory(&pool, "규칙").await;
    designate(&pool, id).await;

    memory::archive(&pool, id, 500).await.unwrap();

    assert_eq!(policy_of(&pool, id).await, policy::RELEVANCE);
    assert_eq!(policy_event_count(&pool, id).await, 2);

    remove_database_files(&path);
}

#[tokio::test]
async fn version_restore_resets_policy_in_the_same_transaction() {
    let (pool, path) = fresh_pool("restore").await;
    let id = verified_memory(&pool, "버전 1").await;
    memory::update_knowledge(&pool, id, "버전 2", memory::knowledge_type::DECISION, 500)
        .await
        .unwrap();
    memory::confirm_and_approve(&pool, id, 2, 510)
        .await
        .unwrap();
    application_policy::set_policy(&pool, id, policy::MUST_APPLY, 2, policy::RELEVANCE, 520)
        .await
        .unwrap();

    memory::management::restore_version(&pool, id, 1, 2, memory::knowledge_status::VERIFIED, 600)
        .await
        .unwrap();

    assert_eq!(
        policy_of(&pool, id).await,
        policy::RELEVANCE,
        "복원은 새 version을 만든다 — 지정을 물려받지 않는다"
    );

    remove_database_files(&path);
}
