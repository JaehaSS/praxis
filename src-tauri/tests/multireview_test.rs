//! 멀티벤더 리뷰 이력 영속화(reviews 테이블) 테스트. `cargo test`

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::db;
use praxis_lib::multireview;

static COUNTER: AtomicU32 = AtomicU32::new(0);

async fn setup() -> (sqlx::SqlitePool, String) {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = temp_root::dir()
        .join(format!(
            "praxis-mr-test-{}-{}.sqlite",
            std::process::id(),
            n
        ))
        .to_string_lossy()
        .into_owned();
    let pool = db::init_pool(&path).await.unwrap();
    multireview::migrate(&pool).await.unwrap();
    (pool, path)
}

#[tokio::test]
async fn insert_list_get_delete_round_trip() {
    let (pool, path) = setup().await;
    let result_json = r#"{"items":[{"vendor":"claude","ok":true,"text":"good"}],"synthesis":null}"#;
    let model_info_json =
        r#"{"items":[{"vendor":"claude","model":"opus","cmd":"claude"}],"synthesis":null}"#;
    let id = multireview::insert_review(
        &pool,
        1000,
        "/repo",
        "plan",
        "PLAN.md",
        "보안 관점",
        result_json,
        1,
        1,
        "plan 본문",
        "리뷰 프롬프트",
        Some("종합 프롬프트"),
        model_info_json,
    )
    .await
    .unwrap();

    let list = multireview::list_reviews(&pool).await.unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, id);
    assert_eq!(list[0].repo, "/repo");
    assert_eq!(list[0].source_kind, "plan");
    assert_eq!(list[0].source_ref, "PLAN.md");
    assert_eq!(list[0].focus, "보안 관점");
    assert_eq!(list[0].ok_count, 1);
    assert_eq!(list[0].total, 1);

    let (meta, json, detail) = multireview::get_review(&pool, id)
        .await
        .unwrap()
        .expect("exists");
    assert_eq!(meta.id, id);
    assert_eq!(json, result_json);
    assert_eq!(detail.content, "plan 본문");
    assert_eq!(detail.prompt_review, "리뷰 프롬프트");
    assert_eq!(detail.prompt_synthesis.as_deref(), Some("종합 프롬프트"));
    assert_eq!(detail.model_info.len(), 1);
    assert_eq!(detail.model_info[0].vendor, "claude");
    assert_eq!(detail.model_info[0].model, "opus");
    assert!(detail.synthesis_model.is_none());

    multireview::delete_review(&pool, id).await.unwrap();
    assert!(multireview::get_review(&pool, id).await.unwrap().is_none());
    assert_eq!(multireview::list_reviews(&pool).await.unwrap().len(), 0);
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn list_reviews_orders_newest_first() {
    let (pool, path) = setup().await;
    let mi = r#"{"items":[],"synthesis":null}"#;
    let a = multireview::insert_review(
        &pool, 100, "/r", "text", "x", "", "{}", 0, 0, "", "", None, mi,
    )
    .await
    .unwrap();
    let b = multireview::insert_review(
        &pool, 200, "/r", "text", "y", "", "{}", 1, 1, "", "", None, mi,
    )
    .await
    .unwrap();
    let list = multireview::list_reviews(&pool).await.unwrap();
    assert_eq!(
        list.iter().map(|r| r.id).collect::<Vec<_>>(),
        vec![b, a],
        "최신순(id desc)"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn get_review_missing_id_returns_none() {
    let (pool, path) = setup().await;
    assert!(multireview::get_review(&pool, 999).await.unwrap().is_none());
    let _ = std::fs::remove_file(&path);
}
