//! `review_annotations` 스키마·CRUD 통합 테스트. `cargo test`

#[path = "support/temp_root.rs"]
mod temp_root;

use std::sync::atomic::{AtomicU32, Ordering};

use praxis_lib::annotations::{self, status};
use praxis_lib::db;

static COUNTER: AtomicU32 = AtomicU32::new(0);

async fn setup() -> (sqlx::SqlitePool, String) {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = temp_root::dir()
        .join(format!(
            "praxis-annotations-test-{}-{}.sqlite",
            std::process::id(),
            n
        ))
        .to_string_lossy()
        .into_owned();
    let pool = db::init_pool(&path).await.unwrap();
    annotations::migrate(&pool).await.unwrap();
    (pool, path)
}

#[tokio::test]
async fn create_draft_and_list_by_task_round_trip() {
    let (pool, path) = setup().await;
    let a = annotations::create_draft(&pool, 1, "hunk-a", "src/x.ts", 11, "new", "코멘트1", 1000)
        .await
        .unwrap();
    assert_eq!(a.status, status::DRAFT);
    annotations::create_draft(&pool, 1, "hunk-b", "src/y.ts", 4, "old", "코멘트2", 1001)
        .await
        .unwrap();
    annotations::create_draft(&pool, 2, "hunk-c", "src/z.ts", 1, "new", "다른 작업", 1002)
        .await
        .unwrap();

    let list = annotations::list_by_task(&pool, 1).await.unwrap();
    assert_eq!(list.len(), 2, "task 1의 주석만");
    assert_eq!(list[0].body_md, "코멘트1", "created_at 오름차순");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn update_draft_body_only_while_draft() {
    let (pool, path) = setup().await;
    let a = annotations::create_draft(&pool, 1, "hunk-a", "src/x.ts", 11, "new", "초안", 1000)
        .await
        .unwrap();

    assert!(annotations::update_draft_body(&pool, &a.id, "수정된 초안")
        .await
        .unwrap());
    let after = annotations::list_by_task(&pool, 1).await.unwrap();
    assert_eq!(after[0].body_md, "수정된 초안");

    annotations::mark_sent(&pool, 1, std::slice::from_ref(&a.id))
        .await
        .unwrap();
    assert!(
        !annotations::update_draft_body(&pool, &a.id, "전송 후 수정 시도")
            .await
            .unwrap(),
        "sent 상태는 본문을 더 이상 갱신하지 않는다"
    );
    let after_sent = annotations::list_by_task(&pool, 1).await.unwrap();
    assert_eq!(
        after_sent[0].body_md, "수정된 초안",
        "전송 이후 본문은 불변"
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn mark_sent_is_transactional_and_only_affects_drafts_of_the_task() {
    let (pool, path) = setup().await;
    let a = annotations::create_draft(&pool, 1, "h1", "a.ts", 1, "new", "c1", 1000)
        .await
        .unwrap();
    let b = annotations::create_draft(&pool, 1, "h2", "b.ts", 2, "new", "c2", 1001)
        .await
        .unwrap();
    let other_task = annotations::create_draft(&pool, 2, "h3", "c.ts", 3, "new", "c3", 1002)
        .await
        .unwrap();

    let updated = annotations::mark_sent(
        &pool,
        1,
        &[a.id.clone(), b.id.clone(), other_task.id.clone()],
    )
    .await
    .unwrap();
    assert_eq!(
        updated, 2,
        "task 1 소속 draft 2건만 갱신 — 타 작업 id는 무시"
    );

    let list = annotations::list_by_task(&pool, 1).await.unwrap();
    assert!(list.iter().all(|x| x.status == status::SENT));
    let other = annotations::list_by_task(&pool, 2).await.unwrap();
    assert_eq!(other[0].status, status::DRAFT, "타 작업은 영향받지 않음");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn mark_draft_rolls_back_a_failed_resend() {
    let (pool, path) = setup().await;
    let a = annotations::create_draft(&pool, 1, "h1", "a.ts", 1, "new", "c1", 1000)
        .await
        .unwrap();
    annotations::mark_sent(&pool, 1, std::slice::from_ref(&a.id))
        .await
        .unwrap();
    assert_eq!(
        annotations::list_by_task(&pool, 1).await.unwrap()[0].status,
        status::SENT
    );

    let reverted = annotations::mark_draft(&pool, 1, std::slice::from_ref(&a.id))
        .await
        .unwrap();
    assert_eq!(reverted, 1);
    assert_eq!(
        annotations::list_by_task(&pool, 1).await.unwrap()[0].status,
        status::DRAFT
    );
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn list_by_ids_scopes_to_task_and_preserves_requested_set() {
    let (pool, path) = setup().await;
    let a = annotations::create_draft(&pool, 1, "h1", "a.ts", 1, "new", "c1", 1000)
        .await
        .unwrap();
    let cross_task = annotations::create_draft(&pool, 2, "h2", "b.ts", 2, "new", "c2", 1001)
        .await
        .unwrap();

    let result = annotations::list_by_ids(&pool, 1, &[a.id.clone(), cross_task.id.clone()])
        .await
        .unwrap();
    assert_eq!(result.len(), 1, "다른 작업 소속 id는 제외");
    assert_eq!(result[0].id, a.id);

    assert!(annotations::list_by_ids(&pool, 1, &[])
        .await
        .unwrap()
        .is_empty());
    let _ = std::fs::remove_file(&path);
}

/// CHECK(status IN ('draft','sent','resolved')) 제약 — 스키마 레벨에서 임의 값 삽입을 거부.
#[tokio::test]
async fn schema_rejects_unknown_status_values() {
    let (pool, path) = setup().await;
    let result = sqlx::query(
        "INSERT INTO review_annotations (id, task_id, hunk_id, path, line, side, body_md, status, created_at) \
         VALUES ('ann-x', 1, 'h', 'a.ts', 1, 'new', 'c', 'bogus', 1000)",
    )
    .execute(&pool)
    .await;
    assert!(
        result.is_err(),
        "CHECK 제약이 알 수 없는 status를 거부해야 함"
    );
    let _ = std::fs::remove_file(&path);
}

/// 마이그레이션 재실행은 멱등 — 기존 데이터를 보존한 채 에러 없이 통과.
#[tokio::test]
async fn migrate_is_idempotent() {
    let (pool, path) = setup().await;
    annotations::create_draft(&pool, 1, "h1", "a.ts", 1, "new", "c1", 1000)
        .await
        .unwrap();
    annotations::migrate(&pool).await.expect("재실행도 성공");
    assert_eq!(annotations::list_by_task(&pool, 1).await.unwrap().len(), 1);
    let _ = std::fs::remove_file(&path);
}
