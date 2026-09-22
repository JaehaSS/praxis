use crate::{db, notifications};

async fn pool(name: &str) -> sqlx::SqlitePool {
    let path = crate::testtmp::dir().join(format!("notifications-{name}.sqlite"));
    db::init_pool(path.to_str().unwrap()).await.unwrap()
}

fn page(
    source_id: &str,
    after: Option<i64>,
    cursor: i64,
    watermark: i64,
    results: Vec<notifications::ResultNotice>,
) -> notifications::SourcePage {
    notifications::SourcePage {
        source_id: source_id.into(),
        after,
        cursor,
        watermark,
        results,
    }
}

fn result(sequence: i64, task_id: i64) -> notifications::ResultNotice {
    notifications::ResultNotice {
        sequence,
        task_id,
        ts: sequence,
        kind: "result".into(),
        title: format!("task {task_id}"),
        repo: "/tmp/repo".into(),
    }
}

#[tokio::test]
async fn notification_source_page_keeps_highwater_after_deleted_rows() {
    let pool = pool("source-page").await;
    for sequence in 1..=3 {
        sqlx::query("INSERT INTO notification_results(task_id, ts, kind, title, repo) VALUES (?, ?, 'result', 'title', 'repo')")
            .bind(sequence).bind(sequence).execute(&pool).await.unwrap();
    }
    sqlx::query("DELETE FROM notification_results WHERE sequence = 3")
        .execute(&pool)
        .await
        .unwrap();
    let page = notifications::source_page(&pool, Some(0)).await.unwrap();
    assert_eq!(page.watermark, 3);
    assert_eq!(page.cursor, 3);
    assert_eq!(
        page.results
            .iter()
            .map(|item| item.sequence)
            .collect::<Vec<_>>(),
        [1, 2]
    );
}

#[tokio::test]
async fn notification_source_page_pages_more_than_one_hundred_without_a_gap() {
    let pool = pool("paging").await;
    for sequence in 1..=101 {
        sqlx::query("INSERT INTO notification_results(task_id, ts, kind, title, repo) VALUES (?, ?, 'result', 'title', 'repo')")
            .bind(sequence).bind(sequence).execute(&pool).await.unwrap();
    }
    let first = notifications::source_page(&pool, Some(0)).await.unwrap();
    assert_eq!(first.results.len(), 100);
    assert_eq!(first.cursor, 100);
    let second = notifications::source_page(&pool, Some(first.cursor))
        .await
        .unwrap();
    assert_eq!(
        second
            .results
            .iter()
            .map(|item| item.sequence)
            .collect::<Vec<_>>(),
        [101]
    );
    assert_eq!(second.cursor, 101);
}

#[tokio::test]
async fn notification_ingest_deduplicates_acknowledges_and_retires_reset_source() {
    let pool = pool("inbox").await;
    let baseline = page("source-a", None, 0, 0, vec![]);
    assert!(notifications::ingest(&pool, "local", &baseline)
        .await
        .unwrap()
        .is_empty());
    let first = page("source-a", Some(0), 1, 1, vec![result(1, 7)]);
    assert_eq!(
        notifications::ingest(&pool, "local", &first)
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(notifications::ingest(&pool, "local", &first).await.is_err());
    let second = page("source-a", Some(1), 2, 2, vec![result(2, 7)]);
    notifications::ingest(&pool, "local", &second)
        .await
        .unwrap();
    notifications::acknowledge(&pool, "local", "source-a", 7, 1)
        .await
        .unwrap();
    let snapshot = notifications::snapshot(&pool).await.unwrap();
    assert_eq!(snapshot.items[0].sequence, 2);
    let reset = page("source-b", None, 4, 4, vec![]);
    notifications::ingest(&pool, "local", &reset).await.unwrap();
    let snapshot = notifications::snapshot(&pool).await.unwrap();
    assert!(snapshot.items.is_empty());
    assert!(snapshot.sources[0].warning.is_some());
}

#[tokio::test]
async fn notification_hosts_and_acknowledgements_are_isolated() {
    let pool = pool("hosts").await;
    for host in ["local", "remote"] {
        notifications::ingest(&pool, host, &page("source-a", None, 0, 0, vec![]))
            .await
            .unwrap();
        notifications::ingest(
            &pool,
            host,
            &page("source-a", Some(0), 1, 1, vec![result(1, 7)]),
        )
        .await
        .unwrap();
    }
    assert!(notifications::acknowledge(&pool, "local", "source-b", 7, 1)
        .await
        .is_err());
    assert!(notifications::acknowledge(&pool, "local", "source-a", 7, 2)
        .await
        .is_err());
    notifications::reconcile(&pool, "local", &[]).await.unwrap();
    let snapshot = notifications::snapshot(&pool).await.unwrap();
    assert_eq!(snapshot.items.len(), 1);
    assert_eq!(snapshot.items[0].host, "remote");
}

#[tokio::test]
async fn notification_ingest_rejects_invalid_page_without_advancing_cursor() {
    let pool = pool("invalid-page").await;
    notifications::ingest(&pool, "local", &page("source-a", None, 4, 4, vec![]))
        .await
        .unwrap();
    let invalid = page(
        "source-a",
        Some(4),
        5,
        5,
        vec![notifications::ResultNotice {
            kind: "unknown".into(),
            ..result(5, 8)
        }],
    );
    assert!(notifications::ingest(&pool, "local", &invalid)
        .await
        .is_err());
    let snapshot = notifications::snapshot(&pool).await.unwrap();
    assert_eq!(snapshot.sources[0].cursor, 4);
}

#[tokio::test]
async fn cancellation_intent_survives_process_registration_and_suppresses_terminal_result() {
    let pool = pool("cancel-race").await;
    sqlx::query("INSERT INTO tasks(repo, branch, base, worktree_path, instruction, state, created_at, updated_at) VALUES ('repo', 'branch', 'base', 'path', 'task', 'Running', 1, 1)")
        .execute(&pool).await.unwrap();
    assert!(db::record_notification_cancel_intent(&pool, 1)
        .await
        .unwrap());
    db::record_task_process_start(&pool, 1, 42, &"a".repeat(64), "terminal", 2)
        .await
        .unwrap();
    assert!(db::finish_running_task_with_notification(
        &pool,
        1,
        db::state::AWAITING_REVIEW,
        3,
        "completed",
        None,
        "result"
    )
    .await
    .unwrap());
    assert!(notifications::source_page(&pool, Some(0))
        .await
        .unwrap()
        .results
        .is_empty());
    assert!(db::mark_running_from_review(&pool, 1, 4).await.unwrap());
    assert!(db::finish_running_task_with_notification(
        &pool,
        1,
        db::state::AWAITING_REVIEW,
        5,
        "completed",
        None,
        "result"
    )
    .await
    .unwrap());
    assert_eq!(
        notifications::source_page(&pool, Some(0))
            .await
            .unwrap()
            .results
            .len(),
        1
    );
}

#[tokio::test]
async fn created_cancellation_intent_suppresses_local_terminal_exit() {
    let pool = pool("created-cancel").await;
    sqlx::query("INSERT INTO tasks(repo, branch, base, worktree_path, instruction, state, created_at, updated_at) VALUES ('repo', 'branch', 'base', 'path', 'task', 'Created', 1, 1)")
        .execute(&pool).await.unwrap();
    assert!(db::record_notification_cancel_intent(&pool, 1).await.unwrap());
    assert!(db::mark_awaiting_review_with_notification(&pool, 1, 2, None, "result").await.unwrap());
    assert!(notifications::source_page(&pool, Some(0)).await.unwrap().results.is_empty());
}

#[tokio::test]
async fn cancellation_intent_is_removed_when_no_signal_was_sent() {
    let pool = pool("unsent-cancel").await;
    sqlx::query("INSERT INTO tasks(repo, branch, base, worktree_path, instruction, state, created_at, updated_at) VALUES ('repo', 'branch', 'base', 'path', 'task', 'Running', 1, 1)")
        .execute(&pool).await.unwrap();
    assert!(db::record_notification_cancel_intent(&pool, 1).await.unwrap());
    db::clear_notification_cancel_if_signal_not_sent(&pool, 1).await.unwrap();
    assert!(db::finish_running_task_with_notification(&pool, 1, db::state::AWAITING_REVIEW, 2, "completed", None, "result").await.unwrap());
    assert_eq!(notifications::source_page(&pool, Some(0)).await.unwrap().results.len(), 1);
}
