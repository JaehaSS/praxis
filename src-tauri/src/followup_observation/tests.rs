use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use tokio::sync::Barrier;

use super::*;

static DATABASE_COUNTER: AtomicU32 = AtomicU32::new(0);

#[tokio::test]
async fn initial_conversation_input_is_not_a_followup() {
    let pool = test_pool().await;

    record_conversation_input(&pool, 7, ConversationInputOrigin::InitialTask, 100)
        .await
        .unwrap();

    assert_eq!(event_count(&pool, 7).await, 0);
}

#[tokio::test]
async fn explicit_conversation_followup_origins_are_recorded() {
    let pool = test_pool().await;

    for (task_id, origin) in [
        (8, ConversationInputOrigin::UserMessage),
        (9, ConversationInputOrigin::AnnotationResend),
        (10, ConversationInputOrigin::RemoteReviewRetry),
    ] {
        record_conversation_input(&pool, task_id, origin, 100)
            .await
            .unwrap();
        assert_eq!(event_count(&pool, task_id).await, 1);
    }
}

#[tokio::test]
async fn failed_persistence_does_not_forward_input() {
    let pool = test_pool().await;
    pool.close().await;
    let forwarded = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = forwarded.clone();

    let result = record_followup_before_forward(&pool, 9, 100, move || {
        observed.store(true, Ordering::SeqCst);
        Ok(())
    })
    .await;

    assert!(result.is_err());
    assert!(!forwarded.load(Ordering::SeqCst));
}

#[tokio::test]
async fn concurrent_followup_records_create_one_content_free_event() {
    let pool = test_pool().await;
    let barrier = Arc::new(Barrier::new(2));
    let first_pool = pool.clone();
    let first_barrier = barrier.clone();
    let first = tokio::spawn(async move {
        first_barrier.wait().await;
        record_user_followup(&first_pool, 42, 100).await
    });
    let second_pool = pool.clone();
    let second = tokio::spawn(async move {
        barrier.wait().await;
        record_user_followup(&second_pool, 42, 101).await
    });

    first.await.unwrap().unwrap();
    second.await.unwrap().unwrap();

    let rows: Vec<(String, Option<String>)> =
        sqlx::query_as("SELECT kind, detail FROM task_events WHERE task_id = 42")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(rows, vec![(USER_FOLLOWUP_INPUT_OBSERVED.to_string(), None)]);
}

async fn test_pool() -> sqlx::SqlitePool {
    let sequence = DATABASE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = crate::testtmp::dir().join(format!(
        "praxis-followup-observation-{}-{sequence}.sqlite",
        std::process::id()
    ));
    crate::db::init_pool(path.to_str().unwrap()).await.unwrap()
}

async fn event_count(pool: &sqlx::SqlitePool, task_id: i64) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM task_events \
         WHERE task_id = ? AND kind = 'user_followup_input_observed'",
    )
    .bind(task_id)
    .fetch_one(pool)
    .await
    .unwrap()
}
