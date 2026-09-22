use super::test_pool;

#[tokio::test]
async fn migrate_is_idempotent() {
    let pool = test_pool().await;
    // 두 번째 호출이 실패하지 않아야 한다 (앱 기동마다 불린다).
    crate::today::migrate(&pool).await.unwrap();
}

#[tokio::test]
async fn one_task_cannot_bind_to_two_items() {
    let pool = test_pool().await;
    let insert = "INSERT INTO day_items (day, title, status, position, task_id, created_at, updated_at) \
                  VALUES ('2026-08-03', ?, 'open', 0, 7, 0, 0)";
    sqlx::query(insert)
        .bind("첫째")
        .execute(&pool)
        .await
        .unwrap();
    let second = sqlx::query(insert).bind("둘째").execute(&pool).await;
    assert!(
        second.is_err(),
        "task_id 유니크 인덱스가 두 번째 삽입을 막아야 한다"
    );
}

#[tokio::test]
async fn same_day_same_source_ref_is_rejected_but_null_ref_is_free() {
    let pool = test_pool().await;
    let insert = "INSERT INTO day_items (day, title, status, position, source, source_ref, created_at, updated_at) \
                  VALUES ('2026-08-03', ?, 'open', 0, 'github', ?, 0, 0)";
    sqlx::query(insert)
        .bind("이슈 12")
        .bind("12")
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        sqlx::query(insert)
            .bind("이슈 12 재제안")
            .bind("12")
            .execute(&pool)
            .await
            .is_err(),
        "같은 날 같은 출처는 한 번만"
    );
    // 수동 항목(source_ref NULL)은 몇 개든 들어간다 — 부분 인덱스라 NULL은 걸리지 않는다.
    let manual = "INSERT INTO day_items (day, title, status, position, created_at, updated_at) \
                  VALUES ('2026-08-03', ?, 'open', 0, 0, 0)";
    sqlx::query(manual)
        .bind("손으로 적은 것")
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(manual)
        .bind("또 적은 것")
        .execute(&pool)
        .await
        .unwrap();
}
