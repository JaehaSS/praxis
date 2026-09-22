//! `today` 모듈 테스트 공용 헬퍼.

mod backlog;
mod carry;
mod close;
mod day;
mod range;
mod reconcile;
mod schema;
mod store;
mod suggest;

use std::sync::atomic::{AtomicU32, Ordering};

static DATABASE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// 임시 파일 DB — `sqlite::memory:`는 풀의 커넥션마다 별개의 빈 DB를 보므로 쓰지 않는다
/// (`knowledge/tests/mod.rs:16`과 같은 관례).
pub async fn test_pool() -> sqlx::SqlitePool {
    let sequence = DATABASE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = crate::testtmp::dir().join(format!(
        "praxis-today-{}-{sequence}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = crate::db::init_pool(path.to_str().unwrap()).await.unwrap();
    crate::today::migrate(&pool).await.unwrap();
    pool
}
