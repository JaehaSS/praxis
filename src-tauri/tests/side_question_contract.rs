//! Acceptance scenarios use SQLite and a tiny provider executable, never a paid
//! model or the user's repository. CLI policy behavior is verified separately.
#![cfg(unix)]
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{Request, StatusCode},
};
use praxis_lib::{db, side_question as side};
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;

struct Fixture {
    path: PathBuf,
    previous_path: Option<std::ffi::OsString>,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "praxis-side-contract-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        let capture = path.join("prompt.txt");
        let script = format!("#!/bin/sh\nif [ \"$1\" = '--help' ]; then\n printf '%s' '--safe-mode --tools --setting-sources --strict-mcp-config --no-session-persistence'\nelse\n /bin/cat > '{}'\n printf '%s' 'stub-side-answer'\nfi\n", capture.display());
        let executable = path.join("claude");
        std::fs::write(&executable, script).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let previous_path = std::env::var_os("PATH");
        std::env::set_var("PATH", &path);
        Self {
            path,
            previous_path,
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        side::shutdown_all();
        if let Some(value) = &self.previous_path {
            std::env::set_var("PATH", value);
        } else {
            std::env::remove_var("PATH");
        }
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
fn input(id: &str, generation: i64, question: &str) -> side::SideQuestionSend {
    side::SideQuestionSend {
        request_id: id.into(),
        generation,
        question: question.into(),
        contexts: vec![],
    }
}

async fn wait_for_child(pool: &sqlx::SqlitePool, turn: i64) {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let pgid: Option<i64> =
                sqlx::query_scalar("SELECT pgid FROM side_question_turns WHERE id=?")
                    .bind(turn)
                    .fetch_one(pool)
                    .await
                    .unwrap();
            if pgid.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("question must start without waiting for the main lease to finish");
}

// One serial scenario keeps the test binary's PATH/provider cache isolated.
#[tokio::test]
async fn durable_side_question_contract() {
    let fixture = Fixture::new();
    let pool = db::init_pool(fixture.path.join("db.sqlite").to_str().unwrap())
        .await
        .unwrap();
    side::migrate(&pool).await.unwrap();
    let task = db::insert_task(
        &pool,
        "repo",
        "side",
        "base",
        fixture.path.to_str().unwrap(),
        "MAIN_ONLY_SECRET",
        Some("claude"),
        None,
        "conversation",
        1,
    )
    .await
    .unwrap();
    db::update_state(&pool, task, db::state::AWAITING_REVIEW, 1)
        .await
        .unwrap();
    db::set_setting(&pool, "model:claude", "sonnet")
        .await
        .unwrap();
    let first = side::read(&pool, task, 1).await.unwrap();
    assert!(first.supported);
    assert_eq!(
        first.model, "sonnet",
        "freeze the effective parent setting, not only an override"
    );
    db::set_setting(&pool, "model:claude", "opus")
        .await
        .unwrap();
    db::set_task_model(&pool, task, "opus").await.unwrap();
    assert_eq!(side::read(&pool, task, 2).await.unwrap().model, "sonnet");

    // Three turns retain only their own prior questions, answers and selected contexts.
    for sequence in 0..3 {
        let mut request = input(
            &format!("turn-{sequence}"),
            0,
            &format!("SIDE_QUESTION_{sequence}"),
        );
        if sequence == 0 {
            request.contexts = vec![serde_json::from_value(serde_json::json!({ "label": "selected", "text": "EXPLICIT_CONTEXT_ONLY", "path": "a.rs", "source_hash": "fingerprint" })).unwrap()];
        }
        let (turn, inserted) = side::send(&pool, task, request.clone(), 3).await.unwrap();
        assert!(inserted);
        assert!(!side::send(&pool, task, request.clone(), 3).await.unwrap().1);
        let mut conflict = request;
        conflict.question = "changed payload".into();
        assert!(side::send(&pool, task, conflict, 3).await.is_err());
        side::run_turn(pool.clone(), task, turn, 3).await;
        let snap = side::read(&pool, task, 4).await.unwrap();
        let latest = snap.turns.last().unwrap();
        assert_eq!(latest.state, "completed", "{:?}", latest.error);
        assert_eq!(latest.answer, "stub-side-answer");
        let prompt = std::fs::read_to_string(fixture.path.join("prompt.txt")).unwrap();
        assert!(prompt.contains(&format!("SIDE_QUESTION_{sequence}")));
        assert!(
            prompt.contains("EXPLICIT_CONTEXT_ONLY"),
            "prior explicitly selected references must survive later turns"
        );
        assert!(!prompt.contains("MAIN_ONLY_SECRET"));
        if sequence > 0 {
            assert!(prompt.contains("SIDE_QUESTION_0") && prompt.contains("stub-side-answer"));
        }
        assert!(db::list_convo_events(&pool, task).await.unwrap().is_empty());
        assert_eq!(
            serde_json::to_value(&snap.turns[0].contexts[0]).unwrap()["source_hash"],
            "fingerprint"
        );
    }

    // A provider that never consumes a large stdin prompt must still be cancellable.
    let executable = fixture.path.join("claude");
    let original = std::fs::read(&executable).unwrap();
    std::fs::write(&executable, "#!/bin/sh\n/bin/sleep 5\n").unwrap();
    let mut large = input("blocked-stdin", 0, "cancel while writing");
    large.contexts.push(side::SideQuestionContext {
        label: "large reference".into(),
        text: "x".repeat(100 * 1024),
        path: None,
        source_hash: None,
    });
    let (blocked_turn, _) = side::send(&pool, task, large, 4).await.unwrap();
    let began = std::time::Instant::now();
    let (_, cancelled) = tokio::join!(side::run_turn(pool.clone(), task, blocked_turn, 4), async {
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                let owned: bool = sqlx::query_scalar(
                    "SELECT pgid IS NOT NULL FROM side_question_turns WHERE id=?",
                )
                .bind(blocked_turn)
                .fetch_one(&pool)
                .await
                .unwrap();
                if owned {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        side::cancel(&pool, task, blocked_turn, 4).await
    });
    cancelled.unwrap();
    assert!(
        began.elapsed() < std::time::Duration::from_secs(2),
        "blocked stdin must not delay cancellation until provider exits"
    );
    assert_eq!(
        side::read(&pool, task, 4)
            .await
            .unwrap()
            .turns
            .last()
            .unwrap()
            .state,
        "cancelled"
    );
    let ownership: Option<i64> =
        sqlx::query_scalar("SELECT pgid FROM side_question_turns WHERE id=?")
            .bind(blocked_turn)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(
        ownership.is_none(),
        "reaped child must release durable ownership"
    );
    // Runtime cancellation must kill/reap the process even before normal collection.
    let (aborted_turn, _) = side::send(
        &pool,
        task,
        input("aborted-runtime", 0, "abort execution"),
        4,
    )
    .await
    .unwrap();
    let execution = tokio::spawn(side::run_turn(pool.clone(), task, aborted_turn, 4));
    let pgid = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let pgid: Option<i64> =
                sqlx::query_scalar("SELECT pgid FROM side_question_turns WHERE id=?")
                    .bind(aborted_turn)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            if let Some(pgid) = pgid {
                break pgid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let abort_began = std::time::Instant::now();
    execution.abort();
    assert!(execution.await.unwrap_err().is_cancelled());
    assert!(
        abort_began.elapsed() < std::time::Duration::from_secs(2),
        "runtime abort must reap promptly"
    );
    assert!(
        unsafe { nix::libc::kill(pgid as i32, 0) } == -1,
        "aborted future must reap its process"
    );
    assert_eq!(side::recover(&pool).await.unwrap(), 1);
    assert_eq!(
        side::read(&pool, task, 4)
            .await
            .unwrap()
            .turns
            .last()
            .unwrap()
            .state,
        "interrupted"
    );
    std::fs::write(&executable, original).unwrap();

    // Pending cancellation never starts a model and allows reset; stale generations cannot return.
    let (queued, _) = side::send(&pool, task, input("queued", 0, "cancel me"), 5)
        .await
        .unwrap();
    assert!(side::reset(&pool, task, 0, 5).await.is_err());
    side::cancel(&pool, task, queued, 6).await.unwrap();
    side::run_turn(pool.clone(), task, queued, 6).await;
    assert_eq!(
        side::read(&pool, task, 6)
            .await
            .unwrap()
            .turns
            .last()
            .unwrap()
            .state,
        "cancelled"
    );
    side::reset(&pool, task, 0, 7).await.unwrap();
    assert!(
        side::send(&pool, task, input("stale", 0, "old generation"), 7)
            .await
            .is_err()
    );
    assert!(side::read(&pool, task, 7).await.unwrap().turns.is_empty());
    side::send(&pool, task, input("restart", 1, "queued at crash"), 8)
        .await
        .unwrap();
    assert_eq!(side::recover(&pool).await.unwrap(), 1);
    assert_eq!(
        side::read(&pool, task, 9).await.unwrap().turns[0].state,
        "interrupted"
    );

    // Terminal parents and deletion fences cannot admit new work.
    let (terminal_waiter, _) = side::send(
        &pool,
        task,
        input("terminal-waiter", 1, "parent will finish"),
        10,
    )
    .await
    .unwrap();
    let prompt_before = std::fs::read(fixture.path.join("prompt.txt")).unwrap();
    db::update_state(&pool, task, db::state::DONE, 10)
        .await
        .unwrap();
    side::run_turn(pool.clone(), task, terminal_waiter, 10).await;
    let state: String = sqlx::query_scalar("SELECT state FROM side_question_turns WHERE id=?")
        .bind(terminal_waiter)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(state, "cancelled");
    assert_eq!(
        std::fs::read(fixture.path.join("prompt.txt")).unwrap(),
        prompt_before,
        "terminal parent must not dispatch a provider"
    );
    assert!(side::send(&pool, task, input("done", 1, "invalid"), 10)
        .await
        .is_err());
    db::update_state(&pool, task, db::state::AWAITING_REVIEW, 11)
        .await
        .unwrap();
    let deletion = side::begin_task_deletion(task).await.unwrap();
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_millis(50),
            side::send(&pool, task, input("deleting", 1, "invalid"), 11)
        )
        .await
        .is_err(),
        "admission must wait behind deletion ownership"
    );
    drop(deletion);
    let fence = side::try_runner_task_fence(task).unwrap();
    assert!(side::try_runner_task_fence(task).is_none());
    drop(fence);
    assert!(side::try_runner_task_fence(task).is_some());

    // The real Runner scheduler owns both permits concurrently for one parent.
    // Hold a main lease until after the question process starts and is cancelled.
    let original = std::fs::read(&executable).unwrap();
    std::fs::write(&executable, "#!/bin/sh\n/bin/sleep 5\n").unwrap();
    for limit in [2, 1] {
        db::update_state(&pool, task, db::state::QUEUED, 11)
            .await
            .unwrap();
        let queue = praxis_lib::runner::queue::QueueWorker::new(pool.clone(), limit);
        let main = queue.lease_next(11).await.unwrap().unwrap();
        assert_eq!(main.task.id, task);
        db::update_state(&pool, task, db::state::RUNNING, 11)
            .await
            .unwrap();
        let (turn, _) = side::send(
            &pool,
            task,
            input(&format!("parallel-{limit}"), 1, "ask during main"),
            11,
        )
        .await
        .unwrap();
        assert!(!side::blocks_main_execution(&pool, task).await.unwrap());
        assert!(side::send(
            &pool,
            task,
            input(&format!("second-{limit}"), 1, "same thread"),
            11
        )
        .await
        .is_err());
        let execution = queue.spawn_side_question(task, turn, 11);
        if limit == 2 {
            wait_for_child(&pool, turn).await;
            let extra = db::insert_task(
                &pool,
                "repo",
                "extra",
                "base",
                fixture.path.to_str().unwrap(),
                "extra",
                None,
                None,
                "terminal",
                11,
            )
            .await
            .unwrap();
            db::update_state(&pool, extra, db::state::QUEUED, 11)
                .await
                .unwrap();
            assert!(
                queue.lease_next(11).await.unwrap().is_none(),
                "main + question consume both permits"
            );
            side::cancel(&pool, task, turn, 11).await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(2), execution)
                .await
                .unwrap()
                .unwrap();
            let released = queue.lease_next(11).await.unwrap().unwrap();
            assert_eq!(
                released.task.id, extra,
                "question cancellation returns its permit"
            );
            drop(released);
            db::update_state(&pool, extra, db::state::DONE, 11)
                .await
                .unwrap();
        } else {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            let state: String =
                sqlx::query_scalar("SELECT state FROM side_question_turns WHERE id=?")
                    .bind(turn)
                    .fetch_one(&pool)
                    .await
                    .unwrap();
            assert_eq!(state, "queued", "one-slot host must wait for capacity");
            side::cancel(&pool, task, turn, 11).await.unwrap();
            tokio::time::timeout(std::time::Duration::from_secs(2), execution)
                .await
                .unwrap()
                .unwrap();
        }
        assert_eq!(
            db::get_task(&pool, task).await.unwrap().unwrap().state,
            db::state::RUNNING
        );
        assert!(
            side::try_runner_task_fence(task).is_none(),
            "cancelling the question preserves main ownership"
        );
        assert!(db::list_convo_events(&pool, task).await.unwrap().is_empty());
        drop(main);
        // Released capacity lets a waiting question run even if parent state
        // still says Running (the desktop uses Running between main turns).
        let (turn, _) = side::send(
            &pool,
            task,
            input(&format!("released-{limit}"), 1, "next question"),
            11,
        )
        .await
        .unwrap();
        let execution = queue.spawn_side_question(task, turn, 11);
        wait_for_child(&pool, turn).await;
        if limit == 2 {
            db::update_state(&pool, task, db::state::QUEUED, 11)
                .await
                .unwrap();
            let main = queue.lease_next(11).await.unwrap().unwrap();
            assert_eq!(
                main.task.id, task,
                "main can start while the question is running"
            );
            drop(main);
        } else {
            db::update_state(&pool, task, db::state::AWAITING_REVIEW, 11)
                .await
                .unwrap();
            let error = queue
                .resume_conversation(task, "annotation followup".into(), 11)
                .await
                .unwrap_err();
            assert!(
                error.contains("동시 실행 한도"),
                "direct followups must also honor the question's permit: {error}"
            );
            assert_eq!(
                db::get_task(&pool, task).await.unwrap().unwrap().state,
                db::state::AWAITING_REVIEW
            );
        }
        side::cancel(&pool, task, turn, 11).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), execution)
            .await
            .unwrap()
            .unwrap();
        db::update_state(&pool, task, db::state::AWAITING_REVIEW, 11)
            .await
            .unwrap();
    }
    std::fs::write(&executable, original).unwrap();

    // An incomplete durable receipt can be recovered, and changed payloads cannot reuse it.
    assert!(
        side::receipt_begin(&pool, task, "main-id", "MAIN_REQUEST", &[], 12)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        side::receipt_begin(&pool, task, "main-id", "MAIN_REQUEST", &[], 12)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        side::receipt_begin(&pool, task, "main-id", "CHANGED", &[], 12)
            .await
            .is_err()
    );
    let (a, b) = tokio::join!(
        db::requeue_conversation_followup_receipt(&pool, task, "main-id", "MAIN_REQUEST", 13),
        db::requeue_conversation_followup_receipt(&pool, task, "main-id", "MAIN_REQUEST", 13)
    );
    let (a, b) = (a.unwrap(), b.unwrap());
    assert!(a || b);
    assert_eq!(
        side::receipt_read(&pool, task, "main-id")
            .await
            .unwrap()
            .status,
        "accepted"
    );
    let queued_events: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM runner_events WHERE task_id=? AND kind='queued' AND detail='followup'").bind(task).fetch_one(&pool).await.unwrap();
    assert_eq!(queued_events, 1, "duplicate receipt retry must queue once");
    // The HTTP boundary requires pairing and returns the same dedicated DTOs.
    let token_path = fixture.path.join("pairing-token");
    let token = "ab".repeat(32);
    std::fs::write(&token_path, &token).unwrap();
    std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let config = praxis_lib::runner::config::RunnerConfig {
        bind: "127.0.0.1:0".parse().unwrap(),
        repository_roots: vec![fixture.path.clone()],
        max_concurrent_tasks: 1,
        execution_policy: praxis_lib::runner::config::ExecutionPolicy::AlwaysApprove,
        pairing_token_file: token_path.clone(),
    };
    let queue = praxis_lib::runner::queue::QueueWorker::new(pool.clone(), 1);
    let main_lease = queue.lease_next(13).await.unwrap().unwrap();
    assert_eq!(main_lease.task.id, task);
    let router = praxis_lib::runner::http::router(praxis_lib::runner::http::RunnerHttpState {
        auth: praxis_lib::runner::auth::RunnerAuth::from_file(&token_path).unwrap(),
        pool: pool.clone(),
        config,
        recovered_tasks: 0,
        events: praxis_lib::runner::events::EventHub::start(pool.clone()),
        queue,
        started_at: 0,
        review_claims: Default::default(),
    });
    let url = format!("/v1/tasks/{task}/side-question");
    let request = |method: &str, path: &str, value: Option<serde_json::Value>, auth: bool| {
        let mut builder = Request::builder()
            .method(method)
            .uri(path)
            .header("content-type", "application/json")
            .extension(ConnectInfo(
                "127.0.0.1:12345".parse::<std::net::SocketAddr>().unwrap(),
            ));
        if auth {
            builder = builder.header("authorization", format!("Bearer {token}"));
        }
        builder
            .body(Body::from(value.map(|v| v.to_string()).unwrap_or_default()))
            .unwrap()
    };
    assert_eq!(
        router
            .clone()
            .oneshot(request("GET", &url, None, false))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let result = router
        .clone()
        .oneshot(request("GET", &url, None, true))
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    let snapshot: serde_json::Value =
        serde_json::from_slice(&to_bytes(result.into_body(), 1_000_000).await.unwrap()).unwrap();
    assert_eq!(snapshot["generation"], 1);
    let result = router.clone().oneshot(request("POST", &format!("{url}/messages"), Some(serde_json::json!({ "request_id":"http-queued", "generation":1, "question":"wait for capacity", "contexts":[] })), true)).await.unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    let snap: serde_json::Value =
        serde_json::from_slice(&to_bytes(result.into_body(), 1_000_000).await.unwrap()).unwrap();
    let turns = snap["turns"].as_array().unwrap();
    let queued = turns.last().unwrap();
    assert_eq!(queued["state"], "queued");
    let result = router
        .clone()
        .oneshot(request(
            "POST",
            &format!("{url}/cancel"),
            Some(serde_json::json!({"turn_id":queued["id"]})),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    let result = router
        .clone()
        .oneshot(request(
            "POST",
            &format!("{url}/reset"),
            Some(serde_json::json!({"generation":1})),
            true,
        ))
        .await
        .unwrap();
    assert_eq!(result.status(), StatusCode::OK);
    let result = router
        .oneshot(request(
            "GET",
            &format!("/v1/tasks/{task}/message-receipts/main-id"),
            None,
            true,
        ))
        .await
        .unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(result.into_body(), 1_000_000).await.unwrap()).unwrap();
    assert_eq!(value["status"], "accepted");
    assert_eq!(
        db::get_task(&pool, task).await.unwrap().unwrap().state,
        db::state::STARTING,
        "side cancellation/reset cannot interrupt the main leased request"
    );
    drop(main_lease);
    db::append_convo_event(&pool, task, &serde_json::json!({"kind":"user", "text":"accepted once", "receipt_request_id":"rewound-receipt"}).to_string(), 15).await.unwrap();
    sqlx::query("UPDATE convo_events SET rewound_at=16 WHERE task_id=?")
        .bind(task)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        side::receipt_main_admitted(&pool, task, "rewound-receipt")
            .await
            .unwrap(),
        "rewind cannot make an accepted receipt executable again"
    );

    // Missing ownership is a durable quarantine, not permission to delete its evidence.
    let quarantined = db::insert_task(
        &pool,
        "repo",
        "quarantine",
        "base",
        fixture.path.to_str().unwrap(),
        "q",
        Some("claude"),
        None,
        "conversation",
        20,
    )
    .await
    .unwrap();
    db::update_state(&pool, quarantined, db::state::AWAITING_REVIEW, 20)
        .await
        .unwrap();
    let (unknown_turn, _) = side::send(&pool, quarantined, input("unknown-owner", 0, "q"), 20)
        .await
        .unwrap();
    sqlx::query("UPDATE side_question_turns SET state='running' WHERE id=?")
        .bind(unknown_turn)
        .execute(&pool)
        .await
        .unwrap();
    side::recover(&pool).await.unwrap();
    assert!(!side::read(&pool, quarantined, 21).await.unwrap().supported);
    assert!(side::blocks_main_execution(&pool, quarantined)
        .await
        .unwrap());
    assert!(side::reset(&pool, quarantined, 0, 21).await.is_err());
    assert!(db::delete_task(&pool, quarantined).await.is_err());
    assert!(db::get_task(&pool, quarantined).await.unwrap().is_some());
    // Allow the cancelled waiter to observe its terminal record before runtime teardown.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    pool.close().await;
}
