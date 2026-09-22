use super::*;
async fn pool() -> SqlitePool {
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap();
    sqlx::query("CREATE TABLE tasks(id INTEGER PRIMARY KEY,convo_session_id TEXT,pending_capsule TEXT);INSERT INTO tasks(id) VALUES(1),(2)").execute(&pool).await.unwrap();
    migrate(&pool).await.unwrap();
    bind(&pool, 1).await.unwrap();
    pool
}
fn args() -> Value {
    json!({"kind":"clarification","questions":[{"id":"color","question":"색상?","options":[{"id":"blue","label":"파랑","description":""}],"allow_free_text":true,"is_secret":false}]})
}
fn answer() -> Vec<Answer> {
    vec![Answer {
        question_id: "color".into(),
        option_id: Some("blue".into()),
        text: None,
    }]
}
async fn question(pool: &SqlitePool) -> (String, String) {
    let e = begin(pool, 1, 100).await.unwrap();
    started(pool, &e, "thread", "turn").await.unwrap();
    let q = open(pool, &e, &json!(9007199254740993u64), "call", &args(), 100)
        .await
        .unwrap();
    (e, q)
}

#[test]
fn rejects_unknown_approval_secret_and_oversized_envelopes() {
    for (key, value) in [
        ("kind", json!("approval")),
        ("extra", json!("secret-fixture")),
    ] {
        let mut v = args();
        v[key] = value;
        assert!(validate_questions(&v).is_err());
    }
    let mut v = args();
    v["questions"][0]["is_secret"] = json!(true);
    assert!(validate_questions(&v).is_err());
    let mut v = args();
    v["questions"][0]["question"] = json!("a".repeat(2001));
    assert!(validate_questions(&v).is_err());
    let mut v = args();
    v["questions"] = json!([]);
    assert!(validate_questions(&v).is_err());
    let mut v = args();
    v["questions"][0]["options"][0]["unknown"] = json!("secret-fixture");
    assert!(validate_questions(&v).is_err());
}
#[tokio::test]
async fn typed_wire_ids_and_task_execution_isolation() {
    let p = pool().await;
    let (e, q) = question(&p).await;
    let q2 = open(
        &p,
        &e,
        &json!("9007199254740993"),
        "call-string",
        &args(),
        100,
    )
    .await
    .unwrap();
    assert_ne!(q, q2);
    assert!(submit(&p, 2, &e, &q, "request", &answer(), 101)
        .await
        .is_err());
    assert!(
        submit(&p, 1, "other-execution", &q, "request", &answer(), 101)
            .await
            .is_err()
    );
    assert_eq!(receipt(&p, 2, "request").await.unwrap().state, "not_found");
    submit(&p, 1, &e, &q, "request", &answer(), 101)
        .await
        .unwrap();
    let dispatch = take_dispatch(&p, &e, 102).await.unwrap().unwrap();
    assert_eq!(dispatch.wire_id, json!(9007199254740993u64));
    assert!(take_dispatch(&p, &e, 102).await.unwrap().is_none());
}
#[tokio::test]
async fn receipt_retries_never_redispatch_and_ack_requires_exact_output() {
    let p = pool().await;
    let (e, q) = question(&p).await;
    let a = submit(&p, 1, &e, &q, "request", &answer(), 101)
        .await
        .unwrap();
    assert_eq!(a.state, "claimed");
    assert_eq!(
        submit(&p, 1, &e, &q, "request", &answer(), 102)
            .await
            .unwrap(),
        a
    );
    let other = vec![Answer {
        question_id: "color".into(),
        option_id: None,
        text: Some("red".into()),
    }];
    assert!(submit(&p, 1, &e, &q, "request", &other, 102).await.is_err());
    assert!(submit(&p, 1, &e, &q, "different-request", &answer(), 102)
        .await
        .is_err());
    let d = take_dispatch(&p, &e, 102).await.unwrap().unwrap();
    written(&p, &d.answer_id).await.unwrap();
    assert!(!acknowledge(&p, &e, "call", &json!([]), true).await.unwrap());
    assert_eq!(receipt(&p, 1, "request").await.unwrap().state, "written");
    let contents = json!([{"type":"inputText","text":d.output}]);
    assert!(!acknowledge(&p, &e, "wrong-call", &contents, true)
        .await
        .unwrap());
    assert!(!acknowledge(&p, &e, "call", &contents, false).await.unwrap());
    assert!(acknowledge(&p, &e, "call", &contents, true).await.unwrap());
    assert_eq!(
        submit(&p, 1, &e, &q, "request", &answer(), 103)
            .await
            .unwrap()
            .state,
        "acknowledged"
    );
    assert!(take_dispatch(&p, &e, 102).await.unwrap().is_none());
    assert_eq!(
        snapshot(&p, 1).await.unwrap().items[0].reason.as_deref(),
        Some("answered")
    );
}
#[tokio::test]
async fn draft_cas_and_partial_answers_survive_snapshot() {
    let p = pool().await;
    let (e, q) = question(&p).await;
    assert_eq!(draft(&p, 1, &e, &q, &answer(), 0, 101).await.unwrap(), 1);
    assert!(draft(&p, 1, &e, &q, &[], 0, 102).await.is_err());
    assert_eq!(draft(&p, 1, &e, &q, &[], 1, 102).await.unwrap(), 2);
    assert_eq!(snapshot(&p, 1).await.unwrap().items[0].draft_revision, 2);
    draft(&p, 1, &e, &q, &answer(), 2, 103).await.unwrap();
    submit(&p, 1, &e, &q, "request", &answer(), 104)
        .await
        .unwrap();
    assert!(draft(&p, 1, &e, &q, &[], 3, 104).await.is_err());
    let restored = snapshot(&p, 1).await.unwrap();
    assert_eq!(restored.items[0].draft, answer());
    assert_eq!(restored.items[0].draft_revision, 0);
}
#[tokio::test]
async fn expiry_cancel_and_cleanup_failure_keep_admission_closed() {
    let p = pool().await;
    let (e, q) = question(&p).await;
    assert!(
        submit(&p, 1, &e, &q, "expired", &answer(), 100 + QUESTION_TTL)
            .await
            .is_err()
    );
    phase(&p, &e, "cancelling").await.unwrap();
    assert!(submit(&p, 1, &e, &q, "cancel", &answer(), 101)
        .await
        .is_err());
    phase(&p, &e, "cleanup_failed").await.unwrap();
    assert!(begin(&p, 1, 102).await.is_err());
    assert!(blocked(&p, 1).await.unwrap());
    finish(&p, &e, "failed", Some("cancelled")).await.unwrap();
    assert!(begin(&p, 1, 103).await.is_ok());
    assert!(submit(&p, 1, &e, &q, "old", &answer(), 104).await.is_err());
}
#[tokio::test]
async fn accepted_or_partial_write_crashes_become_unknown_without_replay() {
    for dispatch in [false, true] {
        let p = pool().await;
        let (e, q) = question(&p).await;
        submit(&p, 1, &e, &q, "request", &answer(), 101)
            .await
            .unwrap();
        if dispatch {
            take_dispatch(&p, &e, 102).await.unwrap().unwrap();
        }
        close_questions(&p, &e, "connection_lost").await.unwrap();
        assert_eq!(receipt(&p, 1, "request").await.unwrap().state, "unknown");
        assert!(take_dispatch(&p, &e, 102).await.unwrap().is_none());
        assert_eq!(snapshot(&p, 1).await.unwrap().items[0].draft, answer());
    }
}
#[tokio::test]
async fn failed_answer_insert_rolls_back_claim_and_keeps_draft() {
    let p = pool().await;
    let (e, q) = question(&p).await;
    draft(&p, 1, &e, &q, &answer(), 0, 101).await.unwrap();
    sqlx::query("CREATE TRIGGER deny_answer BEFORE INSERT ON convo_interaction_answers BEGIN SELECT RAISE(ABORT,'injected storage failure');END").execute(&p).await.unwrap();
    assert!(submit(&p, 1, &e, &q, "request", &answer(), 102)
        .await
        .is_err());
    assert_eq!(receipt(&p, 1, "request").await.unwrap().state, "not_found");
    assert!(take_dispatch(&p, &e, 102).await.unwrap().is_none());
    assert_eq!(snapshot(&p, 1).await.unwrap().items[0].draft, answer());
    sqlx::query("DROP TRIGGER deny_answer")
        .execute(&p)
        .await
        .unwrap();
    assert_eq!(
        submit(&p, 1, &e, &q, "request", &answer(), 103)
            .await
            .unwrap()
            .state,
        "claimed"
    );
}
#[tokio::test]
async fn deleting_task_cascades_question_draft_and_receipt() {
    let p = pool().await;
    let (e, q) = question(&p).await;
    submit(&p, 1, &e, &q, "request", &answer(), 101)
        .await
        .unwrap();
    sqlx::query("DELETE FROM tasks WHERE id=1")
        .execute(&p)
        .await
        .unwrap();
    for table in [
        "convo_runtime_bindings",
        "convo_executions",
        "convo_interactions",
        "convo_interaction_answers",
        "convo_interaction_drafts",
    ] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
            .fetch_one(&p)
            .await
            .unwrap();
        assert_eq!(n, 0, "{table}");
    }
}
#[tokio::test]
async fn secret_fixture_is_rejected_before_it_reaches_storage() {
    let p = pool().await;
    let e = begin(&p, 1, 100).await.unwrap();
    started(&p, &e, "t", "u").await.unwrap();
    let mut v = args();
    v["questions"][0]["is_secret"] = json!(true);
    v["questions"][0]["question"] = json!("NEVER-PERSIST-SECRET");
    assert!(open(&p, &e, &json!(1), "call", &v, 100).await.is_err());
    assert!(snapshot(&p, 1).await.unwrap().items.is_empty());
}
#[tokio::test]
async fn simultaneous_duplicate_receipts_converge_to_one_claim() {
    let p = pool().await;
    let (e, q) = question(&p).await;
    let answers = answer();
    let (a, b) = tokio::join!(
        submit(&p, 1, &e, &q, "request", &answers, 101),
        submit(&p, 1, &e, &q, "request", &answers, 101)
    );
    assert_eq!(a.unwrap(), b.unwrap());
    assert!(take_dispatch(&p, &e, 102).await.unwrap().is_some());
    assert!(take_dispatch(&p, &e, 102).await.unwrap().is_none());
}
#[tokio::test]
async fn session_binding_and_capsule_clear_commit_with_turn_acceptance() {
    let p = pool().await;
    sqlx::query("UPDATE tasks SET convo_session_id='old',pending_capsule='handoff' WHERE id=1")
        .execute(&p)
        .await
        .unwrap();
    let execution = begin(&p, 1, 100).await.unwrap();
    sqlx::query("CREATE TRIGGER deny_session BEFORE UPDATE ON tasks BEGIN SELECT RAISE(ABORT,'injected session failure');END").execute(&p).await.unwrap();
    assert!(started(&p, &execution, "new", "turn").await.is_err());
    let current: (String, String) =
        sqlx::query_as("SELECT convo_session_id,pending_capsule FROM tasks WHERE id=1")
            .fetch_one(&p)
            .await
            .unwrap();
    assert_eq!(current, ("old".into(), "handoff".into()));
    let phase: String = sqlx::query_scalar("SELECT state FROM convo_executions WHERE id=?")
        .bind(&execution)
        .fetch_one(&p)
        .await
        .unwrap();
    assert_eq!(phase, "starting");
    sqlx::query("DROP TRIGGER deny_session")
        .execute(&p)
        .await
        .unwrap();
    started(&p, &execution, "new", "turn").await.unwrap();
    let current: (String, Option<String>) =
        sqlx::query_as("SELECT convo_session_id,pending_capsule FROM tasks WHERE id=1")
            .fetch_one(&p)
            .await
            .unwrap();
    assert_eq!(current, ("new".into(), None));
}

#[tokio::test]
async fn accepted_answer_cannot_start_dispatch_at_or_after_expiry() {
    let p = pool().await;
    let (e, q) = question(&p).await;
    submit(
        &p,
        1,
        &e,
        &q,
        "near-expiry",
        &answer(),
        100 + QUESTION_TTL - 1,
    )
    .await
    .unwrap();
    assert!(take_dispatch(&p, &e, 100 + QUESTION_TTL)
        .await
        .unwrap()
        .is_none());
    assert!(take_dispatch(&p, &e, 101 + QUESTION_TTL)
        .await
        .unwrap()
        .is_none());
    assert_eq!(
        receipt(&p, 1, "near-expiry").await.unwrap().state,
        "claimed"
    );
    close_questions(&p, &e, "expired").await.unwrap();
    assert_eq!(
        receipt(&p, 1, "near-expiry").await.unwrap().state,
        "unknown"
    );
    assert!(take_dispatch(&p, &e, 102 + QUESTION_TTL)
        .await
        .unwrap()
        .is_none());
}
