//! Runner의 항상-적용 정책 엔드포인트 — Desktop과 같은 도메인 함수를 거치는지.
//!
//! 두 경로가 각자 판정을 구현하면 한쪽에서만 지정되는 상태가 생긴다. 여기서는
//! HTTP 계층이 도메인 실패를 올바른 상태 코드로 옮기고, 저장소 오류 원문을
//! 노출하지 않는지를 고정한다.

#[path = "support/temp_root.rs"]
mod temp_root;

#[path = "support/memory_version_http.rs"]
mod memory_version_http;

use memory_version_http::{authenticated_client, Fixture};
use praxis_lib::memory;

async fn verified_memory(fixture: &Fixture, content: &str) -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let id = memory::create_candidate(
        &fixture.pool,
        memory::tier::PROJECT,
        Some(fixture.root.to_string_lossy().as_ref()),
        memory::knowledge_type::DECISION,
        content,
        Some("test"),
        now,
    )
    .await
    .unwrap();
    memory::confirm_and_approve(&fixture.pool, id, 1, now)
        .await
        .unwrap();
    id
}

fn body(policy: &str, expected_version: i64, expected_policy: &str) -> serde_json::Value {
    serde_json::json!({
        "policy": policy,
        "expected_version": expected_version,
        "expected_policy": expected_policy,
    })
}

#[tokio::test]
async fn designation_round_trips_and_absorbs_a_lost_response_retry() {
    let fixture = Fixture::start().await;
    let client = authenticated_client();
    let id = verified_memory(&fixture, "락파일은 직접 수정하지 않는다").await;
    let url = fixture.url(&format!("/v1/memories/{id}/application-policy"));

    let changed: bool = client
        .put(&url)
        .json(&body("must_apply", 1, "relevance"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(changed);

    // 응답을 잃은 클라이언트의 재시도 — 충돌이 아니라 "변경 없음"으로 흡수돼야 한다.
    let changed: bool = client
        .put(&url)
        .json(&body("must_apply", 1, "relevance"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!changed);

    let listed: serde_json::Value = client
        .get(fixture.url("/v1/memories"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let row = listed
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id)
        .expect("목록에 있어야 한다");
    assert_eq!(row["application_policy"], "must_apply");
}

#[tokio::test]
async fn stale_expectation_is_a_conflict() {
    let fixture = Fixture::start().await;
    let client = authenticated_client();
    let id = verified_memory(&fixture, "규칙").await;

    let response = client
        .put(fixture.url(&format!("/v1/memories/{id}/application-policy")))
        .json(&body("must_apply", 99, "relevance"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 409);
}

#[tokio::test]
async fn ineligible_memory_is_a_bad_request() {
    let fixture = Fixture::start().await;
    let client = authenticated_client();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    // 승인되지 않은 후보 — 진실성 gate를 우회할 수 없다.
    let id = memory::create_candidate(
        &fixture.pool,
        memory::tier::PROJECT,
        Some(fixture.root.to_string_lossy().as_ref()),
        memory::knowledge_type::DECISION,
        "미승인 결정",
        Some("test"),
        now,
    )
    .await
    .unwrap();

    let response = client
        .put(fixture.url(&format!("/v1/memories/{id}/application-policy")))
        .json(&body("must_apply", 1, "relevance"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let text = response.text().await.unwrap();
    assert!(
        text.contains("검증"),
        "왜 거부됐는지 사용자가 알 수 있어야 한다: {text}"
    );
}

#[tokio::test]
async fn unknown_policy_value_is_rejected() {
    let fixture = Fixture::start().await;
    let client = authenticated_client();
    let id = verified_memory(&fixture, "규칙").await;

    let response = client
        .put(fixture.url(&format!("/v1/memories/{id}/application-policy")))
        .json(&body("always", 1, "relevance"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
}

#[tokio::test]
async fn unauthenticated_request_is_rejected() {
    let fixture = Fixture::start().await;
    let id = verified_memory(&fixture, "규칙").await;

    let response = reqwest::Client::new()
        .put(fixture.url(&format!("/v1/memories/{id}/application-policy")))
        .json(&body("must_apply", 1, "relevance"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
}
