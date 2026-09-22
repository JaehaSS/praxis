#[path = "support/temp_root.rs"]
mod temp_root;

#[path = "support/memory_version_http.rs"]
mod memory_version_http;

use memory_version_http::{authenticated_client, Fixture};

#[tokio::test]
async fn concurrent_runner_restores_return_one_success_and_one_conflict() {
    let fixture = Fixture::start_with_restore_barrier().await;
    let client = authenticated_client();
    let memory_id: i64 = client
        .post(fixture.url("/v1/memories"))
        .json(&serde_json::json!({
            "repository": fixture.root,
            "kind": "claim",
            "content": "version one",
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    client
        .put(fixture.url(&format!("/v1/memories/{memory_id}")))
        .json(&serde_json::json!({
            "kind": "claim",
            "content": "version two",
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    let url = fixture.url(&format!("/v1/memories/{memory_id}/versions/1/restore"));
    let request = serde_json::json!({
        "expected_current_version": 2,
        "expected_status": "candidate",
    });
    let first_client = client.clone();
    let first_url = url.clone();
    let first_request = request.clone();
    let first = tokio::spawn(async move {
        first_client
            .post(first_url)
            .json(&first_request)
            .send()
            .await
            .unwrap()
            .status()
    });
    let second = tokio::spawn(async move {
        client
            .post(url)
            .json(&request)
            .send()
            .await
            .unwrap()
            .status()
    });
    let mut statuses = vec![first.await.unwrap(), second.await.unwrap()];
    statuses.sort();

    assert_eq!(
        statuses,
        vec![reqwest::StatusCode::OK, reqwest::StatusCode::CONFLICT]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM memory_versions WHERE memory_id = ?",)
            .bind(memory_id)
            .fetch_one(&fixture.pool)
            .await
            .unwrap(),
        3
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM memory_events
             WHERE memory_id = ? AND action = 'version_restored'",
        )
        .bind(memory_id)
        .fetch_one(&fixture.pool)
        .await
        .unwrap(),
        1
    );
}
