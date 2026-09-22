#[path = "support/temp_root.rs"]
mod temp_root;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use futures_util::StreamExt;
use praxis_lib::db;
use praxis_lib::runner::auth::RunnerAuth;
use praxis_lib::runner::config::RunnerConfig;
use praxis_lib::runner::events::EventHub;
use praxis_lib::runner::http::{self, RunnerHttpState};
use praxis_lib::runner::queue::QueueWorker;
use praxis_lib::runner::{create_queued_task, QueuedTaskRequest};

static COUNTER: AtomicU32 = AtomicU32::new(0);

#[tokio::test]
async fn versioned_health_tasks_schedule_and_event_replay_are_read_only() {
    let (pool, db_path) = test_pool("api").await;
    praxis_lib::schedule::migrate(&pool).await.unwrap();
    let task_id = db::insert_task(
        &pool,
        "/tmp",
        "branch",
        "main",
        "/tmp",
        "http marker",
        None,
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();
    db::append_runner_event(&pool, task_id, 2, "queued", None)
        .await
        .unwrap();
    db::append_runner_event(&pool, task_id, 3, "running", None)
        .await
        .unwrap();
    let schedule_id =
        db::insert_schedule(&pool, "http schedule", "0 * * * * *", "task", "{}", 1, 0)
            .await
            .unwrap();
    let (address, server, token_path) = serve(pool.clone()).await;

    let client = authenticated_client();
    let health: serde_json::Value = client
        .get(format!("http://{address}/v1/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(health["status"], "ok");
    assert_eq!(health["max_concurrent_tasks"], 2);

    let tasks: Vec<db::Task> = client
        .get(format!("http://{address}/v1/tasks"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, task_id);

    let schedules: Vec<db::Schedule> = client
        .get(format!("http://{address}/v1/schedules"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(schedules[0].id, schedule_id);

    let events: Vec<db::RunnerEvent> = client
        .get(format!("http://{address}/v1/events?after=1"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        events
            .iter()
            .map(|event| event.sequence)
            .collect::<Vec<_>>(),
        vec![2]
    );
    assert_eq!(
        db::get_task(&pool, task_id).await.unwrap().unwrap().state,
        db::state::CREATED
    );

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
}

#[tokio::test]
async fn quickopen_endpoint_matches_query_and_filters_by_scope() {
    let (pool, db_path) = test_pool("quickopen").await;
    praxis_lib::schedule::migrate(&pool).await.unwrap();
    db::insert_task(
        &pool,
        "/tmp",
        "b1",
        "main",
        "/tmp/w1",
        "fix login bug",
        None,
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();
    db::insert_task(
        &pool,
        "/tmp",
        "b2",
        "main",
        "/tmp/w2",
        "fix login redirect",
        None,
        None,
        "conversation",
        2,
    )
    .await
    .unwrap();
    let (address, server, token_path) = serve(pool.clone()).await;
    let client = authenticated_client();

    let all: Vec<serde_json::Value> = client
        .get(format!("http://{address}/v1/quickopen?query=login"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        all.len(),
        2,
        "runner도 local과 동일한 db::quickopen_search를 공유"
    );

    let sessions_only: Vec<serde_json::Value> = client
        .get(format!(
            "http://{address}/v1/quickopen?query=login&scopes=session"
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(sessions_only.len(), 1);
    assert_eq!(sessions_only[0]["scope"], "session");

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
}

#[tokio::test]
async fn runner_http_round_trips_goal_contract_and_rejects_invalid_paths() {
    let (pool, db_path) = test_pool("goal-contract").await;
    let root = temp_root::dir().join(format!(
        "praxis-runner-goal-contract-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "runner@example.test"]);
    git(&root, &["config", "user.name", "Runner Test"]);
    std::fs::write(root.join("README.md"), "goal contract\n").unwrap();
    git(&root, &["add", "README.md"]);
    git(&root, &["commit", "-qm", "initial"]);
    let canonical = root.canonicalize().unwrap();
    let (address, server, token_path) = serve_with_roots(pool.clone(), vec![canonical]).await;
    let client = authenticated_client();
    let request = serde_json::json!({
        "repository": root,
        "instruction": "ship it",
        "agent": "codex",
        "model": "gpt-5.6-luna",
        "reasoning_effort": "max",
        "mode": "terminal",
        "goal_contract": {
            "schema_version": 1,
            "objective": "ship with evidence",
            "acceptance": ["tests pass"],
            "stop_conditions": [],
            "must_preserve": [],
            "protected_paths": ["deploy/**"],
            "non_goals": ["auto merge"]
        }
    });

    let task: db::Task = client
        .post(format!("http://{address}/v1/tasks"))
        .json(&request)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        task.goal_contract
            .as_deref()
            .map(|contract| contract.objective.as_str()),
        Some("ship with evidence")
    );
    assert_eq!(task.model.as_deref(), Some("gpt-5.6-luna"));
    assert_eq!(task.reasoning_effort.as_deref(), Some("max"));

    let mut legacy = request.clone();
    legacy.as_object_mut().unwrap().remove("goal_contract");
    legacy["instruction"] = serde_json::json!("legacy instruction");
    let legacy_task: db::Task = client
        .post(format!("http://{address}/v1/tasks"))
        .json(&legacy)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(legacy_task.goal_contract.is_none());

    let mut unsupported_effort = request.clone();
    unsupported_effort["reasoning_effort"] = serde_json::json!("ultra");
    let response = client
        .post(format!("http://{address}/v1/tasks"))
        .json(&unsupported_effort)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);

    let mut invalid = request;
    invalid["goal_contract"]["protected_paths"] = serde_json::json!(["../secrets"]);
    let response = client
        .post(format!("http://{address}/v1/tasks"))
        .json(&invalid)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(db::list_tasks(&pool).await.unwrap().len(), 2);

    server.abort();
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
}

#[tokio::test]
async fn websocket_replays_through_watermark_then_emits_only_new_events() {
    let (pool, db_path) = test_pool("websocket").await;
    let task_id = db::insert_task(
        &pool,
        "/tmp",
        "branch",
        "main",
        "/tmp",
        "ws marker",
        None,
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();
    db::append_runner_event(&pool, task_id, 2, "queued", None)
        .await
        .unwrap();
    let (address, server, token_path) = serve(pool.clone()).await;

    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    let mut request = format!("ws://{address}/v1/events/live?after=0")
        .into_client_request()
        .unwrap();
    request.headers_mut().insert(
        "Authorization",
        format!("Bearer {TEST_TOKEN}").parse().unwrap(),
    );
    let (mut socket, _) = tokio_tungstenite::connect_async(request).await.unwrap();
    let watermark = receive_json(&mut socket).await;
    assert_eq!(watermark["kind"], "watermark");
    let replay = receive_json(&mut socket).await;
    assert_eq!(replay["sequence"], 1);

    db::append_runner_event(&pool, task_id, 3, "running", None)
        .await
        .unwrap();
    let live = receive_json(&mut socket).await;
    assert_eq!(live["sequence"], 2);
    assert_eq!(live["kind"], "running");

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
}

#[tokio::test]
async fn token_is_required_for_every_versioned_endpoint() {
    let (pool, db_path) = test_pool("auth").await;
    let (address, server, token_path) = serve(pool).await;
    let response = reqwest::get(format!("http://{address}/v1/health"))
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert!(response.text().await.unwrap().is_empty());

    let wrong = reqwest::Client::builder()
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", "ef".repeat(32)).parse().unwrap(),
            );
            headers
        })
        .build()
        .unwrap()
        .get(format!("http://{address}/v1/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(wrong.status(), reqwest::StatusCode::UNAUTHORIZED);

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
}

#[tokio::test]
async fn repositories_discovers_git_repos_under_configured_roots() {
    let (pool, db_path) = test_pool("repo-discovery").await;
    // 부모 디렉터리 root 아래: 직계 repo, 2단계 repo, git 아닌 디렉터리, 숨김 밑 repo.
    let parent = temp_root::dir().join(format!(
        "praxis-runner-http-discovery-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let direct = git_repository("direct");
    let nested_parent = parent.join("team");
    let deep_parent = parent.join("a").join("b").join("c");
    let hidden = parent.join(".hidden");
    std::fs::create_dir_all(parent.join("plain")).unwrap();
    std::fs::create_dir_all(&nested_parent).unwrap();
    std::fs::create_dir_all(&deep_parent).unwrap();
    std::fs::create_dir_all(&hidden).unwrap();
    let move_repo = |from: &std::path::Path, to: &std::path::Path| {
        std::fs::rename(from, to).unwrap();
    };
    move_repo(&direct, &parent.join("direct-repo"));
    move_repo(
        &git_repository("nested"),
        &nested_parent.join("nested-repo"),
    );
    move_repo(&git_repository("deep"), &deep_parent.join("deep-repo"));
    move_repo(&git_repository("hidden"), &hidden.join("secret-repo"));

    let (address, server, token_path) = serve_with_roots(pool, vec![parent.clone()]).await;
    let listed: Vec<String> = authenticated_client()
        .get(format!("http://{address}/v1/repositories"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    let has = |suffix: &str| listed.iter().any(|path| path.ends_with(suffix));
    assert!(has("direct-repo"), "{listed:?}");
    assert!(has("nested-repo"), "{listed:?}");
    // 깊이 제한 없음 — 3단계 이상도 스캔 예산 안에서 발견된다.
    assert!(has("deep-repo"), "{listed:?}");
    // git 아닌 디렉터리(root 자신 포함)와 숨김 디렉터리 밑은 목록에 없다.
    assert!(!has("plain"), "{listed:?}");
    assert!(!has("secret-repo"), "{listed:?}");
    assert!(!listed.iter().any(|path| path == &parent.to_string_lossy()));

    // root 자신이 repository면 그대로 1건.
    let own_repo = git_repository("own");
    let (pool2, db_path2) = test_pool("repo-discovery-own").await;
    let (address2, server2, token_path2) = serve_with_roots(pool2, vec![own_repo.clone()]).await;
    let own_listed: Vec<String> = authenticated_client()
        .get(format!("http://{address2}/v1/repositories"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(own_listed, vec![own_repo.to_string_lossy().into_owned()]);

    server.abort();
    server2.abort();
    let _ = std::fs::remove_dir_all(parent);
    let _ = std::fs::remove_dir_all(own_repo);
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(db_path2);
    let _ = std::fs::remove_file(token_path);
    let _ = std::fs::remove_file(token_path2);
}

#[tokio::test]
async fn browser_cors_preflight_and_response_headers_are_supported() {
    let (pool, db_path) = test_pool("cors").await;
    let (address, server, token_path) = serve(pool).await;

    // 브라우저 preflight는 Authorization 없이 오며, 인증보다 먼저 응답해야 한다.
    let preflight = reqwest::Client::new()
        .request(
            reqwest::Method::OPTIONS,
            format!("http://{address}/v1/health"),
        )
        .header("Origin", "http://localhost:1420")
        .header("Access-Control-Request-Method", "GET")
        .header("Access-Control-Request-Headers", "authorization")
        .send()
        .await
        .unwrap();
    assert_eq!(preflight.status(), reqwest::StatusCode::NO_CONTENT);
    assert_eq!(
        preflight
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("*")
    );
    let allow_headers = preflight
        .headers()
        .get("access-control-allow-headers")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_ascii_lowercase();
    assert!(allow_headers.contains("authorization"), "{allow_headers}");

    // 실제 응답에도 ACAO가 실려야 브라우저가 본문을 넘겨준다. 인증 게이트는 그대로다.
    let authed = authenticated_client()
        .get(format!("http://{address}/v1/health"))
        .header("Origin", "http://localhost:1420")
        .send()
        .await
        .unwrap();
    assert_eq!(authed.status(), reqwest::StatusCode::OK);
    assert_eq!(
        authed
            .headers()
            .get("access-control-allow-origin")
            .and_then(|value| value.to_str().ok()),
        Some("*")
    );

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
}

#[tokio::test]
async fn authenticated_file_and_diff_handlers_enforce_repository_roots() {
    let (pool, db_path) = test_pool("files").await;
    let root = temp_root::dir().join(format!(
        "praxis-runner-http-repo-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["config", "user.email", "runner@example.test"]);
    git(&root, &["config", "user.name", "Runner Test"]);
    std::fs::write(root.join("note.txt"), "before\n").unwrap();
    git(&root, &["add", "note.txt"]);
    git(&root, &["commit", "-qm", "initial"]);
    std::fs::write(root.join("note.txt"), "after\n").unwrap();
    let root_text = root.to_string_lossy().into_owned();
    let task_id = db::insert_task(
        &pool, &root_text, "branch", "main", &root_text, "diff", None, None, "terminal", 1,
    )
    .await
    .unwrap();
    let (address, server, token_path) =
        serve_with_roots(pool, vec![root.canonicalize().unwrap()]).await;
    let client = authenticated_client();

    let tree: serde_json::Value = client
        .get(format!("http://{address}/v1/files/tree"))
        .query(&[("repository", &root_text)])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(tree
        .as_array()
        .unwrap()
        .iter()
        .any(|entry| entry["name"] == "note.txt"));
    let file: serde_json::Value = client
        .get(format!("http://{address}/v1/files/read"))
        .query(&[
            ("repository", &root_text),
            ("path", &"note.txt".to_string()),
        ])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(file["content"], "after\n");
    let diff: serde_json::Value = client
        .get(format!("http://{address}/v1/tasks/{task_id}/diff"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(diff["files"]
        .as_array()
        .expect("diff 응답은 files와 baseline을 함께 싣는다")
        .iter()
        .any(|entry| entry["path"] == "note.txt"));
    assert!(
        diff["baseline"]["kind"].is_string(),
        "화면이 근사치를 보고 있는지 알려면 기준점 상태가 함께 와야 한다: {diff}"
    );

    let hunks: serde_json::Value = client
        .get(format!("http://{address}/v1/tasks/{task_id}/diff/hunks"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let hunks = hunks.as_array().unwrap();
    assert!(
        hunks.iter().any(|entry| entry["path"] == "note.txt"
            && entry["risk"] == "low"
            && entry["protected"] == false),
        "hunks: {hunks:?}"
    );

    // B-1: 주석 저장/목록/재전송 — local(commands.rs)과 동일한 형식을 runner HTTP로도 노출.
    let hunk_id = hunks[0]["id"].as_str().unwrap().to_string();
    let created: serde_json::Value = client
        .post(format!("http://{address}/v1/tasks/{task_id}/annotations"))
        .json(&serde_json::json!({
            "hunk_id": hunk_id,
            "path": "note.txt",
            "line": 1,
            "side": "new",
            "body_md": "고쳐줘"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["status"], "draft");
    let annotation_id = created["id"].as_str().unwrap().to_string();

    let updated: serde_json::Value = client
        .post(format!("http://{address}/v1/tasks/{task_id}/annotations"))
        .json(&serde_json::json!({
            "id": annotation_id,
            "hunk_id": hunk_id,
            "path": "note.txt",
            "line": 1,
            "side": "new",
            "body_md": "코멘트 수정"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        updated["body_md"], "코멘트 수정",
        "draft 본문 자동 저장(onBlur)"
    );

    let listed: serde_json::Value = client
        .get(format!("http://{address}/v1/tasks/{task_id}/annotations"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let listed = listed.as_array().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(
        listed[0]["matched_hunk_id"], hunk_id,
        "현재 diff에 hunk_id로 재매칭됨"
    );
    assert_eq!(listed[0]["orphaned"], false);

    // task는 mode=terminal이라 대화 재개 가드가 동기적으로 거부 — status는 draft로 롤백된다
    // (실제 벤더 프로세스는 전혀 건드리지 않음 — resolve/spawn 이전에 거부).
    let resend = client
        .post(format!(
            "http://{address}/v1/tasks/{task_id}/annotations/resend"
        ))
        .json(&serde_json::json!({ "ids": [annotation_id] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resend.status(), reqwest::StatusCode::CONFLICT);
    let after_rollback: serde_json::Value = client
        .get(format!("http://{address}/v1/tasks/{task_id}/annotations"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        after_rollback[0]["status"], "draft",
        "resume 동기 가드 실패는 status를 draft로 롤백한다(부분 갱신 금지)"
    );

    let denied = client
        .get(format!("http://{address}/v1/files/tree"))
        .query(&[("repository", &"/tmp".to_string())])
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn skills_endpoint_lists_repository_skills_and_enforces_roots() {
    let (pool, db_path) = test_pool("skills").await;
    let root = temp_root::dir().join(format!(
        "praxis-runner-http-skills-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let outside = temp_root::dir().join(format!(
        "praxis-runner-http-outside-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let skill_dir = root.join(".claude").join("skills").join("remote-demo");
    std::fs::create_dir_all(&skill_dir).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(
        skill_dir.join("SKILL.md"),
        "---\nname: remote-demo\ndescription: 원격 세션 스킬 목록 픽스처\n---\n\n본문\n",
    )
    .unwrap();
    let root = root.canonicalize().unwrap();
    let root_text = root.to_string_lossy().into_owned();
    let (address, server, token_path) = serve_with_roots(pool, vec![root.clone()]).await;
    let client = authenticated_client();

    let skills: serde_json::Value = client
        .get(format!("http://{address}/v1/skills"))
        .query(&[("repository", &root_text)])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let listed = skills.as_array().expect("목록은 배열이다");
    let demo = listed
        .iter()
        .find(|entry| entry["name"] == "remote-demo")
        .unwrap_or_else(|| panic!("저장소 스킬이 목록에 없다: {listed:?}"));
    assert_eq!(demo["description"], "원격 세션 스킬 목록 픽스처");
    assert_eq!(demo["global"], false, "저장소 스킬은 프로젝트 스코프다");

    let denied = client
        .get(format!("http://{address}/v1/skills"))
        .query(&[("repository", &outside.to_string_lossy().into_owned())])
        .send()
        .await
        .unwrap();
    assert_eq!(denied.status(), reqwest::StatusCode::FORBIDDEN);

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[tokio::test]
async fn task_finalization_endpoints_merge_or_discard_review_worktrees() {
    let (pool, db_path) = test_pool("finalize").await;
    let root = git_repository("finalize");
    let root = root.canonicalize().unwrap();
    let config = runner_config(vec![root.clone()]);
    let approved = create_review_task(&config, &pool, &root, "approved", "approved\n").await;
    let discarded = create_review_task(&config, &pool, &root, "discarded", "discarded\n").await;
    let protected =
        create_review_task_with_contract(&config, &pool, &root, "protected", "protected\n").await;
    let (address, server, token_path) = serve_with_roots(pool.clone(), vec![root.clone()]).await;
    let client = authenticated_client();

    let approve = client
        .post(format!("http://{address}/v1/tasks/{approved}/approve"))
        .send()
        .await
        .unwrap();
    assert_eq!(approve.status(), reqwest::StatusCode::NO_CONTENT);
    assert_eq!(
        std::fs::read_to_string(root.join("note.txt")).unwrap(),
        "approved\n"
    );

    let blocked = client
        .post(format!("http://{address}/v1/tasks/{protected}/approve"))
        .send()
        .await
        .unwrap();
    assert_eq!(blocked.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(
        db::get_task(&pool, protected).await.unwrap().unwrap().state,
        db::state::AWAITING_REVIEW
    );
    assert_eq!(
        std::fs::read_to_string(root.join("note.txt")).unwrap(),
        "approved\n"
    );

    let protected_discard = client
        .post(format!("http://{address}/v1/tasks/{protected}/discard"))
        .send()
        .await
        .unwrap();
    assert_eq!(protected_discard.status(), reqwest::StatusCode::NO_CONTENT);
    assert_eq!(
        db::get_task(&pool, approved).await.unwrap().unwrap().state,
        db::state::DONE
    );

    let discard = client
        .post(format!("http://{address}/v1/tasks/{discarded}/discard"))
        .send()
        .await
        .unwrap();
    assert_eq!(discard.status(), reqwest::StatusCode::NO_CONTENT);
    assert_eq!(
        db::get_task(&pool, discarded).await.unwrap().unwrap().state,
        db::state::DISCARDED
    );
    assert_eq!(
        std::fs::read_to_string(root.join("note.txt")).unwrap(),
        "approved\n"
    );

    db::append_task_output(&pool, discarded, 3, "discard output")
        .await
        .unwrap();
    let delete = client
        .delete(format!("http://{address}/v1/tasks/{discarded}"))
        .send()
        .await
        .unwrap();
    assert_eq!(delete.status(), reqwest::StatusCode::NO_CONTENT);
    assert!(db::get_task(&pool, discarded).await.unwrap().is_none());
    assert!(db::list_runner_events_after(&pool, 0, 100)
        .await
        .unwrap()
        .iter()
        .all(|event| event.task_id != discarded));
    assert!(db::list_task_output_after(&pool, 0, 100)
        .await
        .unwrap()
        .iter()
        .all(|output| output.task_id != discarded));

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
    let _ = std::fs::remove_dir_all(root);
}

#[tokio::test]
async fn runner_memory_and_context_endpoints_use_the_server_owned_store() {
    let (pool, db_path) = test_pool("memory").await;
    let root = git_repository("memory").canonicalize().unwrap();
    let root_text = root.to_string_lossy().into_owned();
    let (address, server, token_path) = serve_with_roots(pool.clone(), vec![root.clone()]).await;
    let client = authenticated_client();

    let memory_id: i64 = client
        .post(format!("http://{address}/v1/memories"))
        .json(&serde_json::json!({
            "repository": root_text,
            "kind": "decision",
            "content": "runner memory endpoint"
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
        .post(format!(
            "http://{address}/v1/memories/{memory_id}/evidence/code-locations"
        ))
        .json(&serde_json::json!({
            "relative_path": "note.txt",
            "line_start": 1,
            "line_end": 1
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
    for path in [
        format!("/v1/memories/{memory_id}/confirmations"),
        format!("/v1/memories/{memory_id}/review"),
        format!("/v1/memories/{memory_id}/approve"),
    ] {
        client
            .post(format!("http://{address}{path}"))
            .json(&serde_json::json!({ "expires_at": null }))
            .send()
            .await
            .unwrap()
            .error_for_status()
            .unwrap();
    }
    let outside = root.with_extension("outside");
    std::fs::create_dir_all(&outside).unwrap();
    let hidden_id = praxis_lib::memory::create_candidate(
        &pool,
        praxis_lib::memory::tier::PROJECT,
        Some(outside.to_string_lossy().as_ref()),
        praxis_lib::memory::knowledge_type::CLAIM,
        "out of scope",
        Some("test"),
        1,
    )
    .await
    .unwrap();
    assert_eq!(
        client
            .post(format!(
                "http://{address}/v1/memories/{hidden_id}/confirmations"
            ))
            .json(&serde_json::json!({ "expires_at": null }))
            .send()
            .await
            .unwrap()
            .status(),
        reqwest::StatusCode::BAD_REQUEST
    );
    let memories: serde_json::Value = client
        .get(format!("http://{address}/v1/memories"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(memories[0]["status"], "verified");
    assert_eq!(memories[0]["evidence_count"], 2);
    assert_eq!(memories[0]["blocking_evidence_count"], 0);
    assert_eq!(memories.as_array().unwrap().len(), 1);
    assert_eq!(memories[0]["scope_key"], root.to_string_lossy().as_ref());
    let preview: serde_json::Value = client
        .post(format!("http://{address}/v1/memories/preview"))
        .json(&serde_json::json!({
            "repository": root,
            "instruction": "runner memory endpoint"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(preview.as_array().unwrap().len(), 1);

    let task: db::Task = client
        .post(format!("http://{address}/v1/tasks"))
        .json(&serde_json::json!({
            "repository": root_text,
            "instruction": "runner memory endpoint",
            "agent": "claude",
            "mode": "terminal"
        }))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    let report: serde_json::Value = client
        .get(format!("http://{address}/v1/tasks/{}/context", task.id))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(report["memory_count"], 1);
    assert_eq!(report["memory_counts"]["scope_total"], 1);
    assert_eq!(report["memory_counts"]["actionable"], 0);
    assert_eq!(report["memory_counts"]["verified"], 1);
    assert_eq!(report["memory_counts"]["eligible"], 1);
    // 파일형 투영(설계 2026-09-13)은 DB 원장을 남기지 않는다 — 보고서의 투영 칸은 비어 있다.
    assert!(report["projection"].is_null(), "{report}");
    // 보고서는 벤더 × {글로벌, 프로젝트} 후보를 모두 싣는다 — 지금 투영이 쓰는 파일은
    // AGENTS.md 하나뿐(ADR 2026-09-19)이라 위치로 고르면 없는 파일이나 글로벌 파일을 집는다.
    // 블록을 실제로 가진 것이 이 작업의 컨텍스트 파일이다.
    let context_path = report["vendors"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|v| v["files"].as_array().unwrap())
        .find(|f| f["has_praxis_block"] == true)
        .and_then(|f| f["path"].as_str())
        .expect("투영된 컨텍스트 파일이 보고서에 있어야 한다");
    let content: String = client
        .get(format!(
            "http://{address}/v1/tasks/{}/context/file",
            task.id
        ))
        .query(&[("path", context_path)])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    // 컨텍스트 파일에는 창고 파일의 사본(규칙 포함)이 실린다.
    assert!(content.contains("Praxis Memory"), "{content}");
    let evidence: serde_json::Value = client
        .get(format!("http://{address}/v1/memories/{memory_id}/evidence"))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(evidence.as_array().unwrap().len(), 2);
    std::fs::write(root.join("note.txt"), "changed\n").unwrap();
    let revalidation: serde_json::Value = client
        .post(format!(
            "http://{address}/v1/memories/{memory_id}/revalidate"
        ))
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(revalidation["stale"], true);
    let stale_memories: serde_json::Value = client
        .get(format!("http://{address}/v1/memories"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(stale_memories[0]["evidence_count"], 2);
    assert_eq!(stale_memories[0]["blocking_evidence_count"], 1);

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
    let _ = std::fs::remove_dir_all(root);
    let _ = std::fs::remove_dir_all(outside);
}

#[tokio::test]
async fn conversation_message_requeues_review_task_with_new_instruction() {
    let (pool, db_path) = test_pool("message").await;
    let convo_id = db::insert_task(
        &pool,
        "/tmp",
        "branch",
        "main",
        "/tmp",
        "first ask",
        Some("claude"),
        None,
        "conversation",
        1,
    )
    .await
    .unwrap();
    db::update_state(&pool, convo_id, db::state::AWAITING_REVIEW, 2)
        .await
        .unwrap();
    db::set_convo_session(&pool, convo_id, "session-1")
        .await
        .unwrap();
    let (address, server, token_path) = serve(pool.clone()).await;
    let client = authenticated_client();

    let accepted = client
        .post(format!("http://{address}/v1/tasks/{convo_id}/message"))
        .json(&serde_json::json!({ "message": "follow-up ask" }))
        .send()
        .await
        .unwrap();
    assert_eq!(accepted.status(), reqwest::StatusCode::NO_CONTENT);
    let task = db::get_task(&pool, convo_id).await.unwrap().unwrap();
    assert_eq!(task.state, db::state::QUEUED);
    assert_eq!(task.instruction, "follow-up ask");
    // resume 근거는 그대로 남아야 다음 턴이 이전 세션으로 이어진다.
    assert_eq!(task.convo_session_id.as_deref(), Some("session-1"));
    assert!(db::list_runner_events_after(&pool, 0, 100)
        .await
        .unwrap()
        .iter()
        .any(|event| event.task_id == convo_id && event.kind == "queued"));

    // 이미 Queued — 턴 진행/대기 중 중복 재큐잉 금지.
    let busy = client
        .post(format!("http://{address}/v1/tasks/{convo_id}/message"))
        .json(&serde_json::json!({ "message": "again" }))
        .send()
        .await
        .unwrap();
    assert_eq!(busy.status(), reqwest::StatusCode::CONFLICT);

    let blank = client
        .post(format!("http://{address}/v1/tasks/{convo_id}/message"))
        .json(&serde_json::json!({ "message": "  " }))
        .send()
        .await
        .unwrap();
    assert_eq!(blank.status(), reqwest::StatusCode::BAD_REQUEST);

    let missing = client
        .post(format!("http://{address}/v1/tasks/9999/message"))
        .json(&serde_json::json!({ "message": "hello" }))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
}

#[tokio::test]
async fn conversation_message_rejects_terminal_tasks() {
    let (pool, db_path) = test_pool("message-terminal").await;
    let terminal_id = db::insert_task(
        &pool,
        "/tmp",
        "branch",
        "main",
        "/tmp",
        "terminal ask",
        Some("claude"),
        None,
        "terminal",
        1,
    )
    .await
    .unwrap();
    db::update_state(&pool, terminal_id, db::state::AWAITING_REVIEW, 2)
        .await
        .unwrap();
    let (address, server, token_path) = serve(pool.clone()).await;

    let response = authenticated_client()
        .post(format!("http://{address}/v1/tasks/{terminal_id}/message"))
        .json(&serde_json::json!({ "message": "hello" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);
    assert_eq!(
        db::get_task(&pool, terminal_id)
            .await
            .unwrap()
            .unwrap()
            .state,
        db::state::AWAITING_REVIEW
    );

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
}

/// 원격 세션도 대화 도중 모델을 바꾼다 — 데스크톱 `task_model_set`의 짝이다.
///
/// 진행 중인 턴은 안 바뀐다. 다음 턴이 행을 새로 읽으므로 여기서 쓴 값이 그때 실린다.
#[tokio::test]
async fn conversation_model_override_is_replaced_for_the_next_turn() {
    let (pool, db_path) = test_pool("model").await;
    let convo_id = db::insert_task(
        &pool,
        "/tmp",
        "branch",
        "main",
        "/tmp",
        "first ask",
        Some("claude"),
        None,
        "conversation",
        1,
    )
    .await
    .unwrap();
    db::update_state(&pool, convo_id, db::state::AWAITING_REVIEW, 2)
        .await
        .unwrap();
    db::set_task_model(&pool, convo_id, "haiku").await.unwrap();
    let (address, server, token_path) = serve(pool.clone()).await;
    let client = authenticated_client();

    let replaced = client
        .put(format!("http://{address}/v1/tasks/{convo_id}/model"))
        .json(&serde_json::json!({ "model": "  opus  " }))
        .send()
        .await
        .unwrap();
    assert_eq!(replaced.status(), reqwest::StatusCode::NO_CONTENT);
    assert_eq!(
        db::get_task(&pool, convo_id)
            .await
            .unwrap()
            .unwrap()
            .model
            .as_deref(),
        Some("opus")
    );
    // 데스크톱과 같은 본체를 지나므로 컨텍스트 무효화도 함께 적재된다 — 이것이 없으면 원격
    // 게이지가 옛 모델의 윈도로 잔량을 계산한다.
    assert!(db::list_convo_events(&pool, convo_id)
        .await
        .unwrap()
        .iter()
        .any(|event| event.contains("model_change")));

    // 빈 문자열은 해제다 — 원격만 벤더 기본으로 못 돌아가면 세션 헤더가 막다른 길이 된다.
    let cleared = client
        .put(format!("http://{address}/v1/tasks/{convo_id}/model"))
        .json(&serde_json::json!({ "model": "" }))
        .send()
        .await
        .unwrap();
    assert_eq!(cleared.status(), reqwest::StatusCode::NO_CONTENT);
    assert!(db::get_task(&pool, convo_id)
        .await
        .unwrap()
        .unwrap()
        .model
        .unwrap_or_default()
        .trim()
        .is_empty());

    // 없는 작업을 400으로 돌려주면 클라이언트가 "모델 이름이 틀렸다"로 읽는다.
    let missing = client
        .put(format!("http://{address}/v1/tasks/9999/model"))
        .json(&serde_json::json!({ "model": "opus" }))
        .send()
        .await
        .unwrap();
    assert_eq!(missing.status(), reqwest::StatusCode::NOT_FOUND);

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
}

/// 커스텀 에이전트는 `agent_args`의 커스텀 분기가 model을 아예 싣지 않는다 — 저장해도 CLI에
/// 닿지 못한다. 데스크톱과 같은 게이트가 원격에도 걸려야 두 경로가 갈라지지 않는다.
#[tokio::test]
async fn model_override_is_refused_for_a_custom_agent() {
    let (pool, db_path) = test_pool("model-custom").await;
    let convo_id = db::insert_task(
        &pool,
        "/tmp",
        "branch",
        "main",
        "/tmp",
        "first ask",
        Some("mybin --foo"),
        None,
        "conversation",
        1,
    )
    .await
    .unwrap();
    let (address, server, token_path) = serve(pool.clone()).await;

    let refused = authenticated_client()
        .put(format!("http://{address}/v1/tasks/{convo_id}/model"))
        .json(&serde_json::json!({ "model": "opus" }))
        .send()
        .await
        .unwrap();
    assert_eq!(refused.status(), reqwest::StatusCode::BAD_REQUEST);
    assert!(refused
        .text()
        .await
        .unwrap()
        .contains("모델을 바꿀 수 없는 에이전트입니다"));
    assert!(db::get_task(&pool, convo_id)
        .await
        .unwrap()
        .unwrap()
        .model
        .unwrap_or_default()
        .is_empty());

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
}

#[tokio::test]
async fn terminal_input_requires_an_active_pty_session() {
    let (pool, db_path) = test_pool("input").await;
    let (address, server, token_path) = serve(pool).await;

    let response = authenticated_client()
        .post(format!("http://{address}/v1/tasks/42/input"))
        .json(&serde_json::json!({ "data": "hello\r" }))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), reqwest::StatusCode::CONFLICT);

    server.abort();
    let _ = std::fs::remove_file(db_path);
    let _ = std::fs::remove_file(token_path);
}

async fn test_pool(label: &str) -> (sqlx::SqlitePool, String) {
    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let db_path = temp_root::dir()
        .join(format!(
            "praxis-runner-http-{label}-{}-{suffix}.sqlite",
            std::process::id()
        ))
        .to_string_lossy()
        .into_owned();
    let pool = db::init_pool(&db_path).await.unwrap();
    praxis_lib::memory::migrate(&pool).await.unwrap();
    praxis_lib::runner::finalization::migrate(&pool)
        .await
        .unwrap();
    praxis_lib::annotations::migrate(&pool).await.unwrap();
    (pool, db_path)
}

async fn create_review_task(
    config: &RunnerConfig,
    pool: &sqlx::SqlitePool,
    root: &std::path::Path,
    instruction: &str,
    content: &str,
) -> i64 {
    let task = create_queued_task(
        config,
        pool,
        &praxis_lib::runner::worktree_lock::WorktreeLocks::default(),
        QueuedTaskRequest {
            repository: root.to_string_lossy().into_owned(),
            instruction: instruction.to_string(),
            agent: "claude".to_string(),
            role: "implementer".to_string(),
            model: String::new(),
            reasoning_effort: String::new(),
            mode: "terminal".to_string(),
            goal_contract: None,
            resume_session: None,
        },
        1,
    )
    .await
    .unwrap();
    std::fs::write(
        std::path::Path::new(&task.worktree_path).join("note.txt"),
        content,
    )
    .unwrap();
    db::transition_state_with_runner_event(
        pool,
        task.id,
        db::state::AWAITING_REVIEW,
        2,
        "review",
        None,
    )
    .await
    .unwrap();
    task.id
}

async fn create_review_task_with_contract(
    config: &RunnerConfig,
    pool: &sqlx::SqlitePool,
    root: &std::path::Path,
    instruction: &str,
    content: &str,
) -> i64 {
    let task = create_queued_task(
        config,
        pool,
        &praxis_lib::runner::worktree_lock::WorktreeLocks::default(),
        QueuedTaskRequest {
            repository: root.to_string_lossy().into_owned(),
            instruction: instruction.to_string(),
            agent: "claude".to_string(),
            role: "implementer".to_string(),
            model: String::new(),
            reasoning_effort: String::new(),
            mode: "terminal".to_string(),
            goal_contract: Some(praxis_lib::goal_contract::GoalContract {
                schema_version: 1,
                objective: instruction.to_string(),
                acceptance: vec![],
                stop_conditions: vec![],
                must_preserve: vec![],
                protected_paths: vec!["note.txt".to_string()],
                non_goals: vec![],
            }),
            resume_session: None,
        },
        1,
    )
    .await
    .unwrap();
    std::fs::write(
        std::path::Path::new(&task.worktree_path).join("note.txt"),
        content,
    )
    .unwrap();
    db::transition_state_with_runner_event(
        pool,
        task.id,
        db::state::AWAITING_REVIEW,
        2,
        "review",
        None,
    )
    .await
    .unwrap();
    task.id
}

fn git_repository(label: &str) -> std::path::PathBuf {
    let root = temp_root::dir().join(format!(
        "praxis-runner-http-repo-{label}-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q"]);
    git(&root, &["config", "user.email", "runner@example.test"]);
    git(&root, &["config", "user.name", "Runner Test"]);
    std::fs::write(root.join("note.txt"), "before\n").unwrap();
    git(&root, &["add", "note.txt"]);
    git(&root, &["commit", "-qm", "initial"]);
    root
}

fn runner_config(repository_roots: Vec<std::path::PathBuf>) -> RunnerConfig {
    RunnerConfig {
        bind: "127.0.0.1:47831".parse().unwrap(),
        repository_roots,
        max_concurrent_tasks: 2,
        execution_policy: praxis_lib::runner::config::ExecutionPolicy::AlwaysApprove,
        pairing_token_file: temp_root::dir().join("unused-runner-token"),
    }
}

const TEST_TOKEN: &str = "abababababababababababababababababababababababababababababababab";

async fn serve(pool: sqlx::SqlitePool) -> (SocketAddr, tokio::task::JoinHandle<()>, String) {
    serve_with_roots(pool, vec![]).await
}

async fn serve_with_roots(
    pool: sqlx::SqlitePool,
    repository_roots: Vec<std::path::PathBuf>,
) -> (SocketAddr, tokio::task::JoinHandle<()>, String) {
    let token_path = write_token_file();
    let config = RunnerConfig {
        bind: "127.0.0.1:47831".parse().unwrap(),
        repository_roots,
        max_concurrent_tasks: 2,
        execution_policy: praxis_lib::runner::config::ExecutionPolicy::AlwaysApprove,
        pairing_token_file: token_path.clone().into(),
    };
    let queue = QueueWorker::new(pool.clone(), 2);
    let state = RunnerHttpState {
        auth: RunnerAuth::from_file(token_path.as_ref()).unwrap(),
        events: EventHub::start(pool.clone()),
        pool,
        config,
        recovered_tasks: 0,
        queue,
        started_at: 0,
        review_claims: Default::default(),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            http::router(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    (address, server, token_path)
}

fn git(root: &std::path::Path, args: &[&str]) {
    let status = std::process::Command::new("git")
        .current_dir(root)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn authenticated_client() -> reqwest::Client {
    reqwest::Client::builder()
        .default_headers({
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {TEST_TOKEN}").parse().unwrap(),
            );
            headers
        })
        .build()
        .unwrap()
}

fn write_token_file() -> String {
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    let suffix = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = temp_root::dir().join(format!(
        "praxis-runner-token-{}-{suffix}",
        std::process::id()
    ));
    std::fs::write(&path, TEST_TOKEN).unwrap();
    #[cfg(unix)]
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    path.to_string_lossy().into_owned()
}

async fn receive_json<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>) -> serde_json::Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    serde_json::from_str(message.into_text().unwrap().as_ref()).unwrap()
}
