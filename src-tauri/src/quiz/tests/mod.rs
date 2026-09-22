mod inbox;
mod schema;
mod serve;
mod source;

use std::sync::atomic::{AtomicU32, Ordering};

static DATABASE_COUNTER: AtomicU32 = AtomicU32::new(0);

/// quiz 스키마까지 세운 풀.
pub async fn test_pool() -> sqlx::SqlitePool {
    let pool = raw_pool().await;
    crate::quiz::migrate(&pool).await.unwrap();
    pool
}

/// `quiz::migrate`를 부르지 않은 풀 — 멱등성을 검증할 때 쓴다.
///
/// 임시 파일 DB인 이유는 `sqlite::memory:`가 풀의 커넥션마다 별개의 빈 DB를 보기 때문이다
/// (`knowledge/tests/mod.rs:20`과 같은 관례).
///
/// `knowledge`는 세운다 — `quiz_items.chunk_id`가 `knowledge_chunks`를 참조하므로
/// 그것이 없으면 quiz 쪽 CREATE 자체가 실패한다.
pub async fn raw_pool() -> sqlx::SqlitePool {
    let sequence = DATABASE_COUNTER.fetch_add(1, Ordering::SeqCst);
    let path = crate::testtmp::dir().join(format!(
        "praxis-quiz-{}-{sequence}.sqlite",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let pool = crate::db::init_pool(path.to_str().unwrap()).await.unwrap();
    crate::knowledge::migrate(&pool).await.unwrap();
    pool
}
