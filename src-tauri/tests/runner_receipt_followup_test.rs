//! 대화 후속 턴의 시작 영수증 (설계 0013 §10 M2 후속 · ADR 0029)
//!
//! 영수증은 **작업 시작마다 하나**다. task_id를 기본키로 두면 같은 task가 다시 시작하는
//! 대화 후속 턴이 UNIQUE 위반으로 시작 게이트에서 막히고, 작업이 Failed로 떨어진다.
//! 실제로 2026-07-26 폰↔데스크톱 대화에서 이 경로가 터졌다.

#[path = "support/temp_root.rs"]
mod temp_root;

use praxis_lib::db;

#[tokio::test]
async fn 같은_작업이_다시_시작해도_영수증이_쌓인다() {
    let (pool, path) = pool("receipt-followup").await;
    let task_id = insert_task(&pool).await;

    // 시작 게이트가 실제로 넣는 것과 같은 모양으로, 두 번의 시작을 기록한다.
    insert_receipt(&pool, task_id, 1, "[1]", 100).await.unwrap();
    insert_receipt(&pool, task_id, 2, "[2]", 200)
        .await
        .expect("후속 턴의 영수증이 거절되면 대화가 두 번째 턴에서 끊긴다");

    let count: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM task_start_receipts WHERE task_id = ?")
            .bind(task_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 2);

    // 각 시작이 어떤 투영 위에서 돌았는지 순서대로 남아야 원장으로서 의미가 있다.
    let projections: Vec<i64> = sqlx::query_scalar(
        "SELECT projection_id FROM task_start_receipts WHERE task_id = ? ORDER BY id",
    )
    .bind(task_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(projections, vec![1, 2]);

    cleanup(&path);
}

#[tokio::test]
async fn 영수증은_여전히_수정도_삭제도_되지_않는다() {
    // 스키마를 바꾸면서 immutable 계약(ADR 0029)이 풀리면 안 된다.
    let (pool, path) = pool("receipt-immutable").await;
    let task_id = insert_task(&pool).await;
    insert_receipt(&pool, task_id, 1, "[1]", 100).await.unwrap();

    let updated = sqlx::query("UPDATE task_start_receipts SET projection_id = 9 WHERE task_id = ?")
        .bind(task_id)
        .execute(&pool)
        .await;
    assert!(updated.is_err(), "영수증은 수정할 수 없어야 한다");

    let deleted = sqlx::query("DELETE FROM task_start_receipts WHERE task_id = ?")
        .bind(task_id)
        .execute(&pool)
        .await;
    assert!(deleted.is_err(), "영수증은 삭제할 수 없어야 한다");

    cleanup(&path);
}

#[tokio::test]
async fn 구버전_스키마를_시작별_영수증으로_옮긴다() {
    // 이미 돌고 있는 Runner의 DB는 task_id PRIMARY KEY 스키마다. 기존 원장을 잃지 않고
    // 새 스키마로 넘어가야 한다.
    let path = temp_path("receipt-migrate");
    let url = format!(
        "sqlite://{}?mode=rwc",
        path.to_string_lossy().replace('\\', "/")
    );
    let legacy = sqlx::SqlitePool::connect(&url).await.unwrap();
    sqlx::query(
        "CREATE TABLE task_start_receipts (\
           task_id INTEGER PRIMARY KEY, \
           projection_id INTEGER NOT NULL, \
           source_checks_json TEXT NOT NULL, \
           created_at INTEGER NOT NULL)",
    )
    .execute(&legacy)
    .await
    .unwrap();
    sqlx::query(
        "CREATE TRIGGER task_start_receipts_immutable BEFORE UPDATE ON task_start_receipts \
         BEGIN SELECT RAISE(ABORT, 'task start receipt is immutable'); END",
    )
    .execute(&legacy)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO task_start_receipts (task_id, projection_id, source_checks_json, created_at) \
         VALUES (7, 3, '[5]', 42)",
    )
    .execute(&legacy)
    .await
    .unwrap();
    legacy.close().await;

    // init_pool이 마이그레이션을 수행한다.
    let pool = db::init_pool(path.to_str().unwrap()).await.unwrap();

    let row: (i64, i64, String, i64) = sqlx::query_as(
        "SELECT task_id, projection_id, source_checks_json, created_at FROM task_start_receipts",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        row,
        (7, 3, "[5]".to_string(), 42),
        "기존 원장을 잃으면 안 된다"
    );

    // 옮긴 뒤에는 같은 작업의 두 번째 시작이 통과해야 한다.
    insert_receipt(&pool, 7, 4, "[6]", 50).await.unwrap();

    // 트리거도 새 테이블에 다시 붙어 있어야 한다.
    let updated = sqlx::query("UPDATE task_start_receipts SET projection_id = 9 WHERE task_id = 7")
        .execute(&pool)
        .await;
    assert!(updated.is_err(), "마이그레이션 후에도 immutable이어야 한다");

    cleanup(&path);
}

async fn insert_receipt(
    pool: &sqlx::SqlitePool,
    task_id: i64,
    projection_id: i64,
    checks: &str,
    now: i64,
) -> Result<sqlx::sqlite::SqliteQueryResult, sqlx::Error> {
    sqlx::query(
        "INSERT INTO task_start_receipts (task_id, projection_id, source_checks_json, created_at) \
         VALUES (?, ?, ?, ?)",
    )
    .bind(task_id)
    .bind(projection_id)
    .bind(checks)
    .bind(now)
    .execute(pool)
    .await
}

async fn insert_task(pool: &sqlx::SqlitePool) -> i64 {
    db::insert_task(
        pool,
        "/tmp",
        "branch",
        "main",
        "/tmp",
        "대화",
        None,
        None,
        "conversation",
        1,
    )
    .await
    .unwrap()
}

fn temp_path(label: &str) -> std::path::PathBuf {
    let path = temp_root::dir().join(format!("praxis-{label}-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    path
}

async fn pool(label: &str) -> (sqlx::SqlitePool, std::path::PathBuf) {
    let path = temp_path(label);
    let pool = db::init_pool(path.to_str().unwrap()).await.unwrap();
    (pool, path)
}

fn cleanup(path: &std::path::Path) {
    let _ = std::fs::remove_file(path);
}
