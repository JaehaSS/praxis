//! `task_issue_refs` 스키마·CRUD 통합 테스트. `cargo test`

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::db;
use praxis_lib::github;

static COUNTER: AtomicU32 = AtomicU32::new(0);

async fn setup() -> (sqlx::SqlitePool, String) {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = temp_root::dir()
        .join(format!(
            "praxis-github-test-{}-{}.sqlite",
            std::process::id(),
            n
        ))
        .to_string_lossy()
        .into_owned();
    let pool = db::init_pool(&path).await.unwrap();
    github::migrate(&pool).await.unwrap();
    (pool, path)
}

#[tokio::test]
async fn set_and_get_issue_ref_round_trip() {
    let (pool, path) = setup().await;
    assert_eq!(github::get_issue_ref(&pool, 1).await.unwrap(), None);

    github::set_issue_ref(&pool, 1, "acme/widgets#42")
        .await
        .unwrap();
    assert_eq!(
        github::get_issue_ref(&pool, 1).await.unwrap(),
        Some("acme/widgets#42".to_string())
    );
    // 다른 task_id는 영향 없음.
    assert_eq!(github::get_issue_ref(&pool, 2).await.unwrap(), None);

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn set_issue_ref_replaces_existing_value() {
    let (pool, path) = setup().await;
    github::set_issue_ref(&pool, 5, "acme/widgets#1")
        .await
        .unwrap();
    github::set_issue_ref(&pool, 5, "acme/widgets#2")
        .await
        .unwrap();
    assert_eq!(
        github::get_issue_ref(&pool, 5).await.unwrap(),
        Some("acme/widgets#2".to_string())
    );
    let _ = std::fs::remove_file(&path);
}
