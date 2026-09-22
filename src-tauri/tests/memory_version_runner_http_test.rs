#[path = "support/temp_root.rs"]
mod temp_root;

#[path = "support/memory_version_http.rs"]
mod memory_version_http;

use memory_version_http::{authenticated_client, Fixture};

#[tokio::test]
async fn runner_versions_restore_with_scope_and_both_cas_owners() {
    let fixture = Fixture::start().await;
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
            "kind": "decision",
            "content": "version two",
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let versions: serde_json::Value = client
        .get(fixture.url(&format!("/v1/memories/{memory_id}/versions")))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        versions[0],
        serde_json::json!({
            "memory_id": memory_id,
            "version": 2,
            "content": "version two",
            "knowledge_type": "decision",
            "scope_snapshot": fixture.root,
            "created_at": versions[0]["created_at"],
            "editor_kind": "human_edit",
            "evidence_count": 0,
        })
    );
    assert_eq!(versions[1]["version"], 1);

    let restored: i64 = client
        .post(fixture.url(&format!("/v1/memories/{memory_id}/versions/1/restore")))
        .json(&serde_json::json!({
            "expected_current_version": 2,
            "expected_status": "candidate",
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(restored, 3);
    let conflict = client
        .post(fixture.url(&format!("/v1/memories/{memory_id}/versions/1/restore")))
        .json(&serde_json::json!({
            "expected_current_version": 2,
            "expected_status": "candidate",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(conflict.status(), reqwest::StatusCode::CONFLICT);
    let current_rejected = client
        .post(fixture.url(&format!("/v1/memories/{memory_id}/versions/3/restore")))
        .json(&serde_json::json!({
            "expected_current_version": 3,
            "expected_status": "candidate",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(current_rejected.status(), reqwest::StatusCode::BAD_REQUEST);
    let missing = client
        .post(fixture.url(&format!("/v1/memories/{memory_id}/versions/0/restore")))
        .json(&serde_json::json!({
            "expected_current_version": 3,
            "expected_status": "candidate",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);

    let hidden_id = praxis_lib::memory::create_candidate(
        &fixture.pool,
        praxis_lib::memory::tier::PROJECT,
        Some(fixture.outside.to_string_lossy().as_ref()),
        praxis_lib::memory::knowledge_type::CLAIM,
        "hidden",
        Some("test"),
        100,
    )
    .await
    .unwrap();
    let hidden = client
        .get(fixture.url(&format!("/v1/memories/{hidden_id}/versions")))
        .send()
        .await
        .unwrap();
    assert_eq!(hidden.status(), reqwest::StatusCode::BAD_REQUEST);

    let before: (i64, i64, i64) = sqlx::query_as(
        "SELECT current_version,
                (SELECT COUNT(*) FROM memory_versions WHERE memory_id = ?),
                (SELECT COUNT(*) FROM memory_events WHERE memory_id = ?)
         FROM memories WHERE id = ?",
    )
    .bind(hidden_id)
    .bind(hidden_id)
    .bind(hidden_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    let denied_restore = client
        .post(fixture.url(&format!("/v1/memories/{hidden_id}/versions/1/restore")))
        .json(&serde_json::json!({
            "expected_current_version": 1,
            "expected_status": "candidate",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(denied_restore.status(), reqwest::StatusCode::BAD_REQUEST);
    let after: (i64, i64, i64) = sqlx::query_as(
        "SELECT current_version,
                (SELECT COUNT(*) FROM memory_versions WHERE memory_id = ?),
                (SELECT COUNT(*) FROM memory_events WHERE memory_id = ?)
         FROM memories WHERE id = ?",
    )
    .bind(hidden_id)
    .bind(hidden_id)
    .bind(hidden_id)
    .fetch_one(&fixture.pool)
    .await
    .unwrap();
    assert_eq!(after, before);

    sqlx::query(
        "CREATE TRIGGER reject_http_restore_event BEFORE INSERT ON memory_events
         WHEN NEW.action = 'version_restored'
         BEGIN SELECT RAISE(ABORT, 'restore audit rejected'); END",
    )
    .execute(&fixture.pool)
    .await
    .unwrap();
    let internal = client
        .post(fixture.url(&format!("/v1/memories/{memory_id}/versions/1/restore")))
        .json(&serde_json::json!({
            "expected_current_version": 3,
            "expected_status": "candidate",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        internal.status(),
        reqwest::StatusCode::INTERNAL_SERVER_ERROR
    );
}
